# PR Deployments

Every pull request gets its own isolated deployment with its own database, cache, and URL.

## Lifecycle

1. PR opened: kennel builds the PR branch, provisions resources, and deploys. The deployment is available at `{project}-pr-{number}.scottylabs.net`.
1. Push to PR: kennel rebuilds. Unchanged services (same nix store path) are skipped. Changed services are redeployed.
1. PR closed: kennel tears down all deployments for the branch, deprovisions resources (drops the database, flushes the cache, deletes the storage bucket), and removes the Caddy route.

## Status comments

After each successful PR deploy, kennel posts (and on subsequent deploys edits) a sticky comment on the pull request listing every service URL for that branch. When the PR closes and deployments are torn down, the comment is updated to reflect the teardown. Comments are identified by an HTML marker in the body so kennel can update the same comment instead of creating duplicates.

This requires the operator to configure `services.kennel.forgejo.apiTokenFile` with a Forgejo API token that has the `write:issue` scope. See the [NixOS Module reference](../reference/nixos-module.md#serviceskennelforgejo).

## Resource isolation

Each PR deployment gets:

- Its own PostgreSQL database (`kennel_{project}_{branch}`)
- Its own Valkey DB number
- Its own Garage S3 bucket and API key

Connection strings are injected as environment variables. Your application code does not need to know whether it is running in production or a PR preview.

## Expiry

PR deployments that have had no activity for 7 days are hibernated: the process is stopped but the database is kept. After 30 days, the deployment and its resources are fully torn down.

## URLs

PR URLs follow the flat scheme `{project}-pr-{number}.scottylabs.net`, covered by a single wildcard DNS record. No per-deployment DNS management is needed.
