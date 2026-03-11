use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PortAllocations::Table).to_owned())
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PortAllocations::Table)
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
                    .to_owned(),
            )
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
