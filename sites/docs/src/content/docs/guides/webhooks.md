---
title: Webhook Setup
description: Configure Forgejo or GitHub to send webhooks to Kennel
---

Kennel receives push and pull request events via webhooks. You need to configure your Git server to send webhooks to Kennel.

## Webhook URL

Webhooks are sent to:

```
https://kennel.example.com/webhook/<project-name>
```

Where `<project-name>` matches the project name in Kennel's database.

## Security

Kennel verifies webhook signatures using HMAC-SHA256. Each project has a webhook secret stored in the database. The Git server signs the payload with this secret, and Kennel verifies it before processing.

Without a valid signature, webhooks are rejected with 401 Unauthorized.

## Forgejo Setup

1. Go to your repository settings
2. Navigate to Webhooks
3. Click "Add Webhook" and select "Forgejo"
4. Configure:
   - **Payload URL**: `https://kennel.example.com/webhook/<project-name>`
   - **HTTP Method**: POST
   - **POST Content Type**: application/json
   - **Secret**: Use the webhook secret from Kennel's projects table
   - **Trigger On**: Select "Push events" and "Pull request events"
   - **Branch filter**: Leave empty (Kennel handles all branches)
5. Click "Add Webhook"

Test the webhook by pushing a commit or opening a PR.

## GitHub Setup

1. Go to repository Settings -> Webhooks
2. Click "Add webhook"
3. Configure:
   - **Payload URL**: `https://kennel.example.com/webhook/<project-name>`
   - **Content type**: application/json
   - **Secret**: Use the webhook secret from Kennel's projects table
   - **SSL verification**: Enable
   - **Events**: Select "Just the push event" and "Pull requests"
4. Click "Add webhook"

Test the webhook by pushing a commit or opening a PR.

## Supported Events

### Push Events

Triggered when commits are pushed to any branch.

Kennel processes these to:
- Create a new build for the commit
- Deploy the new version
- Update the existing deployment for that branch

### Branch Deletion

When a branch is deleted (push event with all-zero commit SHA), Kennel:
- Marks all deployments for that branch as tearing down
- Stops services, removes symlinks
- Releases ports and preview databases
- Removes system users (if no other deployments for that service)

### Pull Request Events

Supported actions:
- **opened**: Creates a deployment on `pr-<number>` branch
- **synchronize** / **synchronized**: Updates the PR deployment with new commits
- **reopened**: Creates deployment if torn down
- **closed**: Tears down all deployments for `pr-<number>`

Other PR actions (labeled, assigned, etc.) are ignored.

## Webhook Payload

Kennel parses these fields from the JSON payload:

For push events:
- `ref` - Git ref like `refs/heads/main`
- `after` - Commit SHA
- `pusher.name` or `sender.login` - Author

For PR events:
- `action` - PR action (opened, synchronize, closed, etc.)
- `number` - PR number
- `pull_request.head.sha` - Commit SHA
- `sender.login` - Author

## Signature Verification

### Forgejo

Forgejo sends signature in `X-Forgejo-Signature` header:

```
X-Forgejo-Signature: <hex-encoded-hmac-sha256>
```

Kennel computes HMAC-SHA256 of the raw request body using the project's webhook secret and compares.

### GitHub

GitHub sends signature in `X-Hub-Signature-256` header:

```
X-Hub-Signature-256: sha256=<hex-encoded-hmac-sha256>
```

Same verification process, just different header format.

## Troubleshooting

### 401 Unauthorized

Signature verification failed. Check:
- Webhook secret matches between Git server and Kennel database
- Payload URL is correct
- No proxy is modifying the request body

Kennel logs signature failures with project name, IP address, and event type for debugging.

### 404 Not Found

Project doesn't exist in Kennel's database. Verify the project name in the URL matches exactly.

### 503 Service Unavailable

Builder queue is full or unavailable. This is rare and usually means Kennel is overloaded. The webhook sender will retry.

### No Deployment Happens

Check:
- Webhook was sent (check delivery history in Git server settings)
- Kennel received it (check logs for "Received webhook for project: ...")
- Build succeeded (check build logs at `/var/lib/kennel/logs/<build-id>/`)
- `kennel.toml` exists in repository root
- Nix packages are defined for all services/sites

## Webhook Retries

Both Forgejo and GitHub retry failed webhooks automatically. If Kennel is temporarily down, webhooks will be retried.

Kennel handles duplicate webhooks idempotently - if a build already exists for the same project/ref/commit, it returns 200 OK without creating a duplicate.
