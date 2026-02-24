use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Services::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Services::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Services::ProjectName).text().not_null())
                    .col(ColumnDef::new(Services::Name).text().not_null())
                    .col(
                        ColumnDef::new(Services::Type)
                            .custom(Alias::new("service_type"))
                            .not_null(),
                    )
                    .col(ColumnDef::new(Services::Package).text().not_null())
                    .col(ColumnDef::new(Services::HealthCheck).text())
                    .col(ColumnDef::new(Services::CustomDomain).text())
                    .col(
                        ColumnDef::new(Services::Spa)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Services::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Services::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_services_project")
                            .from(Services::Table, Services::ProjectName)
                            .to(Projects::Table, Projects::Name)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_services_project_name")
                    .table(Services::Table)
                    .col(Services::ProjectName)
                    .col(Services::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_services_project")
                    .table(Services::Table)
                    .col(Services::ProjectName)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_services_custom_domain")
                    .table(Services::Table)
                    .col(Services::CustomDomain)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Services::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Services {
    Table,
    Id,
    ProjectName,
    Name,
    Type,
    Package,
    HealthCheck,
    CustomDomain,
    Spa,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Name,
}
