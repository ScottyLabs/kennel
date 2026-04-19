use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Deployments::Table)
                    .add_column(string_null(Deployments::ConfigStorePath))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Builds::Table)
                    .add_column(string_null(Builds::ConfigStorePath))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Deployments::Table)
                    .drop_column(Deployments::ConfigStorePath)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Builds::Table)
                    .drop_column(Builds::ConfigStorePath)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Deployments {
    Table,
    ConfigStorePath,
}

#[derive(DeriveIden)]
enum Builds {
    Table,
    ConfigStorePath,
}
