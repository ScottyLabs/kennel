{ config, lib, pkgs, ... }:

{
  options.services.kennel = {
    enable = lib.mkEnableOption "Kennel deployment platform";
  };

  config = lib.mkIf config.services.kennel.enable {
    # TODO: systemd service, nginx config, postgres setup
  };
}
