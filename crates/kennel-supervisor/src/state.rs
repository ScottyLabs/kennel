use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ProcessState {
    Starting,
    Ready,
    Running,
    Unhealthy,
    Restarting { attempt: u32 },
    Failed { error: String, restarts: u32 },
    Stopping,
    Stopped,
}
