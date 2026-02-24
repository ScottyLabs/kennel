use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{DbBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PreviewDatabases::Table)
                    .if_not_exists()
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
                            .not_null(),
                    )
                    .col(ColumnDef::new(PreviewDatabases::ValkeyDb).integer())
                    .col(
                        ColumnDef::new(PreviewDatabases::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_preview_databases_project")
                            .from(PreviewDatabases::Table, PreviewDatabases::ProjectName)
                            .to(Projects::Table, Projects::Name)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Add check constraint for valkey_db range
        manager
            .get_connection()
            .execute(Statement::from_string(
                DbBackend::Postgres,
                "ALTER TABLE preview_databases ADD CONSTRAINT chk_valkey_db_range CHECK (valkey_db >= 0 AND valkey_db <= 15)".to_string(),
            ))
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_preview_databases_project_branch")
                    .table(PreviewDatabases::Table)
                    .col(PreviewDatabases::ProjectName)
                    .col(PreviewDatabases::Branch)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_preview_databases_database_name")
                    .table(PreviewDatabases::Table)
                    .col(PreviewDatabases::DatabaseName)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_preview_databases_project")
                    .table(PreviewDatabases::Table)
                    .col(PreviewDatabases::ProjectName)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PreviewDatabases::Table).to_owned())
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

#[derive(DeriveIden)]
enum Projects {
    Table,
    Name,
}
