# Kennel

Kennel is the deployment platform for [ScottyLabs](https://scottylabs.org). When you push code to a repository, kennel deploys what nixbot built for that commit and routes traffic to it.

## What it does

- Deploys each push to `main`, `staging`, `dev`, and every pull request
- Deploys services as systemd units and static sites via [Caddy](https://caddyserver.com)
- Provisions per-deployment databases (PostgreSQL, SQLite), caches (Valkey), and object storage (Garage)
- Resolves secrets from [OpenBao](https://openbao.org) via [secretspec](https://secretspec.dev)
- Generates HTTPS URLs for every deployment, including PR previews
- Tears down deployments and their resources when branches are deleted or PRs are closed

## How it works

A project lists its services and static sites in `flake.nix`, along with the databases and secrets they use. For each commit, nixbot builds the project and pushes the result to the binary cache. Kennel then deploys it with isolated resources per branch.

Every deployment gets a URL at `{project}-{branch}.scottylabs.net`. Production deployments can also have custom domains.

## Getting started

See the [enabling features](./guides/features.md) and [deploying a project](./guides/deploying.md) guides.
