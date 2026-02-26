use ::entity::{prelude::*, projects};
use sea_orm::*;

pub struct ProjectRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> ProjectRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_name(&self, name: &str) -> crate::Result<Option<projects::Model>> {
        Ok(Projects::find_by_id(name).one(self.db).await?)
    }

    pub async fn list_all(&self) -> crate::Result<Vec<projects::Model>> {
        Ok(Projects::find().all(self.db).await?)
    }

    pub async fn create(&self, project: projects::ActiveModel) -> crate::Result<projects::Model> {
        Ok(project.insert(self.db).await?)
    }

    pub async fn update(&self, project: projects::ActiveModel) -> crate::Result<projects::Model> {
        Ok(project.update(self.db).await?)
    }

    pub async fn delete(&self, name: &str) -> crate::Result<DeleteResult> {
        Ok(Projects::delete_by_id(name).exec(self.db).await?)
    }
}
