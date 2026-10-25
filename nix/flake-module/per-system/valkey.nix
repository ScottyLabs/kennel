{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.valkey;
in
{
  options.scottylabs.valkey.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = ''
      Run [Valkey](https://valkey.io/) locally over a Unix socket and export
      `VALKEY_URL`. Kennel provisions a database for each deployment.
    '';
  };

  config = lib.mkIf cfg.enable {
    scottylabs = {
      shell = {
        packages = [ pkgs.valkey ];
        hook = ''
          export VALKEY_URL="redis+unix://$SCOTTYLABS_RUNTIME/valkey.sock?db=0"
        '';
      };

      processes.valkey = {
        run = ''
          mkdir -p "$SCOTTYLABS_STATE/valkey"
          exec ${pkgs.valkey}/bin/valkey-server --port 0 --unixsocket "$SCOTTYLABS_RUNTIME/valkey.sock" --dir "$SCOTTYLABS_STATE/valkey"
        '';
        ready = ''${pkgs.valkey}/bin/valkey-cli -s "$SCOTTYLABS_RUNTIME/valkey.sock" ping'';
      };

      kennel.resources = [ "valkey" ];
    };
  };
}
