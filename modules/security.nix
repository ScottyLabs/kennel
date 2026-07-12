{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.security;

  # semgrep's own pytest suite fails with BrokenPipeError inside the Nix
  # sandbox on some nixpkgs revs, which otherwise blocks the whole devenv
  # shell from realizing. Skip its tests; we only need the built binary.
  patchedPkgs = pkgs.extend (
    final: prev: {
      semgrep = prev.semgrep.overrideAttrs (_: { doCheck = false; });
    }
  );
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
        Run [semgrep](https://semgrep.dev)'s default (`auto`) ruleset for
        common security footguns, excluding `.forgejo/workflows`.
      '';
    };

    vulnix.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Scan the devenv shell's Nix closure (`$DEVENV_PROFILE`) against NVD
        CVEs via [vulnix](https://github.com/nix-community/vulnix), covering
        nixpkgs-pinned dependencies like C libraries and interpreters that
        `osv-scanner`'s lockfile scan can't see. Off by default: vulnix's
        CVE matching is a coarse heuristic on package names and reports
        false positives without a whitelist (`vulnix.whitelist`).
      '';
    };

    vulnix.whitelist = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        Path or URL to a TOML whitelist of accepted CVE matches, passed as
        `vulnix -w`. Generate a starting point with `vulnix -W
        whitelist.toml` after an initial scan.
      '';
    };
  };

  config = lib.mkIf config.scottylabs.enable {
    packages =
      lib.optional cfg.osvScanner.enable pkgs.osv-scanner
      ++ lib.optional cfg.semgrep.enable patchedPkgs.semgrep
      ++ lib.optional cfg.vulnix.enable pkgs.vulnix;

    tasks = lib.mkMerge [
      (lib.mkIf cfg.osvScanner.enable {
        "security:osv-scanner".exec = "${pkgs.osv-scanner}/bin/osv-scanner scan source .";
      })
      (lib.mkIf cfg.semgrep.enable {
        "security:semgrep".exec =
          "${patchedPkgs.semgrep}/bin/semgrep --config auto --error --exclude '.forgejo/workflows' .";
      })
      (lib.mkIf cfg.vulnix.enable {
        "security:vulnix".exec = "${pkgs.vulnix}/bin/vulnix${
          lib.optionalString (cfg.vulnix.whitelist != null) " -w ${cfg.vulnix.whitelist}"
        } \"$DEVENV_PROFILE\"";
      })
    ];
  };
}
