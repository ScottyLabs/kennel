# RFC 0007: Process Supervision

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-03-10
- **Updated:** 2026-03-10

## Overview

Replace systemd-based process management with a Rust-native process supervisor built on [watchexec-supervisor](https://crates.io/crates/watchexec-supervisor). The supervisor manages process lifecycles directly -- spawning, readiness probes, restart policies, socket activation, dependency ordering, and event-driven health monitoring -- eliminating the split-brain reconciliation between database state and systemd state.

## Motivation

Kennel currently manages deployed services by generating systemd unit files, writing them to `/etc/systemd/system/`, and shelling out to `systemctl`. This introduces several categories of complexity:

**Split-brain state.** The database says a deployment is "active," but the actual process state lives in systemd. If a process crashes, systemd may restart it independently, or not. Kennel discovers this mismatch only through startup reconciliation or the router's health polling. The reconciliation logic in `reconcile.rs` runs three separate passes -- diffing systemd units, port allocations, and symlinks against the database -- because the two sources of truth can diverge after any unclean shutdown.

**Shelling out.** The deployer generates unit file content via string templating, writes it to disk, then calls `systemctl daemon-reload`, `systemctl enable`, and `systemctl start` as subprocesses. Each call can fail independently, and error handling relies on parsing subprocess output. A single service deployment involves four subprocess calls and a file write just for process management.

**Duplicated health monitoring.** The deployer runs a one-time health check with exponential backoff (1s, 2s, 4s, 8s, 15s) after starting a service. The router independently runs its own continuous health polling loop every 30 seconds, tracking consecutive failures per domain and removing routes after 3 failures. These are two separate implementations answering the same question: is this process alive and ready?

**Port allocation race.** The deployer allocates a port in the database, writes it into the unit file as an environment variable, and starts the process. Between allocation and the process binding, there is a window where another process could claim the port. Socket activation eliminates this race by binding the socket before spawning the child.

**Privilege requirements.** Writing to `/etc/systemd/system/` and calling `systemctl` requires root or polkit privileges. A userspace supervisor that directly spawns and monitors children can operate with fewer privileges.

## Goals

- Unified process state: the supervisor is the single source of truth for "what is running"
- Socket activation: bind sockets before spawning children, pass file descriptors
- Readiness probes: HTTP GET, exec (exit code), and systemd notify protocol
- Continuous liveness monitoring using the same probe configuration
- Restart policies: never, always, on-failure, with configurable limits and sliding windows
- Event-driven notifications to the router (no separate polling loop)
- Dependency ordering between services within a deployment
- Blue-green deployment as a first-class supervisor operation
- Simplified crash recovery: redeploy from database records, no reconciliation diffing
- Process group management for clean signal propagation

## Non-Goals

- Container runtime integration (Docker, podman, OCI images)
- Replacing systemd for Kennel itself (Kennel's own process is still a systemd service)
- Local development process management (that is devenv's responsibility)
- Windows or macOS support for the supervisor (production target is NixOS/Linux)
- Process output multiplexing or TUI (logs go to files, the dashboard reads them via API)
- Resource isolation

## Detailed Design

### Crate Structure

A new `kennel-supervisor` crate in `crates/kennel-supervisor/`. Dependencies:

| Crate | Purpose |
|---|---|
| `watchexec-supervisor` | Per-process job control (start, stop, restart, signal, state) |
| `process-wrap` | Composable spawn wrappers (process groups, sessions, signal mask reset) |
| `socket2` | Socket creation, binding, and FD management for socket activation |
| `signal-hook-tokio` | Async signal stream for SIGTERM, SIGCHLD |
| `nix` | Unix primitives (setuid, setgid, fcntl, dup2) |
| `tokio` | Async runtime, channels, timers |
| `reqwest` | HTTP readiness probes |
| `tracing` | Structured logging |

The crate does not depend on `kennel-store` or any other Kennel crate. It is a standalone process supervision library that communicates with the rest of Kennel via its event channel.

### Process Configuration

The `ProcessConfig` type describes everything the supervisor needs to manage a process. This schema is designed to be deserializable from devenv's task configuration JSON, but the supervisor itself is agnostic to the config source.

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ProcessConfig {
    /// Unique name for this process within its deployment.
    pub name: String,

    /// Command to execute. Absolute path to a binary (typically a Nix store path).
    pub exec: String,

    /// Command arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// Working directory. If None, inherits from the supervisor.
    pub cwd: Option<PathBuf>,

    /// Environment variables for this process.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Processes that must be ready before this one starts.
    #[serde(default)]
    pub after: Vec<String>,

    /// Socket activation specifications. The supervisor binds these sockets
    /// before spawning the process and passes the file descriptors.
    #[serde(default)]
    pub listen: Vec<ListenSpec>,

    /// Named ports. If no listen spec is provided, the supervisor allocates
    /// a port from the pool and sets it as an environment variable.
    #[serde(default)]
    pub ports: HashMap<String, u16>,

    /// Readiness probe configuration. If None, the process is considered
    /// ready immediately after spawning.
    pub ready: Option<ReadyConfig>,

    /// Restart policy.
    #[serde(default)]
    pub restart: RestartConfig,

    /// File watch configuration. When watched files change, the process
    /// is restarted.
    #[serde(default)]
    pub watch: WatchConfig,

    /// Watchdog configuration. The process must send periodic heartbeats
    /// or it is considered unhealthy.
    pub watchdog: Option<WatchdogConfig>,

    /// Unix user to run the process as. If None, inherits from the supervisor.
    pub user: Option<String>,

    /// Linux ambient capabilities to grant the process.
    #[serde(default)]
    pub capabilities: Vec<String>,
}
```

#### Readiness Probes

Three probe types are supported. Only one may be active per process.

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ReadyConfig {
    /// Shell command to execute. Exit code 0 means ready.
    pub exec: Option<String>,

    /// HTTP GET probe.
    pub http: Option<HttpProbe>,

    /// Systemd notify protocol. The process sends READY=1 to $NOTIFY_SOCKET.
    #[serde(default)]
    pub notify: bool,

    /// Delay before the first probe attempt.
    #[serde(default = "default_initial_delay")]
    pub initial_delay: Duration,

    /// Interval between probe attempts.
    #[serde(default = "default_period")]
    pub period: Duration,

    /// Timeout for a single probe attempt.
    #[serde(default = "default_probe_timeout")]
    pub probe_timeout: Duration,

    /// Overall timeout. If the process is not ready within this duration,
    /// it is considered failed. None means no overall timeout.
    pub timeout: Option<Duration>,

    /// Consecutive successes required to transition to ready.
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,

    /// Consecutive failures required to transition to unhealthy.
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpProbe {
    #[serde(default = "default_probe_host")]
    pub host: String,
    pub port: u16,
    #[serde(default = "default_probe_path")]
    pub path: String,
    #[serde(default = "default_probe_scheme")]
    pub scheme: String,
}
```

Defaults: `initial_delay` = 0s, `period` = 10s, `probe_timeout` = 4s, `success_threshold` = 1, `failure_threshold` = 5.

The same probe configuration serves dual purpose. During startup, it gates dependency ordering and deployment activation (blocking until the threshold is met). After the process is ready, the supervisor continues running the probe at the configured `period` for continuous liveness monitoring. If `failure_threshold` consecutive failures occur, the supervisor emits `ProcessUnhealthy`.

#### Restart Policies

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct RestartConfig {
    #[serde(default = "default_restart_on")]
    pub on: RestartPolicy,

    /// Maximum restarts within the sliding window. None means unlimited.
    pub max: Option<u32>,

    /// Sliding window duration for counting restarts. None means the count
    /// never resets.
    pub window: Option<Duration>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    Never,
    Always,
    #[default]
    OnFailure,
}
```

Default: `on` = `on_failure`, `max` = 5, `window` = None.

When a process exits, the supervisor checks the restart policy. For `OnFailure`, only non-zero exit codes trigger a restart. The sliding window resets the restart counter if `window` has elapsed since the first restart, allowing processes that crash occasionally but run stably between crashes.

#### Socket Activation

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ListenSpec {
    pub name: String,
    #[serde(default)]
    pub kind: ListenKind,
    pub address: Option<String>,
    pub path: Option<PathBuf>,
    #[serde(default = "default_backlog")]
    pub backlog: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ListenKind {
    #[default]
    Tcp,
    UnixStream,
}
```

The supervisor binds sockets before spawning the child process using `socket2`. It clears `CLOEXEC` on the bound FDs so they survive exec, then sets `LISTEN_FDS` and `LISTEN_PID` in the child's environment following the [systemd socket activation protocol](https://www.freedesktop.org/software/systemd/man/latest/sd_listen_fds.html). FDs are passed starting at file descriptor 3.

#### File Watch and Watchdog

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WatchConfig {
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WatchdogConfig {
    /// The process must send a heartbeat within this interval.
    /// Passed to the process as $WATCHDOG_USEC.
    pub usec: u64,
    #[serde(default)]
    pub require_ready: bool,
}
```

### Supervisor Core

```rust
pub struct Supervisor {
    processes: HashMap<String, ManagedProcess>,
    event_tx: broadcast::Sender<SupervisorEvent>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

struct ManagedProcess {
    config: ProcessConfig,
    job: watchexec_supervisor::Job,
    join_handle: tokio::task::JoinHandle<()>,
    state: ProcessState,
    bound_sockets: Vec<BoundSocket>,
    notify_socket: Option<PathBuf>,
}

struct BoundSocket {
    fd: std::os::unix::io::RawFd,
    address: std::net::SocketAddr,
    _socket: socket2::Socket,
}
```

#### Process States

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessState {
    Pending,
    Starting,
    Ready,
    Running,
    Unhealthy,
    Restarting { attempt: u32 },
    Failed { error: String, restarts: u32 },
    Stopping,
    Stopped,
}
```

Transitions: `Pending` -> `Starting` (spawned, probe running) -> `Ready` (probe passed) or `Failed` (probe timed out). From `Ready`, a process can transition to `Unhealthy` (liveness probe failing), `Stopping` (graceful shutdown), or back to `Starting` via `Restarting` (process exited, restart policy triggered). `Unhealthy` -> `Ready` on probe recovery. `Stopping` -> `Stopped` is terminal.

#### Supervisor Events

The event channel replaces both the `RouterUpdate` broadcast channel and the router's health polling loop.

```rust
#[derive(Debug, Clone)]
pub enum SupervisorEvent {
    ProcessReady { name: String, port: Option<u16>, store_path: Option<String> },
    ProcessUnhealthy { name: String },
    ProcessHealthy { name: String, port: Option<u16> },
    ProcessRestarting { name: String, attempt: u32 },
    ProcessStopped { name: String },
    ProcessFailed { name: String, error: String },
}
```

### Supervisor API

```rust
impl Supervisor {
    pub fn new(event_tx: broadcast::Sender<SupervisorEvent>) -> Self;

    /// Spawn a process. Binds sockets, sets up the spawn hook (env, user
    /// switching, FD passing), starts the watchexec job, and spawns the
    /// supervision task (readiness probe, liveness monitoring, restart loop).
    pub async fn start(&mut self, config: ProcessConfig) -> Result<()>;

    /// Stop a process. Sends SIGTERM, waits up to `grace`, then SIGKILL.
    /// Releases bound sockets and cleans up the notify socket.
    pub async fn stop(&mut self, name: &str, grace: Duration) -> Result<()>;

    /// Stop all processes in reverse dependency order.
    pub async fn stop_all(&mut self, grace: Duration) -> Result<()>;

    /// Start all processes respecting dependency order. Topologically sorts
    /// by `after` fields, starts each batch concurrently, and waits for all
    /// processes in a batch to pass their readiness probes before starting
    /// the next batch.
    pub async fn start_all(&mut self, configs: Vec<ProcessConfig>) -> Result<()>;

    /// Blue-green deployment. Starts the new process, waits for readiness,
    /// drains for `drain_period`, then stops the old process. On failure,
    /// restores the old process.
    pub async fn blue_green_deploy(
        &mut self,
        new_config: ProcessConfig,
        drain_period: Duration,
    ) -> Result<()>;
}
```

### Spawn Hook

Each process is spawned via `watchexec_supervisor::start_job` with a spawn hook that configures the child before exec:

- Set environment variables from `ProcessConfig.env`
- Set `LISTEN_FDS` and clear `CLOEXEC` on socket FDs for socket activation
- Set `NOTIFY_SOCKET` if using notify readiness
- Call `setgid`/`setuid` in `pre_exec` if `ProcessConfig.user` is set
- Set `WATCHDOG_USEC` if watchdog is configured

Process group management is handled by `watchexec_supervisor::SpawnOptions { grouped: true, reset_sigmask: true }`, ensuring clean signal propagation and a reset signal mask in the child.

### Supervision Task

Each started process gets a background tokio task that manages its lifecycle:

- Runs the initial readiness probe (blocks until threshold met or timeout)
- Emits `ProcessReady` on success or `ProcessFailed` on timeout
- Enters a `select!` loop monitoring three sources:
  - **Process exit** (`job.to_wait()`): checks restart policy, emits `ProcessRestarting` or `ProcessStopped`
  - **Liveness probe tick**: runs the same probe as readiness, emits `ProcessUnhealthy`/`ProcessHealthy` on state changes
  - **Shutdown signal**: sends SIGTERM and emits `ProcessStopped`

After a restart, the readiness probe runs again before the process is considered ready.

### Security Considerations

**User switching.** The supervisor uses `setuid`/`setgid` in a `pre_exec` hook to run each process as a dedicated system user (e.g., `kennel-myproject`). The Kennel process itself must run as root or have `CAP_SETUID`/`CAP_SETGID`.

**Process groups.** Each supervised process runs in its own process group (`grouped: true`). When the supervisor sends SIGTERM, it goes to the entire group, preventing orphaned child processes.

**File descriptor leakage.** Socket activation requires clearing `CLOEXEC` on specific FDs. All other FDs retain `CLOEXEC` (the default for `socket2::Socket::new`). The spawn hook only clears `CLOEXEC` on the intended listener FDs.

**Signal mask.** `reset_sigmask: true` resets the inherited signal mask before exec. This prevents the supervisor's signal handling from affecting child processes.

**Crash recovery.** If Kennel crashes, supervised processes become orphans re-parented to PID 1. They continue running but are no longer monitored. When Kennel restarts, it redeploys from database state. During the gap between crash and restart, orphaned processes serve traffic but are not health-checked. The NixOS module's `Restart=on-failure` for Kennel's own service limits this gap to seconds.

## Alternatives Considered

**Keep systemd, use D-Bus instead of shelling out.** Use systemd's D-Bus API (via the `zbus` crate) instead of subprocess calls to `systemctl`. This eliminates subprocess overhead but retains the split-brain problem and duplicated health checks. The fundamental architecture does not change.

**Embed devenv-processes.** Vendor the `devenv-processes` crate and its internal dependencies (`devenv-activity`, `devenv-event-sources`) from the devenv repository. This gives a battle-tested supervisor but couples Kennel to devenv's internal API surface, which has no stability guarantees.

**Use containers (podman/Docker).** Run each deployed service in a container for stronger isolation. This adds significant complexity (image building, registry, container runtime dependency) and does not align with the Nix-native deployment model where services are store paths, not images.

**Fork devenv-processes into a standalone crate.** Extract `devenv-processes` into an independent crate and publish it. This requires upstream buy-in from the devenv team. Building on the same primitives (watchexec-supervisor, socket2) without the devenv-specific coupling is more practical.

## Open Questions

- **Orphaned process cleanup.** When Kennel restarts after a crash, orphaned processes from the previous run may still be listening on their ports. Should Kennel attempt to kill processes by PID file before redeploying, or let the socket bind fail and retry?

- **Log capture.** The current systemd integration relies on journald for log capture. With a userspace supervisor, Kennel must capture stdout/stderr itself. Should it pipe output to log files, or use a PTY for terminal-aware capture? Log files are simpler but lose terminal formatting.

## Implementation Phases

### Crate Skeleton and Config Types

Create `crates/kennel-supervisor/` with `cargo init`. Define all config types (`ProcessConfig`, `ReadyConfig`, `RestartConfig`, `ListenSpec`, `WatchConfig`, `WatchdogConfig`), event types (`SupervisorEvent`), and state types (`ProcessState`). Add serde deserialization. Write unit tests for config parsing.

### Core Supervisor Lifecycle

Implement `Supervisor::start()`, `Supervisor::stop()`, and `Supervisor::stop_all()`. Integrate `watchexec-supervisor` for job control. Implement spawn hooks for environment variables, working directory, and user switching. Write integration tests that start and stop processes.

### Readiness Probes

Implement HTTP GET, exec, and notify probe types. Implement the readiness probe runner with thresholds and timeouts. Write tests for each probe type using a mock HTTP server and simple scripts.

### Supervision Task and Restart Logic

Implement the per-process supervision task with the `select!` loop. Implement restart policies (never, always, on-failure) with sliding window support. Implement continuous liveness monitoring. Write tests for restart behavior and liveness detection.

### Socket Activation

Implement socket binding with CLOEXEC clearing. Implement FD passing via `LISTEN_FDS` environment variable following the systemd socket activation protocol. Write tests that verify a child process can accept connections on an inherited socket.

### Dependency Ordering

Implement topological sort over `after` fields and `start_all()`. Implement `wait_for_ready()` for blocking on dependency readiness. Write tests with multi-process dependency chains and cycle detection.

### Blue-Green Deployment

Implement `blue_green_deploy()` with the rename-start-wait-drain-stop sequence. Write tests that verify zero-downtime transitions and rollback on failure.
