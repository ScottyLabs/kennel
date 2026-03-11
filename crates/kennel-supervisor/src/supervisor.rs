use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use watchexec_signals::Signal;
use watchexec_supervisor::command::{Command, Program, SpawnOptions};
use watchexec_supervisor::job::{Job, start_job};

use crate::cgroup::{CgroupStats, CgroupWrapper};
use crate::config::{ProcessConfig, RestartPolicy};
use crate::error::{Result, SupervisorError};
use crate::event::SupervisorEvent;
use crate::probe;
use crate::socket::BoundSocket;
use crate::state::ProcessState;

struct ManagedProcess {
    config: ProcessConfig,
    job: Job,
    _job_handle: JoinHandle<()>,
    supervision_handle: JoinHandle<()>,
    state: Arc<StdMutex<ProcessState>>,
    _bound_sockets: Vec<BoundSocket>,
    cgroup: Option<CgroupWrapper>,
}

pub struct Supervisor {
    processes: HashMap<String, ManagedProcess>,
    event_tx: broadcast::Sender<SupervisorEvent>,
}

impl Supervisor {
    pub fn new(event_tx: broadcast::Sender<SupervisorEvent>) -> Self {
        Self {
            processes: HashMap::new(),
            event_tx,
        }
    }

    pub fn event_sender(&self) -> &broadcast::Sender<SupervisorEvent> {
        &self.event_tx
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.processes.get(name).is_some_and(|p| p.job.is_running())
    }

    pub fn state(&self, name: &str) -> Option<ProcessState> {
        self.processes
            .get(name)
            .map(|p| p.state.lock().unwrap().clone())
    }

    pub fn process_names(&self) -> impl Iterator<Item = &str> {
        self.processes.keys().map(String::as_str)
    }

    pub fn stats(&self, name: &str) -> Result<Option<CgroupStats>> {
        let managed = self
            .processes
            .get(name)
            .ok_or_else(|| SupervisorError::UnknownProcess(name.to_string()))?;

        match &managed.cgroup {
            Some(cg) => Ok(Some(cg.stats()?)),
            None => Ok(None),
        }
    }

    pub async fn start(&mut self, config: ProcessConfig) -> Result<()> {
        let name = config.name.clone();
        tracing::info!(process = %name, exec = %config.exec, "starting process");

        let bound_sockets = crate::socket::bind_sockets(&config.listen)?;

        let cgroup = if let Some(ref limits) = config.resources {
            Some(CgroupWrapper::create(&name, limits)?)
        } else {
            None
        };

        let command = build_command(&config);
        let (job, job_handle) = start_job(command);

        // Create notify socket if the process uses notify readiness.
        let notify_socket_path = if config.ready.as_ref().is_some_and(|r| r.notify) {
            let path = std::env::temp_dir().join(format!("kennel-notify-{}", name));
            Some(path)
        } else {
            None
        };

        let spawn_env = config.env.clone();
        let spawn_cwd = config.cwd.clone();
        let socket_fds: Vec<_> = bound_sockets.iter().map(|s| s.fd).collect();
        let has_sockets = !socket_fds.is_empty();
        let notify_path = notify_socket_path.clone();

        #[cfg(target_os = "linux")]
        let spawn_user = config.user.clone();
        #[cfg(target_os = "linux")]
        let spawn_caps = config.capabilities.clone();
        #[cfg(target_os = "linux")]
        let cgroup_procs_path = cgroup.as_ref().map(|cg| cg.path().join("cgroup.procs"));

        job.set_spawn_hook(move |cmd, _ctx| {
            let inner = cmd.command_mut();

            for (k, v) in &spawn_env {
                inner.env(k, v);
            }

            if let Some(cwd) = &spawn_cwd {
                inner.current_dir(cwd);
            }

            if has_sockets {
                inner.env("LISTEN_FDS", socket_fds.len().to_string());
            }

            if let Some(ref path) = notify_path {
                inner.env("NOTIFY_SOCKET", path.to_string_lossy().as_ref());
            }

            let pre_exec_has_sockets = has_sockets;
            #[cfg(target_os = "linux")]
            let pre_exec_user = spawn_user.clone();
            #[cfg(target_os = "linux")]
            let pre_exec_caps = spawn_caps.clone();
            #[cfg(target_os = "linux")]
            let pre_exec_cgroup = cgroup_procs_path.clone();

            // SAFETY: pre_exec runs between fork and exec in the child
            // process. Only the calling thread exists in the child, so
            // there are no data races. The operations (setenv, setuid,
            // setgid, fs::write) are all async-signal-safe or only
            // affect the child's own state before exec replaces it.
            unsafe {
                inner.pre_exec(move || {
                    if pre_exec_has_sockets {
                        std::env::set_var("LISTEN_PID", std::process::id().to_string());
                    }

                    #[cfg(target_os = "linux")]
                    if let Some(ref username) = pre_exec_user {
                        if let Ok(uid) = resolve_uid(username) {
                            if let Ok(gid) = resolve_gid(username) {
                                let _ = nix::unistd::setgid(gid);
                                let _ = nix::unistd::setuid(uid);
                            }
                        }
                    }

                    #[cfg(target_os = "linux")]
                    for cap_name in &pre_exec_caps {
                        if let Ok(cap) = cap_name.parse::<caps::Capability>() {
                            let _ = caps::raise(None, caps::CapSet::Ambient, cap);
                        }
                    }

                    #[cfg(target_os = "linux")]
                    if let Some(ref procs_path) = pre_exec_cgroup {
                        std::fs::write(procs_path, std::process::id().to_string())?;
                    }

                    Ok(())
                });
            }
        })
        .await;

        job.start().await;

        let state = Arc::new(StdMutex::new(ProcessState::Starting));
        let supervision_handle = spawn_supervision_task(
            config.clone(),
            job.clone(),
            state.clone(),
            self.event_tx.clone(),
            notify_socket_path,
        );

        self.processes.insert(
            name,
            ManagedProcess {
                config,
                job,
                _job_handle: job_handle,
                supervision_handle,
                state,
                _bound_sockets: bound_sockets,
                cgroup,
            },
        );

        Ok(())
    }

    pub async fn stop(&mut self, name: &str, grace: Duration) -> Result<()> {
        let managed = self
            .processes
            .get_mut(name)
            .ok_or_else(|| SupervisorError::UnknownProcess(name.to_string()))?;

        tracing::info!(process = %name, ?grace, "stopping process");

        // Abort the supervision task first so it doesn't try to restart
        // the process after we stop it.
        managed.supervision_handle.abort();

        *managed.state.lock().unwrap() = ProcessState::Stopping;
        managed.job.stop_with_signal(Signal::Terminate, grace).await;
        *managed.state.lock().unwrap() = ProcessState::Stopped;

        let _ = self.event_tx.send(SupervisorEvent::ProcessStopped {
            name: name.to_string(),
        });

        Ok(())
    }

    pub async fn stop_all(&mut self, grace: Duration) -> Result<()> {
        let names: Vec<String> = self.processes.keys().cloned().collect();

        for name in &names {
            if let Err(e) = self.stop(name, grace).await {
                tracing::warn!(process = %name, error = %e, "failed to stop process");
            }
        }

        Ok(())
    }

    /// Start all processes respecting dependency order. Processes in the
    /// same topological batch start concurrently. Each batch waits for all
    /// readiness probes to pass before the next batch starts.
    pub async fn start_all(&mut self, configs: Vec<ProcessConfig>) -> Result<()> {
        let batches = crate::order::topological_sort(&configs)?;

        for batch in batches {
            for config in &batch {
                self.start((*config).clone()).await?;
            }

            let names: Vec<String> = batch.iter().map(|c| c.name.clone()).collect();
            self.wait_for_ready(&names).await?;
        }

        Ok(())
    }

    async fn wait_for_ready(&self, names: &[String]) -> Result<()> {
        let mut remaining: std::collections::HashSet<_> = names.iter().cloned().collect();
        let mut event_rx = self.event_tx.subscribe();

        while !remaining.is_empty() {
            match event_rx.recv().await {
                Ok(SupervisorEvent::ProcessReady { ref name, .. }) => {
                    remaining.remove(name);
                }
                Ok(SupervisorEvent::ProcessFailed {
                    ref name,
                    ref error,
                }) => {
                    if remaining.contains(name) {
                        return Err(SupervisorError::ProcessFailed {
                            name: name.clone(),
                            reason: error.clone(),
                        });
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        Ok(())
    }

    /// Deploy a new version of a process. Starts the new process, waits for
    /// readiness, drains for `drain_period`, then stops the old process. On
    /// failure, restores the old process.
    pub async fn blue_green_deploy(
        &mut self,
        new_config: ProcessConfig,
        drain_period: Duration,
    ) -> Result<()> {
        let name = new_config.name.clone();
        let old_name = format!("{name}--old");

        // Rename the current process to make room for the new one.
        if let Some(old) = self.processes.remove(&name) {
            self.processes.insert(old_name.clone(), old);
        }

        self.start(new_config).await?;

        // Wait for the new process to become ready.
        let mut event_rx = self.event_tx.subscribe();
        loop {
            match event_rx.recv().await {
                Ok(SupervisorEvent::ProcessReady { name: ref n, .. }) if *n == name => break,
                Ok(SupervisorEvent::ProcessFailed {
                    name: ref n,
                    ref error,
                }) if *n == name => {
                    // Restore the old process on failure.
                    if let Some(old) = self.processes.remove(&old_name) {
                        self.processes.insert(name.clone(), old);
                    }
                    return Err(SupervisorError::ProcessFailed {
                        name: name.clone(),
                        reason: error.clone(),
                    });
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        tokio::time::sleep(drain_period).await;

        // Stop the old process if it exists.
        if self.processes.contains_key(&old_name) {
            self.stop(&old_name, Duration::from_secs(10)).await?;
            self.processes.remove(&old_name);
        }

        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Option<ProcessConfig> {
        self.processes.remove(name).map(|p| p.config)
    }
}

fn build_command(config: &ProcessConfig) -> Arc<Command> {
    Arc::new(Command {
        program: Program::Exec {
            prog: PathBuf::from(&config.exec),
            args: config.args.clone(),
        },
        options: SpawnOptions {
            grouped: true,
            session: false,
            reset_sigmask: true,
        },
    })
}

#[cfg(target_os = "linux")]
fn resolve_uid(username: &str) -> std::io::Result<nix::unistd::Uid> {
    use nix::unistd::User;
    User::from_name(username)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        .map(|u| u.uid)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("user not found: {username}"),
            )
        })
}

#[cfg(target_os = "linux")]
fn resolve_gid(username: &str) -> std::io::Result<nix::unistd::Gid> {
    use nix::unistd::User;
    User::from_name(username)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        .map(|u| u.gid)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("user not found: {username}"),
            )
        })
}

fn spawn_supervision_task(
    config: ProcessConfig,
    job: Job,
    state: Arc<StdMutex<ProcessState>>,
    event_tx: broadcast::Sender<SupervisorEvent>,
    notify_socket_path: Option<PathBuf>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let name = config.name.clone();

        // Initial readiness probe. For notify probes, wait for READY=1
        // on the notify socket instead of polling.
        if let Some(ready_config) = &config.ready {
            let probe_result = if ready_config.notify {
                let socket_path = notify_socket_path
                    .as_deref()
                    .unwrap_or_else(|| std::path::Path::new("/dev/null"));
                let timeout = ready_config
                    .timeout
                    .unwrap_or(std::time::Duration::from_secs(30));
                crate::notify::wait_for_ready(socket_path, timeout).await
            } else {
                probe::run_readiness_probe(&name, ready_config).await
            };

            match probe_result {
                Ok(()) => {
                    *state.lock().unwrap() = ProcessState::Ready;
                    let port = config.ports.values().next().copied();
                    let _ = event_tx.send(SupervisorEvent::ProcessReady {
                        name: name.clone(),
                        port,
                        store_path: None,
                    });
                }
                Err(e) => {
                    *state.lock().unwrap() = ProcessState::Failed {
                        error: e.to_string(),
                        restarts: 0,
                    };
                    let _ = event_tx.send(SupervisorEvent::ProcessFailed {
                        name: name.clone(),
                        error: e.to_string(),
                    });
                    let _ = job
                        .stop_with_signal(Signal::Terminate, Duration::from_secs(10))
                        .await;
                    return;
                }
            }
        } else {
            *state.lock().unwrap() = ProcessState::Running;
            let port = config.ports.values().next().copied();
            let _ = event_tx.send(SupervisorEvent::ProcessReady {
                name: name.clone(),
                port,
                store_path: None,
            });
        }

        // Liveness monitoring and restart loop.
        let mut restarts = 0u32;
        let mut window_start = tokio::time::Instant::now();
        let mut healthy = true;

        loop {
            tokio::select! {
                _ = job.to_wait() => {
                    if should_restart(&config.restart, restarts, window_start) {
                        if let Some(window) = config.restart.window
                            && window_start.elapsed() > window {
                                restarts = 0;
                                window_start = tokio::time::Instant::now();
                            }

                        restarts += 1;
                        *state.lock().unwrap() = ProcessState::Restarting { attempt: restarts };
                        let _ = event_tx.send(SupervisorEvent::ProcessRestarting {
                            name: name.clone(),
                            attempt: restarts,
                        });

                        job.start().await;

                        if let Some(ready_config) = &config.ready {
                            match probe::run_readiness_probe(&name, ready_config).await {
                                Ok(()) => {
                                    healthy = true;
                                    *state.lock().unwrap() = ProcessState::Ready;
                                    let port = config.ports.values().next().copied();
                                    let _ = event_tx.send(SupervisorEvent::ProcessReady {
                                        name: name.clone(),
                                        port,
                                        store_path: None,
                                    });
                                }
                                Err(e) => {
                                    *state.lock().unwrap() = ProcessState::Failed {
                                        error: e.to_string(),
                                        restarts,
                                    };
                                    let _ = event_tx.send(SupervisorEvent::ProcessFailed {
                                        name: name.clone(),
                                        error: e.to_string(),
                                    });
                                    let _ = job.stop_with_signal(
                                        Signal::Terminate,
                                        Duration::from_secs(10),
                                    ).await;
                                    return;
                                }
                            }
                        } else {
                            *state.lock().unwrap() = ProcessState::Running;
                            let port = config.ports.values().next().copied();
                            let _ = event_tx.send(SupervisorEvent::ProcessReady {
                                name: name.clone(),
                                port,
                                store_path: None,
                            });
                        }
                    } else {
                        *state.lock().unwrap() = ProcessState::Stopped;
                        let _ = event_tx.send(SupervisorEvent::ProcessStopped {
                            name: name.clone(),
                        });
                        return;
                    }
                }

                _ = probe::liveness_tick(&config.ready), if config.ready.is_some() => {
                    let ready_config = config.ready.as_ref().unwrap();
                    let probe_result = probe::run_single_probe(ready_config).await;

                    if probe_result.is_err() && healthy {
                        healthy = false;
                        *state.lock().unwrap() = ProcessState::Unhealthy;
                        let _ = event_tx.send(SupervisorEvent::ProcessUnhealthy {
                            name: name.clone(),
                        });
                    } else if probe_result.is_ok() && !healthy {
                        healthy = true;
                        *state.lock().unwrap() = ProcessState::Ready;
                        let port = config.ports.values().next().copied();
                        let _ = event_tx.send(SupervisorEvent::ProcessHealthy {
                            name: name.clone(),
                            port,
                        });
                    }
                }
            }
        }
    })
}

fn should_restart(
    config: &crate::config::RestartConfig,
    restarts: u32,
    window_start: tokio::time::Instant,
) -> bool {
    match config.on {
        RestartPolicy::Never => false,
        RestartPolicy::Always | RestartPolicy::OnFailure => match config.max {
            None => true,
            Some(max) => {
                let effective_restarts = match config.window {
                    Some(window) if window_start.elapsed() > window => 0,
                    _ => restarts,
                };
                effective_restarts < max
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::config::ReadyConfig;

    fn test_config(name: &str, exec: &str) -> ProcessConfig {
        ProcessConfig {
            name: name.into(),
            exec: exec.into(),
            args: vec![],
            cwd: None,
            env: HashMap::new(),
            after: vec![],
            listen: vec![],
            ports: HashMap::new(),
            ready: None,
            restart: Default::default(),
            watch: Default::default(),
            watchdog: None,
            resources: None,
            user: None,
            capabilities: vec![],
        }
    }

    #[tokio::test]
    async fn start_and_stop_process() {
        let (tx, mut rx) = broadcast::channel(16);
        let mut supervisor = Supervisor::new(tx);

        let mut config = test_config("sleeper", "sleep");
        config.args = vec!["60".into()];

        supervisor.start(config).await.unwrap();

        // Wait for the supervision task to emit ProcessReady.
        let event = rx.recv().await.unwrap();
        assert!(
            matches!(event, SupervisorEvent::ProcessReady { ref name, .. } if name == "sleeper")
        );

        assert!(supervisor.is_running("sleeper"));

        supervisor
            .stop("sleeper", Duration::from_secs(2))
            .await
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, SupervisorEvent::ProcessStopped { ref name } if name == "sleeper"));

        assert_eq!(supervisor.state("sleeper"), Some(ProcessState::Stopped));
    }

    #[tokio::test]
    async fn stop_unknown_process() {
        let (tx, _rx) = broadcast::channel(16);
        let mut supervisor = Supervisor::new(tx);

        let result = supervisor.stop("nonexistent", Duration::from_secs(1)).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SupervisorError::UnknownProcess(_)
        ));
    }

    #[tokio::test]
    async fn start_with_env() {
        let (tx, mut rx) = broadcast::channel(16);
        let mut supervisor = Supervisor::new(tx);

        let mut config = test_config("echo-env", "sh");
        config.args = vec!["-c".into(), "echo $MY_VAR && sleep 5".into()];
        config.env.insert("MY_VAR".into(), "hello".into());

        supervisor.start(config).await.unwrap();

        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessReady { ref name, .. } if name == "echo-env"
        ));

        supervisor
            .stop("echo-env", Duration::from_secs(2))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn stop_all_processes() {
        let (tx, _rx) = broadcast::channel(16);
        let mut supervisor = Supervisor::new(tx);

        let mut c1 = test_config("proc-a", "sleep");
        c1.args = vec!["60".into()];
        let mut c2 = test_config("proc-b", "sleep");
        c2.args = vec!["60".into()];

        supervisor.start(c1).await.unwrap();
        supervisor.start(c2).await.unwrap();

        // Give supervision tasks time to emit ProcessReady.
        tokio::time::sleep(Duration::from_millis(50)).await;

        supervisor.stop_all(Duration::from_secs(2)).await.unwrap();

        assert_eq!(supervisor.state("proc-a"), Some(ProcessState::Stopped));
        assert_eq!(supervisor.state("proc-b"), Some(ProcessState::Stopped));
    }

    #[tokio::test]
    async fn process_names_iterator() {
        let (tx, _rx) = broadcast::channel(16);
        let mut supervisor = Supervisor::new(tx);

        let mut c1 = test_config("alpha", "sleep");
        c1.args = vec!["60".into()];
        let mut c2 = test_config("beta", "sleep");
        c2.args = vec!["60".into()];

        supervisor.start(c1).await.unwrap();
        supervisor.start(c2).await.unwrap();

        let mut names: Vec<&str> = supervisor.process_names().collect();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);

        supervisor.stop_all(Duration::from_secs(1)).await.unwrap();
    }

    #[tokio::test]
    async fn remove_process() {
        let (tx, mut rx) = broadcast::channel(16);
        let mut supervisor = Supervisor::new(tx);

        let mut config = test_config("removable", "sleep");
        config.args = vec!["60".into()];

        supervisor.start(config).await.unwrap();
        let _ = rx.recv().await.unwrap();

        supervisor
            .stop("removable", Duration::from_secs(1))
            .await
            .unwrap();

        let removed = supervisor.remove("removable");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name, "removable");
        assert!(supervisor.state("removable").is_none());
    }

    #[tokio::test]
    async fn process_restarts_on_exit() {
        let (tx, mut rx) = broadcast::channel(16);
        let mut supervisor = Supervisor::new(tx);

        // Process exits immediately, restart policy should trigger.
        let mut config = test_config("crasher", "true");
        config.restart.on = RestartPolicy::Always;
        config.restart.max = Some(2);

        supervisor.start(config).await.unwrap();

        // Initial ProcessReady
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessReady { ref name, .. } if name == "crasher"
        ));

        // First restart
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessRestarting { ref name, attempt: 1 } if name == "crasher"
        ));

        // Ready after first restart
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessReady { ref name, .. } if name == "crasher"
        ));

        // Second restart
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessRestarting { ref name, attempt: 2 } if name == "crasher"
        ));

        // Ready after second restart
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessReady { ref name, .. } if name == "crasher"
        ));

        // Restart limit reached, process stops.
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessStopped { ref name } if name == "crasher"
        ));
    }

    #[tokio::test]
    async fn process_no_restart_on_never_policy() {
        let (tx, mut rx) = broadcast::channel(16);
        let mut supervisor = Supervisor::new(tx);

        let mut config = test_config("oneshot", "true");
        config.restart.on = RestartPolicy::Never;

        supervisor.start(config).await.unwrap();

        // ProcessReady
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessReady { ref name, .. } if name == "oneshot"
        ));

        // Process exits, no restart, just stopped.
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessStopped { ref name } if name == "oneshot"
        ));
    }

    #[tokio::test]
    async fn readiness_probe_exec_gates_ready() {
        let (tx, mut rx) = broadcast::channel(16);
        let mut supervisor = Supervisor::new(tx);

        let mut config = test_config("probed", "sleep");
        config.args = vec!["60".into()];
        config.ready = Some(ReadyConfig {
            exec: Some("true".into()),
            http: None,
            notify: false,
            initial_delay: Duration::ZERO,
            period: Duration::from_millis(100),
            probe_timeout: Duration::from_secs(5),
            timeout: Some(Duration::from_secs(10)),
            success_threshold: 1,
            failure_threshold: 3,
        });

        supervisor.start(config).await.unwrap();

        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessReady { ref name, .. } if name == "probed"
        ));

        assert_eq!(supervisor.state("probed"), Some(ProcessState::Ready));

        supervisor
            .stop("probed", Duration::from_secs(1))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn readiness_probe_failure_emits_failed() {
        let (tx, mut rx) = broadcast::channel(16);
        let mut supervisor = Supervisor::new(tx);

        let mut config = test_config("bad-probe", "sleep");
        config.args = vec!["60".into()];
        config.ready = Some(ReadyConfig {
            exec: Some("false".into()),
            http: None,
            notify: false,
            initial_delay: Duration::ZERO,
            period: Duration::from_millis(50),
            probe_timeout: Duration::from_secs(5),
            timeout: Some(Duration::from_secs(1)),
            success_threshold: 1,
            failure_threshold: 2,
        });

        supervisor.start(config).await.unwrap();

        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            SupervisorEvent::ProcessFailed { ref name, .. } if name == "bad-probe"
        ));
    }

    #[tokio::test]
    async fn blue_green_deploy_replaces_process() {
        let (tx, mut rx) = broadcast::channel(32);
        let mut supervisor = Supervisor::new(tx);

        let mut old = test_config("svc", "sleep");
        old.args = vec!["60".into()];

        supervisor.start(old).await.unwrap();
        let _ = rx.recv().await.unwrap(); // old ProcessReady

        assert!(supervisor.is_running("svc"));

        let mut new = test_config("svc", "sleep");
        new.args = vec!["120".into()];

        supervisor
            .blue_green_deploy(new, Duration::from_millis(50))
            .await
            .unwrap();

        // The old process should have been stopped.
        assert!(supervisor.state("svc--old").is_none());
        // The new process should be running under the original name.
        assert!(supervisor.is_running("svc"));

        supervisor
            .stop("svc", Duration::from_secs(1))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn blue_green_deploy_no_existing_process() {
        let (tx, _rx) = broadcast::channel(32);
        let mut supervisor = Supervisor::new(tx);

        let mut config = test_config("new-svc", "sleep");
        config.args = vec!["60".into()];

        supervisor
            .blue_green_deploy(config, Duration::from_millis(10))
            .await
            .unwrap();

        assert!(supervisor.is_running("new-svc"));

        supervisor
            .stop("new-svc", Duration::from_secs(1))
            .await
            .unwrap();
    }
}
