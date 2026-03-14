use std::sync::Arc;
use tokio::sync::Notify;

/// Wakeup signals for worker tasks. Each signal notifies the corresponding
/// worker to check the database for new work items.
pub struct Signals {
    pub build: Arc<Notify>,
    pub deploy: Arc<Notify>,
    pub teardown: Arc<Notify>,
}

impl Signals {
    pub fn new() -> Self {
        Self {
            build: Arc::new(Notify::new()),
            deploy: Arc::new(Notify::new()),
            teardown: Arc::new(Notify::new()),
        }
    }
}
