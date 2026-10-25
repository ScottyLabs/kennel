# Continuous Integration

On each push and pull request, [nixbot](https://nixbot.scottylabs.org) builds every flake `check` on the build farm, pushes the results to the binary cache, and posts one commit status per check. nixbot picks up every repository in the organization with the `nixbot` topic, which is set automatically on repos registered in governance.

The shared module's checks are:

- `hooks`: every pre-commit hook your modules enable (formatting, linting, type-checking, tests) except clippy and `cargo-test`, run in the build sandbox without network access
- `clippy` (denying warnings) and `test` for each `buildRustService` target, built against its cached dependencies
- `kennel`: the deploy manifest, which builds every service and site
- `devShell`: the dev shell for `x86_64-linux`, `aarch64-linux`, and `aarch64-darwin`, so it can come from the binary cache

Commit message hooks run only locally.

## Network checks

Checks that need the network run on Forgejo Actions instead. Create `.forgejo/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main, staging, dev]
  pull_request:

jobs:
  checks:
    uses: ScottyLabs/kennel/.forgejo/workflows/ci.yml@main
```

The workflow runs `nix run .#ci`, which runs every check in `scottylabs.ci.checks`. By default these are security scans of the lockfiles with [osv-scanner](https://github.com/google/osv-scanner) and of the code with [semgrep](https://semgrep.dev). [vulnix](https://github.com/nix-community/vulnix) can also scan each service and site's runtime closure once `scottylabs.security.vulnix.enable` is set. Add a check of your own as a shell script:

```nix
scottylabs.ci.checks.links = "${lib.getExe pkgs.lychee} docs/";
```

## Custom workflows

nixbot's checks run in the build sandbox without secrets. A job that needs secrets while it runs, such as a scheduled scraper, runs on Forgejo Actions and resolves a `ci` profile, which signs in to OpenBao with the job's OIDC token. Add the profile to `secretspec.toml`:

```toml
[providers]
openbao-ci = "openbao://secrets.scottylabs.org/secret?auth=jwt&role=ci&audience=openbao"

[profiles.ci.defaults]
required = false
providers = ["openbao-ci"]

[profiles.ci]
CMU_LDAP_PASSWORD = { description = "CMU directory bind password", required = true }
```

The token reads the project's `ci` profile and none of its other profiles, so `required = false` keeps the secrets inherited from `[profiles.default]` from failing resolution, and each secret the job actually needs is marked `required = true`. Then add the job:

```yaml
name: Scrape

on:
  schedule:
    - cron: "0 6 1 * *" # monthly

jobs:
  scrape:
    runs-on: native
    enable-openid-connect: true
    steps:
      - uses: https://code.forgejo.org/actions/checkout@v7

      - name: Scrape
        run: SECRETSPEC_PROFILE=ci nix develop --command cargo run -p scraper
```
