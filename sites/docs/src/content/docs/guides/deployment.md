---
title: Deployment Process
description: How deployments work from push to live
---

This guide explains what happens when you push code and how deployments reach production.

## Push to Deploy

When you push to a Git repository configured in Kennel:

1. Your Git server (Forgejo/GitHub) sends a webhook to `https://kennel.example.com/webhook/<project>`
2. Kennel verifies the signature, creates a build record, and queues it
3. A builder worker picks up the build and clones your repository
4. The builder runs `nix build` for each service and static site
5. The deployer creates systemd units or symlinks
6. The router starts sending traffic to your new deployment

The entire process typically takes 1-5 minutes depending on build complexity.

## Build Process

The builder:

1. Clones your repository at the specific commit SHA
2. Reads `kennel.toml` from the repository root
3. For each service, runs `nix build .#packages.x86_64-linux.<service-name>`
4. For each static site, runs `nix build .#packages.x86_64-linux.<site-name>`
5. Records the Nix store path for each successful build
6. Compares store paths to previous builds - if unchanged, skips rebuild

If any build fails, that specific service/site fails but others can still deploy.

### Unchanged Builds

If the store path matches a recent build (last 5), the build is marked as unchanged. This means Nix determined nothing changed and reused a cached result.

Unchanged builds still deploy - environment variables, secrets, or configuration might have changed.

### Build Cancellation

You can cancel queued or in-progress builds via the API:

```bash
curl -X POST https://kennel.example.com/builds/<id>/cancel
```

The builder checks for cancellation before each major step and stops gracefully.

## Deployment Process

### For Services

The deployer:

1. Checks for an existing active deployment of this service on this branch
2. Allocates a port from 18000-19999
3. Creates system user `kennel-<project>-<branch>-<service>` if it doesn't exist
4. If `preview_database = true`, allocates a Valkey database number (0-15)
5. Generates environment file at `/run/kennel/secrets/<project>-<branch>-<service>.env` with PORT, VALKEY_URL, DATABASE_URL
6. Sets file permissions to 0400 (read-only, owner only)
7. Generates systemd unit file at `/etc/systemd/system/kennel-<project>-<branch>-<service>.service`
8. Runs `systemctl daemon-reload`
9. Runs `systemctl start kennel-<project>-<branch>-<service>`
10. Polls `http://localhost:<port><health_check>` with exponential backoff (1s, 2s, 4s, 8s, 15s)
11. If health check succeeds, marks deployment as active in database
12. Notifies router to add this deployment to routing table
13. If an old deployment existed, waits 30 seconds (connection drain)
14. Stops and tears down the old deployment

If health check fails after 30 seconds, the new deployment is marked as failed and the old one stays running.

### For Static Sites

The deployer:

1. Creates directory `/var/lib/kennel/sites/<project>/<branch>/`
2. Creates temporary symlink pointing to Nix store path
3. Atomically renames symlink to `/var/lib/kennel/sites/<project>/<branch>/<site>`
4. Records deployment in database with `spa` flag from kennel.toml
5. Notifies router to serve files from this path

No process runs for static sites - the router serves files directly.

## Blue-Green Deployment

Services use blue-green deployment for zero downtime:

1. New version starts on a new port
2. Health check confirms it's working
3. Router switches traffic to new version
4. Old version runs for 30 more seconds (connection drain period)
5. Old version stops and port is released

During the 30-second overlap, both versions run simultaneously. This ensures in-flight requests complete before the old version stops.

Static sites don't need blue-green - the symlink atomically switches to the new store path.

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
2. A pull request is closed
3. A deployment is marked for manual teardown
4. Auto-expiry time is reached (if configured)

Teardown process:

1. For services: stops systemd unit, removes unit file, releases port
2. For static sites: removes symlink
3. Removes secrets file
4. If this was the last deployment for the branch: releases preview database
5. If this was the last deployment for project+branch+service: removes system user
6. Updates database to mark deployment as torn down

The teardown worker processes teardown requests asynchronously. The cleanup job runs every 10 minutes to find expired deployments.

## Health Monitoring

After deployment, the router continuously monitors service health:

1. Every 30 seconds, router sends GET to `http://localhost:<port><health_check>`
2. Expects 200 OK within 5 seconds
3. On failure, increments failure counter
4. After 3 consecutive failures, removes deployment from routing (returns 404)
5. On success, resets failure counter to 0

Unhealthy deployments stay in the database but don't receive traffic. They can be manually torn down or will be cleaned up if they expire.

## Monitoring Your Deployment

Cancel a build:
```bash
curl -X POST https://kennel.example.com/builds/<id>/cancel
```

View systemd logs (on the server):
```bash
journalctl -u kennel-myproject-main-api -f
```

Build logs are stored at:
```
/var/lib/kennel/logs/<build-id>/<service-name>.log
```
