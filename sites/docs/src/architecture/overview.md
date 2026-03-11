# Architecture Overview

Kennel runs as a single binary with five subsystems: webhook receiver, builder, deployer, process supervisor, and router.

## Request Flow

```
Git Push -> Webhook -> Builder -> Deployer -> Supervisor -> Router -> Live Site
```

When you push to a Git repository:

1. Forgejo/GitHub sends a webhook to Kennel
1. Kennel verifies the signature and creates a build record
1. The builder clones your repo, evaluates devenv process configuration, and runs `nix build` for each service
1. The deployer merges deployment metadata into the process config and starts the process through the supervisor
1. The supervisor runs readiness probes and emits events when the process is ready
1. The router receives the event and starts sending traffic to your new deployment

## Component Responsibilities

### Webhook Receiver

Accepts POST requests at `/webhook/:project`. Verifies HMAC-SHA256 signatures against the project's webhook secret. Creates build records and enqueues them for building.

Supports push events (new commits) and pull request events (opened, synchronized, closed). Branch deletions trigger teardown of existing deployments.

### Builder

Runs a worker pool that processes builds concurrently (default: 2 at a time). For each build:

- Clones the repository at the specified commit
- Evaluates devenv task configuration (`nix build .#devenv.shells.default.config.task.config`) to discover process definitions
- Filters out infrastructure processes (postgres, redis, garage) and records them as required resources
- Runs `nix build` for each application process's exec derivation
- Compares store paths to detect unchanged builds
- Records success/failure per service
- Sends deployment request with process configs and required resources

Checks for cancellation before each major step.

### Process Supervisor

Manages process lifecycles directly, replacing systemd. Built on [watchexec-supervisor](https://crates.io/crates/watchexec-supervisor). For each process:

- Binds sockets before spawning (socket activation via `LISTEN_FDS`/`LISTEN_PID`)
- Spawns the process with environment variables, user switching, and cgroup placement
- Runs readiness probes (HTTP GET, exec, or systemd notify protocol)
- Monitors liveness continuously with the same probe configuration
- Restarts on failure per configurable policy (never, always, on-failure with max/window)
- Emits events (`ProcessReady`, `ProcessUnhealthy`, `ProcessStopped`, etc.)
- Supports blue-green deployment as a first-class operation

Each process runs in its own process group with a reset signal mask. On Linux, processes are placed in cgroup v2 subtrees for resource isolation (memory, CPU, task limits).

### Deployer

Manages the deployment lifecycle. For services:

- Creates a system user `kennel-<project>-<branch>-<service>`
- Provisions infrastructure resources (databases, key-value stores, object storage) via resource providers
- Merges devenv process config with deployment metadata (user, working directory, environment)
- Starts the process through the supervisor (or blue-green deploys if replacing an existing deployment)
- Records the deployment and process config in the database

For static sites:

- Creates symlink at `/var/lib/kennel/sites/<project>/<branch>/<site>`
- Points symlink to Nix store path
- Emits a supervisor event so the router learns about the new route

Runs cleanup job every 10 minutes to tear down expired deployments.

### Router

Reverse proxy listening on port 80 (and 443 with TLS). Routes based on Host header:

- `<service>-<branch>.<project>.scottylabs.org` -- auto-generated subdomain
- Custom domains configured per service

For services: proxies to `http://127.0.0.1:<port>` with X-Forwarded-\* headers. The port is learned from supervisor `ProcessReady` events.

For static sites: serves files from symlink path with SPA fallback (returns index.html for 404s).

Subscribes to supervisor events for real-time routing table updates. Also reloads static site routes from the database every 60 seconds as a safety net.

Obtains TLS certificates automatically via ACME HTTP-01 and TLS-ALPN-01 challenges.

### Infrastructure Providers

Provision per-deployment isolated resources within shared host-level services:

- **PostgreSQL**: creates a database per deployment, authenticates via Unix socket peer auth
- **Valkey**: allocates a DB number (0-31) per deployment from the shared instance
- **Garage**: creates an S3 bucket and scoped API key per deployment

Providers inject connection URLs as environment variables (`DATABASE_URL`, `VALKEY_URL`, `S3_ENDPOINT`, etc.) into the process config before the supervisor starts the process.

## Database State

All persistent state lives in PostgreSQL:

- `projects` -- Git repositories with webhook secrets
- `builds` -- build records (queued, building, success, failed, cancelled)
- `build_results` -- per-service results with store paths
- `services` -- service definitions with custom domains
- `deployments` -- deployment records with status (`deployed`, `torn_down`), process config (JSONB), and domain
- `dns_records` -- DNS records created for custom domains

Runtime process state (starting, ready, unhealthy, restarting) is owned by the supervisor, not persisted in the database.

## Configuration

Environment variables:

- `DATABASE_URL` -- PostgreSQL connection string
- `BASE_DOMAIN` -- base domain for auto-generated subdomains (default: scottylabs.org)
- `MAX_CONCURRENT_BUILDS` -- build worker pool size (default: 2)
- `WORK_DIR` -- build workspace directory (default: /var/lib/kennel/builds)
- `ROUTER_ADDR` -- router bind address (default: 0.0.0.0:80)
- `API_HOST` / `API_PORT` -- API server bind (default: 0.0.0.0:3000)
- `TLS_ENABLED` -- enable HTTPS (default: false)
- `ACME_EMAIL` -- email for Let's Encrypt
- `ACME_PRODUCTION` -- use Let's Encrypt production (default: false)
- `ACME_CACHE_DIR` -- certificate cache (default: /var/lib/kennel/acme)

All defaults are defined in `kennel-config::constants`.

## Communication Patterns

Components communicate via typed channels and supervisor events:

- Webhook -> Builder: `mpsc::channel<i64>` for build IDs
- Builder -> Deployer: `mpsc::channel<DeploymentRequest>` with process configs and required resources
- Supervisor -> Router: `broadcast::channel<SupervisorEvent>` for routing table changes
- All -> Database: shared `Arc<Store>` with SeaORM repository pattern

The router also reloads static site routes from the database every 60 seconds as a safety net.

## Graceful Shutdown

On SIGTERM or Ctrl-C, Kennel waits up to 300 seconds for all components to finish their current work before forcing exit.
