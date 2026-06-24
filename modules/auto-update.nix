{ pkgs, lib, config, ... }:

{
  config = lib.mkIf config.scottylabs.enable {
    git-hooks.hooks = {
      flake-update = {
        enable = true;
        name = "flake-update";
        entry = "${pkgs.writeShellScript "flake-update" ''
          set -euo pipefail
          ${lib.getExe pkgs.nix} flake update
          git add flake.lock
        ''}";
        language = "system";
        pass_filenames = false;
        always_run = true;
      };

      devenv-update = {
        enable = true;
        name = "devenv-update";
        entry = "${pkgs.writeShellScript "devenv-update" ''
          set -euo pipefail
          ${lib.getExe pkgs.devenv} update
          git add devenv.lock
        ''}";
        language = "system";
        pass_filenames = false;
        always_run = true;
      };
    };
  };
}
