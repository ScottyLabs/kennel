use ::entity::{deployments, prelude::*, sea_orm_active_enums::DeploymentStatus, services};
use sea_orm::{entity::*, query::*, sea_query::Expr, *};

pub struct DeploymentRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> DeploymentRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_id(&self, id: i32) -> Result<Option<deployments::Model>, DbErr> {
        Deployments::find_by_id(id).one(self.db).await
    }

    pub async fn find_by_project_service_branch(
        &self,
        project_name: &str,
        service_name: &str,
        branch: &str,
    ) -> Result<Option<deployments::Model>, DbErr> {
        Deployments::find()
            .filter(deployments::Column::ProjectName.eq(project_name))
            .filter(deployments::Column::ServiceName.eq(service_name))
            .filter(deployments::Column::Branch.eq(branch))
            .one(self.db)
            .await
    }

    pub async fn find_by_domain(&self, domain: &str) -> Result<Option<deployments::Model>, DbErr> {
        Deployments::find()
            .filter(deployments::Column::Domain.eq(domain))
            .one(self.db)
            .await
    }

    pub async fn list_by_project(
        &self,
        project_name: &str,
    ) -> Result<Vec<deployments::Model>, DbErr> {
        Deployments::find()
            .filter(deployments::Column::ProjectName.eq(project_name))
            .all(self.db)
            .await
    }

    pub async fn list_by_status(
        &self,
        status: DeploymentStatus,
    ) -> Result<Vec<deployments::Model>, DbErr> {
        Deployments::find()
            .filter(deployments::Column::Status.eq(status))
            .all(self.db)
            .await
    }

    pub async fn list_active(&self) -> Result<Vec<deployments::Model>, DbErr> {
        self.list_by_status(DeploymentStatus::Active).await
    }

    pub async fn list_active_with_services(
        &self,
    ) -> Result<Vec<(deployments::Model, Option<services::Model>)>, DbErr> {
        Deployments::find()
            .filter(deployments::Column::Status.eq(DeploymentStatus::Active))
            .find_also_related(services::Entity)
            .all(self.db)
            .await
    }

    pub async fn find_active_by_ref(
        &self,
        project_name: &str,
        git_ref: &str,
        service_name: &str,
    ) -> Result<Option<deployments::Model>, DbErr> {
        Deployments::find()
            .filter(deployments::Column::ProjectName.eq(project_name))
            .filter(deployments::Column::GitRef.eq(git_ref))
            .filter(deployments::Column::ServiceName.eq(service_name))
            .filter(deployments::Column::Status.eq(DeploymentStatus::Active))
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

    pub async fn delete(&self, id: i32) -> Result<DeleteResult, DbErr> {
        Deployments::delete_by_id(id).exec(self.db).await
    }

    pub async fn find_expired(
        &self,
        days: i64,
        exclude_environments: &[&str],
    ) -> crate::Result<Vec<deployments::Model>> {
        use chrono::{Duration, Utc};

        let cutoff = Utc::now().naive_utc() - Duration::days(days);

        let mut query = Deployments::find()
            .filter(deployments::Column::LastActivity.lt(cutoff))
            .filter(deployments::Column::Status.eq(DeploymentStatus::Active));

        for env in exclude_environments {
            query = query.filter(deployments::Column::Environment.ne(*env));
        }

        Ok(query.all(self.db).await?)
    }

    pub async fn mark_for_teardown(&self, project_name: &str, git_ref: &str) -> crate::Result<u64> {
        use chrono::Utc;

        Ok(Deployments::update_many()
            .filter(deployments::Column::ProjectName.eq(project_name))
            .filter(deployments::Column::Branch.eq(git_ref))
            .filter(deployments::Column::Status.eq(DeploymentStatus::Active))
            .col_expr(
                deployments::Column::Status,
                Expr::value(DeploymentStatus::TearingDown),
            )
            .col_expr(
                deployments::Column::UpdatedAt,
                Expr::value(Utc::now().naive_utc()),
            )
            .exec(self.db)
            .await
            .map(|result| result.rows_affected)?)
    }
}
