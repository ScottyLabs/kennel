{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.rust;
  projectName = config.scottylabs.project.name;
in
{
  options.scottylabs.rust = {
    enable = lib.mkEnableOption "Rust development toolchain";

    nativeBuildInputs = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      description = "Extra native build inputs (e.g. pkg-config, openssl for crates that link C libraries)";
    };

    cranelift.excludePackages = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "aws-lc-sys"
        "aws-lc-rs"
        "rustls"
      ];
      description = "Crate names forced to LLVM backend when using cranelift";
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [ pkgs.sccache ] ++ cfg.nativeBuildInputs;

    env = {
      CARGO_PROFILE_DEV_DEBUG = "0";
      CARGO_TARGET_DIR = "${config.devenv.root}/.devenv/state/target";
      RUST_LOG = "${builtins.replaceStrings [ "-" ] [ "_" ] projectName}=debug";
      RUSTC_WRAPPER = "${pkgs.sccache}/bin/sccache";
      SCCACHE_BUCKET = "sccache";
      SCCACHE_ENDPOINT = "https://s3.scottylabs.org";
      SCCACHE_REGION = "us-east-1";
    };

    # languages.rust pulls in languages.c (valgrind, gdb, ccls)
    languages.c.enable = lib.mkForce false;

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
      # TODO: wild does not yet support macOS, use lld
      lld.enable = pkgs.stdenv.isDarwin;
      wild.enable = pkgs.stdenv.isLinux;
      cranelift = {
        enable = true;
        forceBuildScriptsLlvm = true;
        excludePackages = cfg.cranelift.excludePackages;
      };
    };

    # fetch sccache S3 creds from OpenBao unless CI already set them
    enterShell = ''
      if [ -z "''${AWS_ACCESS_KEY_ID:-}" ]; then
        export AWS_ACCESS_KEY_ID=$(${pkgs.openbao}/bin/bao kv get -field=AWS_ACCESS_KEY_ID secret/shared/sccache)
        export AWS_SECRET_ACCESS_KEY=$(${pkgs.openbao}/bin/bao kv get -field=AWS_SECRET_ACCESS_KEY secret/shared/sccache)
      fi
    '';

    treefmt.config.programs.rustfmt.enable = true;

    git-hooks.hooks = {
      clippy = {
        enable = true;
        packageOverrides.cargo = config.languages.rust.toolchainPackage;
        packageOverrides.clippy = config.languages.rust.toolchainPackage;
      };
      cargo-test = {
        enable = true;
        name = "cargo-test";
        entry = "cargo test";
        files = "\\.(rs|toml)$";
        pass_filenames = false;
        language = "system";
      };
    };
  };
}
