use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{DbBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Create function for updating updated_at timestamp
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            r#"
            CREATE OR REPLACE FUNCTION update_updated_at_column()
            RETURNS TRIGGER AS $$
            BEGIN
                NEW.updated_at = now();
                RETURN NEW;
            END;
            $$ language 'plpgsql';
            "#
            .to_string(),
        ))
        .await?;

        // Create function for updating last_activity on git_ref change
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            r#"
            CREATE OR REPLACE FUNCTION update_deployment_activity()
            RETURNS TRIGGER AS $$
            BEGIN
                IF NEW.git_ref != OLD.git_ref THEN
                    NEW.last_activity = now();
                END IF;
                RETURN NEW;
            END;
            $$ language 'plpgsql';
            "#
            .to_string(),
        ))
        .await?;

        // Create triggers for updated_at
        for table in &["projects", "services", "deployments", "builds"] {
            db.execute(Statement::from_string(
                DbBackend::Postgres,
                format!(
                    "CREATE TRIGGER update_{}_updated_at BEFORE UPDATE ON {} \
                     FOR EACH ROW EXECUTE FUNCTION update_updated_at_column()",
                    table, table
                ),
            ))
            .await?;
        }

        // Create trigger for deployment activity
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "CREATE TRIGGER update_deployments_activity BEFORE UPDATE ON deployments \
             FOR EACH ROW EXECUTE FUNCTION update_deployment_activity()"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Drop triggers
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "DROP TRIGGER IF EXISTS update_deployments_activity ON deployments".to_string(),
        ))
        .await?;

        for table in &["projects", "services", "deployments", "builds"] {
            db.execute(Statement::from_string(
                DbBackend::Postgres,
                format!(
                    "DROP TRIGGER IF EXISTS update_{}_updated_at ON {}",
                    table, table
                ),
            ))
            .await?;
        }

        // Drop functions
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "DROP FUNCTION IF EXISTS update_deployment_activity()".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "DROP FUNCTION IF EXISTS update_updated_at_column()".to_string(),
        ))
        .await?;

        Ok(())
    }
}
