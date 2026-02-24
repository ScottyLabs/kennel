use sea_orm_migration::prelude::extension::postgres::Type;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_type(
                Type::create()
                    .as_enum(Alias::new("repo_type"))
                    .values(vec![Alias::new("forgejo"), Alias::new("github")])
                    .to_owned(),
            )
            .await?;

        manager
            .create_type(
                Type::create()
                    .as_enum(Alias::new("service_type"))
                    .values(vec![
                        Alias::new("service"),
                        Alias::new("static"),
                        Alias::new("image"),
                    ])
                    .to_owned(),
            )
            .await?;

        manager
            .create_type(
                Type::create()
                    .as_enum(Alias::new("deployment_status"))
                    .values(vec![
                        Alias::new("pending"),
                        Alias::new("building"),
                        Alias::new("active"),
                        Alias::new("failed"),
                        Alias::new("tearing_down"),
                        Alias::new("torn_down"),
                    ])
                    .to_owned(),
            )
            .await?;

        manager
            .create_type(
                Type::create()
                    .as_enum(Alias::new("build_status"))
                    .values(vec![
                        Alias::new("queued"),
                        Alias::new("building"),
                        Alias::new("success"),
                        Alias::new("failed"),
                        Alias::new("cancelled"),
                    ])
                    .to_owned(),
            )
            .await?;

        manager
            .create_type(
                Type::create()
                    .as_enum(Alias::new("build_result_status"))
                    .values(vec![
                        Alias::new("pending"),
                        Alias::new("building"),
                        Alias::new("success"),
                        Alias::new("skipped"),
                        Alias::new("failed"),
                    ])
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_type(
                Type::drop()
                    .name(Alias::new("build_result_status"))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_type(Type::drop().name(Alias::new("build_status")).to_owned())
            .await?;

        manager
            .drop_type(
                Type::drop()
                    .name(Alias::new("deployment_status"))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_type(Type::drop().name(Alias::new("service_type")).to_owned())
            .await?;

        manager
            .drop_type(Type::drop().name(Alias::new("repo_type")).to_owned())
            .await?;

        Ok(())
    }
}
