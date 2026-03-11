use kennel_config::constants;
use tokio::sync::mpsc;

pub struct Channels {
    pub build_tx: mpsc::Sender<i32>,
    pub build_rx: mpsc::Receiver<i32>,
    pub deploy_tx: mpsc::Sender<kennel_deployer::DeploymentRequest>,
    pub deploy_rx: mpsc::Receiver<kennel_deployer::DeploymentRequest>,
    pub teardown_tx: mpsc::Sender<i32>,
    pub teardown_rx: mpsc::Receiver<i32>,
}

pub fn create_channels() -> Channels {
    let (build_tx, build_rx) = mpsc::channel(constants::BUILD_CHANNEL_CAPACITY);
    let (deploy_tx, deploy_rx) = mpsc::channel(constants::DEPLOY_CHANNEL_CAPACITY);
    let (teardown_tx, teardown_rx) = mpsc::channel(constants::TEARDOWN_CHANNEL_CAPACITY);

    Channels {
        build_tx,
        build_rx,
        deploy_tx,
        deploy_rx,
        teardown_tx,
        teardown_rx,
    }
}
