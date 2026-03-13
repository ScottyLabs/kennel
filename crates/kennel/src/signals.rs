use std::sync::Arc;
use tokio::sync::Notify;

/// Subsecond dispatch signals for the DB-as-queue architecture. Each signal
/// wakes the corresponding worker to check the database for new work items.
/// Losing a signal is harmless -- the DB is the source of truth.
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
