use ::entity::deploy_requests::{self, Entity as DeployRequests};
use sea_orm::*;

pub struct DeployRequestRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> DeployRequestRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    /// Upsert a branch's desired deploy at a commit
    pub async fn upsert(
        &self,
        project_id: &str,
        branch: &str,
        git_ref: &str,
        commit_sha: &str,
    ) -> Result<deploy_requests::Model, DbErr> {
        if let Some(existing) = self.find_by_project_branch(project_id, branch).await? {
            // A redelivery of the same already-deployed commit stays deployed
            let already_deployed =
                existing.commit_sha == commit_sha && existing.status == "deployed";
            let mut model: deploy_requests::ActiveModel = existing.into();
            model.git_ref = Set(git_ref.to_string());
            model.commit_sha = Set(commit_sha.to_string());
            if !already_deployed {
                model.status = Set("pending".to_string());
            }
            model.updated_at = Set(chrono::Utc::now());
            model.update(self.db).await
        } else {
            let model = deploy_requests::ActiveModel {
                id: Set(uuid::Uuid::now_v7().to_string()),
                project_id: Set(project_id.to_string()),
                branch: Set(branch.to_string()),
                git_ref: Set(git_ref.to_string()),
                commit_sha: Set(commit_sha.to_string()),
                status: Set("pending".to_string()),
                created_at: Set(chrono::Utc::now()),
                updated_at: Set(chrono::Utc::now()),
            };
            model.insert(self.db).await
        }
    }

    pub async fn find_by_project_branch(
        &self,
        project_id: &str,
        branch: &str,
    ) -> Result<Option<deploy_requests::Model>, DbErr> {
        DeployRequests::find()
            .filter(deploy_requests::Column::ProjectId.eq(project_id))
            .filter(deploy_requests::Column::Branch.eq(branch))
            .one(self.db)
            .await
    }

    pub async fn find_by_status(&self, status: &str) -> Result<Vec<deploy_requests::Model>, DbErr> {
        DeployRequests::find()
            .filter(deploy_requests::Column::Status.eq(status))
            .order_by_asc(deploy_requests::Column::CreatedAt)
            .all(self.db)
            .await
    }

    pub async fn set_status(&self, id: &str, status: &str) -> Result<(), DbErr> {
        let mut model: deploy_requests::ActiveModel = DeployRequests::find_by_id(id)
            .one(self.db)
            .await?
            .ok_or(DbErr::RecordNotFound(id.to_string()))?
            .into();
        model.status = Set(status.to_string());
        model.updated_at = Set(chrono::Utc::now());
        model.update(self.db).await?;
        Ok(())
    }

    pub async fn delete_by_project_branch(
        &self,
        project_id: &str,
        branch: &str,
    ) -> Result<u64, DbErr> {
        let res = DeployRequests::delete_many()
            .filter(deploy_requests::Column::ProjectId.eq(project_id))
            .filter(deploy_requests::Column::Branch.eq(branch))
            .exec(self.db)
            .await?;
        Ok(res.rows_affected)
    }

    /// Commits currently targeted by a deploy request in this project
    pub async fn active_commits(&self, project_id: &str) -> Result<Vec<String>, DbErr> {
        Ok(DeployRequests::find()
            .filter(deploy_requests::Column::ProjectId.eq(project_id))
            .all(self.db)
            .await?
            .into_iter()
            .map(|r| r.commit_sha)
            .collect())
    }
}
