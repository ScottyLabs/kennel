use crate::Result;
use entity::sea_orm_active_enums::{BuildResultStatus, BuildStatus};
use entity::{build_results, builds};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};

pub struct BuildResultRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> BuildResultRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_build_id(&self, build_id: i32) -> Result<Vec<build_results::Model>> {
        Ok(build_results::Entity::find()
            .filter(build_results::Column::BuildId.eq(build_id))
            .all(self.db)
            .await?)
    }

    pub async fn find_successful_by_build_id(
        &self,
        build_id: i32,
    ) -> Result<Vec<build_results::Model>> {
        Ok(build_results::Entity::find()
            .filter(build_results::Column::BuildId.eq(build_id))
            .filter(build_results::Column::Status.eq(BuildResultStatus::Success))
            .all(self.db)
            .await?)
    }

    pub async fn create(
        &self,
        build_result: build_results::ActiveModel,
    ) -> Result<build_results::Model> {
        Ok(build_result.insert(self.db).await?)
    }

    pub async fn find_recent_successful(
        &self,
        project_name: &str,
        git_ref: &str,
        service_name: &str,
        limit: u64,
    ) -> Result<Vec<build_results::Model>> {
        Ok(build_results::Entity::find()
            .inner_join(builds::Entity)
            .filter(builds::Column::ProjectName.eq(project_name))
            .filter(builds::Column::GitRef.eq(git_ref))
            .filter(builds::Column::Status.eq(BuildStatus::Success))
            .filter(build_results::Column::ServiceName.eq(service_name))
            .filter(build_results::Column::Status.eq(BuildResultStatus::Success))
            .order_by_desc(builds::Column::CreatedAt)
            .limit(limit)
            .all(self.db)
            .await?)
    }
}
