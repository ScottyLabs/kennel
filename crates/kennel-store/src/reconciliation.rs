use crate::{Result, Store};
use ::entity::{deployments, sea_orm_active_enums::DeploymentStatus};

/// Summary of reconciliation operations performed during startup
#[derive(Debug, Default, Clone)]
pub struct ReconciliationSummary {
    pub restarted: usize,
    pub broken_symlinks: usize,
}

impl Store {
    /// Find all deployed service deployments (not static sites).
    pub async fn find_deployed_service_deployments(&self) -> Result<Vec<deployments::Model>> {
        use ::entity::prelude::*;
        use ::entity::sea_orm_active_enums::ServiceType;
        use sea_orm::*;

        Ok(Deployments::find()
            .filter(deployments::Column::Status.eq(DeploymentStatus::Deployed))
            .inner_join(Services)
            .filter(::entity::services::Column::Type.eq(ServiceType::Service))
            .all(self.db())
            .await?)
    }

    /// Find all deployed static site deployments.
    pub async fn find_deployed_static_deployments(&self) -> Result<Vec<deployments::Model>> {
        use ::entity::prelude::*;
        use ::entity::sea_orm_active_enums::ServiceType;
        use sea_orm::*;

        Ok(Deployments::find()
            .filter(deployments::Column::Status.eq(DeploymentStatus::Deployed))
            .inner_join(Services)
            .filter(::entity::services::Column::Type.eq(ServiceType::Static))
            .all(self.db())
            .await?)
    }
}
