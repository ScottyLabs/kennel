use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BuildResults::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BuildResults::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BuildResults::BuildId).integer().not_null())
                    .col(ColumnDef::new(BuildResults::ServiceName).text().not_null())
                    .col(ColumnDef::new(BuildResults::StorePath).text())
                    .col(
                        ColumnDef::new(BuildResults::Status)
                            .custom(Alias::new("build_result_status"))
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(BuildResults::Changed)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(ColumnDef::new(BuildResults::LogPath).text())
                    .col(ColumnDef::new(BuildResults::StartedAt).timestamp())
                    .col(ColumnDef::new(BuildResults::FinishedAt).timestamp())
                    .col(
                        ColumnDef::new(BuildResults::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_build_results_build")
                            .from(BuildResults::Table, BuildResults::BuildId)
                            .to(Builds::Table, Builds::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_build_results_build")
                    .table(BuildResults::Table)
                    .col(BuildResults::BuildId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_build_results_status")
                    .table(BuildResults::Table)
                    .col(BuildResults::BuildId)
                    .col(BuildResults::Status)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(BuildResults::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum BuildResults {
    Table,
    Id,
    BuildId,
    ServiceName,
    StorePath,
    Status,
    Changed,
    LogPath,
    StartedAt,
    FinishedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Builds {
    Table,
    Id,
}
