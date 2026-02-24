use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Projects::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Projects::Name)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Projects::RepoUrl).text().not_null())
                    .col(
                        ColumnDef::new(Projects::RepoType)
                            .custom(Alias::new("repo_type"))
                            .not_null(),
                    )
                    .col(ColumnDef::new(Projects::WebhookSecret).text().not_null())
                    .col(
                        ColumnDef::new(Projects::DefaultBranch)
                            .text()
                            .not_null()
                            .default("main"),
                    )
                    .col(
                        ColumnDef::new(Projects::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Projects::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_projects_updated_at")
                    .table(Projects::Table)
                    .col(Projects::UpdatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Projects::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Name,
    RepoUrl,
    RepoType,
    WebhookSecret,
    DefaultBranch,
    CreatedAt,
    UpdatedAt,
}
