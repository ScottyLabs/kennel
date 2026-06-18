{ lib, config, ... }:

let
  cfg = config.scottylabs.garage;
  projectName = config.scottylabs.project.name;
in
{
  options.scottylabs.garage = {
    enable = lib.mkEnableOption "Garage with ScottyLabs defaults";

    accessKey = lib.mkOption {
      type = lib.types.str;
      default = projectName;
      description = "S3 access key";
    };

    secretKey = lib.mkOption {
      type = lib.types.str;
      default = "${projectName}admin";
      description = "S3 secret key";
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    services.garage = {
      enable = true;
      buckets = [ projectName ];
      s3Address = "127.0.0.1:9000";
      afterStart = ''
        garage key import --yes -n ${projectName} ${cfg.accessKey} ${cfg.secretKey}
        garage bucket allow --read --write --owner ${projectName} --key ${cfg.accessKey}
      '';
    };

    env = {
      S3_ENDPOINT = "http://localhost:9000";
      S3_REGION = config.services.garage.region;
      S3_ACCESS_KEY = cfg.accessKey;
      S3_SECRET_KEY = cfg.secretKey;
      S3_BUCKET = projectName;
    };
  };
}
