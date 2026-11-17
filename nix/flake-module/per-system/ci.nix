{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.ci;

  run = pkgs.writeShellApplication {
    name = "ci";
    text = ''
      failed=0
      ${lib.concatStrings (
        lib.mapAttrsToList (name: body: ''
          echo "::group::${name}"
          if ! ${
            lib.getExe (
              pkgs.writeShellApplication {
                inherit name;
                text = body;
              }
            )
          }; then
            echo "${name} failed" >&2
            failed=1
          fi
          echo "::endgroup::"
        '') cfg.checks
      )}
      exit "$failed"
    '';
  };
in
{
  options.scottylabs.ci.checks = lib.mkOption {
    type = lib.types.attrsOf lib.types.lines;
    default = { };
    description = ''
      Checks that need network access, as shell scripts run by `nix run .#ci`.
    '';
  };

  options.scottylabs.ci.system = lib.mkOption {
    type = lib.types.str;
    default = "x86_64-linux";
    description = "System that gets all checks, with other systems getting only `devShell`.";
  };

  config.scottylabs.ci.checks.links = "${lib.getExe pkgs.lychee} --no-progress .";

  config.apps.ci = {
    type = "app";
    program = lib.getExe run;
    meta.description = "Run the project's network checks from the checkout";
  };
}
