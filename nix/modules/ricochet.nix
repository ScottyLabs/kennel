{
  ricochet,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.ricochet;

  # Fixed so governance can register one dev redirect for every app
  port = 8090;
in
{
  options.scottylabs.ricochet = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Run an OAuth relay on `127.0.0.1:8090`, the callback the `dev` Keycloak
        client is registered against, so OAuth flows complete locally.
      '';
    };

    appUrl = lib.mkOption {
      type = lib.types.str;
      description = ''
        URL the service is reached at locally, exported as `APP_URL`. Deployed
        services get `APP_URL` from kennel.
      '';
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    processes.ricochet.exec = "RICOCHET_DEV=1 RICOCHET_BIND=127.0.0.1:${toString port} ${ricochet}/bin/ricochet";
    env.APP_URL = cfg.appUrl;
  };
}
