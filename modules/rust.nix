{ pkgs, lib, config, ... }:

let
  cfg = config.scottylabs.rust;
  projectName = config.scottylabs.project.name;
in
{
  options.scottylabs.rust = {
    enable = lib.mkEnableOption "Rust development toolchain";

    cranelift.excludePackages = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "aws-lc-sys" "aws-lc-rs" "rustls" ];
      description = "Crate names forced to LLVM backend when using cranelift";
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = with pkgs; [
      pkg-config
      openssl
    ];

    env = {
      CARGO_PROFILE_DEV_DEBUG = "0";
      RUST_LOG = "${builtins.replaceStrings ["-"] ["_"] projectName}=debug";
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
      # TODO: wild does not yet support macOS, use lld
      lld.enable = pkgs.stdenv.isDarwin;
      wild.enable = pkgs.stdenv.isLinux;
      cranelift = {
        enable = true;
        forceBuildScriptsLlvm = true;
        excludePackages = cfg.cranelift.excludePackages;
      };
    };

    treefmt.config.programs.rustfmt.enable = true;

    git-hooks.hooks = {
      clippy = {
        enable = true;
        packageOverrides.cargo = config.languages.rust.toolchainPackage;
        packageOverrides.clippy = config.languages.rust.toolchainPackage;
      };
    };
  };
}
