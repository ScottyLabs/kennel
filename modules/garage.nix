{ lib, config, ... }:

let
  cfg = config.scottylabs.garage;
  projectName = config.scottylabs.project.name;
in
{
  options.scottylabs.garage = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable a local [Garage](https://garagehq.deuxfleurs.fr/) S3 instance
        for development. Creates a bucket named after `scottylabs.project.name`
        and exports `S3_ENDPOINT`, `S3_REGION`, `S3_ACCESS_KEY`,
        `S3_SECRET_KEY`, and `S3_BUCKET` into the shell environment.
      '';
    };

    accessKey = lib.mkOption {
      type = lib.types.str;
      default = projectName;
      defaultText = lib.literalExpression "config.scottylabs.project.name";
      description = "S3 access key for the project bucket";
    };

    secretKey = lib.mkOption {
      type = lib.types.str;
      default = "${projectName}admin";
      defaultText = lib.literalExpression ''"''${config.scottylabs.project.name}admin"'';
      description = "S3 secret key for the project bucket";
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

    scottylabs.kennel.requestedResources = [ "garage" ];
  };
}
