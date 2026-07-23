{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.kennel;

  kennelConfigJSON = builtins.toJSON {
    version = cfg.schemaVersion;

    services = lib.mapAttrs (_: svc: {
      custom_domain = svc.customDomain;
    }) cfg.services;

    static_sites = lib.mapAttrs (name: site: {
      package_attr = name;
      spa = site.spa;
      custom_domain = site.customDomain;
    }) cfg.sites;

    preview_deployments = cfg.previewDeployments;
    resources = cfg.requestedResources;
  };
in
{
  options.scottylabs.kennel = {
    # Kennel deployment-config schema version
    #
    # Version of the kennel.json contract between this module and the kennel
    # daemon that reads it. When a project emits a version kennel does not
    # expect (KENNEL_CONFIG_SCHEMA_VERSION), kennel refuses the project and
    # tells it to run `devenv update`.
    #
    # Bump this only for a breaking change to the contract, meaning a field
    # kennel now requires or reads differently so an old config would be
    # handled wrong. Additive, backward-compatible fields do not need a bump.
    #
    # A bump must ship with a matching kennel release. Release this module
    # first, then bump every project's devenv.lock and rebuild, then deploy
    # kennel.
    #
    # History:
    # 1. Provision a resource only when the project lists it in `resources`.
    schemaVersion = lib.mkOption {
      type = lib.types.int;
      default = 1;
      readOnly = true;
      description = "Version of the kennel.json contract emitted by this module.";
    };

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

    requestedResources = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      internal = true;
      description = ''
        Resource providers this project needs (for example `valkey`,
        `postgres`, `garage`). Each resource module appends its own name when
        enabled. The list becomes the `resources` field of `kennel.json`.
        Kennel provisions only the resources named there.
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
