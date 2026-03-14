pub(crate) mod cgroup;
pub mod config;
pub mod devenv;
pub mod error;
pub mod event;
mod handle;
pub(crate) mod notify;
pub(crate) mod order;
pub(crate) mod probe;
pub(crate) mod socket;
pub mod state;
pub(crate) mod supervisor;

pub use config::{
    HttpProbe, HttpReadyConfig, ListenKind, ListenSpec, ProcessConfig, ReadyConfig, ResourceLimits,
    RestartConfig, RestartPolicy, WatchConfig, WatchdogConfig,
};
pub use error::{Result, SupervisorError};
pub use event::SupervisorEvent;
pub use handle::SupervisorHandle;
pub use state::ProcessState;
