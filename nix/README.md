# Shared flake module

Projects import `flakeModules.default` into a [flake-parts](https://flake.parts) flake to get their dev shell, git hooks, local services, and the outputs kennel deploys from.

- [Deploying a Project](https://docs.kennel.scottylabs.org/guides/deploying.html)
- [Secrets](https://docs.kennel.scottylabs.org/guides/secrets.html)
- [Flake Options Reference](https://docs.kennel.scottylabs.org/reference/flake-options.html)
- [Build Helpers](https://docs.kennel.scottylabs.org/reference/build-helpers.html)

`flake-module/default.nix` defines the flake-level outputs, while everything a project configures lives as `scottylabs.*` options in the perSystem modules under `flake-module/per-system/`.

## Adding a language module

Create `flake-module/per-system/<lang>.nix` and add it to `flake-module/per-system/default.nix`.

A language module should:

- Gate on `cfg.enable`
- Add tools with `scottylabs.shell.packages` and variables with `scottylabs.shell.env`
- Put caches under `$SCOTTYLABS_STATE` from `scottylabs.shell.hook` (e.g. `DENO_DIR`, `CARGO_TARGET_DIR`), since the checkout path is only known on shell entry
- Register formatters under `treefmt.programs`
- Register linters and tests as `pre-commit.settings.hooks`, which run on every commit and in the `hooks` flake check
- Stage anything those hooks fetch from the network under `scottylabs.hooksCheck` (`setup`, `nativeBuildInputs`, `buildInputs`), since the `hooks` check runs in the build sandbox

## Adding a service module

Create `flake-module/per-system/<service>.nix` and add it to `flake-module/per-system/default.nix`.

A service module should:

- Define its processes under `scottylabs.processes`, keeping sockets under `$SCOTTYLABS_RUNTIME` and data under `$SCOTTYLABS_STATE`
- Export the same env vars that kennel injects in production (e.g. `DATABASE_URL`, `S3_ENDPOINT`), so the application is environment-agnostic and driven entirely by env var changes ([12factor](https://12factor.net/config))
- Use `scottylabs.project.name` for database/bucket naming
- Add itself to `scottylabs.kennel.resources` so kennel provisions it for each deployment

## Adding a check

- A fast check that only reads the source, such as `cargo-machete` or `gitleaks`, goes in `pre-commit.settings.hooks`. It runs on every commit and again in nixbot's `hooks` check.
- A check that compiles against a dependency build goes in the target's `passthru.checks`, like clippy and `cargo test` in `buildRustService`. Add its hook to `scottylabs.hooksCheck.skip` so the `hooks` check skips it.
- A check that needs the network, such as a vulnerability scan, goes in `scottylabs.ci.checks`, which the Forgejo Actions workflow runs with `nix run .#ci`.

## Adding a build helper

Add a file under `lib/` and add it to `lib/default.nix`. The helper receives the project's `pkgs`, which comes from kennel's nixpkgs, and returns a package for a target's `package`. Document it at `sites/docs/src/reference/build-helpers.md`.
