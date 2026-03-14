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

impl StoreError {
    pub fn is_unique_violation(&self) -> bool {
        match self {
            StoreError::Database(sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(e))) => e
                .as_database_error()
                .is_some_and(|db_err| db_err.code().as_deref() == Some("23505")),
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;
