use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};

use crate::config::ProcessConfig;
use crate::error::{Result, SupervisorError};
use crate::event::SupervisorEvent;
use crate::state::ProcessState;
use crate::supervisor::Supervisor;

enum Command {
    Start {
        config: ProcessConfig,
        reply: oneshot::Sender<Result<()>>,
    },
    Stop {
        name: String,
        grace: Duration,
        reply: oneshot::Sender<Result<()>>,
    },
    StopAll {
        grace: Duration,
        reply: oneshot::Sender<Result<()>>,
    },
    BlueGreenDeploy {
        config: ProcessConfig,
        drain_period: Duration,
        reply: oneshot::Sender<Result<()>>,
    },
    IsRunning {
        name: String,
        reply: oneshot::Sender<bool>,
    },
    GetState {
        name: String,
        reply: oneshot::Sender<Option<ProcessState>>,
    },
    Remove {
        name: String,
        reply: oneshot::Sender<Option<ProcessConfig>>,
    },
}

#[derive(Clone)]
pub struct SupervisorHandle {
    tx: mpsc::Sender<Command>,
    event_tx: broadcast::Sender<SupervisorEvent>,
}

impl SupervisorHandle {
    pub fn spawn(event_tx: broadcast::Sender<SupervisorEvent>) -> Self {
        let (tx, rx) = mpsc::channel(64);
        let handle = Self {
            tx,
            event_tx: event_tx.clone(),
        };
        tokio::spawn(actor_loop(rx, Supervisor::new(event_tx)));
        handle
    }

    pub fn event_sender(&self) -> &broadcast::Sender<SupervisorEvent> {
        &self.event_tx
    }

    pub async fn start(&self, config: ProcessConfig) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::Start { config, reply })
            .await
            .map_err(|_| SupervisorError::Other(anyhow::anyhow!("supervisor actor stopped")))?;
        rx.await
            .map_err(|_| SupervisorError::Other(anyhow::anyhow!("supervisor actor stopped")))?
    }

    pub async fn stop(&self, name: &str, grace: Duration) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::Stop {
                name: name.to_string(),
                grace,
                reply,
            })
            .await
            .map_err(|_| SupervisorError::Other(anyhow::anyhow!("supervisor actor stopped")))?;
        rx.await
            .map_err(|_| SupervisorError::Other(anyhow::anyhow!("supervisor actor stopped")))?
    }

    pub async fn stop_all(&self, grace: Duration) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::StopAll { grace, reply })
            .await
            .map_err(|_| SupervisorError::Other(anyhow::anyhow!("supervisor actor stopped")))?;
        rx.await
            .map_err(|_| SupervisorError::Other(anyhow::anyhow!("supervisor actor stopped")))?
    }

    pub async fn blue_green_deploy(
        &self,
        config: ProcessConfig,
        drain_period: Duration,
    ) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::BlueGreenDeploy {
                config,
                drain_period,
                reply,
            })
            .await
            .map_err(|_| SupervisorError::Other(anyhow::anyhow!("supervisor actor stopped")))?;
        rx.await
            .map_err(|_| SupervisorError::Other(anyhow::anyhow!("supervisor actor stopped")))?
    }

    pub async fn is_running(&self, name: &str) -> bool {
        let (reply, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(Command::IsRunning {
                name: name.to_string(),
                reply,
            })
            .await;
        rx.await.unwrap_or(false)
    }

    pub async fn state(&self, name: &str) -> Option<ProcessState> {
        let (reply, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(Command::GetState {
                name: name.to_string(),
                reply,
            })
            .await;
        rx.await.ok().flatten()
    }

    pub async fn remove(&self, name: &str) -> Option<ProcessConfig> {
        let (reply, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(Command::Remove {
                name: name.to_string(),
                reply,
            })
            .await;
        rx.await.ok().flatten()
    }
}

async fn actor_loop(mut rx: mpsc::Receiver<Command>, mut sup: Supervisor) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Command::Start { config, reply } => {
                let _ = reply.send(sup.start(config).await);
            }
            Command::Stop { name, grace, reply } => {
                let _ = reply.send(sup.stop(&name, grace).await);
            }
            Command::StopAll { grace, reply } => {
                let _ = reply.send(sup.stop_all(grace).await);
            }
            Command::BlueGreenDeploy {
                config,
                drain_period,
                reply,
            } => {
                let _ = reply.send(sup.blue_green_deploy(config, drain_period).await);
            }
            Command::IsRunning { name, reply } => {
                let _ = reply.send(sup.is_running(&name));
            }
            Command::GetState { name, reply } => {
                let _ = reply.send(sup.state(&name));
            }
            Command::Remove { name, reply } => {
                let _ = reply.send(sup.remove(&name));
            }
        }
    }
}
