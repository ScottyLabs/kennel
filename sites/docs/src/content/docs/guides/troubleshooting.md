---
title: Troubleshooting Guide
description: Common errors and how to resolve them
---

This guide covers common issues you may encounter when running Kennel and how to resolve them.

## Webhook Issues

### Webhook Signature Verification Failed

**Symptom**: Webhooks are rejected with a 401 Unauthorized error.

**Cause**: The webhook secret in your Forgejo/GitHub settings doesn't match the secret configured in Kennel.

**Solution**:
1. Check the webhook secret in your Git forge settings
2. Verify it matches the secret configured in Kennel's environment or project settings
3. Ensure the secret is properly URL-encoded if it contains special characters

## Build Issues

### Build Fails: Git Clone Error

**Symptom**: Build fails during git clone with authentication error.

**Cause**: Kennel doesn't have access to the repository.

**Solution**:
1. For private repositories, configure SSH keys or access tokens
2. Ensure the kennel user has read access to the repository
3. Check `/var/lib/kennel/builds/<build_id>/` for detailed error logs

### Build Fails: Nix Build Error

**Symptom**: Build fails during `nix build` step.

**Cause**: Nix flake evaluation or build failure in your project.

**Solution**:
1. Test the build locally: `nix build .#<service-name>`
2. Check build logs at `/var/lib/kennel/logs/builds/<build_id>.log`
3. Ensure your `flake.nix` outputs match the service names in `kennel.toml`
4. Verify all Nix dependencies are available

### Build Fails: kennel.toml Not Found

**Symptom**: Build fails with "kennel.toml not found in repository".

**Cause**: Your repository doesn't have a `kennel.toml` at the root.

**Solution**:
1. Create a `kennel.toml` file at the repository root
2. See [kennel.toml schema](#kenneltoml-schema) for configuration options
3. Commit and push the file

### Build Fails: Invalid kennel.toml

**Symptom**: Build fails with "failed to parse kennel.toml".

**Cause**: Syntax error or invalid configuration in `kennel.toml`.

**Solution**:
1. Validate your TOML syntax using a TOML linter
2. Check the error message for the specific line and issue
3. Refer to the [kennel.toml schema](#kenneltoml-schema) for valid configuration

## Deployment Issues

### Deployment Fails: Port Allocation Exhausted

**Symptom**: Deployment fails with "port pool exhausted".

**Cause**: All ports in range 18000-19999 are allocated.

**Solution**:
1. Check for stale deployments: `systemctl list-units 'kennel-*'`
2. Teardown unused deployments via the API or manually
3. Increase port range in `kennel-config/src/constants.rs` if needed (requires rebuild)
4. Check database: `SELECT * FROM port_allocations WHERE deployment_id IS NULL;`

### Deployment Fails: Health Check Timeout

**Symptom**: Deployment fails with "health check failed after retries".

**Cause**: Service didn't start successfully or health endpoint isn't responding.

**Solution**:
1. Check systemd logs: `journalctl -u kennel-<project>-<branch>-<service> -n 50`
2. Verify the health check path in `kennel.toml` is correct (defaults to `/health`)
3. Ensure the service binds to `0.0.0.0` or `127.0.0.1`, not `localhost`
4. Check port allocation: `ss -tlnp | grep <port>`
5. Verify the service starts quickly enough (health check has 30 second timeout)

### Deployment Fails: systemd Unit Creation Failed

**Symptom**: Deployment fails when creating systemd unit.

**Cause**: Permission issues or invalid unit file.

**Solution**:
1. Ensure Kennel is running with sufficient privileges to manage systemd
2. Check `/etc/systemd/system/kennel-*.service` for syntax errors
3. Run `systemctl daemon-reload` manually
4. Check systemd logs: `journalctl -xe`

### Deployment Fails: User Creation Failed

**Symptom**: Deployment fails with "failed to create system user".

**Cause**: User already exists or permission denied.

**Solution**:
1. Check if user exists: `id kennel-<project>-<branch>-<service>`
2. Ensure Kennel has permission to create users (requires root or sudo)
3. Manually create the user if needed: `useradd -r -s /bin/false <username>`

### Static Site Not Updating

**Symptom**: Static site deployment succeeds but shows old content.

**Cause**: Symlink not updated or cache issue.

**Solution**:
1. Check symlink: `ls -la /var/lib/kennel/sites/<project>/<branch>/current`
2. Verify it points to the new build: `readlink /var/lib/kennel/sites/<project>/<branch>/current`
3. Check Nginx/router logs for cache headers
4. Force browser cache clear (Ctrl+Shift+R)

## Router Issues

### 502 Bad Gateway

**Symptom**: Requests to deployment return 502 Bad Gateway.

**Cause**: Backend service is down or router can't reach it.

**Solution**:
1. Check deployment status: `systemctl status kennel-<project>-<branch>-<service>`
2. Verify port allocation in database matches systemd unit
3. Test backend directly: `curl http://127.0.0.1:<port>/health`
4. Check router logs for connection errors
5. Ensure firewall allows internal connections

### 404 Not Found

**Symptom**: Requests return 404 Not Found for valid deployments.

**Cause**: Routing table not updated or DNS not resolved.

**Solution**:
1. Check routing table: query `deployments` table for project/branch/service
2. Verify DNS record exists: `dig <service>-<branch>.<project>.<base_domain>`
3. Check router logs for routing mismatches
4. Restart router component if routing table is stale

### Static Files Return 404 on SPA

**Symptom**: SPA routing fails, only index.html works.

**Cause**: SPA fallback not configured in `kennel.toml`.

**Solution**:
1. Add `spa = true` to static site config in `kennel.toml`
2. Rebuild and redeploy
3. Verify the site has a valid `index.html` at the root

## Database Issues

### Preview Database Allocation Failed

**Symptom**: Deployment fails with "valkey db pool exhausted".

**Cause**: All Valkey databases (0-15) are allocated.

**Solution**:
1. Check allocated databases: `SELECT * FROM preview_databases;`
2. Teardown unused PR deployments to free databases
3. Consider increasing Valkey databases if using Redis (maxdatabases config)

### Preview Database Missing Assignment

**Symptom**: Deployment fails with "preview database has no valkey db assigned".

**Cause**: Database record exists but Valkey DB wasn't allocated (fixed in recent versions).

**Solution**:
- Upgrade to the latest version of Kennel
- Manually assign Valkey DB: `UPDATE preview_databases SET valkey_db = <num> WHERE id = <id>;`
- Or teardown and recreate the preview environment

### Database Connection Failed

**Symptom**: Kennel fails to start with "failed to connect to database".

**Cause**: PostgreSQL not running or connection settings incorrect.

**Solution**:
1. Verify PostgreSQL is running: `systemctl status postgresql`
2. Check connection settings in `kennel.toml` or environment variables
3. Test connection: `psql -h <host> -U <user> -d <database>`
4. Ensure database exists and user has permissions

## DNS and TLS Issues

### DNS Record Not Created

**Symptom**: Deployment succeeds but DNS doesn't resolve.

**Cause**: DNS manager not configured or DNS update failed.

**Solution**:
1. Check DNS manager configuration in `kennel.toml`
2. Verify DNS provider credentials are valid
3. Check DNS manager logs for API errors
4. Manually create DNS record as temporary workaround

### TLS Certificate Acquisition Failed

**Symptom**: Deployment succeeds but HTTPS doesn't work.

**Cause**: ACME challenge failed or certificate not deployed.

**Solution**:
1. Check if HTTP-01 challenge is accessible: `curl http://<domain>/.well-known/acme-challenge/test`
2. Verify DNS points to correct IP address
3. Check Let's Encrypt rate limits
4. Review ACME client logs
5. Ensure port 80 and 443 are open

## General Debugging

### Enable Debug Logging

Set `RUST_LOG=debug` environment variable when running Kennel:

```bash
RUST_LOG=debug kennel
```

For specific components:

```bash
RUST_LOG=kennel_builder=debug,kennel_deployer=debug kennel
```

### Check Component Status

All Kennel components run as tasks within the main process. Check logs for:
- `[webhook]` - webhook receiver
- `[builder]` - build worker pool
- `[deployer]` - deployment worker
- `[router]` - HTTP router
- `[dns]` - DNS manager

### Inspect Database State

Useful queries for debugging:

```sql
-- Recent builds
SELECT id, project_name, branch, commit_sha, status, created_at 
FROM builds 
ORDER BY created_at DESC 
LIMIT 10;

-- Active deployments
SELECT id, project_name, branch, service_name, port, status 
FROM deployments 
WHERE status = 'active' 
ORDER BY created_at DESC;

-- Port allocations
SELECT port, project_name, service_name, branch 
FROM port_allocations 
ORDER BY port;

-- Preview databases
SELECT id, project_name, branch, database_name, valkey_db, created_at
FROM preview_databases;
```

### Clean Up Stale State

If you encounter inconsistent state, manually clean up:

```bash
# Stop all kennel services
systemctl stop 'kennel-*'

# Clear stale deployments from database
psql kennel -c "DELETE FROM deployments WHERE status = 'pending';"

# Clear stale port allocations
psql kennel -c "DELETE FROM port_allocations WHERE deployment_id NOT IN (SELECT id FROM deployments);"

# Restart Kennel
systemctl restart kennel
```

## Getting Help

If you're still stuck after trying these solutions:

1. Check the [architecture documentation](../architecture/overview.md) to understand how components interact
2. Review relevant RFCs in the `rfcs/` directory
3. Enable debug logging and examine the detailed output
4. File an issue with:
   - Kennel version
   - Error messages and logs
   - Steps to reproduce
   - Database queries showing relevant state
