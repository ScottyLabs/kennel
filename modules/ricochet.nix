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
    enable = lib.mkEnableOption "Local OAuth relay for development";

    appUrl = lib.mkOption {
      type = lib.types.str;
      description = "Public URL of this service in development, exported as APP_URL";
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    processes.ricochet.exec = "RICOCHET_DEV=1 RICOCHET_BIND=127.0.0.1:${toString port} ${ricochet}/bin/ricochet";
    env.APP_URL = cfg.appUrl;
  };
}
