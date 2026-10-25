use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Builds::Table).if_exists().to_owned())
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(DeployRequests::Table)
                    .drop_column(DeployRequests::GitRef)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Artifacts::Table)
                    .if_not_exists()
                    .col(string(Artifacts::Id).primary_key())
                    .col(string(Artifacts::ProjectId))
                    .col(string(Artifacts::Branch))
                    .col(string(Artifacts::CommitSha))
                    .col(string(Artifacts::StorePath))
                    .col(timestamp(Artifacts::CreatedAt).default(Expr::current_timestamp()))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Artifacts::Table, Artifacts::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_artifacts_project_branch_commit")
                    .table(Artifacts::Table)
                    .col(Artifacts::ProjectId)
                    .col(Artifacts::Branch)
                    .col(Artifacts::CommitSha)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration(
            "builds were replaced by nixbot artifacts and cannot be restored".to_string(),
        ))
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Builds {
    Table,
}

#[derive(DeriveIden)]
enum DeployRequests {
    Table,
    GitRef,
}

#[derive(DeriveIden)]
enum Artifacts {
    Table,
    Id,
    ProjectId,
    Branch,
    CommitSha,
    StorePath,
    CreatedAt,
}
