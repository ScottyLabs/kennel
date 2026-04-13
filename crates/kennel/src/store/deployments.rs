use ::entity::deployments::{self, Entity as Deployments};
use sea_orm::*;

pub struct DeploymentRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> DeploymentRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn upsert(
        &self,
        model: deployments::ActiveModel,
    ) -> Result<deployments::Model, DbErr> {
        model.insert(self.db).await
    }

    pub async fn list_all(&self) -> Result<Vec<deployments::Model>, DbErr> {
        Deployments::find().all(self.db).await
    }

    pub async fn find_by_domain(&self, domain: &str) -> Result<Option<deployments::Model>, DbErr> {
        Deployments::find()
            .filter(
                deployments::Column::Domain
                    .eq(domain)
                    .or(deployments::Column::CustomDomain.eq(domain)),
            )
            .one(self.db)
            .await
    }

    pub async fn find_by_project_service_branch(
        &self,
        project_id: &str,
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

    pub async fn delete(&self, id: &str) -> Result<DeleteResult, DbErr> {
        Deployments::delete_by_id(id).exec(self.db).await
    }

    pub async fn delete_by_project_branch(
        &self,
        project_id: &str,
        branch: &str,
    ) -> Result<Vec<deployments::Model>, DbErr> {
        let deployments = Deployments::find()
            .filter(deployments::Column::ProjectId.eq(project_id))
            .filter(deployments::Column::Branch.eq(branch))
            .all(self.db)
            .await?;

        for d in &deployments {
            Deployments::delete_by_id(&d.id).exec(self.db).await?;
        }

        Ok(deployments)
    }
}
