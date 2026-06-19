# Kennel

Kennel is the deployment platform for [ScottyLabs](https://scottylabs.org). When you push code to a repository, kennel builds it with Nix, deploys it, and routes traffic to it.

## What it does

- Builds your project with Nix when you push to any branch
- Deploys services as systemd units and static sites via [Caddy](https://caddyserver.com)
- Provisions per-deployment databases (PostgreSQL), caches (Valkey), and object storage (Garage)
- Resolves secrets from [OpenBao](https://openbao.org) via [secretspec](https://secretspec.dev)
- Generates HTTPS URLs for every deployment, including PR previews
- Tears down deployments and their resources when branches are deleted or PRs are closed

## How it works

Your project's `devenv.nix` declares what it needs to run: processes, databases, secrets, static sites. Kennel evaluates this configuration, builds the Nix packages, and deploys everything with isolated resources per branch.

Every deployment gets a URL at `{project}-{branch}.scottylabs.net`. Production deployments can also have custom domains.

## Getting started

See the [enabling features](./guides/features.md) and [deploying a project](./guides/deploying.md) guides.
