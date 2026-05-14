# Architecture

Kennel runs as a single binary with four main responsibilities: receiving webhooks, building Nix packages, deploying them, and reconciling desired state against actual state.

## Request flow

```
Git push -> Webhook -> Build (nix) -> Deploy (systemd + Caddy) -> Live
```

1. Forgejo sends a webhook to kennel's `/webhook` endpoint.
1. Kennel parses the repository name from the payload, verifies the HMAC signature, and creates a build record.
1. The build worker clones the repo, runs `devenv build scottylabs.kennel.config` to discover declared services and sites, then runs `nix build` for each package. Every subprocess (git, devenv, nix, cachix) streams its stdout and stderr line-by-line through structured tracing, so the build log shows up in journald (and downstream Loki) labelled by `build_id` and `phase`. The full per-phase log is also persisted to the `builds.log` column for later retrieval.
1. The reconciler picks up the completed build, provisions resources (database, cache, storage), resolves secrets from OpenBao, starts a systemd transient unit for services, and adds a Caddy route for each deployment.
1. Caddy serves traffic over HTTPS with on-demand TLS.

## Delegation

Kennel delegates process supervision to systemd and HTTP routing to Caddy, keeping the core focused on build orchestration and resource provisioning.

Systemd transient units are created via D-Bus using the zbus crate. Units are placed in the `kennel.slice` cgroup for aggregate accounting, with `CPUAccounting`, `MemoryAccounting`, `IOAccounting`, and `TasksAccounting` enabled so per-deployment resource usage is queryable from cgroup metrics by anything scraping `systemd_unit_*` or `systemd_slice_*` (e.g. `prometheus-systemd-exporter` filtered to `kennel-*` units). Transient units survive kennel crashes since they are independent of the kennel process.

Caddy routes are managed via the [admin API](https://caddyserver.com/docs/api). Each deployment gets a route identified by `@id` for individual add/remove operations. Caddy handles TLS certificate provisioning, HTTP/3, static file serving, reverse proxying, and SPA fallback.

## HTTP API

Kennel exposes a small set of HTTP endpoints alongside the webhook receiver:

| Method | Path | Purpose |
| ------ | ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| POST | `/webhook` | Git push and pull request events from Forgejo, HMAC-verified. |
| GET | `/metrics` | Prometheus exposition: `kennel_builds{status=...}`, `kennel_deployments`, `kennel_projects` gauges. |
| GET | `/builds/:id/log` | Plaintext concatenation of every subprocess's output captured during the build, with `=== phase: <name> ===` separators. |
| GET | `/deployments/:id/logs` | journald output for the deployment's systemd unit. Query params: `?follow=true` for chunked live tail, `?lines=N&since=...`. |
| GET | `/deployments/:id/health` | JSON: `active`, `active_state`, `sub_state`, `active_enter_usec`, `n_restarts` from the unit's D-Bus properties. |
| GET | `/internal/caddy/check-domain` | Used by Caddy's on-demand TLS to validate a hostname is a registered deployment before acquiring a cert. |

All endpoints other than `/webhook` are unauthenticated and read-only. Caddy's `services.kennel.domain` virtualhost reverse-proxies these to the kennel API server, which only listens on localhost; the trust boundary is the host firewall plus tailnet, not application-level auth.

Routes are mounted in `http.rs`; per-resource handlers live under `handlers/`.

## Reconciliation

A single reconciliation loop handles all deployment convergence. It runs on startup, when signaled by a webhook or build completion, and on a periodic 30-second timer.

The reconciler compares desired state (deployment rows in the database) against actual state (systemd units and Caddy routes) and converges:

- A deployment row with no running unit gets its unit started.
- A running unit with no deployment row gets stopped.
- All Caddy routes are re-added on each pass since Caddy config is ephemeral.

There are no intermediate deployment states like "deploying" or "tearing down" that could get stuck. A deployment either has a row in the database or it doesn't, which eliminates stuck-state bugs by construction.

## State

Kennel stores state in SQLite with three tables:

- `projects` -- registered repositories with webhook secrets
- `builds` -- build queue and history (queued, building, built, done, failed, cancelled), plus the captured per-phase `log` of subprocess output
- `deployments` -- active deployments with store paths, domains, unit names, and ports

Runtime process state (running, stopped, failed) is owned by systemd and queried via D-Bus. Routing state is owned by Caddy and queried via the admin API. Kennel's database only tracks intent plus the historical build artifacts (logs) systemd doesn't keep.

## OIDC client reconciliation

For services declaring `oidc.redirectPaths`, kennel keeps a pair of Keycloak confidential clients in sync per project: `{slug}` for prod and `{slug}-staging` for staging. On each deploy of a service with OIDC, kennel calls Keycloak's admin API to ensure the client exists with the correct `valid_redirect_uris` (kennel-default URL + `customDomain` if set for prod; kennel-default URL for staging). PR-preview URLs are added to the staging client on PR open and removed on PR close.

Kennel authenticates as a service-account client (`services.kennel.keycloak.adminClientId`) holding the `realm-management/manage-clients` role. The client itself is provisioned in tofu under `infrastructure/tofu/identity/kennel.tf`; its secret is stored at `secret/data/infra/kennel-keycloak-admin` and rendered to disk by bao-agent.

Reconciliation is fire-and-forget: a failure logs a warning but does not block the deploy. The next deploy retries.

## Crate structure

- `kennel` -- main binary. HTTP router lives in `src/http.rs`, request handlers under `src/handlers/{webhook,metrics,builds,deployments,caddy}.rs`. Build orchestration in `src/build.rs`, deploy in `src/deploy.rs`, reconciliation in `src/reconcile.rs`. Systemd, Caddy, Keycloak, and OpenBao clients each have their own module.
- `kennel-config` -- shared types, constants, environment enum
- `kennel-provision` -- resource provisioning trait and implementations (PostgreSQL, Valkey, Garage)
- `entity` -- SeaORM generated entities
- `migration` -- SQLite schema migrations
