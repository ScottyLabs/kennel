use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum SupervisorEvent {
    ProcessReady {
        name: String,
        port: Option<u16>,
        store_path: Option<String>,
    },
    ProcessUnhealthy {
        name: String,
    },
    ProcessHealthy {
        name: String,
        port: Option<u16>,
    },
    ProcessRestarting {
        name: String,
        attempt: u32,
    },
    ProcessStopped {
        name: String,
    },
    ProcessFailed {
        name: String,
        error: String,
    },
}
