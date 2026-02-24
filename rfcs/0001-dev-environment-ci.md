# RFC 0001: Dev Environment & CI

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-02-24
- **Updated:** 2026-02-24

## Overview

Define the development environment, build system, and CI pipeline for Kennel. This is the foundational scaffolding that must exist before any feature work begins.

## Motivation

Kennel is a Rust project packaged with Nix and deployed as a NixOS module. Before building any components, we need a consistent dev environment, a reproducible build, and CI that catches issues early. Getting this right upfront avoids rework later.

## Goals

- Reproducible dev environment via devenv
- Nix packaging via `crate2nix` for fine-grained per-crate caching
- Forgejo Actions CI pipeline for automated checks on every push and PR
- Formatting and linting enforced from day one via treefmt

## Non-Goals

- NixOS module configuration (that's a separate RFC)
- Dashboard frontend build setup (Svelte tooling comes later)
- Deployment to `deploy-01` (handled by comin once the NixOS module exists)

## Detailed Design

### Nix Flake

The flake uses devenv for the development environment and `crate2nix` for building. This mirrors the setup used by Terrier.

```nix
{
  description = "Kennel";

  nixConfig = {
    extra-substituters = [ "https://scottylabs.cachix.org" ];
    extra-trusted-public-keys = [
      "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
    ];
  };

  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    devenv.url = "github:cachix/devenv";
  };

  outputs = { self, nixpkgs, devenv, ... }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
          cargoNix = pkgs.callPackage ./Cargo.nix { };
          kennel = cargoNix.rootCrate.build;
        in
        {
          inherit kennel;
          default = kennel;
          devenv = devenv.packages.${system}.devenv;
        }
      );

      nixosModules.default = import ./nixos;
    };
}
```

`crate2nix` generates a `Cargo.nix` file that builds each crate individually, giving Nix fine-grained caching at the per-crate level. This means changing one source file only rebuilds the affected crate(s), not the entire dependency tree. The `Cargo.nix` file is checked into the repo and regenerated automatically via a git hook.

### devenv.nix

The development environment follows the same conventions as Terrier:

```nix
{ pkgs, config, ... }:

let
  cargoNix = pkgs.callPackage ./Cargo.nix { };
  kennel = cargoNix.rootCrate.build;
in
{
  cachix.pull = [ "scottylabs" ];

  packages = [
    kennel
  ] ++ (with pkgs; [
    pkg-config
    openssl
    postgresql_18
    sea-orm-cli
  ]);

  outputs = { inherit kennel; };

  env = {
    DATABASE_URL = "postgres:///kennel?host=$PGHOST";
    RUST_LOG = "kennel=debug";
  };

  languages.rust = {
    enable = true;
    channel = "nightly";
    components = [
      "rustc"
      "cargo"
      "clippy"
      "rustfmt"
      "rust-analyzer"
      "rust-src"
      "llvm-tools-preview"
    ];
    mold.enable = pkgs.stdenv.isLinux;
    rustflags = "-Zthreads=8";
  };

  services.postgres = {
    enable = true;
    package = pkgs.postgresql_18;
    listen_addresses = "127.0.0.1";
    port = 5432;
    initialDatabases = [
      { name = "kennel"; }
    ];
  };

  claude.code.enable = true;

  treefmt = {
    enable = true;
    config.programs = {
      nixpkgs-fmt = {
        enable = true;
        excludes = [ "Cargo.nix" ];
      };
      rustfmt.enable = true;
      mdformat.enable = true;
    };
  };

  git-hooks.hooks = {
    treefmt.enable = true;
    clippy = {
      enable = true;
      packageOverrides.cargo = config.languages.rust.toolchainPackage;
      packageOverrides.clippy = config.languages.rust.toolchainPackage;
    };
    cargo-nix-update = {
      enable = true;
      name = "cargo-nix-update";
      entry = "${pkgs.writeShellScript "cargo-nix-update" ''
        if git diff --cached --name-only | grep -q '^Cargo\.\(toml\|lock\)'; then
          ${pkgs.crate2nix}/bin/crate2nix generate
          git add Cargo.nix
        fi
      ''}";
      files = "Cargo\\.(toml|lock)$";
      language = "system";
      pass_filenames = false;
    };
  };
}
```

Key features:

- **Nightly Rust** with cranelift codegen backend for faster dev builds
- **PostgreSQL 18** via devenv services — starts automatically with `devenv up`
- **treefmt** for unified formatting across Nix, Rust, and Markdown
- **Git hooks**: treefmt on commit, clippy on commit, and automatic `Cargo.nix` regeneration when `Cargo.toml` or `Cargo.lock` change
- **Cachix** pulls from `scottylabs` for shared build artifacts

### .cargo/config.toml

Compilation speed settings that complement the devenv configuration:

```toml
# lld on macOS (devenv's mold.enable handles Linux)
[target.aarch64-apple-darwin]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]

[target.x86_64-apple-darwin]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]

[unstable]
codegen-backend = true
```

The cranelift backend is set via `CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift` in devenv. Specific crates that are incompatible with cranelift (e.g., FFI-heavy crypto crates) can be forced to LLVM via `[profile.dev.package.<crate>] codegen-backend = "llvm"` as needed.

### Cargo Workspace

Kennel is structured as a Cargo workspace with per-component crates. This pairs well with `crate2nix`, which builds each crate as its own Nix derivation — changing one crate only rebuilds that crate and its dependents, not the entire project.

```
crates/
├── kennel/          # binary — main.rs, CLI, wiring
├── kennel-api/      # axum routes, dashboard API
├── kennel-builder/  # nix build orchestration, cachix
├── kennel-config/   # kennel.toml parsing, shared types
├── kennel-deployer/ # systemd, static sites, port allocation
├── kennel-dns/      # Cloudflare API
├── kennel-router/   # reverse proxy, host-header routing
├── kennel-secrets/  # OpenBao client
├── kennel-store/    # database queries, repository layer
├── kennel-webhook/  # forgejo/github event parsing
├── entity/          # SeaORM generated entities
└── migration/       # SeaORM migrations
```

The `kennel` binary crate wires everything together. Shared types (project names, deployment IDs, domain types) live in `kennel-config`. Database access goes through `kennel-store`, which depends on `entity`. Each component crate has a focused dependency set — `kennel-dns` only pulls in `reqwest`, `kennel-deployer` only pulls in `tokio::process`, etc.

### Forgejo Actions CI

CI runs on every push and PR. The workflow file lives at `.forgejo/workflows/ci.yml`:

```yaml
name: CI
on:
  push:
    branches: [main, staging, dev]
  pull_request:

jobs:
  check:
    runs-on: docker
    steps:
      - uses: actions/checkout@v4

      - name: Check formatting
        run: nix develop -c treefmt --fail-on-change

      - name: Clippy
        run: nix develop -c cargo clippy -- -D warnings

      - name: Test
        run: nix develop -c cargo test

      - name: Verify Cargo.nix is up to date
        run: |
          nix develop -c crate2nix generate
          git diff --exit-code Cargo.nix

      - name: Build
        run: nix build
```

All CI steps run inside `nix develop` to ensure the same toolchain as local development. The `nix build` step at the end verifies the Nix package builds successfully — this is what comin will use to deploy. The `Cargo.nix` freshness check ensures nobody forgets to regenerate after changing dependencies.

The `runs-on: docker` label targets the Forgejo runner with Nix available.

### Database Migrations

SeaORM is used as the ORM and query layer. Migrations are managed via `sea-orm-cli` and live in a `migration` crate (SeaORM's standard convention). Migrations are written in Rust, providing type safety and the ability to use SeaORM's schema builder API.

New migrations are created with `sea-orm-cli migrate generate <name>` and run with `sea-orm-cli migrate up`. In production, Kennel runs pending migrations on startup.

### NixOS Module Stub

The `nixos/` directory contains a stub module that will be fleshed out in a later RFC:

```nix
# nixos/default.nix
{ config, lib, pkgs, ... }:

{
  options.services.kennel = {
    enable = lib.mkEnableOption "Kennel deployment platform";
  };

  config = lib.mkIf config.services.kennel.enable {
    # TODO: systemd service, nginx config, postgres setup
  };
}
```

This is enough for `nix build` to succeed and for the flake to export `nixosModules.default`.

## Alternatives Considered

**`rustPlatform.buildRustPackage`** — Simpler but rebuilds the entire crate graph on any change. `crate2nix` gives per-crate granularity, so changing application code doesn't rebuild all dependencies. Worth the complexity given how frequently we'll iterate.

**crane or naersk** — These provide incremental Rust builds in Nix but at a coarser granularity than `crate2nix` (typically splitting into deps + source layers). `crate2nix` is the most granular option and matches the existing Terrier setup.

**Single crate instead of workspace** — Simpler to set up, but wastes the per-crate caching that `crate2nix` provides. With a single crate, any source change rebuilds the entire Nix derivation. A workspace gives both Nix and Cargo finer-grained incremental compilation boundaries.

**SQLx** — Compile-time checked queries are appealing, but require either a live database or checked-in offline data (`.sqlx/` directory) during builds. SeaORM provides a more conventional ORM experience with its own migration system (`sea-orm-cli`), entity generation, and query builder. It also matches the Terrier codebase, keeping patterns consistent across ScottyLabs projects.

**GitHub Actions (mirrored repo)** — Not needed. Forgejo Actions covers our requirements and keeps everything on our own infrastructure.

## Open Questions

None.

## Implementation Phases

1. Initialize Cargo project and generate `Cargo.nix` via `crate2nix`
1. Write `flake.nix`, `devenv.nix`, and `devenv.yaml`
1. Add `.forgejo/workflows/ci.yml`
1. Add NixOS module stub
1. Set up SeaORM migration crate with initial schema
1. Verify `devenv shell`, `cargo build`, `cargo test`, and `nix build` all work
