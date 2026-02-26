use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Builds::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Builds::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Builds::ProjectName).text().not_null())
                    .col(ColumnDef::new(Builds::Branch).text().not_null())
                    .col(ColumnDef::new(Builds::GitRef).text().not_null())
                    .col(ColumnDef::new(Builds::CommitSha).text().not_null())
                    .col(
                        ColumnDef::new(Builds::Status)
                            .custom(Alias::new("build_status"))
                            .not_null()
                            .default("queued"),
                    )
                    .col(ColumnDef::new(Builds::StartedAt).timestamp())
                    .col(ColumnDef::new(Builds::FinishedAt).timestamp())
                    .col(
                        ColumnDef::new(Builds::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Builds::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_builds_project")
                            .from(Builds::Table, Builds::ProjectName)
                            .to(Projects::Table, Projects::Name)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_builds_project_branch")
                    .table(Builds::Table)
                    .col(Builds::ProjectName)
                    .col(Builds::Branch)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_builds_status")
                    .table(Builds::Table)
                    .col(Builds::Status)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_builds_created_at")
                    .table(Builds::Table)
                    .col(Builds::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Builds::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Builds {
    Table,
    Id,
    ProjectName,
    Branch,
    GitRef,
    CommitSha,
    Status,
    StartedAt,
    FinishedAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Name,
}
