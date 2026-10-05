use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_builds_project_commit")
                    .table(Builds::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_builds_project_branch_commit")
                    .table(Builds::Table)
                    .col(Builds::ProjectId)
                    .col(Builds::Branch)
                    .col(Builds::CommitSha)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_builds_project_branch_commit")
                    .table(Builds::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_builds_project_commit")
                    .table(Builds::Table)
                    .col(Builds::ProjectId)
                    .col(Builds::CommitSha)
                    .unique()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Builds {
    Table,
    ProjectId,
    Branch,
    CommitSha,
}
