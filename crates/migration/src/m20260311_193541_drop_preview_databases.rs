use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PreviewDatabases::Table).to_owned())
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PreviewDatabases::Table)
                    .col(
                        ColumnDef::new(PreviewDatabases::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PreviewDatabases::ProjectName)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PreviewDatabases::Branch).text().not_null())
                    .col(
                        ColumnDef::new(PreviewDatabases::DatabaseName)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(PreviewDatabases::ValkeyDb).integer())
                    .col(
                        ColumnDef::new(PreviewDatabases::CreatedAt)
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
enum PreviewDatabases {
    Table,
    Id,
    ProjectName,
    Branch,
    DatabaseName,
    ValkeyDb,
    CreatedAt,
}
