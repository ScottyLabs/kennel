# Usage

This guide explains how to use Kennel to deploy your applications, from configuring your project to accessing deployments.

## Push to Deploy

When you push to a Git repository configured in Kennel:

1. Your Git server (Forgejo/GitHub) sends a webhook to `https://kennel.example.com/webhook/<project>`
1. Kennel verifies the signature, creates a build record, and queues it
1. A builder worker picks up the build and clones your repository
1. The builder evaluates devenv task configuration and runs `nix build` for each service
1. The deployer provisions infrastructure resources and starts processes through the supervisor
1. The supervisor runs readiness probes and notifies the router when processes are ready
1. The router starts sending traffic to your new deployment

The entire process typically takes 1-5 minutes depending on build complexity.

## Build Process

The builder:

1. Clones your repository at the specific commit SHA
1. Evaluates devenv task configuration (`nix build .#devenv.shells.default.config.task.config`)
1. Parses the resulting `tasks.json` to discover application processes and infrastructure requirements
1. For each application process, runs `nix build` on its exec derivation
1. Records the Nix store path for each successful build
1. Compares store paths to previous builds -- if unchanged, skips deployment
1. Sends a deployment request with process configs and required resource names

Infrastructure processes (`devenv:processes:postgres`, `devenv:processes:redis`, etc.) are not deployed as application services. Instead, their presence tells Kennel which resource providers to activate.

### Unchanged Builds

If the store path matches a recent build (last 5), the build is marked as unchanged. This means Nix determined nothing changed and reused a cached result.

### Build Cancellation

You can cancel queued or in-progress builds via the API:

```bash
curl -X POST https://kennel.example.com/builds/<id>/cancel
```

The builder checks for cancellation before each major step and stops gracefully.

## Deployment Environments

Kennel automatically assigns deployments to environments based on the branch name:

| Branch | Environment |
|--------|-------------|
| `main` | `prod` |
| `staging` | `staging` |
| `dev` | `dev` |
| `pr-*` | `preview` |
| Other | `dev` |

The environment affects secrets isolation, auto-expiry behavior, and is injected as the `ENVIRONMENT` variable.

## Deployment Process

### For Services

The deployer:

1. Creates system user `kennel-<project>-<branch>-<service>` if needed
1. Provisions infrastructure resources (PostgreSQL database, Valkey DB, Garage bucket) based on devenv config
1. Merges devenv process config with deployment metadata (user, working directory, environment, resource URLs)
1. Generates secrets environment file
1. If replacing an existing deployment: calls `supervisor.blue_green_deploy()` (starts new, waits for ready, drains, stops old)
1. Otherwise: calls `supervisor.start()` with the merged process config
1. The supervisor binds sockets, spawns the process, and runs readiness probes
1. On readiness, the supervisor emits `ProcessReady` and the router adds the route
1. Records the deployment in the database with the full process config as JSONB

### For Static Sites

The deployer:

1. Creates directory `/var/lib/kennel/sites/<project>/<branch>/`
1. Creates temporary symlink pointing to Nix store path
1. Atomically renames symlink to final path
1. Records deployment in database with `spa` flag
1. Emits a supervisor event so the router learns about the new route

No process runs for static sites -- the router serves files directly.

## Blue-Green Deployment

Services use blue-green deployment for zero downtime:

1. New version starts via the supervisor
1. Readiness probe confirms it's working
1. Router receives `ProcessReady` event and switches traffic to new version
1. Old version runs for 30 more seconds (connection drain period)
1. Old version is stopped via `supervisor.stop()`

During the overlap, both versions run simultaneously. Static sites don't need blue-green -- the symlink atomically switches to the new store path.

## Routing

After deployment, your service/site is accessible at:

Auto-generated subdomain:

```
https://<service>-<branch>.<project>.scottylabs.org
```

For example:

- `https://api-main.myproject.scottylabs.org`
- `https://web-feature-x.myproject.scottylabs.org`

Custom domains (if configured in kennel.toml):

```
https://yourdomain.com
```

Both work simultaneously if a custom domain is configured.

## Pull Request Deployments

Opening or updating a pull request triggers a deployment on a `pr-<number>` branch:

```
https://api-pr-42.myproject.scottylabs.org
```

Closing the PR triggers automatic teardown of all `pr-<number>` deployments.

## Teardown

Deployments are torn down when:

1. A branch is deleted
1. A pull request is closed
1. A deployment is marked for manual teardown
1. Auto-expiry time is reached

Teardown process:

1. For services: `supervisor.stop()` sends SIGTERM with grace period, then SIGKILL
1. For static sites: removes symlink
1. Removes secrets file
1. Resource providers release provisioned resources (databases, Valkey DBs, Garage buckets)
1. If this was the last deployment for project+branch+service: removes system user
1. DNS records are deleted for custom domains
1. Database record is updated to `torn_down`

## Health Monitoring

The supervisor continuously monitors process health using the same readiness probe configuration from devenv:

- HTTP GET probes check a configured endpoint
- Exec probes run a command and check the exit code
- Notify probes wait for `READY=1` on a Unix socket

When a probe fails `failure_threshold` times consecutively, the supervisor emits `ProcessUnhealthy` and the router removes the route. When the probe recovers, `ProcessHealthy` is emitted and the route is restored.

If the process exits, the supervisor checks the restart policy. With `on_failure` (the default), the process is restarted up to `max` times within the sliding `window`.

## Monitoring Your Deployment

Cancel a build:

```bash
curl -X POST https://kennel.example.com/builds/<id>/cancel
```

Build logs are stored at:

```
/var/lib/kennel/logs/<build-id>/<service-name>.log
```
