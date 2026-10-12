use crate::{ResourceProvider, ResourceRequest};
use kennel_config::KennelConfig;
use std::collections::HashMap;

pub struct PostgresProvider {
    socket_dir: String,
}

impl PostgresProvider {
    pub fn new(socket_dir: String) -> Self {
        Self { socket_dir }
    }

    fn db_name(request: &ResourceRequest) -> String {
        format!(
            "kennel_{}_{}",
            request.project_name.replace('-', "_"),
            request.branch_slug.replace('-', "_"),
        )
    }

    fn owner_role(request: &ResourceRequest) -> String {
        format!("{}_owner", Self::db_name(request))
    }

    async fn psql(&self, db: &str, sql: &str) -> anyhow::Result<String> {
        let out = tokio::process::Command::new("psql")
            .args(["-h", &self.socket_dir, "-d", db, "-tAc", sql])
            .output()
            .await?;

        anyhow::ensure!(
            out.status.success(),
            "psql failed [{sql}]: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Runs CREATE EXTENSION as the database owner
    async fn create_extensions(&self, db_name: &str, extensions: &[String]) -> anyhow::Result<()> {
        if extensions.is_empty() {
            return Ok(());
        }

        for extension in extensions {
            anyhow::ensure!(
                !extension.is_empty()
                    && extension
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "invalid postgres extension name '{extension}'"
            );
        }

        let row = self
            .psql(
                "postgres",
                &format!(
                    "SELECT pg_get_userbyid(datdba), current_user FROM pg_database WHERE datname = '{db_name}'"
                ),
            )
            .await?;
        let (owner, current) = row
            .split_once('|')
            .ok_or_else(|| anyhow::anyhow!("database {db_name} not found"))?;

        // Databases made by older kennel versions have owners kennel cannot SET ROLE to yet
        if owner != current {
            self.psql(
                "postgres",
                &format!("GRANT \"{owner}\" TO CURRENT_USER WITH SET TRUE"),
            )
            .await?;
        }

        let mut sql = format!("SET ROLE \"{owner}\";");
        for extension in extensions {
            sql.push_str(&format!(" CREATE EXTENSION IF NOT EXISTS \"{extension}\";"));
        }

        self.psql(db_name, &sql).await?;
        Ok(())
    }
}

impl ResourceProvider for PostgresProvider {
    fn name(&self) -> &str {
        "postgres"
    }

    async fn provision(
        &self,
        request: &ResourceRequest,
        config: &KennelConfig,
    ) -> anyhow::Result<HashMap<String, String>> {
        let db_name = Self::db_name(request);
        let user = &request.system_user;
        let owner = Self::owner_role(request);

        if self
            .psql(
                "postgres",
                &format!("SELECT 1 FROM pg_roles WHERE rolname = '{owner}'"),
            )
            .await?
            .is_empty()
        {
            self.psql("postgres", &format!("CREATE ROLE \"{owner}\" NOLOGIN"))
                .await?;

            tracing::info!(role = %owner, "created owner role");
        }

        if self
            .psql(
                "postgres",
                &format!("SELECT 1 FROM pg_roles WHERE rolname = '{user}'"),
            )
            .await?
            .is_empty()
        {
            let create = tokio::process::Command::new("createuser")
                .args(["-h", &self.socket_dir, user])
                .output()
                .await?;

            anyhow::ensure!(
                create.status.success(),
                "createuser failed: {}",
                String::from_utf8_lossy(&create.stderr)
            );

            tracing::info!(user = %user, "created role");
        }

        // Members inherit CREATE on public via pg_database_owner.
        self.psql(
            "postgres",
            &format!("GRANT \"{owner}\" TO \"{user}\" WITH INHERIT TRUE"),
        )
        .await?;

        if self
            .psql(
                "postgres",
                &format!("SELECT 1 FROM pg_database WHERE datname = '{db_name}'"),
            )
            .await?
            .is_empty()
        {
            // createdb -O owner requires the caller to be able to SET ROLE to the owner
            self.psql(
                "postgres",
                &format!("GRANT \"{owner}\" TO CURRENT_USER WITH SET TRUE"),
            )
            .await?;

            let create = tokio::process::Command::new("createdb")
                .args(["-h", &self.socket_dir, "-O", &owner, &db_name])
                .output()
                .await?;

            anyhow::ensure!(
                create.status.success(),
                "createdb failed: {}",
                String::from_utf8_lossy(&create.stderr)
            );

            tracing::info!(db = %db_name, "created database");
        }

        self.psql(
            "postgres",
            &format!("GRANT CREATE ON DATABASE \"{db_name}\" TO \"{owner}\""),
        )
        .await?;

        self.create_extensions(&db_name, &config.postgres_extensions)
            .await?;

        let mut env = HashMap::new();
        env.insert(
            "DATABASE_URL".into(),
            format!("postgresql:///{db_name}?host={}", self.socket_dir),
        );
        Ok(env)
    }

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()> {
        let db_name = Self::db_name(request);
        tokio::process::Command::new("dropdb")
            .args(["-h", &self.socket_dir, "--if-exists", &db_name])
            .output()
            .await?;
        tracing::info!(db = %db_name, "dropped database");
        Ok(())
    }

    async fn reconcile(&self, active: &[ResourceRequest]) -> anyhow::Result<()> {
        let active_dbs: std::collections::HashSet<String> =
            active.iter().map(Self::db_name).collect();

        let output = tokio::process::Command::new("psql")
            .args([
                "-h",
                &self.socket_dir,
                "-d",
                "postgres",
                "-tAc",
                "SELECT datname FROM pg_database WHERE datname LIKE 'kennel_%'",
            ])
            .output()
            .await?;

        let existing: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(String::from)
            .collect();

        for db in existing {
            if !active_dbs.contains(&db) {
                tracing::info!(db = %db, "dropping orphaned database");
                let _ = tokio::process::Command::new("dropdb")
                    .args(["-h", &self.socket_dir, "--if-exists", &db])
                    .output()
                    .await;
            }
        }

        Ok(())
    }
}
