use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DeployRequests::Table)
                    .if_not_exists()
                    .col(string(DeployRequests::Id).primary_key())
                    .col(string(DeployRequests::ProjectId))
                    .col(string(DeployRequests::Branch))
                    .col(string(DeployRequests::GitRef))
                    .col(string(DeployRequests::CommitSha))
                    .col(string(DeployRequests::Status).default("pending"))
                    .col(timestamp(DeployRequests::CreatedAt).default(Expr::current_timestamp()))
                    .col(timestamp(DeployRequests::UpdatedAt).default(Expr::current_timestamp()))
                    .foreign_key(
                        ForeignKey::create()
                            .from(DeployRequests::Table, DeployRequests::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_deploy_requests_project_branch")
                    .table(DeployRequests::Table)
                    .col(DeployRequests::ProjectId)
                    .col(DeployRequests::Branch)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_deploy_requests_status")
                    .table(DeployRequests::Table)
                    .col(DeployRequests::Status)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DeployRequests::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum DeployRequests {
    Table,
    Id,
    ProjectId,
    Branch,
    GitRef,
    CommitSha,
    Status,
    CreatedAt,
    UpdatedAt,
}
