# Troubleshooting Guide

Common issues and how to resolve them.

## Webhook Issues

### Webhook Signature Verification Failed

**Symptom**: Webhooks are rejected with 401 Unauthorized.

**Cause**: The webhook secret in your Forgejo/GitHub settings doesn't match the secret configured in Kennel.

**Solution**:

1. Check the webhook secret in your Git forge settings
1. Verify it matches the secret configured in Kennel's project settings
1. Ensure the secret is properly URL-encoded if it contains special characters

## Build Issues

### Build Fails: Git Clone Error

**Symptom**: Build fails during git clone with authentication error.

**Solution**:

1. For private repositories, configure SSH keys or access tokens
1. Ensure the kennel user has read access to the repository
1. Check `/var/lib/kennel/builds/<build_id>/` for detailed error logs

### Build Fails: devenv Task Config Evaluation

**Symptom**: Build fails with "Failed to evaluate devenv task config".

**Cause**: The project's `devenv.nix` or flake doesn't produce a valid task configuration.

**Solution**:

1. Verify `devenv up` works locally in the project
1. Test the evaluation: `nix build .#devenv.shells.default.config.task.config --no-link --print-out-paths`
1. Check that the project uses flake-parts with devenv integration

### Build Fails: Nix Build Error

**Symptom**: Build fails during `nix build` step.

**Solution**:

1. Test the build locally: `nix build .#<service-name>`
1. Check build logs at `/var/lib/kennel/logs/<build_id>/<service>.log`
1. Verify all Nix dependencies are available

### Build Fails: kennel.toml Not Found

**Symptom**: Build fails with "kennel.toml not found in repository".

**Solution**: Create a `kennel.toml` file at the repository root. See the [kennel.toml reference](../reference/kennel-toml.md).

### Build Fails: Invalid kennel.toml

**Symptom**: Build fails with "failed to parse kennel.toml".

**Solution**:

1. Validate your TOML syntax using a TOML linter
1. Check the error message for the specific line and issue
1. Refer to the [kennel.toml reference](../reference/kennel-toml.md)

## Deployment Issues

### Deployment Fails: Readiness Probe Timeout

**Symptom**: Deployment fails with "readiness probe timed out".

**Cause**: The service didn't pass its readiness probe within the configured timeout.

**Solution**:

1. Check that the service starts and listens on the expected port
1. Verify the readiness probe configuration in `devenv.nix` (path, port, timeout)
1. Test the health endpoint locally: `curl http://localhost:<port><path>`
1. Increase the `timeout` or `failure_threshold` in the devenv process config
1. Check if the service is crashing on startup (look at its output logs)

### Deployment Fails: Resource Provisioning Error

**Symptom**: Deployment fails with "resource provider 'postgres' failed" or similar.

**Cause**: The infrastructure provider couldn't create the required resource.

**Solution**:

1. Verify the shared PostgreSQL/Valkey/Garage instance is running
1. Check that Kennel has permission to create databases/buckets
1. For PostgreSQL: ensure the kennel user has CREATEDB privilege
1. For Garage: verify the admin API token is valid and has bucket/key management permissions

### Deployment Fails: User Creation Failed

**Symptom**: Deployment fails with "failed to create system user".

**Solution**:

1. Check if user exists: `id kennel-<project>-<branch>-<service>`
1. Ensure Kennel has permission to create users (requires root or CAP_SETUID/CAP_SETGID)
1. Manually create the user if needed: `useradd -r -s /bin/false <username>`

### Static Site Not Updating

**Symptom**: Static site deployment succeeds but shows old content.

**Solution**:

1. Check symlink: `ls -la /var/lib/kennel/sites/<project>/<branch>/<site>`
1. Verify it points to the new store path
1. Force browser cache clear

## Router Issues

### 502 Bad Gateway

**Symptom**: Requests to deployment return 502 Bad Gateway.

**Cause**: Backend service is down or the router can't reach it.

**Solution**:

1. Check if the supervisor reports the process as ready: look for `ProcessReady` events in logs
1. Verify the process is actually running and listening
1. Test the backend directly: `curl http://127.0.0.1:<port>/`
1. Check for `ProcessUnhealthy` events in the logs

### 404 Not Found

**Symptom**: Requests return 404 for valid deployments.

**Solution**:

1. Check that the deployment exists in the database with status `deployed`
1. Verify DNS resolves: `dig <service>-<branch>.<project>.<base_domain>`
1. Check if the supervisor emitted a `ProcessReady` event for this deployment
1. The router reloads static routes every 60 seconds -- wait and retry

### Static Files Return 404 on SPA

**Symptom**: SPA routing fails, only index.html works.

**Solution**:

1. Add `spa = true` to the static site config in `kennel.toml`
1. Rebuild and redeploy
1. Verify the site has a valid `index.html` at the root

## Database Issues

### Database Connection Failed

**Symptom**: Kennel fails to start with "failed to connect to database".

**Solution**:

1. Verify PostgreSQL is running: `systemctl status postgresql`
1. Check DATABASE_URL environment variable
1. Test connection: `psql $DATABASE_URL`
1. Ensure database exists and user has permissions

## DNS and TLS Issues

### DNS Record Not Created

**Symptom**: Deployment succeeds but DNS doesn't resolve.

**Solution**:

1. Check DNS manager configuration in the NixOS module
1. Verify DNS provider credentials are valid
1. Check Kennel logs for DNS API errors

### TLS Certificate Acquisition Failed

**Symptom**: HTTPS doesn't work after deployment.

**Solution**:

1. Check if HTTP-01 challenge is accessible: `curl http://<domain>/.well-known/acme-challenge/test`
1. Verify DNS points to the correct IP
1. Check Let's Encrypt rate limits
1. Ensure ports 80 and 443 are open

## General Debugging

### Enable Debug Logging

```bash
RUST_LOG=debug kennel
```

For specific components:

```bash
RUST_LOG=kennel_supervisor=debug,kennel_deployer=debug kennel
```

### Inspect Database State

Useful queries:

```sql
-- Recent builds
SELECT id, project_name, branch, status, created_at
FROM builds ORDER BY created_at DESC LIMIT 10;

-- Active deployments
SELECT id, project_name, branch, service_name, status, domain
FROM deployments WHERE status = 'deployed'
ORDER BY created_at DESC;

-- Check process configs
SELECT id, project_name, service_name, process_config->>'name' as process_name
FROM deployments WHERE status = 'deployed';
```

## Getting Help

If you're still stuck:

1. Check the [architecture documentation](../architecture/overview.md)
1. Review relevant RFCs in the `rfcs/` directory
1. Enable debug logging and examine the output
1. File an issue with Kennel version, error messages, and steps to reproduce
