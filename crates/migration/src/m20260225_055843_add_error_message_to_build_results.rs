use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(BuildResults::Table)
                    .add_column(text_null(BuildResults::ErrorMessage))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(BuildResults::Table)
                    .drop_column(BuildResults::ErrorMessage)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum BuildResults {
    Table,
    ErrorMessage,
}
