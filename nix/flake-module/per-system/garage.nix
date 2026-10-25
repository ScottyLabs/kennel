{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.garage;
  projectName = config.scottylabs.project.name;

  digest = builtins.hashString "sha256" "garage-${projectName}";
  # Fixed per project so two projects' garages do not collide
  basePort = 20000 + 3 * lib.fromHexString (builtins.substring 0 3 digest);
  s3Port = toString basePort;
  rpcPort = toString (basePort + 1);
  adminPort = toString (basePort + 2);
  adminToken = builtins.substring 32 32 digest;

  garage = "${lib.getExe pkgs.garage} -c \"$SCOTTYLABS_STATE/garage/garage.toml\"";
in
{
  options.scottylabs.garage = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Run a [Garage](https://garagehq.deuxfleurs.fr/) S3 server locally with
        a bucket named after `scottylabs.project.name`, and export
        `S3_ENDPOINT`, `S3_REGION`, `S3_ACCESS_KEY`, `S3_SECRET_KEY`, and
        `S3_BUCKET`. Kennel provisions a bucket for each deployment.
      '';
    };

    accessKey = lib.mkOption {
      type = lib.types.str;
      default = "GK${builtins.substring 0 24 digest}";
      defaultText = lib.literalMD "derived from `scottylabs.project.name`";
      description = "S3 access key for the local bucket";
    };

    secretKey = lib.mkOption {
      type = lib.types.str;
      default = digest;
      defaultText = lib.literalMD "derived from `scottylabs.project.name`";
      description = "S3 secret key for the local bucket";
    };
  };

  config = lib.mkIf cfg.enable {
    scottylabs = {
      shell = {
        packages = [ pkgs.garage ];
        env = {
          S3_ENDPOINT = "http://127.0.0.1:${s3Port}";
          S3_REGION = "garage";
          S3_ACCESS_KEY = cfg.accessKey;
          S3_SECRET_KEY = cfg.secretKey;
          S3_BUCKET = projectName;
        };
      };

      processes = {
        garage = {
          run = ''
            dir="$SCOTTYLABS_STATE/garage"
            mkdir -p "$dir"
            cat > "$dir/garage.toml" <<EOF
            metadata_dir = "$dir/meta"
            data_dir = "$dir/data"
            db_engine = "sqlite"
            replication_factor = 1
            rpc_bind_addr = "127.0.0.1:${rpcPort}"
            rpc_public_addr = "127.0.0.1:${rpcPort}"
            rpc_secret = "${digest}"

            [s3_api]
            s3_region = "garage"
            api_bind_addr = "127.0.0.1:${s3Port}"

            [admin]
            api_bind_addr = "127.0.0.1:${adminPort}"
            admin_token = "${adminToken}"
            EOF
            exec ${garage} server
          '';
          ready = "${lib.getExe pkgs.curl} -fsS -H 'Authorization: Bearer ${adminToken}' http://127.0.0.1:${adminPort}/health";
        };

        garage-setup = {
          run = ''
            if ${garage} status | grep -q "NO ROLE ASSIGNED"; then
              ${garage} layout assign -z dc1 -c 1G "$(${garage} node id | cut -d@ -f1)"
              ${garage} layout apply --version 1
            fi
            ${garage} bucket info ${projectName} >/dev/null 2>&1 || ${garage} bucket create ${projectName}
            ${garage} key info ${cfg.accessKey} >/dev/null 2>&1 || ${garage} key import --yes -n ${projectName} ${cfg.accessKey} ${cfg.secretKey}
            ${garage} bucket allow --read --write --owner ${projectName} --key ${cfg.accessKey}
          '';
          depends = [ "garage" ];
          oneshot = true;
        };
      };

      kennel.resources = [ "garage" ];
    };
  };
}
