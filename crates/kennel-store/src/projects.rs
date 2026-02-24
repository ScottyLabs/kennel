use ::entity::{prelude::*, projects};
use sea_orm::*;

pub struct ProjectRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> ProjectRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_name(&self, name: &str) -> Result<Option<projects::Model>, DbErr> {
        Projects::find_by_id(name).one(self.db).await
    }

    pub async fn list_all(&self) -> Result<Vec<projects::Model>, DbErr> {
        Projects::find().all(self.db).await
    }

    pub async fn create(&self, project: projects::ActiveModel) -> Result<projects::Model, DbErr> {
        project.insert(self.db).await
    }

    pub async fn update(&self, project: projects::ActiveModel) -> Result<projects::Model, DbErr> {
        project.update(self.db).await
    }

    pub async fn delete(&self, name: &str) -> Result<DeleteResult, DbErr> {
        Projects::delete_by_id(name).exec(self.db).await
    }
}
