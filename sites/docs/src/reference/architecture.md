# Architecture

Kennel runs as a single binary with four main responsibilities: receiving webhooks, recording the artifacts nixbot builds, deploying them, and reconciling desired state against actual state.

## Request flow

```
Git push -> nixbot (eval, build, push to cache) -> kennel (substitute, deploy with systemd + Caddy) -> Live
```

1. Forgejo sends a webhook to kennel's `/webhook` endpoint. Kennel verifies the HMAC signature, registers the project if it is new, and records a deploy request naming the commit the branch should run. Pull requests become the branch `pr-{number}`.
1. Forgejo also notifies nixbot, which evaluates the project's flake once per commit and builds its `checks` on the build farm. One of them is `checks.<system>.kennel`, which holds a `kennel.json` listing the store paths of the project's services and sites, and a copy of `secretspec.toml`. nixbot pushes the outputs to the binary cache.
1. A nixbot post-build step posts that attribute's `out_path` to kennel's `/nixbot` endpoint with a bearer token, along with the project, branch, commit, and pull request number. Kennel records it as the artifact for that branch and commit.
1. For each pending deploy request with an artifact, the reconciler substitutes the artifact with `nix-store --realise`, reads its `kennel.json`, provisions resources (database, cache, storage), and resolves secrets from OpenBao for the branch's profile (`main` is `prod`, pull requests are `preview`). It starts a systemd transient unit for each service, writes each site's [public values](../guides/deploying.md#public-values), and adds Caddy routes. A request whose artifact has not reached the cache yet stays pending for up to 15 minutes before it fails at the `fetch` stage. Requests for a preview branch are skipped when the project disables `previewDeployments`.
1. Cloudflare's edge terminates public TLS and routes each request through the host's tunnel to Caddy.

## Delegation

Kennel delegates process supervision to systemd and HTTP routing to Caddy, keeping the core focused on deployment and resource provisioning.

Systemd transient units are created via D-Bus using the zbus crate. Units are placed in the `kennel.slice` cgroup for aggregate accounting, with `CPUAccounting`, `MemoryAccounting`, `IOAccounting`, and `TasksAccounting` enabled so per-deployment resource usage is queryable from cgroup metrics by anything scraping `systemd_unit_*` or `systemd_slice_*` (e.g. `prometheus-systemd-exporter` filtered to `kennel-*` units). Each unit runs sandboxed as a `DynamicUser` with no daemon credentials, a read-only filesystem (`ProtectSystem=strict`, `ProtectHome=yes`) except a private `/tmp` (`PrivateTmp=yes`), and `NoNewPrivileges`. Transient units survive kennel crashes since they are independent of the kennel process.

Units restart on failure (`Restart=on-failure`, 5s delay) but are bounded by a start limit of 5 starts per 5 minutes (`StartLimitBurst` and `StartLimitIntervalUSec`). Once systemd exhausts the limit it stops restarting and holds the unit `failed`. That crash-loop cap is durable across kennel restarts, so reconcile leaves a `failed` unit stopped.

Caddy routes are managed via the [admin API](https://caddyserver.com/docs/api). Each deployment gets a route identified by `@id` for individual add/remove operations. Caddy handles static file serving, reverse proxying, and SPA fallback; public TLS and HTTP/3 terminate at Cloudflare's edge.

Nix-store files carry epoch modification times, so a frozen `Last-Modified` strands proxied clients on a stale shell. Reverse-proxy routes mark `text/html` responses `Cache-Control: no-store`. Static-site routes need none, since Caddy's file server drops epoch validators itself.

## HTTP API

Kennel exposes a small set of HTTP endpoints alongside the webhook receiver:

| Method | Path | Purpose |
| ------ | ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| POST | `/webhook` | Git push and pull request events from Forgejo, HMAC-verified. |
| POST | `/nixbot` | nixbot's post-build reports, a form with `project`, `attr`, `branch`, `revision`, `pr_number`, and `out_path`, authenticated with a bearer token. Attributes other than `checks.<system>.kennel` are ignored. |
| GET | `/metrics` | Prometheus exposition: `kennel_deploy_requests{status=...}`, `kennel_deployments`, `kennel_projects`, and `kennel_deploy_failures{stage=...}` gauges. |
| GET | `/deployments/:id/health` | JSON: `active`, `active_state`, `sub_state`, `result`, `active_enter_usec`, `n_restarts`, and `app_healthy` (a live probe of the service's `/api/health`) from the unit's D-Bus properties. |

All endpoints other than `/webhook` and `/nixbot` are unauthenticated and read-only. Caddy's `services.kennel.domain` virtualhost reverse-proxies these to the kennel API server, which only listens on localhost; the trust boundary is the host firewall plus tailnet, not application-level auth.

Routes are mounted in `http.rs`; per-resource handlers live under `handlers/`.

`kennel_deploy_failures` is the number of deploy requests currently marked failed, with a `stage` label for where the deploy stopped:

| Stage | Failed while |
| ----- | ------------ |
| `fetch` | substituting the commit's artifact from the binary cache |
| `config` | checking the `kennel.json` schema version |
| `provision` | creating postgres, sqlite, valkey, or garage resources |
| `secrets` | resolving secrets from OpenBao |
| `systemd` | starting the service's unit |
| `healthcheck` | waiting for the service to pass its health check |
| `routing` | adding Caddy routes |
| `internal` | updating kennel's database, gc roots, or site links |

A request stops counting once it is redeployed or its branch is closed. Stages with no failures are exported as `0`.

## Reconciliation

A single reconciliation loop handles all deployment convergence. It runs on startup, when signaled by a webhook or a nixbot report, and on a periodic 30-second timer.

The reconciler compares desired state (deployment rows in the database) against actual state (systemd units and Caddy routes) and converges:

- A pending deploy request gets deployed once nixbot reports its artifact.
- A deployment row whose unit is not running gets it restarted, unless systemd has given up on a crash loop (`failed`), in which case it is left stopped.
- A running unit with no deployment row gets stopped.
- All Caddy routes are re-added on each pass since Caddy config is ephemeral.
- The custom domains of all live deployments are written to `customDomainsFile`, if configured.
- Each deployment's gc root is re-asserted against its recorded store path.
- A deployment whose artifact is missing from the store is fetched again from the binary cache, and its unit is stopped if the cache no longer has it.

## Artifact lifecycle and gc roots

Every store path a deployment runs is protected from `nix-gc` by a gc root under `/nix/var/nix/gcroots/kennel/`. Without a root, the nightly `nix.gc --delete-older-than` sweep would reclaim the path, including one a live service is executing, which then fails it with `203/EXEC`.

Services run from their profile's directory in the artifact, which contains `kennel.json` and `secretspec.toml`. From a successful deploy until teardown, kennel keeps two roots for each deployment, `{deployment_id}` for the service or site and `{deployment_id}-config` for that directory. If `nix-gc` deletes an artifact before it is deployed, kennel fetches it from the binary cache again.

Each row in the `deployments` table names the store path its deployment runs. On every pass, reconcile reads each deployment's gc root and moves any root that points elsewhere back to the path in the table. When that path is missing from the Nix store, reconcile downloads the path from the binary cache again, and when the cache has lost the path as well, reconcile stops the unit until a later push deploys a new artifact.

Because reconcile repairs roots from the table, a deploy writes the new store path to the table before moving the gc root. A deploy that crashes before the write leaves the old deployment untouched, while one that crashes between the write and the root move leaves the next reconcile pass to move the root.

## State

Kennel stores state in SQLite with four tables:

- `projects` -- registered repositories with webhook secrets
- `artifacts` -- the deploy manifests nixbot reported, per branch and commit, pruned once a newer one deploys
- `deploy_requests` -- per-branch deploy intents naming the commit each branch wants, with status pending/deployed/skipped/failed
- `deployments` -- active deployments with store paths, domains, unit names, and ports

Runtime process state (running, stopped, failed) is owned by systemd and queried via D-Bus. Routing state is owned by Caddy and queried via the admin API.

Artifact retention is likewise external: gc roots under `/nix/var/nix/gcroots/kennel/`, keyed by deployment id, are what stop `nix-gc` from reclaiming the store paths deployments depend on.

## Crate structure

- `kennel` -- main binary. HTTP router lives in `src/http.rs`, request handlers under `src/handlers/{webhook,nixbot,metrics,deployments}.rs`. Deploy in `src/deploy.rs`, reconciliation in `src/reconcile.rs`. Systemd, Caddy, and OpenBao clients each have their own module.
- `kennel-config` -- shared types, constants, environment enum
- `kennel-provision` -- resource provisioning trait and implementations (PostgreSQL, SQLite, Valkey, Garage)
- `entity` -- SeaORM generated entities
- `migration` -- SQLite schema migrations
