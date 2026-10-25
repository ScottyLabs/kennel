use ::entity::artifacts::{self, Entity as Artifacts};
use sea_orm::*;

pub struct ArtifactRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> ArtifactRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    /// Records the store path nixbot built for a branch's commit, replacing an earlier one
    pub async fn upsert(
        &self,
        project_id: &str,
        branch: &str,
        commit_sha: &str,
        store_path: &str,
    ) -> Result<artifacts::Model, DbErr> {
        if let Some(existing) = self.find(project_id, branch, commit_sha).await? {
            if existing.store_path == store_path {
                return Ok(existing);
            }

            let mut model: artifacts::ActiveModel = existing.into();
            model.store_path = Set(store_path.to_string());
            model.created_at = Set(chrono::Utc::now());
            return model.update(self.db).await;
        }

        artifacts::ActiveModel {
            id: Set(uuid::Uuid::now_v7().to_string()),
            project_id: Set(project_id.to_string()),
            branch: Set(branch.to_string()),
            commit_sha: Set(commit_sha.to_string()),
            store_path: Set(store_path.to_string()),
            created_at: Set(chrono::Utc::now()),
        }
        .insert(self.db)
        .await
    }

    pub async fn find(
        &self,
        project_id: &str,
        branch: &str,
        commit_sha: &str,
    ) -> Result<Option<artifacts::Model>, DbErr> {
        Artifacts::find()
            .filter(artifacts::Column::ProjectId.eq(project_id))
            .filter(artifacts::Column::Branch.eq(branch))
            .filter(artifacts::Column::CommitSha.eq(commit_sha))
            .one(self.db)
            .await
    }

    /// Drops a branch's artifacts recorded before `deployed`, which a newer push superseded
    pub async fn prune_superseded(&self, deployed: &artifacts::Model) -> Result<u64, DbErr> {
        let res = Artifacts::delete_many()
            .filter(artifacts::Column::ProjectId.eq(&deployed.project_id))
            .filter(artifacts::Column::Branch.eq(&deployed.branch))
            .filter(artifacts::Column::CreatedAt.lt(deployed.created_at))
            .exec(self.db)
            .await?;
        Ok(res.rows_affected)
    }

    pub async fn delete_by_project_branch(
        &self,
        project_id: &str,
        branch: &str,
    ) -> Result<u64, DbErr> {
        let res = Artifacts::delete_many()
            .filter(artifacts::Column::ProjectId.eq(project_id))
            .filter(artifacts::Column::Branch.eq(branch))
            .exec(self.db)
            .await?;
        Ok(res.rows_affected)
    }
}
