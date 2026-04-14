use sea_orm_migration::{prelude::*, schema::*};

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
                    .col(string(Projects::Id).primary_key())
                    .col(string_uniq(Projects::Name))
                    .col(string(Projects::RepoUrl))
                    .col(string(Projects::RepoType))
                    .col(string(Projects::DefaultBranch).default("main"))
                    .col(timestamp(Projects::CreatedAt).default(Expr::current_timestamp()))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Builds::Table)
                    .if_not_exists()
                    .col(string(Builds::Id).primary_key())
                    .col(string(Builds::ProjectId))
                    .col(string(Builds::Branch))
                    .col(string(Builds::GitRef))
                    .col(string(Builds::CommitSha))
                    .col(string(Builds::Status).default("queued"))
                    .col(json_null(Builds::StorePaths))
                    .col(json_null(Builds::KennelConfig))
                    .col(timestamp_null(Builds::StartedAt))
                    .col(timestamp_null(Builds::FinishedAt))
                    .col(timestamp(Builds::CreatedAt).default(Expr::current_timestamp()))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Builds::Table, Builds::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
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
            .create_table(
                Table::create()
                    .table(Deployments::Table)
                    .if_not_exists()
                    .col(string(Deployments::Id).primary_key())
                    .col(string(Deployments::ProjectId))
                    .col(string(Deployments::ServiceName))
                    .col(string(Deployments::ServiceType))
                    .col(string(Deployments::Branch))
                    .col(string(Deployments::BranchSlug))
                    .col(string(Deployments::Environment))
                    .col(string(Deployments::CommitSha))
                    .col(string(Deployments::StorePath))
                    .col(string(Deployments::Domain))
                    .col(string_null(Deployments::CustomDomain))
                    .col(boolean(Deployments::Spa).default(false))
                    .col(string_null(Deployments::UnitName))
                    .col(integer_null(Deployments::Port))
                    .col(timestamp(Deployments::CreatedAt).default(Expr::current_timestamp()))
                    .col(timestamp(Deployments::UpdatedAt).default(Expr::current_timestamp()))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Deployments::Table, Deployments::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_deployments_project_service_branch")
                    .table(Deployments::Table)
                    .col(Deployments::ProjectId)
                    .col(Deployments::ServiceName)
                    .col(Deployments::Branch)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_deployments_domain")
                    .table(Deployments::Table)
                    .col(Deployments::Domain)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Deployments::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Builds::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Projects::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
    Name,
    RepoUrl,
    RepoType,
    DefaultBranch,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Builds {
    Table,
    Id,
    ProjectId,
    Branch,
    GitRef,
    CommitSha,
    Status,
    StorePaths,
    KennelConfig,
    StartedAt,
    FinishedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Deployments {
    Table,
    Id,
    ProjectId,
    ServiceName,
    ServiceType,
    Branch,
    BranchSlug,
    Environment,
    CommitSha,
    StorePath,
    Domain,
    CustomDomain,
    Spa,
    UnitName,
    Port,
    CreatedAt,
    UpdatedAt,
}
