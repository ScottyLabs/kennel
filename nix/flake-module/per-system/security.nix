{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.security;

  # TODO: https://github.com/NixOS/nixpkgs/pull/569851
  semgrep =
    if pkgs.stdenv.hostPlatform.isLinux && pkgs.stdenv.hostPlatform.isAarch64 then
      pkgs.semgrep.overridePythonAttrs (_: {
        doCheck = false;
      })
    else
      pkgs.semgrep;

  targets = map (t: t.package) (
    builtins.attrValues config.services ++ builtins.attrValues config.sites
  );
in
{
  options.scottylabs.security = {
    osvScanner.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Scan lockfiles (`Cargo.lock`, `uv.lock`, ...) against the
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
        Scan the runtime closure of every service and site against NVD CVEs
        via [vulnix](https://github.com/nix-community/vulnix). List accepted
        matches in `vulnix.whitelist`.
      '';
    };

    vulnix.whitelist = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Path or URL to a TOML whitelist of accepted CVE matches, passed as `vulnix -w`";
    };
  };

  config.scottylabs.ci.checks = lib.mkMerge [
    (lib.mkIf cfg.osvScanner.enable {
      # TODO: deno.lock is unsupported
      osv-scanner = ''
        rc=0
        ${lib.getExe pkgs.osv-scanner} scan source . || rc=$?
        [ "$rc" -eq 0 ] || [ "$rc" -eq 128 ]
      '';
    })
    (lib.mkIf cfg.semgrep.enable {
      semgrep = "${lib.getExe semgrep} --config auto --error --exclude .forgejo .";
    })
    (lib.mkIf (cfg.vulnix.enable && targets != [ ]) {
      vulnix = "${lib.getExe pkgs.vulnix} --closure ${
        lib.optionalString (cfg.vulnix.whitelist != null) "-w ${lib.escapeShellArg cfg.vulnix.whitelist}"
      } ${lib.escapeShellArgs targets}";
    })
  ];
}
