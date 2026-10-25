{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.ci;

  run = pkgs.writeShellScript "ci" ''
    failed=0
    ${lib.concatStrings (
      lib.mapAttrsToList (name: body: ''
        echo "::group::${name}"
        if ! (${pkgs.writeShellScript name body}); then
          echo "${name} failed" >&2
          failed=1
        fi
        echo "::endgroup::"
      '') cfg.checks
    )}
    exit "$failed"
  '';
in
{
  options.scottylabs.ci.checks = lib.mkOption {
    type = lib.types.attrsOf lib.types.lines;
    default = { };
    description = ''
      Checks that need network access, as shell scripts run by `nix run .#ci`.
    '';
  };

  config.apps.ci = {
    type = "app";
    program = "${run}";
    meta.description = "Run the project's network checks from the checkout";
  };
}
