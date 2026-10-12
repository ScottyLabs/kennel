use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DeployRequests::Table)
                    .add_column(text_null(DeployRequests::FailureStage))
                    .to_owned(),
            )
            .await?;

        // Requests that failed before stages were recorded are attributed to the build
        manager
            .exec_stmt(
                Query::update()
                    .table(DeployRequests::Table)
                    .value(DeployRequests::FailureStage, "build")
                    .and_where(Expr::col(DeployRequests::Status).eq("failed"))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DeployRequests::Table)
                    .drop_column(DeployRequests::FailureStage)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum DeployRequests {
    Table,
    Status,
    FailureStage,
}
