use ::entity::{
    deployments,
    prelude::*,
    sea_orm_active_enums::{DeploymentStatus, DnsStatus},
    services,
};
use sea_orm::sea_query::{LockBehavior, LockType};
use sea_orm::{entity::*, query::*, sea_query::Expr, *};
use uuid::Uuid;

pub struct DeploymentRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> DeploymentRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<deployments::Model>, DbErr> {
        Deployments::find_by_id(id).one(self.db).await
    }

    pub async fn find_deployed_by_process_name(
        &self,
        process_name: &str,
    ) -> crate::Result<Option<(deployments::Model, Option<services::Model>)>> {
        Ok(Deployments::find()
            .filter(deployments::Column::ProcessName.eq(process_name))
            .filter(
                deployments::Column::Status
                    .is_in([DeploymentStatus::Deployed, DeploymentStatus::Deploying]),
            )
            .find_also_related(services::Entity)
            .one(self.db)
            .await?)
    }

    pub async fn find_by_project_service_branch(
        &self,
        project_id: Uuid,
        service_name: &str,
        branch: &str,
    ) -> Result<Option<deployments::Model>, DbErr> {
        Deployments::find()
            .filter(deployments::Column::ProjectId.eq(project_id))
            .filter(deployments::Column::ServiceName.eq(service_name))
            .filter(deployments::Column::Branch.eq(branch))
            .one(self.db)
            .await
    }

    pub async fn list_by_project(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<deployments::Model>, DbErr> {
        Deployments::find()
            .filter(deployments::Column::ProjectId.eq(project_id))
            .all(self.db)
            .await
    }

    pub async fn list_deployed_with_services(
        &self,
    ) -> Result<Vec<(deployments::Model, Option<services::Model>)>, DbErr> {
        Deployments::find()
            .filter(deployments::Column::Status.eq(DeploymentStatus::Deployed))
            .find_also_related(services::Entity)
            .all(self.db)
            .await
    }

    pub async fn find_deployed_by_ref(
        &self,
        project_id: Uuid,
        git_ref: &str,
        service_name: &str,
    ) -> Result<Option<deployments::Model>, DbErr> {
        Deployments::find()
            .filter(deployments::Column::ProjectId.eq(project_id))
            .filter(deployments::Column::GitRef.eq(git_ref))
            .filter(deployments::Column::ServiceName.eq(service_name))
            .filter(deployments::Column::Status.eq(DeploymentStatus::Deployed))
            .one(self.db)
            .await
    }

    pub async fn create(
        &self,
        deployment: deployments::ActiveModel,
    ) -> Result<deployments::Model, DbErr> {
        deployment.insert(self.db).await
    }

    pub async fn update(
        &self,
        deployment: deployments::ActiveModel,
    ) -> Result<deployments::Model, DbErr> {
        deployment.update(self.db).await
    }

    pub async fn delete(&self, id: Uuid) -> Result<DeleteResult, DbErr> {
        Deployments::delete_by_id(id).exec(self.db).await
    }

    pub async fn find_expired(
        &self,
        days: i64,
        exclude_environments: &[::entity::sea_orm_active_enums::Environment],
    ) -> crate::Result<Vec<deployments::Model>> {
        use chrono::{Duration, Utc};

        let cutoff = Utc::now().naive_utc() - Duration::days(days);

        let mut query = Deployments::find()
            .filter(deployments::Column::LastActivity.lt(cutoff))
            .filter(deployments::Column::Status.eq(DeploymentStatus::Deployed));

        for env in exclude_environments {
            query = query.filter(
                deployments::Column::Environment
                    .ne::<::entity::sea_orm_active_enums::Environment>(env.clone()),
            );
        }

        Ok(query.all(self.db).await?)
    }

    pub async fn mark_ids_torn_down(&self, ids: &[Uuid]) -> crate::Result<()> {
        use chrono::Utc;

        if !ids.is_empty() {
            Deployments::update_many()
                .filter(deployments::Column::Id.is_in(ids.iter().copied()))
                .col_expr(
                    deployments::Column::Status,
                    Expr::value(DeploymentStatus::TornDown),
                )
                .col_expr(
                    deployments::Column::UpdatedAt,
                    Expr::value(Utc::now().naive_utc()),
                )
                .exec(self.db)
                .await?;
        }

        Ok(())
    }

    pub async fn mark_for_teardown(
        &self,
        project_id: Uuid,
        git_ref: &str,
    ) -> crate::Result<Vec<Uuid>> {
        let ids: Vec<Uuid> = Deployments::find()
            .filter(deployments::Column::ProjectId.eq(project_id))
            .filter(deployments::Column::Branch.eq(git_ref))
            .filter(deployments::Column::Status.eq(DeploymentStatus::Deployed))
            .all(self.db)
            .await?
            .iter()
            .map(|d| d.id)
            .collect();

        self.mark_tearing_down(&ids).await?;

        Ok(ids)
    }

    pub async fn find_by_dns_status(
        &self,
        dns_status: DnsStatus,
    ) -> crate::Result<Vec<deployments::Model>> {
        Ok(Deployments::find()
            .filter(deployments::Column::DnsStatus.eq(dns_status))
            .all(self.db)
            .await?)
    }

    pub async fn update_dns_status(&self, id: Uuid, dns_status: DnsStatus) -> crate::Result<()> {
        use chrono::Utc;

        Deployments::update_many()
            .filter(deployments::Column::Id.eq(id))
            .col_expr(deployments::Column::DnsStatus, Expr::value(dns_status))
            .col_expr(
                deployments::Column::UpdatedAt,
                Expr::value(Utc::now().naive_utc()),
            )
            .exec(self.db)
            .await?;

        Ok(())
    }

    /// Atomically claim the oldest `TearingDown` deployment using
    /// `FOR UPDATE SKIP LOCKED` to prevent double-claiming across concurrent
    /// teardown workers. Returns the deployment without transitioning its
    /// status, since the teardown worker will handle final cleanup.
    pub async fn claim_tearing_down(&self) -> crate::Result<Option<deployments::Model>> {
        let txn = self.db.begin().await?;

        let deployment = Deployments::find()
            .filter(deployments::Column::Status.eq(DeploymentStatus::TearingDown))
            .order_by_asc(deployments::Column::UpdatedAt)
            .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
            .one(&txn)
            .await?;

        txn.commit().await?;

        Ok(deployment)
    }

    /// Mark the given deployment IDs as `TearingDown` so they are picked up
    /// by the teardown worker.
    pub async fn mark_tearing_down(&self, ids: &[Uuid]) -> crate::Result<()> {
        use chrono::Utc;

        if !ids.is_empty() {
            Deployments::update_many()
                .filter(deployments::Column::Id.is_in(ids.iter().copied()))
                .col_expr(
                    deployments::Column::Status,
                    Expr::value(DeploymentStatus::TearingDown),
                )
                .col_expr(
                    deployments::Column::UpdatedAt,
                    Expr::value(Utc::now().naive_utc()),
                )
                .exec(self.db)
                .await?;
        }

        Ok(())
    }

    /// Find all `Deployed` deployments that have a non-null `process_config`,
    /// indicating they are running services. Used for reconciliation on startup
    /// to verify running processes match the expected state.
    pub async fn find_deployed_service_deployments(
        &self,
    ) -> crate::Result<Vec<deployments::Model>> {
        Ok(Deployments::find()
            .filter(deployments::Column::Status.eq(DeploymentStatus::Deployed))
            .filter(deployments::Column::ProcessConfig.is_not_null())
            .all(self.db)
            .await?)
    }
}
