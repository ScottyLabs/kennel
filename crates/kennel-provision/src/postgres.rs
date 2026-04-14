use crate::{ResourceProvider, ResourceRequest};
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
}

impl ResourceProvider for PostgresProvider {
    fn name(&self) -> &str {
        "postgres"
    }

    async fn provision(
        &self,
        request: &ResourceRequest,
    ) -> anyhow::Result<HashMap<String, String>> {
        let db_name = Self::db_name(request);

        let output = tokio::process::Command::new("psql")
            .args([
                "-h",
                &self.socket_dir,
                "-tAc",
                &format!("SELECT 1 FROM pg_database WHERE datname = '{db_name}'"),
            ])
            .output()
            .await?;

        if String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            let create = tokio::process::Command::new("createdb")
                .args(["-h", &self.socket_dir, &db_name])
                .output()
                .await?;

            anyhow::ensure!(
                create.status.success(),
                "createdb failed: {}",
                String::from_utf8_lossy(&create.stderr)
            );

            tracing::info!(db = %db_name, "created database");
        }

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
