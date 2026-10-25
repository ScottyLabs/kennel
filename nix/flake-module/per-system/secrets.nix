{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.secrets;
in
{
  options.scottylabs.secrets.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      Add the `bao` and `secretspec` CLIs and export the project's secrets into
      the shell environment (see [Secrets](../guides/secrets.md)).
    '';
  };

  config = lib.mkIf cfg.enable {
    scottylabs.shell = {
      packages = [
        pkgs.openbao
        pkgs.secretspec
      ];

      env.BAO_ADDR = "https://secrets.scottylabs.org";

      initHook = ''
        if [ -f "$SCOTTYLABS_ROOT/secretspec.toml" ]; then
          if _scottylabs_secrets=$(secretspec export -f "$SCOTTYLABS_ROOT/secretspec.toml" -P "''${SECRETSPEC_PROFILE:-dev}" --caller scottylabs-shell --reason "load the dev shell"); then
            eval "$_scottylabs_secrets"
          else
            echo "secretspec could not resolve this project's secrets, run 'nix run git+https://git.cmu.dev/ScottyLabs/kennel#login'" >&2
          fi
          unset _scottylabs_secrets
        fi
      '';

      hook = ''
        if bao token lookup >/dev/null 2>&1; then
          bao token renew >/dev/null 2>&1 || true
        fi
      '';
    };
  };
}
