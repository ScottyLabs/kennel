use ::entity::{prelude::*, services};
use sea_orm::*;

pub struct ServiceRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> ServiceRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_id(&self, id: i32) -> Result<Option<services::Model>, DbErr> {
        Services::find_by_id(id).one(self.db).await
    }

    pub async fn find_by_project_and_name(
        &self,
        project_name: &str,
        service_name: &str,
    ) -> Result<Option<services::Model>, DbErr> {
        Services::find()
            .filter(services::Column::ProjectName.eq(project_name))
            .filter(services::Column::Name.eq(service_name))
            .one(self.db)
            .await
    }

    pub async fn list_by_project(&self, project_name: &str) -> Result<Vec<services::Model>, DbErr> {
        Services::find()
            .filter(services::Column::ProjectName.eq(project_name))
            .all(self.db)
            .await
    }

    pub async fn create(&self, service: services::ActiveModel) -> Result<services::Model, DbErr> {
        service.insert(self.db).await
    }

    pub async fn update(&self, service: services::ActiveModel) -> Result<services::Model, DbErr> {
        service.update(self.db).await
    }

    pub async fn delete(&self, id: i32) -> Result<DeleteResult, DbErr> {
        Services::delete_by_id(id).exec(self.db).await
    }
}
