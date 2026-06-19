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
    enable = lib.mkEnableOption "local OAuth relay for development";
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    processes.ricochet.exec = "RICOCHET_DEV=1 RICOCHET_BIND=127.0.0.1:${toString port} ${ricochet}/bin/ricochet";
  };
}
