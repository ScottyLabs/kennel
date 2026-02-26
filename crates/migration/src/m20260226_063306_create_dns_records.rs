use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DnsRecords::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DnsRecords::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DnsRecords::Domain)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(DnsRecords::DeploymentId).integer())
                    .col(
                        ColumnDef::new(DnsRecords::ProviderRecordId)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DnsRecords::RecordType).text().not_null())
                    .col(ColumnDef::new(DnsRecords::IpAddress).text().not_null())
                    .col(
                        ColumnDef::new(DnsRecords::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(DnsRecords::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_dns_records_deployment_id")
                            .from(DnsRecords::Table, DnsRecords::DeploymentId)
                            .to(Deployments::Table, Deployments::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_dns_records_deployment_id")
                    .table(DnsRecords::Table)
                    .col(DnsRecords::DeploymentId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_dns_records_domain")
                    .table(DnsRecords::Table)
                    .col(DnsRecords::Domain)
                    .to_owned(),
            )
            .await?;

        // Add dns_status column to deployments table
        manager
            .alter_table(
                Table::alter()
                    .table(Deployments::Table)
                    .add_column(
                        ColumnDef::new(Deployments::DnsStatus)
                            .text()
                            .not_null()
                            .default("pending"),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DnsRecords::Table).to_owned())
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Deployments::Table)
                    .drop_column(Deployments::DnsStatus)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum DnsRecords {
    Table,
    Id,
    Domain,
    DeploymentId,
    ProviderRecordId,
    RecordType,
    IpAddress,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Deployments {
    Table,
    Id,
    DnsStatus,
}
