use std::collections::HashMap;

use async_trait::async_trait;
use sea_orm::{ConnectionTrait, DatabaseConnection};

use crate::{ReconciliationSummary, ResourceProvider, ResourceRequest};

pub struct PostgresProvider {
    db: DatabaseConnection,
    socket_dir: String,
}

impl PostgresProvider {
    pub fn new(db: DatabaseConnection, socket_dir: String) -> Self {
        Self { db, socket_dir }
    }

    fn database_name(request: &ResourceRequest) -> String {
        let name = format!(
            "kennel_{}_{}",
            request.project_name.replace('-', "_"),
            request.branch_slug.replace('-', "_")
        );
        // Only allow alphanumeric and underscores to prevent SQL injection.
        assert!(
            name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "invalid database name: {name}"
        );
        name
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
        let db_name = Self::database_name(request);

        // Create database if it doesn't exist.
        let exists: Vec<sea_orm::QueryResult> = self
            .db
            .query_all(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                format!("SELECT 1 FROM pg_database WHERE datname = '{db_name}'"),
            ))
            .await?;

        if exists.is_empty() {
            self.db
                .execute_unprepared(&format!("CREATE DATABASE \"{db_name}\""))
                .await?;

            tracing::info!("Created database {db_name}");

            // Grant access to the deployment's system user.
            let _ = self
                .db
                .execute_unprepared(&format!(
                    "GRANT ALL PRIVILEGES ON DATABASE \"{db_name}\" TO \"{}\"",
                    request.system_user
                ))
                .await;
        }

        let mut env = HashMap::new();
        env.insert(
            "DATABASE_URL".into(),
            format!("postgresql:///{db_name}?host={}", self.socket_dir),
        );

        Ok(env)
    }

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()> {
        let db_name = Self::database_name(request);

        // Terminate active connections.
        let _ = self
            .db
            .execute_unprepared(&format!(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{db_name}' AND pid <> pg_backend_pid()"
            ))
            .await;

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
        let active_db_names: std::collections::HashSet<String> =
            active_deployments.iter().map(Self::database_name).collect();

        let rows: Vec<sea_orm::QueryResult> = self
            .db
            .query_all(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT datname FROM pg_database WHERE datname LIKE 'kennel_%'".to_string(),
            ))
            .await?;

        let mut summary = ReconciliationSummary::default();

        for row in rows {
            let db_name: String = row.try_get("", "datname")?;
            if !active_db_names.contains(&db_name) {
                tracing::info!("Removing orphaned database: {db_name}");
                let _ = self
                    .db
                    .execute_unprepared(&format!("DROP DATABASE IF EXISTS \"{db_name}\""))
                    .await;
                summary.orphaned_resources_removed += 1;
            }
        }

        Ok(summary)
    }
}
