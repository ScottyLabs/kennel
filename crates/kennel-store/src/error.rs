use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("port pool exhausted (18000-19999)")]
    PortPoolExhausted,

    #[error("port {0} already allocated")]
    PortAlreadyAllocated(i32),

    #[error("port allocation conflict after retries")]
    PortAllocationConflict,

    #[error("valkey database pool exhausted (0-15)")]
    ValkeyDbPoolExhausted,

    #[error("valkey database {0} already allocated")]
    ValkeyDbAlreadyAllocated(i32),

    #[error("preview database has no valkey db assigned")]
    ValkeyDbNotAssigned,

    #[error("deployment not found: {0}")]
    DeploymentNotFound(i32),

    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("service not found: {project}/{service}")]
    ServiceNotFound { project: String, service: String },
}

pub type Result<T> = std::result::Result<T, StoreError>;
