{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.valkey;
in
{
  options.scottylabs.valkey = {
    enable = lib.mkEnableOption "Valkey with ScottyLabs defaults";
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [ pkgs.valkey ];

    services.redis = {
      enable = true;
      package = pkgs.valkey;
    };
  };
}
