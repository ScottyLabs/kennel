use ::entity::builds::{self, Entity as Builds};
use sea_orm::prelude::Expr;
use sea_orm::*;

pub struct BuildRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> BuildRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn create(&self, model: builds::ActiveModel) -> Result<builds::Model, DbErr> {
        model.insert(self.db).await
    }

    pub async fn find_by_status(&self, status: &str) -> Result<Vec<builds::Model>, DbErr> {
        Builds::find()
            .filter(builds::Column::Status.eq(status))
            .order_by_asc(builds::Column::CreatedAt)
            .all(self.db)
            .await
    }

    pub async fn count_by_status(&self) -> Result<Vec<(String, i64)>, DbErr> {
        Builds::find()
            .select_only()
            .column(builds::Column::Status)
            .column_as(builds::Column::Id.count(), "count")
            .group_by(builds::Column::Status)
            .into_tuple()
            .all(self.db)
            .await
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<builds::Model>, DbErr> {
        Builds::find_by_id(id).one(self.db).await
    }

    pub async fn set_log(&self, id: &str, log: &str) -> Result<(), DbErr> {
        let mut model: builds::ActiveModel = Builds::find_by_id(id)
            .one(self.db)
            .await?
            .ok_or(DbErr::RecordNotFound(id.to_string()))?
            .into();
        model.log = Set(Some(log.to_string()));
        model.update(self.db).await?;
        Ok(())
    }

    pub async fn set_status(&self, id: &str, status: &str) -> Result<(), DbErr> {
        let mut model: builds::ActiveModel = Builds::find_by_id(id)
            .one(self.db)
            .await?
            .ok_or(DbErr::RecordNotFound(id.to_string()))?
            .into();

        model.status = Set(status.to_string());
        model.update(self.db).await?;
        Ok(())
    }

    pub async fn reset_stuck(&self) -> Result<u64, DbErr> {
        let result = Builds::update_many()
            .col_expr(builds::Column::Status, Expr::value("queued"))
            .filter(builds::Column::Status.eq("building"))
            .exec(self.db)
            .await?;
        Ok(result.rows_affected)
    }

    pub async fn set_result(
        &self,
        id: &str,
        store_paths: &str,
        kennel_config: &str,
        config_store_path: Option<&str>,
    ) -> Result<(), DbErr> {
        let mut model: builds::ActiveModel = Builds::find_by_id(id)
            .one(self.db)
            .await?
            .ok_or(DbErr::RecordNotFound(id.to_string()))?
            .into();

        model.status = Set("built".to_string());
        model.store_paths = Set(Some(serde_json::from_str(store_paths).unwrap_or_default()));
        model.kennel_config = Set(Some(
            serde_json::from_str(kennel_config).unwrap_or_default(),
        ));
        model.config_store_path = Set(config_store_path.map(String::from));
        model.update(self.db).await?;
        Ok(())
    }

    pub async fn find_by_project_commit(
        &self,
        project_id: &str,
        commit_sha: &str,
    ) -> Result<Option<builds::Model>, DbErr> {
        Builds::find()
            .filter(builds::Column::ProjectId.eq(project_id))
            .filter(builds::Column::CommitSha.eq(commit_sha))
            .one(self.db)
            .await
    }

    pub async fn requeue(&self, id: &str) -> Result<(), DbErr> {
        let mut model: builds::ActiveModel = Builds::find_by_id(id)
            .one(self.db)
            .await?
            .ok_or(DbErr::RecordNotFound(id.to_string()))?
            .into();

        model.status = Set("queued".to_string());
        model.store_paths = Set(None);
        model.kennel_config = Set(None);
        model.config_store_path = Set(None);
        model.started_at = Set(None);
        model.finished_at = Set(None);
        model.update(self.db).await?;
        Ok(())
    }

    /// Cancel queued or built builds for commits no deploy request targets
    /// anymore and returns the built ones so their gc roots can be reaped
    pub async fn cancel_unreferenced(
        &self,
        project_id: &str,
        referenced: &[String],
    ) -> Result<Vec<String>, DbErr> {
        let stale_built: Vec<String> = Builds::find()
            .filter(builds::Column::ProjectId.eq(project_id))
            .filter(builds::Column::Status.eq("built"))
            .filter(builds::Column::CommitSha.is_not_in(referenced.iter().map(String::as_str)))
            .all(self.db)
            .await?
            .into_iter()
            .map(|b| b.id)
            .collect();

        Builds::update_many()
            .col_expr(builds::Column::Status, Expr::value("cancelled"))
            .filter(builds::Column::ProjectId.eq(project_id))
            .filter(builds::Column::Status.is_in(["queued", "built"]))
            .filter(builds::Column::CommitSha.is_not_in(referenced.iter().map(String::as_str)))
            .exec(self.db)
            .await?;

        Ok(stale_built)
    }
}
