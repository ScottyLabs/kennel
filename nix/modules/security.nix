{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.security;

  patchelfIsBroken = pkgs.stdenv.hostPlatform.isLinux && pkgs.patchelf.version == "0.19.2";

  patchelfStable = pkgs.patchelf.overrideAttrs (
    finalAttrs: _: {
      version = "0.15.2";
      src = pkgs.fetchurl {
        url = "https://github.com/NixOS/patchelf/releases/download/${finalAttrs.version}/patchelf-${finalAttrs.version}.tar.bz2";
        hash = "sha256-F3RfVkFZyOIo/EEtplogSLhGxLa0Igt3y/IkFuAvLXw=";
      };
    }
  );

  semgrepCore = pkgs.semgrep-core.override (
    lib.optionalAttrs patchelfIsBroken { patchelf = patchelfStable; }
  );

  # TODO: https://github.com/NixOS/nixpkgs/pull/569851
  semgrepPkg =
    let
      base = pkgs.semgrep.override { semgrep-core = semgrepCore; };
    in
    if pkgs.stdenv.hostPlatform.isLinux && pkgs.stdenv.hostPlatform.isAarch64 then
      base.overridePythonAttrs (_: {
        doCheck = false;
      })
    else
      base;
in
{
  options.scottylabs.security = {
    osvScanner.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Scan lockfiles (`Cargo.lock`, `deno.lock`, `uv.lock`, ...) against the
        [OSV.dev](https://osv.dev) vulnerability database via
        [osv-scanner](https://github.com/google/osv-scanner).
      '';
    };

    semgrep.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Run [semgrep](https://semgrep.dev)'s default (`auto`) ruleset,
        excluding `.forgejo`.
      '';
    };

    vulnix.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Scan the devenv shell's Nix closure against NVD CVEs via
        [vulnix](https://github.com/nix-community/vulnix). Reports false
        positives without `vulnix.whitelist`.
      '';
    };

    vulnix.whitelist = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        Path or URL to a TOML whitelist of accepted CVE matches, passed as
        `vulnix -w`.
      '';
    };
  };

  config = lib.mkIf config.scottylabs.enable {
    packages =
      lib.optional cfg.osvScanner.enable pkgs.osv-scanner
      ++ lib.optional cfg.semgrep.enable semgrepPkg
      ++ lib.optional cfg.vulnix.enable pkgs.vulnix;

    tasks = lib.mkMerge [
      (lib.mkIf cfg.osvScanner.enable {
        # TODO: deno.lock is unsupported
        "security:osv-scanner".exec = ''
          rc=0
          ${pkgs.osv-scanner}/bin/osv-scanner scan source . || rc=$?
          [ "$rc" -eq 0 ] || [ "$rc" -eq 128 ]
        '';
      })
      (lib.mkIf cfg.semgrep.enable {
        "security:semgrep".exec = "${semgrepPkg}/bin/semgrep --config auto --error --exclude '.forgejo' .";
      })
      (lib.mkIf cfg.vulnix.enable {
        "security:vulnix".exec = "${pkgs.vulnix}/bin/vulnix${
          lib.optionalString (cfg.vulnix.whitelist != null) " -w ${cfg.vulnix.whitelist}"
        } \"$DEVENV_PROFILE\"";
      })
    ];
  };
}
