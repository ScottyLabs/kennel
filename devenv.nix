{ pkgs, config, inputs, ... }:

let
  cargoNix = pkgs.callPackage ./Cargo.nix {
    defaultCrateOverrides = pkgs.defaultCrateOverrides // {
      libdbus-sys = attrs: {
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.dbus ];
      };
    };
  };
  kennel = cargoNix.workspaceMembers.kennel.build;
in
{
  packages = [
    kennel
    inputs.bun2nix.packages.${pkgs.stdenv.system}.default
  ] ++ (with pkgs; [
    pkg-config
    openssl
    dbus
    postgresql_18
    sea-orm-cli
    mdbook
    bun
    just
  ]);

  outputs = { inherit kennel; };

  env = {
    CARGO_PROFILE_DEV_DEBUG = "0";
    DATABASE_URL = "postgresql://127.0.0.1:5432/kennel";
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
    lld.enable = pkgs.stdenv.isDarwin;
    wild.enable = pkgs.stdenv.isLinux;
    cranelift = {
      enable = true;
      forceBuildScriptsLlvm = true;
      excludePackages = [ "aws-lc-sys" "aws-lc-rs" "rustls" "linkme" "secretspec" ];
    };
  };

  services.postgres = {
    enable = true;
    package = pkgs.postgresql_18;
    extensions = e: [ e.pg_uuidv7 ];
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
        excludes = [ "Cargo.nix" "bun.nix" ];
      };
      rustfmt.enable = true;
      mdformat = {
        enable = true;
        excludes = [ "sites/docs/src/content/**" ];
      };
    };
    config.settings.formatter.biome = {
      command = "${pkgs.biome}/bin/biome";
      options = [ "check" "--write" "--no-errors-on-unmatched" "--config-path" "${config.devenv.root}/biome.json" ];
      includes = [ "*.js" "*.ts" "*.mjs" "*.mts" "*.cjs" "*.cts" "*.jsx" "*.tsx" "*.d.ts" "*.d.cts" "*.d.mts" "*.json" "*.jsonc" "*.css" ];
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
