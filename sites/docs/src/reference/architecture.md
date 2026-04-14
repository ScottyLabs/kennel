# Architecture

Kennel runs as a single binary with four main responsibilities: receiving webhooks, building Nix packages, deploying them, and reconciling desired state against actual state.

## Request flow

```
Git push -> Webhook -> Build (nix) -> Deploy (systemd + Caddy) -> Live
```

1. Forgejo sends a webhook to kennel's single `/webhook` endpoint.
2. Kennel parses the repository name from the payload, verifies the HMAC signature, and creates a build record.
3. The build worker clones the repo, evaluates the devenv kennel config, and runs `nix build` for each service and static site.
4. The reconciler picks up the completed build, provisions resources (database, cache, storage), resolves secrets from OpenBao, starts a systemd transient unit for services, and adds a Caddy route for each deployment.
5. Caddy serves traffic over HTTPS with on-demand TLS.

## Delegation

Kennel delegates process supervision to systemd and HTTP routing to Caddy, keeping the core focused on build orchestration and resource provisioning.

Systemd transient units are created via D-Bus using the zbus crate. Units are placed in the `kennel.slice` cgroup for aggregate accounting. Transient units survive kennel crashes since they are independent of the kennel process.

Caddy routes are managed via the [admin API](https://caddyserver.com/docs/api). Each deployment gets a route identified by `@id` for individual add/remove operations. Caddy handles TLS certificate provisioning, HTTP/3, static file serving, reverse proxying, and SPA fallback.

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
- `builds` -- build queue and history (queued, building, built, done, failed, cancelled)
- `deployments` -- active deployments with store paths, domains, unit names, and ports

Runtime process state (running, stopped, failed) is owned by systemd and queried via D-Bus. Routing state is owned by Caddy and queried via the admin API. Kennel's database only tracks intent.

## Crate structure

- `kennel` -- main binary with webhook, build, deploy, reconcile, caddy client, systemd client, store, secrets
- `kennel-config` -- shared types, constants, environment enum
- `kennel-provision` -- resource provisioning trait and implementations (PostgreSQL, Valkey, Garage)
- `entity` -- SeaORM generated entities
- `migration` -- SQLite schema migrations
