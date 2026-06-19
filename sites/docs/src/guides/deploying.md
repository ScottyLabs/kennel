# Deploying a Project

This guide walks through setting up a ScottyLabs project for deployment with kennel.

## Prerequisites

- A repository in the ScottyLabs Forgejo organization
- [devenv](https://devenv.sh) and [direnv](https://direnv.net/) installed locally
- A `flake.nix` and `devenv.nix` in your project root

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

If your service needs OIDC, add `oidc_client` to its `features` in governance (and `admin_client` if it needs a privileged service-account client). Governance provisions the Keycloak clients and writes the credentials to Vault.

Your service receives the client credentials and Keycloak connection settings as env vars, declared in your `secretspec.toml`:

```toml
[profiles.prod]
OIDC_CLIENT_ID = { description = "Keycloak OIDC client ID" }
OIDC_CLIENT_SECRET = { description = "Keycloak OIDC client secret" }
KEYCLOAK_URL = { description = "Keycloak base URL" }
KEYCLOAK_REALM = { description = "Keycloak realm" }
OAUTH_RELAY_URL = { description = "OAuth relay callback URL" }
```

For a static site:

```nix
scottylabs.kennel.sites.docs = {
  spa = false;
};
```

The site name (`docs`) must match a package in your `flake.nix` outputs. Kennel builds it with `nix build .#packages.{system}.docs`.

### Runtime environment

Kennel injects these variables into every backend service it deploys:

- `PORT`: the port your service must bind to. Kennel allocates it and routes the public domain to it through Caddy, so read it at startup instead of hardcoding a port.
- `COMMIT_HASH`: the full Git commit SHA of the running build.

Resolved secrets from your `secretspec.toml` are injected alongside these.

## 4. Enable infrastructure

If your project needs a database:

```nix
scottylabs.postgres.enable = true;
```

This gives you a local PostgreSQL instance in development and a provisioned per-deployment database in production. Your app reads `DATABASE_URL` from the environment in both cases.

## 5. Register your repository in governance

In the ScottyLabs governance repository, add your repo to its team's TOML file and list its `features`. Governance provisions everything those features imply.

`features` is an array of:

- `kennel` provisions the webhook that connects your repository to kennel for builds and deployments
- `sentry` creates a Sentry project and writes its DSN to Vault
- `oidc_client` provisions prod and staging Keycloak OIDC clients (redirect URI fixed at `/oauth2/callback`) and writes `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, `KEYCLOAK_URL`, `KEYCLOAK_REALM`, and `OAUTH_RELAY_URL` to Vault for each profile
- `admin_client` provisions a Keycloak service-account client with the `view-users` and `manage-users` roles, written to Vault as `KEYCLOAK_ADMIN_CLIENT_ID` and `KEYCLOAK_ADMIN_CLIENT_SECRET`

Documentation is controlled separately by `docs` (boolean, default `true`), which builds the repository's `./docs` directory into the documentation hub.

## 6. Push

Push to any branch. Kennel receives the webhook, builds your project, and deploys it. Your deployment will be available at:

- `my-project-main.scottylabs.net` for the main branch
- `my-project-pr-42.scottylabs.net` for PR #42
- `my-project-feature-x.scottylabs.net` for a feature branch

## Flake packages

Your `flake.nix` must expose packages that kennel can build. The package names must match the keys in `scottylabs.kennel.services` and `scottylabs.kennel.sites`. For Rust projects, the supported pattern is [crane](https://crane.dev):

```nix
inputs = {
  nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
  crane.url = "github:ipetkov/crane";
};

outputs = { self, nixpkgs, crane, ... }:
  let
    forAllSystems = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ];
  in {
    packages = forAllSystems (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;
        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
        };
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      in {
        api = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "api";
          cargoExtraArgs = "-p my-project";
          doCheck = false;
        });
        docs = pkgs.stdenv.mkDerivation { ... };
        default = self.packages.${system}.api;
      }
    );
  };
```

Kennel builds each package with `nix build .#packages.{system}.{name}`.
