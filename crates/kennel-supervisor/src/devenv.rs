use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::config::{
    HttpReadyConfig, ListenSpec, ProcessConfig, ReadyConfig, RestartConfig, WatchConfig,
    WatchdogConfig,
};

const DEVENV_PROCESS_PREFIX: &str = "devenv:processes:";

const INFRASTRUCTURE_PROCESSES: &[&str] = &[
    "devenv:processes:postgres",
    "devenv:processes:redis",
    "devenv:processes:valkey",
    "devenv:processes:garage",
    "devenv:processes:mysql",
    "devenv:processes:mongodb",
    "devenv:processes:elasticsearch",
    "devenv:processes:rabbitmq",
    "devenv:processes:minio",
];

#[derive(Debug, Clone, Deserialize)]
pub struct TaskConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub task_type: String,
    pub command: String,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub process: Option<DevenvProcessConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DevenvProcessConfig {
    pub ready: Option<DevenvReadyConfig>,
    pub restart: Option<DevenvRestartConfig>,
    #[serde(default)]
    pub listen: Vec<ListenSpec>,
    #[serde(default)]
    pub ports: HashMap<String, u16>,
    #[serde(default)]
    pub watch: WatchConfig,
    pub watchdog: Option<WatchdogConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DevenvReadyConfig {
    pub http: Option<HttpReadyConfig>,
    pub exec: Option<String>,
    #[serde(default)]
    pub notify: bool,
    #[serde(default)]
    pub initial_delay: f64,
    #[serde(default = "default_period")]
    pub period: f64,
    #[serde(default = "default_probe_timeout")]
    pub probe_timeout: f64,
    pub timeout: Option<f64>,
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
}

fn default_period() -> f64 {
    10.0
}
fn default_probe_timeout() -> f64 {
    4.0
}
fn default_success_threshold() -> u32 {
    1
}
fn default_failure_threshold() -> u32 {
    5
}

#[derive(Debug, Clone, Deserialize)]
pub struct DevenvRestartConfig {
    pub on: Option<String>,
    pub max: Option<u32>,
    pub window: Option<f64>,
}

impl TaskConfig {
    pub fn is_process(&self) -> bool {
        self.task_type == "process"
    }

    pub fn is_infrastructure(&self) -> bool {
        INFRASTRUCTURE_PROCESSES
            .iter()
            .any(|name| self.name == *name)
    }

    /// Strip the `devenv:processes:` prefix from the task name.
    pub fn stripped_name(&self) -> &str {
        self.name
            .strip_prefix(DEVENV_PROCESS_PREFIX)
            .unwrap_or(&self.name)
    }

    /// Extract the resource provider name from an infrastructure process.
    /// e.g., "devenv:processes:postgres" -> "postgres"
    pub fn resource_provider_name(&self) -> Option<&str> {
        if self.is_infrastructure() {
            Some(self.stripped_name())
        } else {
            None
        }
    }

    pub fn into_process_config(self) -> ProcessConfig {
        let name = self
            .name
            .strip_prefix(DEVENV_PROCESS_PREFIX)
            .unwrap_or(&self.name)
            .to_string();

        let after: Vec<String> = self
            .after
            .into_iter()
            .map(|dep| {
                dep.strip_prefix(DEVENV_PROCESS_PREFIX)
                    .unwrap_or(&dep)
                    .to_string()
            })
            .collect();

        let (ready, restart, listen, ports, watch, watchdog) = if let Some(proc) = self.process {
            let ready = proc.ready.map(|r| ReadyConfig {
                exec: r.exec,
                http: r.http,
                notify: r.notify,
                initial_delay: std::time::Duration::from_secs_f64(r.initial_delay),
                period: std::time::Duration::from_secs_f64(r.period),
                probe_timeout: std::time::Duration::from_secs_f64(r.probe_timeout),
                timeout: r.timeout.map(std::time::Duration::from_secs_f64),
                success_threshold: r.success_threshold,
                failure_threshold: r.failure_threshold,
            });

            let restart = proc.restart.map(|r| {
                let policy = match r.on.as_deref() {
                    Some("never") => crate::config::RestartPolicy::Never,
                    Some("always") => crate::config::RestartPolicy::Always,
                    _ => crate::config::RestartPolicy::OnFailure,
                };
                RestartConfig {
                    on: policy,
                    max: r.max,
                    window: r.window.map(std::time::Duration::from_secs_f64),
                }
            });

            (
                ready,
                restart.unwrap_or_default(),
                proc.listen,
                proc.ports,
                proc.watch,
                proc.watchdog,
            )
        } else {
            (
                None,
                RestartConfig::default(),
                vec![],
                HashMap::new(),
                WatchConfig::default(),
                None,
            )
        };

        ProcessConfig {
            name,
            exec: self.command,
            args: vec![],
            cwd: self.cwd,
            env: self.env,
            after,
            listen,
            ports,
            ready,
            restart,
            watch,
            watchdog,
            resources: None,
            user: None,
            capabilities: vec![],
        }
    }
}

/// Parse devenv's tasks.json into task configs, filtering to application
/// processes and returning both process configs and required resource names.
pub fn parse_tasks(json: &str) -> crate::Result<(Vec<ProcessConfig>, Vec<String>)> {
    let tasks: Vec<TaskConfig> = serde_json::from_str(json).map_err(|e| {
        crate::SupervisorError::Other(anyhow::anyhow!("failed to parse tasks.json: {e}"))
    })?;

    let mut process_configs = Vec::new();
    let mut required_resources = Vec::new();

    for task in tasks {
        if !task.is_process() {
            continue;
        }

        if task.is_infrastructure() {
            if let Some(provider) = task.resource_provider_name() {
                required_resources.push(provider.to_string());
            }
            continue;
        }

        process_configs.push(task.into_process_config());
    }

    Ok((process_configs, required_resources))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_devenv_tasks_json() {
        let json = r#"[
            {
                "name": "devenv:processes:postgres",
                "type": "process",
                "command": "/nix/store/abc-postgres-start",
                "after": [],
                "env": {},
                "cwd": null,
                "process": null
            },
            {
                "name": "devenv:processes:api",
                "type": "process",
                "command": "/nix/store/xyz-start-api",
                "after": ["devenv:processes:postgres"],
                "env": {"RUST_LOG": "debug"},
                "cwd": null,
                "process": {
                    "ready": {
                        "http": {
                            "get": {
                                "host": "127.0.0.1",
                                "port": 8080,
                                "path": "/health",
                                "scheme": "http"
                            }
                        },
                        "exec": null,
                        "notify": false,
                        "initial_delay": 0,
                        "period": 10,
                        "probe_timeout": 4,
                        "timeout": null,
                        "success_threshold": 1,
                        "failure_threshold": 5
                    },
                    "restart": {"on": "on_failure", "max": 5, "window": null},
                    "listen": [],
                    "ports": {"http": 8080},
                    "watch": {"paths": [], "extensions": [], "ignore": []}
                }
            },
            {
                "name": "devenv:processes:worker",
                "type": "process",
                "command": "/nix/store/xyz-start-worker",
                "after": ["devenv:processes:api"],
                "env": {},
                "cwd": null,
                "process": null
            }
        ]"#;

        let (configs, resources) = parse_tasks(json).unwrap();

        // postgres is infrastructure, filtered out
        assert_eq!(resources, vec!["postgres"]);

        // api and worker are application processes
        assert_eq!(configs.len(), 2);

        assert_eq!(configs[0].name, "api");
        assert_eq!(configs[0].exec, "/nix/store/xyz-start-api");
        assert_eq!(configs[0].after, vec!["postgres"]);
        assert_eq!(configs[0].env["RUST_LOG"], "debug");

        let ready = configs[0].ready.as_ref().unwrap();
        let http = ready.http.as_ref().unwrap().get.as_ref().unwrap();
        assert_eq!(http.port, 8080);
        assert_eq!(http.path, "/health");
        assert_eq!(ready.failure_threshold, 5);

        assert_eq!(configs[1].name, "worker");
        assert_eq!(configs[1].after, vec!["api"]);
    }

    #[test]
    fn infrastructure_detection() {
        let task = TaskConfig {
            name: "devenv:processes:postgres".into(),
            task_type: "process".into(),
            command: "/nix/store/abc".into(),
            after: vec![],
            env: HashMap::new(),
            cwd: None,
            process: None,
        };

        assert!(task.is_infrastructure());
        assert_eq!(task.resource_provider_name(), Some("postgres"));
    }

    #[test]
    fn application_process_not_infrastructure() {
        let task = TaskConfig {
            name: "devenv:processes:api".into(),
            task_type: "process".into(),
            command: "/nix/store/abc".into(),
            after: vec![],
            env: HashMap::new(),
            cwd: None,
            process: None,
        };

        assert!(!task.is_infrastructure());
        assert_eq!(task.resource_provider_name(), None);
    }

    #[test]
    fn prefix_stripping() {
        let task = TaskConfig {
            name: "devenv:processes:my-service".into(),
            task_type: "process".into(),
            command: "/bin/test".into(),
            after: vec!["devenv:processes:db".into()],
            env: HashMap::new(),
            cwd: None,
            process: None,
        };

        let config = task.into_process_config();
        assert_eq!(config.name, "my-service");
        assert_eq!(config.after, vec!["db"]);
    }

    #[test]
    fn task_type_filtering() {
        let json = r#"[
            {"name": "build-assets", "type": "task", "command": "make build", "after": [], "env": {}, "cwd": null, "process": null},
            {"name": "devenv:processes:api", "type": "process", "command": "/bin/api", "after": [], "env": {}, "cwd": null, "process": null}
        ]"#;

        let (configs, resources) = parse_tasks(json).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "api");
        assert!(resources.is_empty());
    }

    #[test]
    fn empty_tasks() {
        let (configs, resources) = parse_tasks("[]").unwrap();
        assert!(configs.is_empty());
        assert!(resources.is_empty());
    }
}
