use crate::{Result, Store};
use ::entity::{deployments, sea_orm_active_enums::DeploymentStatus};

/// Summary of reconciliation operations performed during startup
#[derive(Debug, Default, Clone)]
pub struct ReconciliationSummary {
    pub orphaned_units: usize,
    pub restarted: usize,
    pub marked_failed: usize,
    pub broken_symlinks: usize,
    pub released_ports: usize,
    pub nginx_configs_regenerated: usize,
    pub nginx_configs_removed: usize,
    pub secrets_refreshed: usize,
    pub healthy: usize,
}

impl Store {
    /// Find all active service deployments (not static sites).
    /// Used during reconciliation to verify systemd units.
    pub async fn find_active_service_deployments(&self) -> Result<Vec<deployments::Model>> {
        use ::entity::prelude::*;
        use ::entity::sea_orm_active_enums::ServiceType;
        use sea_orm::*;

        Ok(Deployments::find()
            .filter(deployments::Column::Status.eq(DeploymentStatus::Active))
            .inner_join(Services)
            .filter(::entity::services::Column::Type.eq(ServiceType::Service))
            .all(self.db())
            .await?)
    }

    /// Find all active static site deployments.
    /// Used during reconciliation to verify symlinks.
    pub async fn find_active_static_deployments(&self) -> Result<Vec<deployments::Model>> {
        use ::entity::prelude::*;
        use ::entity::sea_orm_active_enums::ServiceType;
        use sea_orm::*;

        Ok(Deployments::find()
            .filter(deployments::Column::Status.eq(DeploymentStatus::Active))
            .inner_join(Services)
            .filter(::entity::services::Column::Type.eq(ServiceType::Static))
            .all(self.db())
            .await?)
    }

    /// Find all port allocations with their associated deployment info.
    /// Used during reconciliation to audit port usage.
    pub async fn find_all_port_allocations_with_deployment(
        &self,
    ) -> Result<
        Vec<(
            ::entity::port_allocations::Model,
            Option<deployments::Model>,
        )>,
    > {
        use ::entity::prelude::*;
        use sea_orm::*;

        Ok(PortAllocations::find()
            .find_also_related(Deployments)
            .all(self.db())
            .await?)
    }

    /// Mark a deployment as failed.
    /// Used during reconciliation when deployment state is invalid.
    pub async fn mark_deployment_failed(&self, deployment_id: i32) -> Result<()> {
        use sea_orm::*;

        let deployment = self
            .deployments()
            .find_by_id(deployment_id)
            .await?
            .ok_or(crate::StoreError::DeploymentNotFound(deployment_id))?;

        let mut active: ::entity::deployments::ActiveModel = deployment.into();
        active.status = Set(DeploymentStatus::Failed);

        self.deployments().update(active).await?;
        Ok(())
    }
}
