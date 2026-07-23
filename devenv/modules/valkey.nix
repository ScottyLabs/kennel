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
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable a local [Valkey](https://valkey.io/) instance for development.
        Layers `services.redis.package = pkgs.valkey` under the hood, so the
        upstream `services.redis` devenv module drives the process while the
        binary is the wire-compatible Valkey fork. Adds `pkgs.valkey` to the
        shell so `valkey-cli` is on the path.
      '';
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [ pkgs.valkey ];

    services.redis = {
      enable = true;
      package = pkgs.valkey;
    };

    scottylabs.kennel.requestedResources = [ "valkey" ];
  };
}
