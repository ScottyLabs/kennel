pub use sea_orm_migration::prelude::*;

mod m20260224_193925_create_enums;
mod m20260224_193940_create_builds;
mod m20260224_193940_create_deployments;
mod m20260224_193940_create_port_allocations;
mod m20260224_193940_create_preview_databases;
mod m20260224_193940_create_projects;
mod m20260224_193940_create_services;
mod m20260224_194026_create_triggers;
mod m20260224_195047_create_build_results;
mod m20260225_055843_add_error_message_to_build_results;
mod m20260226_063306_create_dns_records;
mod m20260226_215312_add_builds_unique_constraint;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260224_193925_create_enums::Migration),
            Box::new(m20260224_193940_create_builds::Migration),
            Box::new(m20260224_193940_create_deployments::Migration),
            Box::new(m20260224_193940_create_port_allocations::Migration),
            Box::new(m20260224_193940_create_preview_databases::Migration),
            Box::new(m20260224_193940_create_projects::Migration),
            Box::new(m20260224_193940_create_services::Migration),
            Box::new(m20260224_194026_create_triggers::Migration),
            Box::new(m20260224_195047_create_build_results::Migration),
            Box::new(m20260225_055843_add_error_message_to_build_results::Migration),
            Box::new(m20260226_063306_create_dns_records::Migration),
            Box::new(m20260226_215312_add_builds_unique_constraint::Migration),
        ]
    }
}
