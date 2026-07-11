# devenv

Shared [devenv](https://devenv.sh) modules and build helpers for ScottyLabs. Provides language toolchains (Rust, Deno, Python), formatting and linting via treefmt and git hooks, local database services (Postgres, SQLite, Valkey, Garage), OpenBao secrets, Cachix binary caching, and kennel deployment config.

- [Deploying a Project](https://docs.kennel.scottylabs.org/guides/deploying.html)
- [Secrets](https://docs.kennel.scottylabs.org/guides/secrets.html)
- [devenv Options Reference](https://docs.kennel.scottylabs.org/reference/devenv-options.html)
- [Build Helpers](https://docs.kennel.scottylabs.org/reference/build-helpers.html)

## Adding a language module

Create `modules/<lang>.nix` and add it to the imports in `modules/default.nix`.

A language module should:

- Gate on `config.scottylabs.enable && cfg.enable`
- Put caches under `.devenv/state/` (e.g. `env.DENO_DIR`, `env.CARGO_TARGET_DIR`) so the CI workflow caches them generically
- Register formatters under `treefmt.config.programs`
- Register linters and tests as `git-hooks.hooks` (these are what CI runs)
- Avoid pulling in heavy dev tools the language runtime doesn't need (see `languages.c.enable = lib.mkForce false` in `rust.nix`)

Document the new options in kennel at `sites/docs/src/reference/devenv-options.md`.

## Adding a service module

Create `modules/<service>.nix` and add it to the imports in `modules/default.nix`.

A service module should:

- Wrap an upstream devenv service (e.g. `services.postgres`, `services.redis`)
- Export the same env vars that kennel injects in production (e.g. `DATABASE_URL`, `S3_ENDPOINT`), so the application is environment-agnostic and driven entirely by env var changes ([12factor](https://12factor.net/config))
- Use `scottylabs.project.name` for database/bucket naming

## Adding a check

A tier-1 check is a `git-hooks.hooks` entry that runs on every commit, so it must stay fast (e.g. `cargo-machete` in `rust.nix`, `gitleaks` in `default.nix`, since it only diffs the staged patch). A tier-2 check is a devenv task namespaced by concern (e.g. `security:` in `modules/security.nix`) for anything too slow or noisy for a commit hook; CI runs each namespace with `devenv tasks run <namespace>`, the same way it runs tier 1 with `prek run --all-files`.

To add a check, add a `git-hooks.hooks.<tool>` entry (tier 1) or a `tasks."<namespace>:<tool>".exec` entry (tier 2), gated on `config.scottylabs.enable` and an `enable` option for the tool, in whichever module owns it: `modules/security.nix` for security tools, or the relevant module for tool-specific checks.

## Adding a build helper

Add a file under `lib/` and add it to `mkLib` in `flake.nix`. The helper receives the consumer's `pkgs` so it builds against their nixpkgs pin. Document it in kennel at `sites/docs/src/reference/build-helpers.md`.
