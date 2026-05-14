use ::entity::projects::{self, Entity as Projects};
use sea_orm::*;

pub struct ProjectRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> ProjectRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<projects::Model>, DbErr> {
        Projects::find_by_id(id).one(self.db).await
    }

    pub async fn find_by_name(&self, name: &str) -> Result<Option<projects::Model>, DbErr> {
        Projects::find()
            .filter(projects::Column::Name.eq(name))
            .one(self.db)
            .await
    }

    pub async fn upsert(&self, model: projects::ActiveModel) -> Result<projects::Model, DbErr> {
        model.insert(self.db).await
    }

    pub async fn list(&self) -> Result<Vec<projects::Model>, DbErr> {
        Projects::find().all(self.db).await
    }

    pub async fn delete_by_name(&self, name: &str) -> Result<DeleteResult, DbErr> {
        Projects::delete_many()
            .filter(projects::Column::Name.eq(name))
            .exec(self.db)
            .await
    }

    pub async fn count(&self) -> Result<u64, DbErr> {
        Projects::find().count(self.db).await
    }
}
