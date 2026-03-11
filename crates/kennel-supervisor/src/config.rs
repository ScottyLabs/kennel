use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessConfig {
    pub name: String,
    pub exec: String,

    #[serde(default)]
    pub args: Vec<String>,

    pub cwd: Option<PathBuf>,

    #[serde(default)]
    pub env: HashMap<String, String>,

    #[serde(default)]
    pub after: Vec<String>,

    #[serde(default)]
    pub listen: Vec<ListenSpec>,

    #[serde(default)]
    pub ports: HashMap<String, u16>,

    pub ready: Option<ReadyConfig>,

    #[serde(default)]
    pub restart: RestartConfig,

    #[serde(default)]
    pub watch: WatchConfig,

    pub watchdog: Option<WatchdogConfig>,

    pub resources: Option<ResourceLimits>,

    pub user: Option<String>,

    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReadyConfig {
    pub exec: Option<String>,
    pub http: Option<HttpReadyConfig>,

    #[serde(default)]
    pub notify: bool,

    #[serde(default = "defaults::initial_delay", with = "duration_secs")]
    pub initial_delay: Duration,

    #[serde(default = "defaults::period", with = "duration_secs")]
    pub period: Duration,

    #[serde(default = "defaults::probe_timeout", with = "duration_secs")]
    pub probe_timeout: Duration,

    #[serde(default, with = "option_duration_secs")]
    pub timeout: Option<Duration>,

    #[serde(default = "defaults::success_threshold")]
    pub success_threshold: u32,

    #[serde(default = "defaults::failure_threshold")]
    pub failure_threshold: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpReadyConfig {
    pub get: Option<HttpProbe>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpProbe {
    #[serde(default = "defaults::probe_host")]
    pub host: String,
    pub port: u16,
    #[serde(default = "defaults::probe_path")]
    pub path: String,
    #[serde(default = "defaults::probe_scheme")]
    pub scheme: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RestartConfig {
    #[serde(default)]
    pub on: RestartPolicy,

    #[serde(default = "defaults::restart_max")]
    pub max: Option<u32>,

    #[serde(default, with = "option_duration_secs")]
    pub window: Option<Duration>,
}

impl Default for RestartConfig {
    fn default() -> Self {
        Self {
            on: RestartPolicy::default(),
            max: Some(5),
            window: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    Never,
    Always,
    #[default]
    OnFailure,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ListenSpec {
    pub name: String,

    #[serde(default)]
    pub kind: ListenKind,

    pub address: Option<String>,
    pub path: Option<PathBuf>,

    #[serde(default = "defaults::backlog")]
    pub backlog: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListenKind {
    #[default]
    Tcp,
    UnixStream,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WatchConfig {
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WatchdogConfig {
    pub usec: u64,
    #[serde(default)]
    pub require_ready: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResourceLimits {
    pub memory_max: Option<u64>,
    pub memory_high: Option<u64>,
    pub cpu_max: Option<f64>,
    pub cpu_weight: Option<u32>,
    pub tasks_max: Option<u64>,
}

mod defaults {
    use std::time::Duration;

    pub fn initial_delay() -> Duration {
        Duration::ZERO
    }

    pub fn period() -> Duration {
        Duration::from_secs(10)
    }

    pub fn probe_timeout() -> Duration {
        Duration::from_secs(4)
    }

    pub fn success_threshold() -> u32 {
        1
    }

    pub fn failure_threshold() -> u32 {
        5
    }

    pub fn probe_host() -> String {
        "127.0.0.1".into()
    }

    pub fn probe_path() -> String {
        "/".into()
    }

    pub fn probe_scheme() -> String {
        "http".into()
    }

    pub fn restart_max() -> Option<u32> {
        Some(5)
    }

    pub fn backlog() -> u32 {
        128
    }
}

mod duration_secs {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        duration.as_secs_f64().serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let secs = f64::deserialize(deserializer)?;
        Ok(Duration::from_secs_f64(secs))
    }
}

mod option_duration_secs {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        duration: &Option<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match duration {
            Some(d) => serializer.serialize_some(&d.as_secs_f64()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Duration>, D::Error> {
        let secs: Option<f64> = Option::deserialize(deserializer)?;
        Ok(secs.map(Duration::from_secs_f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal_process_config() {
        let json = r#"{
            "name": "api",
            "exec": "/nix/store/abc123-api/bin/api"
        }"#;

        let config: ProcessConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.name, "api");
        assert_eq!(config.exec, "/nix/store/abc123-api/bin/api");
        assert!(config.args.is_empty());
        assert!(config.env.is_empty());
        assert!(config.after.is_empty());
        assert!(config.ready.is_none());
        assert_eq!(config.restart.on, RestartPolicy::OnFailure);
        assert_eq!(config.restart.max, Some(5));
        assert!(config.user.is_none());
    }

    #[test]
    fn deserialize_full_process_config() {
        let json = r#"{
            "name": "api",
            "exec": "/nix/store/abc123-api/bin/api",
            "args": ["--port", "8080"],
            "cwd": "/var/lib/kennel/services/myapp",
            "env": {"RUST_LOG": "debug"},
            "after": ["postgres"],
            "ports": {"http": 8080},
            "ready": {
                "http": {
                    "get": {
                        "host": "127.0.0.1",
                        "port": 8080,
                        "path": "/health",
                        "scheme": "http"
                    }
                },
                "initial_delay": 2,
                "period": 5,
                "probe_timeout": 3,
                "timeout": 30,
                "success_threshold": 1,
                "failure_threshold": 3
            },
            "restart": {
                "on": "on_failure",
                "max": 10,
                "window": 300
            },
            "user": "kennel-myapp",
            "capabilities": ["CAP_NET_BIND_SERVICE"]
        }"#;

        let config: ProcessConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.name, "api");
        assert_eq!(config.args, vec!["--port", "8080"]);
        assert_eq!(config.env["RUST_LOG"], "debug");
        assert_eq!(config.after, vec!["postgres"]);
        assert_eq!(config.ports["http"], 8080);

        let ready = config.ready.unwrap();
        let http = ready.http.unwrap().get.unwrap();
        assert_eq!(http.port, 8080);
        assert_eq!(http.path, "/health");
        assert_eq!(ready.initial_delay, Duration::from_secs(2));
        assert_eq!(ready.period, Duration::from_secs(5));
        assert_eq!(ready.timeout, Some(Duration::from_secs(30)));
        assert_eq!(ready.failure_threshold, 3);

        assert_eq!(config.restart.on, RestartPolicy::OnFailure);
        assert_eq!(config.restart.max, Some(10));
        assert_eq!(config.restart.window, Some(Duration::from_secs(300)));

        assert_eq!(config.user.unwrap(), "kennel-myapp");
        assert_eq!(config.capabilities, vec!["CAP_NET_BIND_SERVICE"]);
    }

    #[test]
    fn deserialize_devenv_task_json() {
        let json = r#"{
            "name": "devenv:processes:api",
            "exec": "/nix/store/xyz-start-api",
            "after": ["devenv:processes:postgres"],
            "env": {},
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
            "ports": {"http": 8080},
            "watch": {"paths": [], "extensions": [], "ignore": []}
        }"#;

        let config: ProcessConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.name, "devenv:processes:api");
        assert_eq!(config.after, vec!["devenv:processes:postgres"]);

        let ready = config.ready.unwrap();
        assert!(!ready.notify);
        assert_eq!(ready.initial_delay, Duration::ZERO);
        assert_eq!(ready.period, Duration::from_secs(10));
        assert_eq!(ready.probe_timeout, Duration::from_secs(4));
        assert!(ready.timeout.is_none());
    }

    #[test]
    fn restart_policy_default() {
        let config: RestartConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.on, RestartPolicy::OnFailure);
        assert_eq!(config.max, Some(5));
        assert!(config.window.is_none());
    }

    #[test]
    fn restart_policy_never() {
        let json = r#"{"on": "never"}"#;
        let config: RestartConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.on, RestartPolicy::Never);
    }

    #[test]
    fn listen_spec_tcp() {
        let json = r#"{
            "name": "http",
            "kind": "tcp",
            "address": "127.0.0.1:8080",
            "backlog": 256
        }"#;

        let spec: ListenSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.name, "http");
        assert_eq!(spec.kind, ListenKind::Tcp);
        assert_eq!(spec.address.unwrap(), "127.0.0.1:8080");
        assert_eq!(spec.backlog, 256);
    }

    #[test]
    fn listen_spec_unix() {
        let json = r#"{
            "name": "control",
            "kind": "unix_stream",
            "path": "/run/myapp.sock"
        }"#;

        let spec: ListenSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.kind, ListenKind::UnixStream);
        assert_eq!(spec.path.unwrap(), PathBuf::from("/run/myapp.sock"));
        assert_eq!(spec.backlog, 128);
    }

    #[test]
    fn resource_limits() {
        let json = r#"{
            "memory_max": 536870912,
            "memory_high": 268435456,
            "cpu_max": 1.5,
            "cpu_weight": 100,
            "tasks_max": 256
        }"#;

        let limits: ResourceLimits = serde_json::from_str(json).unwrap();
        assert_eq!(limits.memory_max, Some(536870912));
        assert_eq!(limits.cpu_max, Some(1.5));
        assert_eq!(limits.tasks_max, Some(256));
    }

    #[test]
    fn config_round_trip() {
        let config = ProcessConfig {
            name: "test".into(),
            exec: "/bin/test".into(),
            args: vec![],
            cwd: None,
            env: HashMap::new(),
            after: vec![],
            listen: vec![],
            ports: HashMap::new(),
            ready: None,
            restart: RestartConfig::default(),
            watch: WatchConfig::default(),
            watchdog: None,
            resources: None,
            user: None,
            capabilities: vec![],
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: ProcessConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "test");
        assert_eq!(parsed.restart.on, RestartPolicy::OnFailure);
    }
}
