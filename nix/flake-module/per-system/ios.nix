kennelInputs:
{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.ios;
  mobile = kennelInputs.mobile-apps.lib.mkLib pkgs;
in
{
  options.scottylabs.ios.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = "Add [xtool](https://xtool.sh), Swift, and libimobiledevice for building iOS apps on Linux";
  };

  config = lib.mkIf cfg.enable {
    scottylabs.shell = {
      packages = [
        mobile.xtool
        mobile.xtoolPkgs.swift
        mobile.xtoolPkgs.swiftpm
        pkgs.libimobiledevice
        pkgs.ideviceinstaller
      ];

      # swift-build treats environment variables as build settings
      hook = ''
        unset CC LD AR
      '';
    };
  };
}
