# Deploying a Project

This guide walks through setting up a ScottyLabs project for deployment with kennel.

## Prerequisites

- A repository in the ScottyLabs Forgejo organization
- [devenv](https://devenv.sh) installed locally
- A `flake.nix` and `devenv.nix` in your project root

## 1. Import the shared module

Add the ScottyLabs devenv input to your `devenv.yaml`:

```yaml
inputs:
  scottylabs:
    url: github:ScottyLabs/devenv
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
  package = pkgs.stdenv.mkDerivation {
    pname = "my-project-docs";
    version = "0.1.0";
    src = ./sites/docs;
    nativeBuildInputs = [ pkgs.mdbook ];
    buildPhase = "mdbook build";
    installPhase = "mkdir -p $out && cp -r book/* $out/";
  };
  spa = false;
};
```

## 4. Enable infrastructure

If your project needs a database:

```nix
scottylabs.postgres.enable = true;
```

This gives you a local PostgreSQL instance in development and a provisioned per-deployment database in production. Your app reads `DATABASE_URL` from the environment in both cases.

## 5. Enable kennel in governance

In the ScottyLabs governance repository, set the kennel flag for your project. Governance provisions the webhook that connects your repository to kennel.

## 6. Push

Push to any branch. Kennel receives the webhook, builds your project, and deploys it. Your deployment will be available at:

- `my-project-main.scottylabs.net` for the main branch
- `my-project-pr-42.scottylabs.net` for PR #42
- `my-project-feature-x.scottylabs.net` for a feature branch

## Flake packages

Your `flake.nix` must expose packages that kennel can build. The package names must match the keys in `scottylabs.kennel.services` and `scottylabs.kennel.sites`:

```nix
packages = forAllSystems (system:
  let pkgs = pkgsFor system;
  in {
    api = cargoNix.workspaceMembers.my-project.build;
    docs = pkgs.stdenv.mkDerivation { ... };
    default = self.packages.${system}.api;
  }
);
```

Kennel builds each package with `nix build .#packages.{system}.{name}`.
