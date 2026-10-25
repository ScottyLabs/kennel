kennelInputs:
{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.rust;
  projectName = config.scottylabs.project.name;
  inherit (pkgs.stdenv) hostPlatform;

  rustBin = kennelInputs.rust-overlay.lib.mkRustBin { } pkgs.buildPackages;
  toolchain = rustBin.selectLatestNightlyWith (
    t:
    t.minimal.override {
      extensions = [
        "clippy"
        "rustfmt"
        "rust-analyzer"
        "rust-src"
        "llvm-tools-preview"
        "rustc-codegen-cranelift-preview"
      ];
    }
  );

  # TODO: wild needs a GCC with native support before it can use -fuse-ld=wild
  linkerFlags =
    if hostPlatform.isLinux then
      "-C linker-features=-lld -C link-arg=-B${pkgs.wild}/bin"
      + lib.optionalString hostPlatform.isAarch64 " -Z unstable-options"
    else
      "-C link-arg=-fuse-ld=lld";

  cargoConfig = (pkgs.formats.toml { }).generate "cargo-config.toml" {
    profile.dev.package = lib.genAttrs cfg.cranelift.excludePackages (_: {
      codegen-backend = "llvm";
    });
  };
in
{
  options.scottylabs.rust = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable the Rust toolchain. Adds nightly Rust with
        [cranelift](https://github.com/rust-lang/rustc_codegen_cranelift),
        clippy, rustfmt, the [wild](https://github.com/davidlattimore/wild)
        linker, and [sccache](https://github.com/mozilla/sccache). Runs clippy,
        `cargo test`, and [cargo-machete](https://github.com/bnjbvr/cargo-machete)
        on every commit.
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
      description = "Crates built with LLVM instead of cranelift";
    };

    toolchain = lib.mkOption {
      type = lib.types.package;
      default = toolchain;
      defaultText = lib.literalMD "the latest nightly with every component the shell needs";
      readOnly = true;
      description = "The Rust toolchain in the dev shell";
    };
  };

  config = lib.mkIf cfg.enable {
    scottylabs.shell = {
      packages = [
        toolchain
        pkgs.sccache
      ]
      ++ (
        if hostPlatform.isLinux then
          [
            pkgs.wild
            pkgs.clang
          ]
        else
          [ pkgs.llvmPackages.bintools ]
      )
      ++ lib.optional hostPlatform.isDarwin pkgs.libiconv
      ++ cfg.nativeBuildInputs;

      env = {
        CARGO_PROFILE_DEV_DEBUG = "0";
        CARGO_UNSTABLE_CODEGEN_BACKEND = "true";
        CARGO_PROFILE_DEV_CODEGEN_BACKEND = "cranelift";
        CARGO_PROFILE_DEV_BUILD_OVERRIDE_CODEGEN_BACKEND = "llvm";
        RUSTFLAGS = linkerFlags;
        RUSTDOCFLAGS = linkerFlags;
        RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
        RUST_LOG = "${builtins.replaceStrings [ "-" ] [ "_" ] projectName}=debug";
        RUSTC_WRAPPER = lib.getExe pkgs.sccache;
        SCCACHE_BUCKET = "sccache";
        SCCACHE_ENDPOINT = "https://s3.scottylabs.org";
        SCCACHE_REGION = "us-east-1";
      }
      // lib.optionalAttrs hostPlatform.isLinux {
        "CARGO_TARGET_${hostPlatform.rust.cargoEnvVarTarget}_LINKER" = lib.getExe pkgs.clang;
      };

      # Default to the read-only sccache key, upgrading to read-write when OpenBao answers
      hook = ''
        export CARGO_TARGET_DIR="$SCOTTYLABS_STATE/target"
        export CARGO_INSTALL_ROOT="$SCOTTYLABS_STATE/cargo-install"
        export PATH="$PATH:$CARGO_INSTALL_ROOT/bin:''${CARGO_HOME:-$HOME/.cargo}/bin"

        if [ -z "''${AWS_ACCESS_KEY_ID:-}" ]; then
          export AWS_ACCESS_KEY_ID=GK86a1015b84fc446d260e66b9
          export AWS_SECRET_ACCESS_KEY=298e02b2ee7eb97c8112ee97876feea9894e7916457efee2fbf255ffdb676b38 # gitleaks:allow nosemgrep: detected-aws-secret-access-key
          if _ak=$(${lib.getExe' pkgs.openbao "bao"} kv get -field=AWS_ACCESS_KEY_ID secret/shared/sccache 2>/dev/null); then
            export AWS_ACCESS_KEY_ID=$_ak
            export AWS_SECRET_ACCESS_KEY=$(${lib.getExe' pkgs.openbao "bao"} kv get -field=AWS_SECRET_ACCESS_KEY secret/shared/sccache)
          fi
          unset _ak
        fi
      '';

      files.".cargo/config.toml" = cargoConfig;
    };

    treefmt.programs.rustfmt.enable = true;

    # buildRustService checks run these against cached dependencies
    scottylabs.hooksCheck.skip = [
      "clippy"
      "cargo-test"
    ];

    pre-commit.settings.hooks = {
      clippy = {
        enable = true;
        packageOverrides = {
          cargo = toolchain;
          clippy = toolchain;
        };
        settings.offline = false;
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
        entry = lib.getExe pkgs.cargo-machete;
        files = "Cargo\\.toml$";
        pass_filenames = false;
        language = "system";
      };
    };
  };
}
