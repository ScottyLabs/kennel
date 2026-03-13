use std::collections::HashMap;

use async_trait::async_trait;
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

use crate::{ReconciliationSummary, ResourceProvider, ResourceRequest};

pub struct PostgresProvider {
    db: DatabaseConnection,
    socket_dir: String,
}

/// Validate that an identifier contains only safe characters for use in SQL DDL.
/// Returns an error instead of panicking.
fn validate_identifier(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("identifier cannot be empty");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!(
            "identifier contains invalid characters (only alphanumeric, underscore, hyphen allowed): {name}"
        );
    }
    Ok(())
}

impl PostgresProvider {
    pub fn new(db: DatabaseConnection, socket_dir: String) -> Self {
        Self { db, socket_dir }
    }

    fn database_name(request: &ResourceRequest) -> anyhow::Result<String> {
        let name = format!(
            "kennel_{}_{}",
            request.project_name.replace('-', "_"),
            request.branch_slug.replace('-', "_")
        );
        validate_identifier(&name)?;
        Ok(name)
    }
}

#[async_trait]
impl ResourceProvider for PostgresProvider {
    fn name(&self) -> &str {
        "postgres"
    }

    async fn provision(
        &self,
        request: &ResourceRequest,
    ) -> anyhow::Result<HashMap<String, String>> {
        let db_name = Self::database_name(request)?;
        validate_identifier(&request.system_user)?;

        // Use a parameterized query for the existence check.
        let exists: Vec<sea_orm::QueryResult> = self
            .db
            .query_all(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT 1 FROM pg_database WHERE datname = $1",
                [db_name.clone().into()],
            ))
            .await?;

        if exists.is_empty() {
            self.db
                .execute_unprepared(&format!("CREATE DATABASE \"{db_name}\""))
                .await?;

            tracing::info!("Created database {db_name}");

            // Create the role if it doesn't exist (peer auth requires it).
            self.db
                .execute_unprepared(&format!(
                    "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{user}') THEN CREATE ROLE \"{user}\" WITH LOGIN; END IF; END $$;",
                    user = request.system_user
                ))
                .await?;

            self.db
                .execute_unprepared(&format!(
                    "GRANT ALL PRIVILEGES ON DATABASE \"{db_name}\" TO \"{}\"",
                    request.system_user
                ))
                .await?;
        }

        let mut env = HashMap::new();
        env.insert(
            "DATABASE_URL".into(),
            format!("postgresql:///{db_name}?host={}", self.socket_dir),
        );

        Ok(env)
    }

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()> {
        let db_name = Self::database_name(request)?;

        // Terminate active connections before dropping.
        self.db
            .execute_unprepared(&format!(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{db_name}' AND pid <> pg_backend_pid()"
            ))
            .await?;

        self.db
            .execute_unprepared(&format!("DROP DATABASE IF EXISTS \"{db_name}\""))
            .await?;

        tracing::info!("Dropped database {db_name}");
        Ok(())
    }

    async fn reconcile(
        &self,
        active_deployments: &[ResourceRequest],
    ) -> anyhow::Result<ReconciliationSummary> {
        let active_db_names: std::collections::HashSet<String> = active_deployments
            .iter()
            .filter_map(|r| Self::database_name(r).ok())
            .collect();

        let rows: Vec<sea_orm::QueryResult> = self
            .db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT datname FROM pg_database WHERE datname LIKE 'kennel_%'".to_string(),
            ))
            .await?;

        let mut summary = ReconciliationSummary::default();

        for row in rows {
            let db_name: String = row.try_get("", "datname")?;
            if !active_db_names.contains(&db_name) {
                tracing::info!("Removing orphaned database: {db_name}");
                self.db
                    .execute_unprepared(&format!("DROP DATABASE IF EXISTS \"{db_name}\""))
                    .await?;
                summary.orphaned_resources_removed += 1;
            }
        }

        Ok(summary)
    }
}
