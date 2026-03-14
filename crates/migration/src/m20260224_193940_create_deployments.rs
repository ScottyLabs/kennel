use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Deployments::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Deployments::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Deployments::ProjectName).text().not_null())
                    .col(ColumnDef::new(Deployments::ServiceName).text().not_null())
                    .col(ColumnDef::new(Deployments::Branch).text().not_null())
                    .col(ColumnDef::new(Deployments::BranchSlug).text().not_null())
                    .col(ColumnDef::new(Deployments::Environment).text().not_null())
                    .col(ColumnDef::new(Deployments::GitRef).text().not_null())
                    .col(ColumnDef::new(Deployments::StorePath).text())
                    .col(ColumnDef::new(Deployments::ProcessConfig).json_binary())
                    .col(
                        ColumnDef::new(Deployments::Status)
                            .custom(Alias::new("deployment_status"))
                            .not_null()
                            .default("deployed"),
                    )
                    .col(ColumnDef::new(Deployments::Domain).text().not_null())
                    .col(ColumnDef::new(Deployments::ProcessName).text())
                    .col(
                        ColumnDef::new(Deployments::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Deployments::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Deployments::LastActivity)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_deployments_service")
                            .from(Deployments::Table, Deployments::ProjectName)
                            .from_col(Deployments::ServiceName)
                            .to(Services::Table, Services::ProjectName)
                            .to_col(Services::Name)
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
                    .col(Deployments::ProjectName)
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

        manager
            .create_index(
                Index::create()
                    .name("idx_deployments_status")
                    .table(Deployments::Table)
                    .col(Deployments::Status)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_deployments_activity")
                    .table(Deployments::Table)
                    .col(Deployments::LastActivity)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_deployments_process_name")
                    .table(Deployments::Table)
                    .col(Deployments::ProcessName)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Deployments::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Deployments {
    Table,
    Id,
    ProjectName,
    ServiceName,
    Branch,
    BranchSlug,
    Environment,
    GitRef,
    StorePath,
    ProcessConfig,
    Status,
    Domain,
    ProcessName,
    CreatedAt,
    UpdatedAt,
    LastActivity,
}

#[derive(DeriveIden)]
enum Services {
    Table,
    ProjectName,
    Name,
}
