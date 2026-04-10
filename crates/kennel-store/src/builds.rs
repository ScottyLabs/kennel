use ::entity::{builds, prelude::*, sea_orm_active_enums::BuildStatus};
use sea_orm::sea_query::{LockBehavior, LockType};
use sea_orm::*;
use uuid::Uuid;

pub struct BuildRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> BuildRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_id(&self, id: Uuid) -> crate::Result<Option<builds::Model>> {
        Ok(Builds::find_by_id(id).one(self.db).await?)
    }

    pub async fn list_by_project(&self, project_id: Uuid) -> Result<Vec<builds::Model>, DbErr> {
        Builds::find()
            .filter(builds::Column::ProjectId.eq(project_id))
            .order_by_desc(builds::Column::CreatedAt)
            .all(self.db)
            .await
    }

    pub async fn create(&self, build: builds::ActiveModel) -> Result<builds::Model, DbErr> {
        build.insert(self.db).await
    }

    pub async fn update(&self, build: builds::ActiveModel) -> Result<builds::Model, DbErr> {
        build.update(self.db).await
    }

    pub async fn delete(&self, id: Uuid) -> Result<DeleteResult, DbErr> {
        Builds::delete_by_id(id).exec(self.db).await
    }

    pub async fn find_old_finished_builds(&self, days: i64) -> crate::Result<Vec<builds::Model>> {
        use chrono::{Duration, Utc};

        let cutoff = Utc::now().naive_utc() - Duration::days(days);

        Ok(Builds::find()
            .filter(builds::Column::FinishedAt.is_not_null())
            .filter(builds::Column::FinishedAt.lt(cutoff))
            .all(self.db)
            .await?)
    }

    pub async fn create_build(
        &self,
        project_id: Uuid,
        project_name: String,
        git_ref: String,
        commit_sha: String,
    ) -> crate::Result<builds::Model> {
        use chrono::Utc;

        let now = Utc::now().naive_utc();

        let branch = if git_ref.starts_with("refs/heads/") {
            git_ref.strip_prefix("refs/heads/").unwrap().to_string()
        } else {
            git_ref.clone()
        };

        let build = builds::ActiveModel {
            project_id: Set(project_id),
            project_name: Set(project_name),
            branch: Set(branch),
            git_ref: Set(git_ref),
            commit_sha: Set(commit_sha),
            status: Set(BuildStatus::Queued),
            started_at: NotSet,
            finished_at: NotSet,
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };

        Ok(build.insert(self.db).await?)
    }

    /// Atomically claim the oldest queued build using `FOR UPDATE SKIP LOCKED`
    /// to prevent double-claiming across concurrent workers. Transitions the
    /// build from `Queued` to `Building`.
    pub async fn claim_queued_build(&self) -> crate::Result<Option<builds::Model>> {
        use chrono::Utc;

        let txn = self.db.begin().await?;

        let build = Builds::find()
            .filter(builds::Column::Status.eq(BuildStatus::Queued))
            .order_by_asc(builds::Column::CreatedAt)
            .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
            .one(&txn)
            .await?;

        let Some(build) = build else {
            txn.commit().await?;
            return Ok(None);
        };

        let now = Utc::now().naive_utc();
        let mut active: builds::ActiveModel = build.into();
        active.status = Set(BuildStatus::Building);
        active.started_at = Set(Some(now));
        active.updated_at = Set(now);
        let model = active.update(&txn).await?;

        txn.commit().await?;

        Ok(Some(model))
    }

    /// Atomically claim the oldest built build using `FOR UPDATE SKIP LOCKED`
    /// to prevent double-claiming across concurrent workers. Transitions the
    /// build from `Built` to `Deploying`.
    pub async fn claim_built_build(&self) -> crate::Result<Option<builds::Model>> {
        use chrono::Utc;

        let txn = self.db.begin().await?;

        let build = Builds::find()
            .filter(builds::Column::Status.eq(BuildStatus::Built))
            .order_by_asc(builds::Column::CreatedAt)
            .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
            .one(&txn)
            .await?;

        let Some(build) = build else {
            txn.commit().await?;
            return Ok(None);
        };

        let now = Utc::now().naive_utc();
        let mut active: builds::ActiveModel = build.into();
        active.status = Set(BuildStatus::Deploying);
        active.updated_at = Set(now);
        let model = active.update(&txn).await?;

        txn.commit().await?;

        Ok(Some(model))
    }

    /// Startup recovery: reset all `Building` builds back to `Queued` so they
    /// are re-processed. Returns the number of builds reset.
    pub async fn reset_building_to_queued(&self) -> crate::Result<u64> {
        use chrono::Utc;

        let stuck = Builds::find()
            .filter(builds::Column::Status.eq(BuildStatus::Building))
            .all(self.db)
            .await?;

        let count = stuck.len() as u64;
        let now = Utc::now().naive_utc();

        for build in stuck {
            let mut active: builds::ActiveModel = build.into();
            active.status = Set(BuildStatus::Queued);
            active.started_at = Set(None);
            active.updated_at = Set(now);
            active.update(self.db).await?;
        }

        Ok(count)
    }

    /// Startup recovery: reset all `Deploying` builds back to `Built` so they
    /// are re-processed. Returns the number of builds reset.
    pub async fn reset_deploying_to_built(&self) -> crate::Result<u64> {
        use chrono::Utc;

        let stuck = Builds::find()
            .filter(builds::Column::Status.eq(BuildStatus::Deploying))
            .all(self.db)
            .await?;

        let count = stuck.len() as u64;
        let now = Utc::now().naive_utc();

        for build in stuck {
            let mut active: builds::ActiveModel = build.into();
            active.status = Set(BuildStatus::Built);
            active.updated_at = Set(now);
            active.update(self.db).await?;
        }

        Ok(count)
    }

    /// Mark a build as `Success` (deployment completed).
    pub async fn mark_success(&self, id: Uuid) -> crate::Result<()> {
        use chrono::Utc;

        let build = Builds::find_by_id(id)
            .one(self.db)
            .await?
            .ok_or_else(|| crate::StoreError::NotFound(format!("build {id}")))?;

        let now = Utc::now().naive_utc();
        let mut active: builds::ActiveModel = build.into();
        active.status = Set(BuildStatus::Success);
        active.finished_at = Set(Some(now));
        active.updated_at = Set(now);
        active.update(self.db).await?;

        Ok(())
    }

    /// Mark a build as `Failed`.
    pub async fn mark_failed(&self, id: Uuid) -> crate::Result<()> {
        use chrono::Utc;

        let build = Builds::find_by_id(id)
            .one(self.db)
            .await?
            .ok_or_else(|| crate::StoreError::NotFound(format!("build {id}")))?;

        let now = Utc::now().naive_utc();
        let mut active: builds::ActiveModel = build.into();
        active.status = Set(BuildStatus::Failed);
        active.finished_at = Set(Some(now));
        active.updated_at = Set(now);
        active.update(self.db).await?;

        Ok(())
    }

    /// Cancel any queued or in-progress builds for the same (project, branch)
    /// except the given build ID. Returns the number of cancelled builds.
    pub async fn cancel_stale_builds(
        &self,
        project_id: Uuid,
        branch: &str,
        exclude_id: Uuid,
    ) -> crate::Result<u64> {
        use chrono::Utc;

        let stale = Builds::find()
            .filter(builds::Column::ProjectId.eq(project_id))
            .filter(builds::Column::Branch.eq(branch))
            .filter(builds::Column::Id.ne(exclude_id))
            .filter(builds::Column::Status.is_in([BuildStatus::Queued, BuildStatus::Building]))
            .all(self.db)
            .await?;

        let count = stale.len() as u64;
        let now = Utc::now().naive_utc();

        for build in stale {
            let mut active: builds::ActiveModel = build.into();
            active.status = Set(BuildStatus::Cancelled);
            active.finished_at = Set(Some(now));
            active.updated_at = Set(now);
            active.update(self.db).await?;
        }

        Ok(count)
    }
}
