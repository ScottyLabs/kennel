use ::entity::{build_results, builds, prelude::*, sea_orm_active_enums::BuildStatus};
use sea_orm::*;

pub struct BuildRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> BuildRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_id(&self, id: i32) -> Result<Option<builds::Model>, DbErr> {
        Builds::find_by_id(id).one(self.db).await
    }

    pub async fn find_with_results(
        &self,
        id: i32,
    ) -> Result<Option<(builds::Model, Vec<build_results::Model>)>, DbErr> {
        Builds::find_by_id(id)
            .find_with_related(BuildResults)
            .all(self.db)
            .await
            .map(|results| results.into_iter().next())
    }

    pub async fn list_by_project(&self, project_name: &str) -> Result<Vec<builds::Model>, DbErr> {
        Builds::find()
            .filter(builds::Column::ProjectName.eq(project_name))
            .order_by_desc(builds::Column::CreatedAt)
            .all(self.db)
            .await
    }

    pub async fn list_by_project_and_branch(
        &self,
        project_name: &str,
        branch: &str,
    ) -> Result<Vec<builds::Model>, DbErr> {
        Builds::find()
            .filter(builds::Column::ProjectName.eq(project_name))
            .filter(builds::Column::Branch.eq(branch))
            .order_by_desc(builds::Column::CreatedAt)
            .all(self.db)
            .await
    }

    pub async fn list_by_status(&self, status: BuildStatus) -> Result<Vec<builds::Model>, DbErr> {
        Builds::find()
            .filter(builds::Column::Status.eq(status))
            .order_by_asc(builds::Column::CreatedAt)
            .all(self.db)
            .await
    }

    pub async fn list_queued(&self) -> Result<Vec<builds::Model>, DbErr> {
        self.list_by_status(BuildStatus::Queued).await
    }

    pub async fn create(&self, build: builds::ActiveModel) -> Result<builds::Model, DbErr> {
        build.insert(self.db).await
    }

    pub async fn update(&self, build: builds::ActiveModel) -> Result<builds::Model, DbErr> {
        build.update(self.db).await
    }

    pub async fn delete(&self, id: i32) -> Result<DeleteResult, DbErr> {
        Builds::delete_by_id(id).exec(self.db).await
    }

    pub async fn find_old_finished_builds(&self, days: i64) -> crate::Result<Vec<builds::Model>> {
        use chrono::{Duration, Utc};

        let cutoff = Utc::now().naive_utc() - Duration::days(days);

        Ok(Builds::find()
            .filter(builds::Column::FinishedAt.is_not_null())
            .filter(builds::Column::FinishedAt.lt(cutoff))
            .all(self.db)
            .await?)
    }
}
