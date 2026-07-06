{
  pkgs,
  lib,
  config,
  ...
}:

let
  # refresh each lockfile to its latest inputs on every commit
  updateHooks = {
    flake-update = ''
      ${lib.getExe pkgs.nix} flake update
      git add flake.lock
    '';
    devenv-update = ''
      ${lib.getExe pkgs.devenv} update
      git add devenv.lock
    '';
  };

  mkUpdateHook = name: script: {
    inherit name;
    enable = true;
    entry = "${pkgs.writeShellScript name ''
      set -euo pipefail
      ${script}
    ''}";
    language = "system";
    pass_filenames = false;
    always_run = true;
  };
in
{
  config = lib.mkIf config.scottylabs.enable {
    git-hooks.hooks = lib.mapAttrs mkUpdateHook updateHooks;
  };
}
