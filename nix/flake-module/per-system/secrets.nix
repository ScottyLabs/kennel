{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.secrets;

  specPath = config.scottylabs.root + "/secretspec.toml";
  spec = lib.optionalAttrs (builtins.pathExists specPath) (lib.importTOML specPath);

  # overrides the CLI's own default and the user's secretspec config
  exportProfile = ''export SECRETSPEC_PROFILE="''${SECRETSPEC_PROFILE:-dev}"'';
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

      initHook =
        lib.throwIf (spec != { } && !(spec ? profiles.dev))
          "secretspec.toml: declare a [profiles.dev] header, the profile the dev shell and local processes resolve"
          ''
            ${exportProfile}
            if [ -f "$SCOTTYLABS_ROOT/secretspec.toml" ]; then
              if _scottylabs_secrets=$(secretspec export -f "$SCOTTYLABS_ROOT/secretspec.toml" -P "$SECRETSPEC_PROFILE" --caller scottylabs-shell --reason "load the dev shell"); then
                eval "$_scottylabs_secrets"
              else
                echo "secretspec could not resolve this project's secrets, run 'nix run git+https://git.cmu.dev/ScottyLabs/kennel#login'" >&2
              fi
              unset _scottylabs_secrets
            fi
          '';

      # local processes skip initHook
      exports = exportProfile;

      hook = ''
        if bao token lookup >/dev/null 2>&1; then
          bao token renew >/dev/null 2>&1 || true
        fi
      '';
    };
  };
}
