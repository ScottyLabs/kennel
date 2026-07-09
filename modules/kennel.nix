{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.kennel;

  kennelConfigJSON = builtins.toJSON {
    services = lib.mapAttrs (_: svc: {
      custom_domain = svc.customDomain;
    }) cfg.services;

    static_sites = lib.mapAttrs (name: site: {
      package_attr = name;
      spa = site.spa;
      custom_domain = site.customDomain;
    }) cfg.sites;

    preview_deployments = cfg.previewDeployments;
  };
in
{
  options.scottylabs.kennel = {
    services = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.submodule {
          options = {
            customDomain = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "Custom domain for this service";
            };
          };
        }
      );
      default = { };
      description = ''
        Backend services deployed by kennel. Each key must match a devenv
        process name. Kennel builds the corresponding flake package and deploys
        it as a systemd transient unit.
      '';
    };

    sites = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.submodule {
          options = {
            spa = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = "Serve index.html for all routes";
            };

            customDomain = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "Custom domain for this static site";
            };
          };
        }
      );
      default = { };
      description = ''
        Static sites deployed by kennel. Each key names a site. Kennel builds
        the corresponding flake package and serves it via Caddy's file server.
      '';
    };

    previewDeployments = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Deploy a preview environment for each open pull request. Disable for
        singleton services such as bots, where a preview would run a second
        instance alongside production. When disabled, pull request commits
        still build and report `kennel/build`, but kennel does not deploy the
        preview.
      '';
    };

    config = lib.mkOption {
      type = lib.types.package;
      readOnly = true;
      description = "The generated `kennel.json` derivation that the kennel builder evaluates at build time";
    };
  };

  config = lib.mkIf config.scottylabs.enable {
    scottylabs.kennel.config =
      let
        secretspecPath = /${config.devenv.root}/secretspec.toml;
        hasSecretspec = builtins.pathExists secretspecPath;
      in
      pkgs.runCommand "kennel-config" { } (
        ''
          mkdir -p $out
          echo '${kennelConfigJSON}' > $out/kennel.json
        ''
        + lib.optionalString hasSecretspec ''
          cp ${secretspecPath} $out/secretspec.toml
        ''
      );
  };
}
