# Continuous Integration

ScottyLabs projects share one reusable Forgejo Actions workflow from the [devenv repo](https://codeberg.org/ScottyLabs/devenv). It runs the git hooks your `devenv.nix` enables and builds the project, matching what runs on commit.

## Adding CI to a project

Create `.forgejo/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main, staging, dev]
  pull_request:

jobs:
  check:
    uses: ScottyLabs/devenv/.forgejo/workflows/ci.yml@main
    secrets: inherit
```

It installs Lix and devenv, then:

- runs every git hook your modules enable (formatting, linting, type-checking, tests, commit message checks) with `devenv shell -- pre-commit run --all-files`
- builds every service and site declared in `scottylabs.kennel.services` and `scottylabs.kennel.sites` with `nix build`, the same set kennel deploys

## Caching

- Cachix (`scottylabs`) as a substituter for pulls and a push target for Nix store paths.
- sccache for Rust compilation, backed by the shared S3 bucket.
- The Rust `target` cache ([Swatinem/rust-cache](https://github.com/Swatinem/rust-cache)) and the Deno and uv caches under `.devenv/state`.
- devenv's Nix eval cache.

## Secrets

`secrets: inherit` passes the caller's org-level credentials (the `CACHIX_AUTH_TOKEN` and sccache S3 keys) to the workflow so it can reach the shared caches. A project's own secrets never enter CI, because the `ci` secretspec profile marks them optional and the workflow resolves nothing from OpenBao.
