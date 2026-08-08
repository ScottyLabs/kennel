# Continuous Integration

ScottyLabs projects share one reusable Forgejo Actions workflow from the [kennel repo](https://git.cmu.dev/ScottyLabs/kennel). It runs the git hooks your `devenv.nix` enables and builds the project, matching what runs on commit.

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
    uses: https://git.cmu.dev/ScottyLabs/kennel/.forgejo/workflows/ci.yml@main
    enable-openid-connect: true
```

It installs Lix and devenv, then:

- runs every git hook your modules enable (formatting, linting, type-checking, tests, commit message checks) with `devenv shell -- prek run --all-files`
- builds every service and site declared in `scottylabs.kennel.services` and `scottylabs.kennel.sites` with `nix build`, the same set kennel deploys

## Options

The workflow takes one input, `build` (boolean, default `true`), which gates the build job. A project with nothing for kennel to build, or whose build is too expensive to run per commit, can keep the checks and skip it:

```yaml
jobs:
  check:
    uses: https://git.cmu.dev/ScottyLabs/kennel/.forgejo/workflows/ci.yml@main
    enable-openid-connect: true
    with:
      build: false
```

## Caching

- Cachix (`scottylabs`) as a substituter for pulls and a push target for Nix store paths.
- sccache for Rust compilation, backed by the shared S3 bucket.
- The Rust `target` cache ([Swatinem/rust-cache](https://github.com/Swatinem/rust-cache)) and the Deno and uv caches under `.devenv/state`.
- devenv's Nix eval cache.

## Secrets

The reusable signs in to OpenBao with the job's Forgejo Actions OIDC token and reads the shared Cachix token and sccache S3 keys from there to reach the binary caches. A project's own secrets are optional under the `ci` profile.
