# RFC 0010: devenv Integration

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-03-10
- **Updated:** 2026-03-10

## Overview

Integrate Kennel with [devenv](https://devenv.sh)'s process configuration system, allowing developers to define service processes once in `devenv.nix` and have Kennel consume those definitions for production deployment. This replaces most of `kennel.toml` with devenv's richer process model and establishes parity between local development (`devenv up`) and production (Kennel).

## Motivation

Developers currently maintain two separate configurations for how their services run:

- **`devenv.nix`** defines processes for local development, including readiness probes, restart policies, ports, and dependencies between services
- **`kennel.toml`** defines services for production deployment, with a much thinner model (just a flake output, health check path, and timeout)

This duplication leads to configuration drift. A developer adds a readiness probe in `devenv.nix` but forgets to update the health check path in `kennel.toml`. Or they add a dependency between services locally but Kennel has no way to express that, so production starts services in arbitrary order.

devenv 2.0's process configuration is strictly richer than `kennel.toml`'s service model. It supports the full set of features the supervisor (RFC 0007) can consume: readiness probes (HTTP, exec, notify), restart policies, socket activation, dependency ordering, port allocation, and file watching. Rather than duplicating this expressiveness in `kennel.toml`, Kennel can evaluate the project's devenv configuration directly.

## Goals

- Evaluate devenv process configurations during the build phase
- Consume devenv's `tasks.json` as the process configuration source
- Simplify `kennel.toml` to Kennel-specific metadata only
- Support configuration merging (devenv process config + Kennel deployment metadata)

## Non-Goals

- Embedding the Nix evaluator in Kennel (use `nix build` CLI)
- Managing devenv's built-in services (postgres, redis) in production -- these are infrastructure, not application services
- Supporting non-flake devenv projects (only flake-parts integration)

## Detailed Design

### devenv Process Configuration Schema

devenv 2.0 defines processes in the `processes` attribute of `devenv.nix`. Each process supports:

```nix
processes.api = {
  exec = "${pkgs.myapp}/bin/api";
  after = [ "devenv:processes:postgres" ];
  ports.http.allocate = 8080;
  ready.http.get = {
    port = config.processes.api.ports.http.value;
    path = "/health";
  };
  restart = {
    on = "on_failure";
    max = 5;
  };
  watch = {
    paths = [ ./src ];
    extensions = [ "rs" ];
  };
  listen = [{
    name = "http";
    kind = "tcp";
    address = "127.0.0.1:8080";
  }];
  env = {
    RUST_LOG = "debug";
  };
};
```

These process definitions are compiled into a `tasks.json` file as part of devenv's Nix evaluation. The JSON contains all process definitions with their full configuration, including the `exec` command (a Nix store path), probes, restart policy, ports, and dependencies.

### Configuration Evaluation Path

For projects using flake-parts (the standard devenv-with-flakes pattern), Kennel builds the task configuration as a Nix derivation:

```bash
nix build .#devenv.shells.default.config.task.config --no-link --print-out-paths
```

This outputs a Nix store path containing `tasks.json`. Kennel already runs `nix build` for each service during the build phase -- this is one additional attribute to evaluate.

The resulting JSON contains an array of task objects. Process tasks have a `process` field containing the full process configuration:

```json
[
  {
    "name": "devenv:processes:api",
    "type": "process",
    "command": "/nix/store/...-start-api",
    "after": ["devenv:processes:postgres"],
    "env": {},
    "cwd": null,
    "process": {
      "start": { "enable": true },
      "ready": {
        "http": {
          "get": { "host": "127.0.0.1", "port": 8080, "path": "/health", "scheme": "http" }
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
      "restart": { "on": "on_failure", "max": 5, "window": null },
      "listen": [],
      "ports": { "http": 8080 },
      "watch": { "paths": [], "extensions": [], "ignore": [] },
      "watchdog": null
    }
  }
]
```

This schema maps directly to the supervisor's `ProcessConfig` type (RFC 0007). Kennel deserializes it, strips the `devenv:processes:` prefix from names, and passes the configs to the supervisor.

### Port Allocation in Evaluation

devenv uses a custom Nix primop (`allocatePort`) for dynamic port allocation during evaluation. When Kennel evaluates the config via `nix build`, this primop is not available. Ports fall back to their base values (the number passed to `ports.<name>.allocate`).

This is acceptable because the supervisor does its own socket activation (RFC 0007). The port value from devenv serves as a hint for the bind address. If the port is already in use, the supervisor can bind to port 0 and let the OS assign one.

### kennel.toml Simplification

With process configuration coming from devenv, `kennel.toml` shrinks to Kennel-specific deployment metadata:

```toml
[project]
name = "myapp"

[cachix]
cache_name = "myorg"

# Kennel-specific metadata per service.
# Process config (exec, probes, restart, ports, dependencies) comes from devenv.
[services.api]
custom_domain = "api.myapp.com"
preview_database = true

[static_sites.docs]
flake_output = "packages.x86_64-linux.docs"
custom_domain = "docs.myapp.com"
spa = true
```

Fields removed from `[services.*]` (now in devenv):

- `flake_output` -- the `exec` field in devenv's process config points to the built binary
- `health_check_path` -- replaced by devenv's `ready.http.get.path`
- `health_check_timeout_secs` -- replaced by devenv's `ready.timeout`
- `env` -- replaced by devenv's process `env` (with Kennel-specific overrides merged at deploy time)
- `secrets` -- remains in `kennel.toml` (secrets are a deployment concern, not a dev concern)

Static sites remain in `kennel.toml` because they are not processes and have no devenv process definition.

### Configuration Merging

The deployer merges devenv process config with Kennel deployment metadata via a standalone `merge_config` function:

- **Base**: `ProcessConfig` deserialized from devenv's `tasks.json`
- **Overlay**: process name (Kennel-namespaced), system user, working directory, environment classification
- **Resource URLs**: injected separately by resource providers after the merge

```rust
fn merge_config(
    devenv_config: &ProcessConfig,
    process_name: &str,
    username: &str,
    cwd: PathBuf,
    git_ref: &str,
) -> ProcessConfig {
    let mut config = devenv_config.clone();
    config.name = process_name.to_string();
    config.user = Some(username.to_string());
    config.cwd = Some(cwd);
    config
        .env
        .insert("ENVIRONMENT".into(), determine_environment(git_ref));
    config
}
```

Resource-specific environment variables (`DATABASE_URL`, `VALKEY_URL`, `S3_ENDPOINT`, etc.) are injected by the infrastructure provisioning system after the merge, not by `merge_config` itself.

### Builder Changes

The builder gains a new step: evaluating the devenv task configuration. After cloning the repository at the target commit:

```rust
async fn process_build(build_id: i32, config: &BuilderConfig) -> Result<()> {
    // ...existing steps: clone, checkout...

    // Evaluate devenv task configuration.
    let task_config_path = nix_build(
        &work_dir,
        "devenv.shells.default.config.task.config",
    ).await?;

    let tasks: Vec<TaskConfig> = serde_json::from_str(
        &tokio::fs::read_to_string(task_config_path).await?,
    )?;

    // Filter to process-type tasks (not one-shot tasks).
    let process_configs: Vec<ProcessConfig> = tasks
        .into_iter()
        .filter(|t| t.task_type == "process")
        .map(|t| t.into_process_config())
        .collect();

    // Build each process's exec derivation.
    for config in &process_configs {
        nix_build(&work_dir, &config.exec).await?;
    }

    // ...existing steps: cachix push, send deployment request...
    // The deployment request now includes the process configs.
    deploy_tx.send(DeploymentRequest {
        build_id,
        process_configs,
        kennel_config,
    }).await?;

    Ok(())
}
```

The `DeploymentRequest` struct gains a `process_configs: Vec<ProcessConfig>` field. The deployer uses these directly with the supervisor instead of constructing configs from `kennel.toml`.

### Service Discovery

devenv process names are prefixed with `devenv:processes:` (e.g., `devenv:processes:api`). Kennel strips this prefix when mapping to deployment names. The `after` field references are also rewritten to strip prefixes, so dependency ordering works within the supervisor.

Built-in devenv services (e.g., `devenv:processes:postgres`) are filtered out. These are infrastructure services managed by the deployment environment, not application services managed by Kennel.

## Alternatives Considered

**Extend kennel.toml to match devenv's expressiveness.** Add readiness probes, restart policies, dependency ordering, and socket activation to `kennel.toml`. This eliminates the devenv dependency but requires developers to maintain two equivalent configurations and ensures they will drift.

**Embed the Nix evaluator.** Link against the Nix evaluator C API (as devenv does via `nix-bindings-expr`) to evaluate configurations without subprocess calls. This adds significant build complexity, a C dependency, and tight coupling to a specific Nix version. Tvix (a Rust Nix evaluator) would avoid the C dependency, but it has no flake support, no fetcher builtins (`fetchGit`/`fetchTree` are unimplemented), and no build capability -- all required for evaluating devenv configurations.

**Read devenv.lock and bootstrap manually.** Replicate devenv's bootstrap evaluation by constructing the Nix expression that imports `bootstrap/default.nix` with the correct arguments. This requires resolving `devenv.lock` and providing runtime parameters (system, hostname, username, etc.) that are not relevant to production. The `nix build` path through flake-parts is simpler and more stable.

## Open Questions

- **Multiple devenv shells.** Projects may define multiple devenv shells (e.g., `default`, `ci`). Which shell should Kennel evaluate? Default to `default`, allow override in `kennel.toml`?

- **Service filtering.** Should `kennel.toml` explicitly list which devenv processes Kennel should deploy, or should it deploy all non-infrastructure processes by default?

## Implementation Phases

### Task Config Deserialization

Define Rust types for devenv's `tasks.json` schema (`TaskConfig`, mapping to `ProcessConfig`). Write unit tests for deserialization from sample JSON.

### Builder Integration

Add devenv task config evaluation to the build pipeline. Implement `nix build` for the task config attribute. Update `DeploymentRequest` to carry process configs.

### kennel.toml Simplification

Remove process-related fields from `ServiceConfig` (`flake_output`, `health_check_path`, `health_check_timeout_secs`). Keep Kennel-specific metadata (`custom_domain`, `secrets`). Update config parsing and validation.

### Configuration Merging

Implement `merge_config()` in the deployer. Inject deployment-specific environment variables, system user, and preview database URLs into the devenv-provided `ProcessConfig`.

### Service Discovery and Filtering

Implement devenv process name prefix stripping. Implement built-in service filtering (exclude `postgres`, `redis`, etc.). Implement dependency rewriting for `after` fields.

### Documentation

Update the docs site with the new configuration model. Add a migration guide for existing projects moving from `kennel.toml`-only to devenv integration.
