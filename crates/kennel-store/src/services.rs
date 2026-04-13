use entity::services::{self, Entity as Services};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter};
use uuid::Uuid;

pub struct ServiceRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> ServiceRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn upsert(&self, model: services::ActiveModel) -> Result<services::Model, DbErr> {
        model.insert(self.db).await
    }

    pub async fn find_by_project_and_name(
        &self,
        project_id: Uuid,
        name: &str,
    ) -> Result<Option<services::Model>, DbErr> {
        Services::find()
            .filter(services::Column::ProjectId.eq(project_id))
            .filter(services::Column::Name.eq(name))
            .one(self.db)
            .await
    }
}
