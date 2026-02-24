use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{DbBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PortAllocations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PortAllocations::Port)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PortAllocations::DeploymentId).integer())
                    .col(ColumnDef::new(PortAllocations::ProjectName).text())
                    .col(ColumnDef::new(PortAllocations::ServiceName).text())
                    .col(ColumnDef::new(PortAllocations::Branch).text())
                    .col(
                        ColumnDef::new(PortAllocations::AllocatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_port_allocations_deployment")
                            .from(PortAllocations::Table, PortAllocations::DeploymentId)
                            .to(Deployments::Table, Deployments::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        // Add check constraint for port range
        manager
            .get_connection()
            .execute(Statement::from_string(
                DbBackend::Postgres,
                "ALTER TABLE port_allocations ADD CONSTRAINT chk_port_range CHECK (port >= 18000 AND port <= 19999)".to_string(),
            ))
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_port_allocations_deployment")
                    .table(PortAllocations::Table)
                    .col(PortAllocations::DeploymentId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PortAllocations::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum PortAllocations {
    Table,
    Port,
    DeploymentId,
    ProjectName,
    ServiceName,
    Branch,
    AllocatedAt,
}

#[derive(DeriveIden)]
enum Deployments {
    Table,
    Id,
}
