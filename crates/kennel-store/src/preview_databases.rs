use ::entity::{prelude::*, preview_databases};
use sea_orm::*;
use std::collections::HashSet;

use crate::{Result, StoreError};

const VALKEY_DB_MIN: i32 = 0;
const VALKEY_DB_MAX: i32 = 15;

pub struct PreviewDatabaseRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> PreviewDatabaseRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_id(&self, id: i32) -> Result<Option<preview_databases::Model>> {
        Ok(PreviewDatabases::find_by_id(id).one(self.db).await?)
    }

    pub async fn find_by_project_and_branch(
        &self,
        project_name: &str,
        branch: &str,
    ) -> Result<Option<preview_databases::Model>> {
        Ok(PreviewDatabases::find()
            .filter(preview_databases::Column::ProjectName.eq(project_name))
            .filter(preview_databases::Column::Branch.eq(branch))
            .one(self.db)
            .await?)
    }

    pub async fn find_by_database_name(
        &self,
        database_name: &str,
    ) -> Result<Option<preview_databases::Model>> {
        Ok(PreviewDatabases::find()
            .filter(preview_databases::Column::DatabaseName.eq(database_name))
            .one(self.db)
            .await?)
    }

    pub async fn list_by_project(
        &self,
        project_name: &str,
    ) -> Result<Vec<preview_databases::Model>> {
        Ok(PreviewDatabases::find()
            .filter(preview_databases::Column::ProjectName.eq(project_name))
            .all(self.db)
            .await?)
    }

    pub async fn create_preview_database(
        &self,
        project_name: &str,
        branch: &str,
        database_name: &str,
    ) -> Result<preview_databases::Model> {
        let valkey_db = self.allocate_valkey_db().await?;

        let preview_db = preview_databases::ActiveModel {
            project_name: Set(project_name.to_string()),
            branch: Set(branch.to_string()),
            database_name: Set(database_name.to_string()),
            valkey_db: Set(Some(valkey_db)),
            ..Default::default()
        };

        Ok(preview_db.insert(self.db).await?)
    }

    pub async fn delete(&self, id: i32) -> Result<()> {
        PreviewDatabases::delete_by_id(id).exec(self.db).await?;
        Ok(())
    }

    pub async fn delete_by_project_and_branch(
        &self,
        project_name: &str,
        branch: &str,
    ) -> Result<()> {
        PreviewDatabases::delete_many()
            .filter(preview_databases::Column::ProjectName.eq(project_name))
            .filter(preview_databases::Column::Branch.eq(branch))
            .exec(self.db)
            .await?;
        Ok(())
    }

    async fn allocate_valkey_db(&self) -> Result<i32> {
        let allocated_dbs: Vec<Option<i32>> = PreviewDatabases::find()
            .select_only()
            .column(preview_databases::Column::ValkeyDb)
            .into_tuple()
            .all(self.db)
            .await?;

        let allocated_set: HashSet<i32> = allocated_dbs.into_iter().flatten().collect();

        (VALKEY_DB_MIN..=VALKEY_DB_MAX)
            .find(|db| !allocated_set.contains(db))
            .ok_or(StoreError::ValkeyDbPoolExhausted)
    }

    pub async fn is_valkey_db_available(&self, db: i32) -> Result<bool> {
        if !(VALKEY_DB_MIN..=VALKEY_DB_MAX).contains(&db) {
            return Ok(false);
        }

        let allocated_dbs: Vec<Option<i32>> = PreviewDatabases::find()
            .select_only()
            .column(preview_databases::Column::ValkeyDb)
            .into_tuple()
            .all(self.db)
            .await?;

        Ok(!allocated_dbs.into_iter().flatten().any(|x| x == db))
    }

    pub fn is_valkey_db_in_range(db: i32) -> bool {
        (VALKEY_DB_MIN..=VALKEY_DB_MAX).contains(&db)
    }

    pub async fn allocate(&self, project_name: &str, branch: &str) -> Result<i32> {
        // Check if already allocated
        if let Some(existing) = self
            .find_by_project_and_branch(project_name, branch)
            .await?
        {
            return Ok(existing.valkey_db.unwrap_or(0));
        }

        let valkey_db = self.allocate_valkey_db().await?;
        let database_name = format!(
            "{}_{}",
            project_name.replace('-', "_"),
            branch.replace('-', "_")
        );

        let preview_db = preview_databases::ActiveModel {
            project_name: Set(project_name.to_string()),
            branch: Set(branch.to_string()),
            database_name: Set(database_name),
            valkey_db: Set(Some(valkey_db)),
            ..Default::default()
        };

        preview_db.insert(self.db).await?;
        Ok(valkey_db)
    }
}
