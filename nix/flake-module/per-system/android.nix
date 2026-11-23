kennelInputs:
{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.android;
  android = (kennelInputs.mobile-apps.lib.mkLib pkgs).androidSdk {
    emulator = cfg.emulator.enable;
  };
in
{
  options.scottylabs.android = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Add the Android SDK and JDK 21 for building Android apps";
    };

    emulator.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Add the Android emulator and its Play Store system images";
    };
  };

  config = lib.mkIf cfg.enable {
    scottylabs.shell = {
      packages = [
        android.sdk
        android.jdk
      ];

      env = {
        ANDROID_HOME = android.home;
        JAVA_HOME = "${android.jdk.home}";
        # gradle's own aapt2 doesn't run on NixOS
        GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${android.buildTools}/aapt2";
      };

      exports = lib.optionalString cfg.emulator.enable ''
        export LD_LIBRARY_PATH="${
          lib.makeLibraryPath [ pkgs.vulkan-loader ]
        }''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
      '';
    };
  };
}
