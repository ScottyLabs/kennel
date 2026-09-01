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
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable the Rust development toolchain. Configures nightly Rust with
        [cranelift](https://github.com/rust-lang/rustc_codegen_cranelift) (fast
        debug-mode codegen), clippy, rustfmt,
        [wild](https://github.com/davidlattimore/wild)/lld linker, and
        [sccache](https://github.com/mozilla/sccache) backed by the org's S3
        cache. Sets `CARGO_TARGET_DIR` to `.devenv/state/target`. Clippy,
        `cargo test`, and
        [cargo-machete](https://github.com/bnjbvr/cargo-machete) (unused
        dependencies) run on every commit.
      '';
    };

    nativeBuildInputs = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      description = ''
        Extra packages for crates that link C libraries (e.g.
        `[ pkgs.pkg-config pkgs.openssl ]`). Prefer `rustls` over `native-tls`
        for TLS (e.g.
        `reqwest = { default-features = false, features = ["rustls-tls"] }`) to
        avoid needing OpenSSL.
      '';
    };

    cranelift.excludePackages = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "aws-lc-sys"
        "aws-lc-rs"
        "rustls"
      ];
      description = ''
        Crate names forced to the LLVM backend instead of cranelift. Some
        crates use features that cranelift does not support (FFI symbol
        emission, linker sections).
      '';
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [ pkgs.sccache ] ++ cfg.nativeBuildInputs;

    env = {
      CARGO_INCREMENTAL = "0"; # sccache requires incremental compilation to be off
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

    # Default to the read-only sccache key, upgrading to read-write when OpenBao answers
    enterShell = ''
      if [ -z "''${AWS_ACCESS_KEY_ID:-}" ]; then
        export AWS_ACCESS_KEY_ID=GK86a1015b84fc446d260e66b9
        export AWS_SECRET_ACCESS_KEY=298e02b2ee7eb97c8112ee97876feea9894e7916457efee2fbf255ffdb676b38 # gitleaks:allow nosemgrep: detected-aws-secret-access-key
        if _ak=$(${pkgs.openbao}/bin/bao kv get -field=AWS_ACCESS_KEY_ID secret/shared/sccache 2>/dev/null); then
          export AWS_ACCESS_KEY_ID=$_ak
          export AWS_SECRET_ACCESS_KEY=$(${pkgs.openbao}/bin/bao kv get -field=AWS_SECRET_ACCESS_KEY secret/shared/sccache)
        fi
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
      cargo-machete = {
        enable = true;
        name = "cargo-machete";
        entry = "${pkgs.cargo-machete}/bin/cargo-machete";
        files = "Cargo\\.toml$";
        pass_filenames = false;
        language = "system";
      };
    };
  };
}
