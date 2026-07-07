pub mod builds;
pub mod deploy_requests;
pub mod deployments;
pub mod projects;

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

    pub fn builds(&self) -> builds::BuildRepository<'_> {
        builds::BuildRepository::new(&self.db)
    }

    pub fn deployments(&self) -> deployments::DeploymentRepository<'_> {
        deployments::DeploymentRepository::new(&self.db)
    }

    pub fn deploy_requests(&self) -> deploy_requests::DeployRequestRepository<'_> {
        deploy_requests::DeployRequestRepository::new(&self.db)
    }
}
