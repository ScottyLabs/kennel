use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("deployment not found: {0}")]
    DeploymentNotFound(i32),

    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("service not found: {project}/{service}")]
    ServiceNotFound { project: String, service: String },
}

pub type Result<T> = std::result::Result<T, StoreError>;
