# Architecture

Kennel runs as a single binary with four main responsibilities: receiving webhooks, building Nix packages, deploying them, and reconciling desired state against actual state.

## Request flow

```
Git push -> Webhook -> Build (nix) -> Deploy (systemd + Caddy) -> Live
```

1. Forgejo sends a webhook to kennel's `/webhook` endpoint.
1. Kennel parses the repository name from the payload, verifies the HMAC signature, ensures a build record for the commit, and records a deploy request for the branch.
1. The build worker runs the build in a per-project/branch systemd unit, as a dynamic user with no daemon credentials. The unit clones the repo, runs `devenv build scottylabs.kennel.config` to discover declared services and sites, and runs `nix build` for each package, streaming output to journald under the unit and on to Loki. The daemon collects the result, pushes artifacts to cachix, and stores the log in the `builds.log` column.
1. For each pending deploy request whose build has finished, the reconciler reuses the build's artifacts to provision resources (database, cache, storage), resolve secrets from OpenBao, start a systemd transient unit for services, and add a Caddy route. Requests for a preview branch are skipped when the project disables `previewDeployments`.
1. Cloudflare's edge terminates public TLS and routes each request through the host's tunnel to Caddy.

## Delegation

Kennel delegates process supervision to systemd and HTTP routing to Caddy, keeping the core focused on build orchestration and resource provisioning.

Systemd transient units are created via D-Bus using the zbus crate. Units are placed in the `kennel.slice` cgroup for aggregate accounting, with `CPUAccounting`, `MemoryAccounting`, `IOAccounting`, and `TasksAccounting` enabled so per-deployment resource usage is queryable from cgroup metrics by anything scraping `systemd_unit_*` or `systemd_slice_*` (e.g. `prometheus-systemd-exporter` filtered to `kennel-*` units). Each unit runs sandboxed as a `DynamicUser` with no daemon credentials, a read-only filesystem (`ProtectSystem=strict`, `ProtectHome=yes`) except a private `/tmp` (`PrivateTmp=yes`), and `NoNewPrivileges`. Transient units survive kennel crashes since they are independent of the kennel process.

Units restart on failure (`Restart=on-failure`, 5s delay) but are bounded by a start limit of 5 starts per 5 minutes (`StartLimitBurst` and `StartLimitIntervalUSec`). Once systemd exhausts the limit it stops restarting and holds the unit `failed`. That crash-loop cap is durable across kennel restarts, so reconcile leaves a `failed` unit stopped.

Caddy routes are managed via the [admin API](https://caddyserver.com/docs/api). Each deployment gets a route identified by `@id` for individual add/remove operations. Caddy handles static file serving, reverse proxying, and SPA fallback; public TLS and HTTP/3 terminate at Cloudflare's edge.

Nix-store files carry epoch modification times, so a frozen `Last-Modified` strands proxied clients on a stale shell. Reverse-proxy routes mark `text/html` responses `Cache-Control: no-store`. Static-site routes need none, since Caddy's file server drops epoch validators itself.

## HTTP API

Kennel exposes a small set of HTTP endpoints alongside the webhook receiver:

| Method | Path | Purpose |
| ------ | ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| POST | `/webhook` | Git push and pull request events from Forgejo, HMAC-verified. |
| GET | `/metrics` | Prometheus exposition: `kennel_builds{status=...}`, `kennel_deployments`, `kennel_projects` gauges. |
| GET | `/deployments/:id/health` | JSON: `active`, `active_state`, `sub_state`, `result`, `active_enter_usec`, `n_restarts`, and `app_healthy` (a live probe of the service's `/api/health`) from the unit's D-Bus properties. |

All endpoints other than `/webhook` are unauthenticated and read-only. Caddy's `services.kennel.domain` virtualhost reverse-proxies these to the kennel API server, which only listens on localhost; the trust boundary is the host firewall plus tailnet, not application-level auth.

Routes are mounted in `http.rs`; per-resource handlers live under `handlers/`.

## Reconciliation

A single reconciliation loop handles all deployment convergence. It runs on startup, when signaled by a webhook or build completion, and on a periodic 30-second timer.

The reconciler compares desired state (deployment rows in the database) against actual state (systemd units and Caddy routes) and converges:

- A pending deploy request gets deployed once its build finishes.
- A deployment row whose unit is not running gets it restarted, unless systemd has given up on a crash loop (`failed`), in which case it is left stopped.
- A running unit with no deployment row gets stopped.
- All Caddy routes are re-added on each pass since Caddy config is ephemeral.
- The custom domains of all live deployments are written to `customDomainsFile`, if configured.
- Each deployment's gc root is re-asserted against its recorded store path.
- A deployment whose artifact is missing from the store is rebuilt from its recorded commit.

## Artifact lifecycle and gc roots

Every store path a build produces or a deployment runs is protected from `nix-gc` by a gc root under `/nix/var/nix/gcroots/kennel/`. Without a root, the nightly `nix.gc --delete-older-than` sweep would reclaim the path, including one a live service is executing, which then fails it with `203/EXEC`.

Roots come in two kinds, with a handoff between them:

- Build roots, named `{build_id}-{service}` per output plus `{build_id}-config`, are created when a build finishes so its outputs survive until they are deployed. `remove_build_gc_roots` drops them once the deploy request succeeds (the deployment roots have taken over), and they are reaped when a build is cancelled or superseded.
- Deployment roots, named `{deployment_id}` plus `{deployment_id}-config`, own the artifact a deployment currently runs. A successful deploy writes them; teardown removes them.

Deploys hold one invariant: the `deployments` record is committed before the deployment gc root is moved onto the new artifact. The recorded store path is therefore always rooted, by the build root before the record lands and by the deployment root after. A deploy that fails partway (a Caddy error, a database error, the process dying) leaves the previous revision's record and root intact and mutually consistent, and never advances the root off a path the record still names.

Reconcile enforces the same invariant on every pass. It re-asserts each deployment's gc root against its recorded store path (a no-op `readlink` when they already agree), so drift is corrected before a sweep can act on it. If a recorded artifact is missing from the store entirely the deployment cannot run, so reconcile stops the unit and rebuilds from the recorded commit, and the normal build and deploy flow then re-pins a fresh artifact.

## State

Kennel stores state in SQLite with four tables:

- `projects` -- registered repositories with webhook secrets
- `builds` -- per-commit build queue and history (queued, building, built, failed, cancelled), plus the captured per-phase `log` of subprocess output
- `deploy_requests` -- per-branch deploy intents naming the commit each branch wants, with status pending/deployed/skipped/failed
- `deployments` -- active deployments with store paths, domains, unit names, and ports

Runtime process state (running, stopped, failed) is owned by systemd and queried via D-Bus. Routing state is owned by Caddy and queried via the admin API. Kennel's database only tracks intent plus the historical build artifacts (logs) systemd doesn't keep.

Artifact retention is likewise external: gc roots under `/nix/var/nix/gcroots/kennel/`, keyed by build id and deployment id, are what stop `nix-gc` from reclaiming the store paths builds and deployments depend on.

## Crate structure

- `kennel` -- main binary. HTTP router lives in `src/http.rs`, request handlers under `src/handlers/{webhook,metrics,deployments,caddy}.rs`. Build orchestration in `src/build.rs`, deploy in `src/deploy.rs`, reconciliation in `src/reconcile.rs`. Systemd, Caddy, and OpenBao clients each have their own module.
- `kennel-config` -- shared types, constants, environment enum
- `kennel-provision` -- resource provisioning trait and implementations (PostgreSQL, Valkey, Garage)
- `entity` -- SeaORM generated entities
- `migration` -- SQLite schema migrations
