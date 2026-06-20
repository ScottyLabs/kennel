# Deploying a Project

This guide walks through setting up a ScottyLabs project for deployment with kennel.

## Prerequisites

- A project registed in governance with an associated Codeberg repository
- [devenv](https://devenv.sh) and [direnv](https://direnv.net/) installed locally

## 1. Import the shared module

Add the ScottyLabs devenv input to your `devenv.yaml`:

```yaml
secretspec:
  enable: true
  provider: vault://secrets2.scottylabs.org/secret
  profile: dev

inputs:
  scottylabs:
    url: git+https://codeberg.org/ScottyLabs/devenv
  rust-overlay:
    url: github:oxalica/rust-overlay
    inputs:
      nixpkgs:
        follows: nixpkgs
  treefmt-nix:
    url: github:numtide/treefmt-nix
  git-hooks:
    url: github:cachix/git-hooks.nix
    inputs:
      nixpkgs:
        follows: nixpkgs
```

Import it in your `devenv.nix`:

```nix
{ pkgs, config, inputs, ... }:
{
  imports = [ inputs.scottylabs.devenvModules.default ];

  scottylabs = {
    enable = true;
    project.name = "my-project";
  };
}
```

The `secretspec` block resolves your project's secrets from OpenBao into the shell. See the [Secrets](./secrets.md) guide to declare and manage them.

## 2. Set up direnv and .gitignore

Create an `.envrc` to automatically activate the devenv environment when you enter the project directory:

```bash
eval "$(devenv direnvrc)"
use devenv
```

Then allow it:

```bash
direnv allow
```

If this command fails with a secret-related error, make sure you have run the one-time [OpenBao setup](./secrets.md).

Add a `.gitignore` for generated and local-only files:

```gitignore
# Nix / devenv
.devenv/
.devenv.flake.nix
.pre-commit-config.yaml
result
result-*

# AI
.mcp.json
.claude

# direnv
.direnv/

# Rust
target/
.cargo/

# Secrets
.env

# OS
.DS_Store
rustc-ice-*.txt
```

Add any project-specific entries as needed (e.g., `sites/docs/book/` for mdbook output, `node_modules/` for JS projects).

## 3. Declare what to deploy

Add kennel options to your `devenv.nix` to tell kennel what your project produces.

For a backend service:

```nix
scottylabs.kennel.services.api = {
  customDomain = "api.my-project.scottylabs.org";
};

processes.api = {
  exec = "${pkgs.my-project}/bin/api";
  ready.http.get = { port = 8080; path = "/health"; };
};
```

For a static site:

```nix
scottylabs.kennel.sites.docs = {
  spa = false;
};
```

Note that custom domains that are not already in use must first have their Cloudflare Zone IDs registered with kennel in the [infrastructure repository](https://codeberg.org/scottylabs/infrastructure).

### Flake packages

Your `flake.nix` must expose these packages, and their names must match the keys in `scottylabs.kennel.services` and `scottylabs.kennel.sites`. Build them with the shared [build helpers](../reference/build-helpers.md):

```nix
inputs = {
  nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
  scottylabs = {
    url = "git+https://codeberg.org/ScottyLabs/devenv";
    inputs.nixpkgs.follows = "nixpkgs";
  };
};

outputs = { self, nixpkgs, scottylabs, ... }:
  let
    forAllSystems = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ];
  in {
    packages = forAllSystems (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        helpers = scottylabs.mkLib pkgs;
      in {
        api = helpers.buildRustService {
          src = ./.;
          pname = "api";
          buildArgs.cargoExtraArgs = "-p api";
        };
        docs = helpers.buildMdbook { src = ./sites/docs; };
        default = self.packages.${system}.api;
      }
    );
  };
```

A Deno or JavaScript front-end uses `buildDenoTask` the same way.

Kennel builds each package with `nix build .#packages.{system}.{name}`.

### Runtime environment

Kennel injects these variables into every backend service it deploys:

- `PORT`: the port your service must bind to. Kennel allocates it and routes the public domain to it through Caddy, so read it at startup instead of hardcoding a port.
- `COMMIT_HASH`: the full Git commit SHA of the running build.
- `APP_URL`: the public URL of this deployment, its custom domain when configured or the generated domain otherwise.

Resolved secrets from your `secretspec.toml` are injected alongside these.

## 4. Enable infrastructure

If your project needs a database:

```nix
scottylabs.postgres.enable = true;
```

This gives you a local PostgreSQL instance in development and a provisioned per-deployment database in production. Your app reads `DATABASE_URL` from the environment in both cases.

`sqlite`, `garage` (S3-compatible object storage), and `valkey` (Redis-compatible key-value store) are also available and documented in the [devenv options](../reference/devenv-options.md). Aside from `garage`, these databases are configured with unix socket auth rather than TCP/password auth.

When developing the OAuth flow locally, enable the relay so the IdP redirects back through it the way it does in production:

```nix
scottylabs.ricochet.enable = true;
scottylabs.ricochet.appUrl = "http://localhost:3000";
```

Governance already seeds the correct `OAUTH_RELAY_URL` for `profiles.dev`, so `enable` only runs Ricochet locally. `appUrl` is your service's local URL, the port your dev server listens on, exported as `APP_URL` so the relay can return to it. Set it only for development: deployed environments receive `APP_URL` from kennel automatically (see [Runtime environment](#runtime-environment)), and it is never declared in `secretspec.toml`.

## 4. Push

Push to any branch. Kennel receives the webhook, builds your project, and deploys it. Your deployment will be available at:

- `my-project-main.scottylabs.net` for the main branch
- `my-project-pr-42.scottylabs.net` for PR #42
- `my-project-feature-x.scottylabs.net` for a feature branch
