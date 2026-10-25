{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs;

  specPath = cfg.root + "/secretspec.toml";
  spec = lib.optionalAttrs (builtins.pathExists specPath) (lib.importTOML specPath);

  # SvelteKit's $env/dynamic/public only exposes PUBLIC_ names
  unprefixed = lib.filter (name: !lib.hasPrefix "PUBLIC_" name) (spec.scopes.public.secrets or [ ]);

  schemaVersion = lib.toInt (
    lib.head (
      builtins.match ".*KENNEL_CONFIG_SCHEMA_VERSION: u32 = ([0-9]+);.*" (
        builtins.readFile ../../../crates/kennel-config/src/constants.rs
      )
    )
  );

  customDomain = lib.mkOption {
    type = lib.types.nullOr lib.types.str;
    default = null;
    description = ''
      Domain the target is also served on in production. Its Cloudflare zone
      must be registered with kennel. An apex domain also gets `www.`
      redirected to it.
    '';
  };

  service.options = {
    package = lib.mkOption {
      type = lib.types.package;
      description = ''
        The service. Kennel runs its `meta.mainProgram`, or the first
        executable in `bin/`, which must answer `/api/health` on `$PORT`.
      '';
    };
    inherit customDomain;
    env = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = lib.literalExpression ''{ STATIC_DIR = "''${web}"; }'';
      description = ''
        Variables the deployed service runs with, for settings that come
        from Nix such as store paths. Secrets and the variables kennel
        sets for resources take precedence on a shared name.
      '';
    };
  };

  site.options = {
    package = lib.mkOption {
      type = lib.types.package;
      description = "The site's files, served from the root of the store path";
    };
    spa = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Serve `index.html` for paths with no matching file, for
        single-page apps. Off serves files as they are, which suits
        pre-rendered sites.
      '';
    };
    inherit customDomain;
  };

  kennelJson = pkgs.writeText "kennel.json" (
    builtins.toJSON {
      version = schemaVersion;
      services = lib.mapAttrs (_: t: {
        store_path = "${t.package}";
        exec = if t.package.meta ? mainProgram then lib.getExe t.package else null;
        custom_domain = t.customDomain;
        inherit (t) env;
      }) config.services;
      static_sites = lib.mapAttrs (_: t: {
        store_path = "${t.package}";
        inherit (t) spa;
        custom_domain = t.customDomain;
      }) config.sites;
      preview_deployments = cfg.previewDeployments;
      resources = lib.unique cfg.kennel.resources;
      postgres_extensions = cfg.kennel.postgresExtensions;
    }
  );
in
{
  options = {
    services = lib.mkOption {
      type = lib.types.lazyAttrsOf (lib.types.submodule service);
      default = { };
      description = "Backend services kennel deploys as systemd units";
    };

    sites = lib.mkOption {
      type = lib.types.lazyAttrsOf (lib.types.submodule site);
      default = { };
      description = "Static sites kennel serves through Caddy";
    };

    scottylabs = {
      previewDeployments = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Deploy a preview environment for each open pull request. Disable for
          singleton services such as bots.
        '';
      };

      kennel = {
        resources = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          internal = true;
          description = "Resources kennel provisions for each deployment";
        };

        postgresExtensions = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          internal = true;
          description = "Extensions kennel creates in each deployment's database";
        };

        manifest = lib.mkOption {
          type = lib.types.package;
          readOnly = true;
          internal = true;
          description = "`kennel.json` and `secretspec.toml` kennel deploys from";
          default =
            lib.throwIf (unprefixed != [ ])
              "secretspec.toml: [scopes.public] names must start with PUBLIC_, found ${lib.concatStringsSep ", " unprefixed}"
              (
                pkgs.linkFarm "kennel-${cfg.project.name}" (
                  {
                    "kennel.json" = kennelJson;
                  }
                  // lib.optionalAttrs (spec != { }) {
                    "secretspec.toml" = pkgs.writeText "secretspec.toml" (builtins.readFile specPath);
                  }
                )
              );
        };
      };
    };
  };
}
