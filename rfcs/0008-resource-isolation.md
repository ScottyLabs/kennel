# RFC 0008: Resource Isolation

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-03-10
- **Updated:** 2026-03-10

## Overview

Enforce per-process resource limits and expose live resource usage metrics via Linux cgroup v2, integrated into the process supervisor (RFC 0007) as a composable `process-wrap` wrapper.

## Motivation

With the supervisor replacing systemd for process management (RFC 0007), Kennel loses systemd's automatic cgroup isolation. Without resource limits, a single misbehaving deployment can consume all available memory or CPU, impacting every other deployment on the host. Resource isolation is not optional -- it is a requirement for running untrusted branch deployments on shared infrastructure.

Beyond isolation, cgroup v2 exposes live resource usage metrics (memory, CPU, task count) as files in the cgroup directory. The supervisor can read these on demand and expose them through the API, giving the dashboard real-time per-deployment resource visibility that Kennel currently lacks entirely.

## Goals

- Per-process memory limits (hard cap and soft throttle threshold)
- Per-process CPU limits (bandwidth cap and relative weight)
- Per-process task count limits (threads + child processes)
- Live resource usage metrics readable by the supervisor
- Integration with the supervisor's `process-wrap` wrapper stack
- NixOS module support for cgroup delegation

## Non-Goals

- IO bandwidth limiting (not needed for the target workloads)
- Network bandwidth limiting (not supported by cgroup v2 directly)
- Per-project aggregate limits (each process is isolated independently)
- Dynamic limit adjustment at runtime (limits are set at spawn time)

## Detailed Design

### Configuration

Resource limits are specified per process via an optional `resources` field on `ProcessConfig` (RFC 0007):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ResourceLimits {
    /// Hard memory limit in bytes. Maps to cgroup v2 memory.max.
    /// The kernel OOM-kills the process if it exceeds this.
    pub memory_max: Option<u64>,

    /// Soft memory threshold in bytes. Maps to cgroup v2 memory.high.
    /// The kernel throttles allocations above this before hitting the
    /// hard limit.
    pub memory_high: Option<u64>,

    /// CPU bandwidth limit as a fraction (e.g., 1.5 = 150% of one core).
    /// Maps to cgroup v2 cpu.max as "$MAX 100000" microseconds.
    pub cpu_max: Option<f64>,

    /// CPU weight (1-10000, default 100). Maps to cgroup v2 cpu.weight.
    /// Controls relative CPU time when cores are contended.
    pub cpu_weight: Option<u32>,

    /// Maximum number of tasks (threads + processes). Maps to cgroup v2
    /// pids.max.
    pub tasks_max: Option<u64>,
}
```

### Cgroup Wrapper

Resource isolation is implemented as a `TokioCommandWrapper` for `process-wrap`, composing with the existing spawn wrapper stack (process groups, sessions, signal mask reset).

```rust
struct CgroupWrapper {
    cgroup_path: PathBuf,
    limits: ResourceLimits,
}
```

The wrapper creates a cgroup subtree under `/sys/fs/cgroup/kennel/<process-name>/`, writes resource limits to the appropriate control files, and moves the child PID into the cgroup after spawn via `post_spawn`.

Cgroup control files written:

| `ResourceLimits` field | cgroup v2 file | Format |
|---|---|---|
| `memory_max` | `memory.max` | bytes as decimal string |
| `memory_high` | `memory.high` | bytes as decimal string |
| `cpu_max` | `cpu.max` | `"$MAX_USEC 100000"` |
| `cpu_weight` | `cpu.weight` | integer 1-10000 |
| `tasks_max` | `pids.max` | integer or `"max"` |

The cgroup directory is removed when the process stops and no PIDs remain.

### Live Resource Metrics

cgroup v2 exposes usage counters as files in the cgroup directory. The supervisor reads these on demand:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct CgroupStats {
    /// Current memory usage in bytes.
    pub memory_current: u64,
    /// Peak memory usage in bytes since cgroup creation.
    pub memory_peak: u64,
    /// Cumulative CPU time in microseconds.
    pub cpu_usage_usec: u64,
    /// Current number of tasks (threads + processes).
    pub pids_current: u64,
}
```

Source files:

| Field | cgroup v2 file |
|---|---|
| `memory_current` | `memory.current` |
| `memory_peak` | `memory.peak` |
| `cpu_usage_usec` | `cpu.stat` (parse `usage_usec` line) |
| `pids_current` | `pids.current` |

The supervisor exposes a `pub fn stats(&self, name: &str) -> Result<CgroupStats>` method. The API can query this for any supervised process, and the dashboard can display live resource usage per deployment.

### NixOS Module

For cgroup v2 delegation to work, the Kennel process must own the `/sys/fs/cgroup/kennel/` subtree. The NixOS module configures this by setting `Delegate=yes` on Kennel's systemd service unit:

```nix
systemd.services.kennel = {
  serviceConfig = {
    Delegate = "yes";
    # ...existing config...
  };
};
```

This grants Kennel permission to create and manage child cgroups within its delegated subtree. Kennel creates `/sys/fs/cgroup/kennel/` on startup if it does not exist.

## Alternatives Considered

**`systemd-run --scope`.** Place each process in a transient systemd scope for cgroup limits without full unit file management. This avoids direct cgroup manipulation but reintroduces a systemd dependency and subprocess calls, partially defeating the purpose of the supervisor.

**`cgroups-rs` crate.** Use an existing Rust crate for cgroup management instead of writing to the filesystem directly. The `cgroups-rs` crate exists but adds a dependency for what amounts to writing strings to files. The cgroup v2 filesystem API is simple enough to use directly.

**No resource limits.** Accept that branch preview deployments are trusted enough to run without limits. This is insufficient for shared infrastructure where a single deployment can starve all others.

## Open Questions

- **Default limits.** Should Kennel apply default resource limits to all processes that do not specify them? If so, what defaults are reasonable (e.g., 512MB memory, 1 CPU core, 256 tasks)?

## Implementation Phases

### Cgroup Wrapper

Implement `CgroupWrapper` as a `TokioCommandWrapper`. Create cgroup subtrees, write resource limit files, move child PIDs in `post_spawn`. Implement cgroup cleanup on process stop. Write tests that verify limits are written correctly.

### Resource Metrics

Implement `read_stats()` to parse cgroup v2 usage files. Add `Supervisor::stats()` method. Write tests that verify metrics are readable after process start.

### Limit Enforcement Tests

Write integration tests that verify memory and CPU limits are enforced (e.g., a process that allocates beyond `memory_max` is OOM-killed, a process that exceeds `cpu_max` is throttled).

### NixOS Module Update

Add `Delegate=yes` to Kennel's systemd service unit. Create `/sys/fs/cgroup/kennel/` on startup. Write a NixOS test that verifies cgroup delegation works.
