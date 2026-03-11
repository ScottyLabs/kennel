# RFC 0009: Supervisor Migration

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-03-10
- **Updated:** 2026-03-10

## Overview

Migrate Kennel from systemd-based process management to the native process supervisor (RFC 0007). This RFC specifies the changes to the deployer, router, database schema, startup reconciliation, and NixOS module required to complete the transition.

## Motivation

RFC 0007 defines the `kennel-supervisor` crate as a standalone library. This RFC describes how to wire it into Kennel's existing crate graph, replacing the systemd integration in `kennel-deployer`, the health polling in `kennel-router`, and the reconciliation logic in `kennel/src/reconcile.rs`. It also defines the database schema changes that follow from the supervisor owning all runtime process state.

## Goals

- Remove `systemd.rs` and `health.rs` from `kennel-deployer`
- Remove `health.rs` from `kennel-router`
- Replace the router's health polling with supervisor event subscription
- Simplify the `deployments` table and drop `port_allocations`
- Replace three-pass startup reconciliation with supervisor-based redeployment
- Update the NixOS module to remove systemd unit file write access

## Non-Goals

- Changing the webhook -> builder -> deployer pipeline (channels remain as-is)
- Modifying the builder crate or how project configurations are evaluated
- Changing static site deployment (symlinks are unaffected by the supervisor)

## Detailed Design

### kennel-deployer Changes

The following modules are deleted entirely:

- `systemd.rs` -- unit file generation and `systemctl` subprocess calls
- `health.rs` -- deployment-time health check with exponential backoff

**`service.rs`** shrinks. The deployment sequence becomes:

- Create system user
- Create working directory
- Allocate preview database if configured
- Generate secrets environment file
- Call `supervisor.start(process_config)` or `supervisor.blue_green_deploy(process_config, drain)` -- this single call replaces port allocation, unit file generation, four `systemctl` subprocess calls, and the health check polling loop
- Insert deployment record in database
- Create DNS records if configured

**`teardown.rs`** shrinks. The teardown sequence becomes:

- Call `supervisor.stop(name, grace)` -- replaces `systemctl stop`, `systemctl disable`, unit file removal, and `systemctl daemon-reload`
- Remove symlink for static sites
- Remove secrets file
- Release preview database
- Remove system user if no remaining deployments
- Delete DNS records
- Update database

The deployer receives an `Arc<Mutex<Supervisor>>` at construction time, shared with the teardown worker.

### kennel-router Changes

The `health.rs` module is deleted entirely. The router no longer polls backends. Instead, it subscribes to the supervisor's `broadcast::Receiver<SupervisorEvent>`:

```rust
async fn handle_supervisor_events(
    mut event_rx: broadcast::Receiver<SupervisorEvent>,
    routing_table: RoutingTable,
    deployment_map: Arc<RwLock<HashMap<String, DeploymentInfo>>>,
) {
    while let Ok(event) = event_rx.recv().await {
        match event {
            SupervisorEvent::ProcessReady { name, port, store_path } => {
                if let Some(info) = deployment_map.read().await.get(&name) {
                    match port {
                        Some(p) => routing_table.insert(
                            &info.domain, Route::service(p),
                        ).await,
                        None => routing_table.insert(
                            &info.domain,
                            Route::static_site(store_path.unwrap(), info.spa),
                        ).await,
                    }
                }
            }
            SupervisorEvent::ProcessUnhealthy { name }
            | SupervisorEvent::ProcessStopped { name } => {
                if let Some(info) = deployment_map.read().await.get(&name) {
                    routing_table.remove(&info.domain).await;
                }
            }
            SupervisorEvent::ProcessHealthy { name, port } => {
                if let Some(info) = deployment_map.read().await.get(&name) {
                    if let Some(p) = port {
                        routing_table.insert(
                            &info.domain, Route::service(p),
                        ).await;
                    }
                }
            }
            _ => {}
        }
    }
}
```

The periodic full-reload safety valve (every 60 seconds, querying all active deployments from the database) remains as a fallback for missed events.

### kennel (main binary) Changes

**`channels.rs`** -- The `router_update_tx`/`router_update_rx` broadcast channel is replaced by the supervisor's event channel. The deploy/teardown mpsc channels remain for the webhook -> builder -> deployer pipeline.

**`reconcile.rs`** -- The three-pass reconciliation (`reconcile_systemd_units`, `reconcile_port_allocations`, `reconcile_static_site_symlinks`) is replaced. With the supervisor owning runtime state, there are no systemd units or port allocations to diff. On startup, Kennel queries all deployments with status `deployed` and starts each one through the supervisor:

```rust
pub async fn reconcile_deployments(
    store: &Store,
    supervisor: &mut Supervisor,
) -> Result<()> {
    let active_deployments = store.deployments()
        .list_active_with_services()
        .await?;

    for deployment in active_deployments {
        let config = build_process_config_from_deployment(&deployment)?;
        supervisor.start(config).await?;
    }

    Ok(())
}
```

`reconcile_static_site_symlinks()` remains unchanged.

**`main.rs`** -- The startup sequence changes. The health monitor task is removed. The supervisor is initialized and shared with the deployer and teardown worker:

```rust
let supervisor = Arc::new(Mutex::new(Supervisor::new(event_tx)));

let builder_handle = tokio::spawn(run_worker_pool(build_rx, builder_config));
let deployer_handle = tokio::spawn(run_deployer(deploy_rx, deployer_config, supervisor.clone()));
let teardown_handle = tokio::spawn(run_teardown_worker(teardown_rx, deployer_config, supervisor.clone()));
let router_handle = tokio::spawn(run_router(router_config, event_rx));
```

### Database Schema

With the supervisor owning all runtime process state, the database's role shifts from "source of truth for what is running" to "persistent record of what should be deployed." The supervisor is the sole authority on whether a process is alive, healthy, or failed. The database stores declarative intent and history.

The `port_allocations` table is no longer needed. With socket activation, the supervisor binds sockets directly and reports the bound port via events. There is no allocation pool to manage or reconcile. The router learns ports from `SupervisorEvent::ProcessReady`.

The `deployments.status` column simplifies. The current six-state machine (`pending`, `building`, `active`, `failed`, `tearing_down`, `torn_down`) tracks runtime process lifecycle that now belongs to the supervisor. The deployment status reduces to `deployed` (the supervisor should be running this process) and `torn_down` (the deployment has been decommissioned). Fine-grained states (starting, ready, unhealthy, restarting) are queryable from the supervisor at runtime, not persisted.

The `deployments.port` column is removed. The port is a runtime property of the supervisor's bound socket, not a persistent deployment attribute. It can change across restarts.

A new `deployments.process_config` JSONB column stores the `ProcessConfig` used for each deployment. On restart, the supervisor reconstructs processes from these stored configs without re-evaluating the project's devenv configuration.

The migration adds the `process_config` column, drops the `port` column from `deployments`, drops the `port_allocations` table entirely, and replaces the `deployment_status` enum with the simplified two-value version (`deployed`, `torn_down`). Existing rows in `pending`, `building`, or `active` status map to `deployed`; all others map to `torn_down`.

### NixOS Module

The NixOS module (`nixos/default.nix`) changes:

- Remove write access to `/etc/systemd/system/`
- Remove `CAP_NET_BIND_SERVICE` unless Kennel needs to bind ports below 1024
- Add `CAP_SETUID` and `CAP_SETGID` for user switching in the supervisor's spawn hook
- Add `Delegate=yes` for cgroup v2 delegation

## Alternatives Considered

**Incremental migration with feature flag.** Keep both systemd and supervisor code paths, controlled by a configuration flag. This allows gradual rollout but doubles the maintenance surface and delays cleanup. A clean cutover is preferred since the supervisor is strictly more capable.

**Keep port_allocations as an audit table.** Retain the table for logging which ports have been used historically. This adds no value since the supervisor's event stream and deployment history provide the same information.

## Open Questions

None.

## Implementation Phases

### Database Migration

Create SeaORM migration to add `process_config` JSONB column, drop `port` column, drop `port_allocations` table, and simplify `deployment_status` enum. Regenerate entities.

### Deployer Refactor

Remove `systemd.rs` and `health.rs`. Update `service.rs` to accept and use the supervisor. Update `teardown.rs` to use `supervisor.stop()`. Update deployer config to include `Arc<Mutex<Supervisor>>`.

### Router Refactor

Remove `health.rs`. Replace `RouterUpdate` subscription with `SupervisorEvent` subscription. Update the routing table update handler.

### Reconciliation Simplification

Replace `reconcile_systemd_units()` and `reconcile_port_allocations()` with `reconcile_deployments()` that starts all `deployed` deployments through the supervisor.

### Main Binary Wiring

Update `main.rs` to initialize the supervisor, share it with deployer and teardown worker, and pass the event receiver to the router. Remove the health monitor task spawn.

### NixOS Module Update

Remove systemd unit file write access. Add `CAP_SETUID`/`CAP_SETGID`. Add `Delegate=yes`. Update directory permissions.

### Port Allocation Cleanup

Remove `kennel-store/src/port_allocations.rs` and all references. Remove port allocation constants from `kennel-config`. Update tests.
