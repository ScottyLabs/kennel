pub mod build_results;
pub mod builds;
pub mod cleanup;
pub mod deployments;
pub mod error;
pub mod port_allocations;
pub mod preview_databases;
pub mod projects;
pub mod reconciliation;
pub mod services;

pub use cleanup::CleanupSummary;
pub use error::{Result, StoreError};
pub use reconciliation::ReconciliationSummary;

use sea_orm::DatabaseConnection;

pub struct Store {
    db: DatabaseConnection,
}

impl Store {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    pub fn projects(&self) -> projects::ProjectRepository<'_> {
        projects::ProjectRepository::new(&self.db)
    }

    pub fn services(&self) -> services::ServiceRepository<'_> {
        services::ServiceRepository::new(&self.db)
    }

    pub fn deployments(&self) -> deployments::DeploymentRepository<'_> {
        deployments::DeploymentRepository::new(&self.db)
    }

    pub fn builds(&self) -> builds::BuildRepository<'_> {
        builds::BuildRepository::new(&self.db)
    }

    pub fn build_results(&self) -> build_results::BuildResultRepository<'_> {
        build_results::BuildResultRepository::new(&self.db)
    }

    pub fn port_allocations(&self) -> port_allocations::PortAllocationRepository<'_> {
        port_allocations::PortAllocationRepository::new(&self.db)
    }

    pub fn preview_databases(&self) -> preview_databases::PreviewDatabaseRepository<'_> {
        preview_databases::PreviewDatabaseRepository::new(&self.db)
    }
}
