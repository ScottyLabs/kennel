use crate::{Result, Store};

impl Store {
    /// Find deployments that have been inactive for the specified number of days
    /// and are not in protected environments (prod, staging by default).
    ///
    /// This returns the list of expired deployments but does not tear them down.
    /// The caller is responsible for actually tearing down the deployments.
    pub async fn find_expired_deployments(
        &self,
        inactive_days: i64,
        protected_environments: &[&str],
    ) -> Result<Vec<::entity::deployments::Model>> {
        self.deployments()
            .find_expired(inactive_days, protected_environments)
            .await
    }

    /// Find builds that finished more than the specified number of days ago.
    ///
    /// This returns the list of old builds but does not delete their logs.
    /// The caller is responsible for filesystem cleanup.
    pub async fn find_old_builds(
        &self,
        retention_days: i64,
    ) -> Result<Vec<::entity::builds::Model>> {
        self.builds().find_old_finished_builds(retention_days).await
    }
}
