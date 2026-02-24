{ pkgs, config, ... }:

let
  cargoNix = pkgs.callPackage ./Cargo.nix { };
  kennel = cargoNix.workspaceMembers.kennel.build;
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
    CARGO_PROFILE_DEV_DEBUG = "0";
    CARGO_PROFILE_DEV_CODEGEN_BACKEND = "cranelift";
    CARGO_PROFILE_DEV_BUILD_OVERRIDE_CODEGEN_BACKEND = "llvm";

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
      "rustc-codegen-cranelift-preview"
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
