{
  config,
  lib,
  pkgs,
  ...
}:

with lib;

let
  cfg = config.services.kennel;

  # nixpkgs builds PostgreSQL with this socket directory
  postgresSocketDir = trim (
    head (
      splitString "," (config.services.postgresql.settings.unix_socket_directories or "/run/postgresql")
    )
  );
  valkeySocket = config.services.redis.servers.kennel.unixSocket;
  garageAdminEndpoint = "http://localhost:${last (splitString ":" config.services.garage.settings.admin.api_bind_addr)}";
in
{
  options.services.kennel = {
    enable = mkEnableOption "Kennel deployment platform";

    package = mkOption {
      type = types.package;
      default = pkgs.kennel;
      defaultText = literalExpression "pkgs.kennel";
      description = "The Kennel package to use";
    };

    nixPackage = mkOption {
      type = types.package;
      default = pkgs.nix;
      defaultText = literalExpression "pkgs.nix";
      description = "The package providing `nix-store`, which kennel substitutes artifacts with";
    };

    webhookSecretFile = mkOption {
      type = types.path;
      description = ''
        Path to a file containing the HMAC secret used to verify all incoming
        webhooks. This is a single secret shared across all projects,
        provisioned by governance.
      '';
    };

    nixbotTokenFile = mkOption {
      type = types.path;
      description = ''
        Path to a file containing the bearer token nixbot's post-build step
        sends to `/nixbot` when it reports a project's deploy manifests.
      '';
    };

    api.port = mkOption {
      type = types.port;
      default = 3000;
      description = "Port the API server listens on at 127.0.0.1, behind Caddy";
    };

    domains = {
      ephemeral = mkOption {
        type = types.str;
        default = "scottylabs.net";
        description = ''
          Base domain for auto-generated deployment URLs. A wildcard DNS
          record should point `*.{domain}` to the kennel server.
        '';
      };

      cloudflare = {
        zones = mkOption {
          type = types.attrsOf types.str;
          default = { };
          description = ''
            Map of domain names to Cloudflare zone IDs. With `tunnelId` and a
            `CLOUDFLARE_API_TOKEN` that has `Zone:DNS:Edit`, kennel manages
            proxied CNAME records pointing custom domains at the tunnel. The
            most specific matching zone wins.
          '';
        };

        tunnelId = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = ''
            Cloudflare Tunnel ID the custom domain records point at. Required
            for DNS automation.
          '';
        };
      };
    };

    domain = mkOption {
      type = types.str;
      default = "kennel.scottylabs.org";
      description = ''
        Public domain for the kennel API and webhook endpoint. The module
        configures a Caddy virtualhost with automatic TLS for this domain,
        reverse-proxying to the API server.
      '';
    };

    grafanaUrl = mkOption {
      type = types.nullOr types.str;
      default = "https://grafana.scottylabs.org";
      description = "Base URL of Grafana for Logs Drilldown links in commit statuses";
    };

    customDomainsFile = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "/run/kennel/custom-domains";
      description = ''
        Writes the custom domains of all live deployments to this path, one
        per line, refreshed on each reconciliation pass. Unset disables it.
      '';
    };

    resources = {
      postgres = {
        enable = mkOption {
          type = types.bool;
          default = false;
          description = ''
            Whether to enable PostgreSQL resource provisioning. Kennel
            creates a database per deployment on the host's
            `services.postgresql`, over its Unix socket with peer
            authentication.
          '';
        };
      };

      valkey = {
        enable = mkOption {
          type = types.bool;
          default = false;
          description = ''
            Whether to enable Valkey resource provisioning. Kennel runs
            `services.redis.servers.kennel` and allocates a DB number per
            deployment from it.
          '';
        };
      };

      garage = {
        enable = mkOption {
          type = types.bool;
          default = false;
          description = ''
            Whether to enable Garage S3 resource provisioning. Kennel creates
            a bucket and API key per deployment on the host's
            `services.garage`, through its admin API. Requires
            `GARAGE_ADMIN_TOKEN` in the environment file.
          '';
        };
        s3Endpoint = mkOption {
          type = types.str;
          default = "http://localhost:3900";
          description = "Garage S3 API endpoint";
        };
      };
    };

    secrets = {
      enable = mkEnableOption "secretspec/OpenBao secret resolution at deploy time";
      vaultEndpoint = mkOption {
        type = types.str;
        default = "vault://secrets.scottylabs.org/secret?auth=approle";
        description = "secretspec provider URI for OpenBao/Vault";
      };
    };

    forgejo = {
      apiUrl = mkOption {
        type = types.str;
        default = "https://git.cmu.dev/api/v1";
        description = "Forgejo API base URL used to post PR deployment comments";
      };

      apiTokenFile = mkOption {
        type = types.path;
        description = ''
          Path to a file containing a Forgejo API token with the
          `write:issue` scope, used to comment deployment URLs on pull
          requests.
        '';
      };
    };

    user = mkOption {
      type = types.str;
      default = "kennel";
      description = "User to run Kennel as";
    };

    group = mkOption {
      type = types.str;
      default = "kennel";
      description = "Group to run Kennel as";
    };

    environmentFile = mkOption {
      type = types.nullOr types.path;
      default = null;
      description = ''
        Path to an environment file containing secrets like `VAULT_TOKEN` and
        `GARAGE_ADMIN_TOKEN`. Loaded by systemd before the service starts.
      '';
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.resources.garage.enable -> cfg.environmentFile != null;
        message = "An environmentFile containing GARAGE_ADMIN_TOKEN is required when Garage is enabled";
      }
      {
        assertion =
          cfg.resources.garage.enable
          -> config.services.garage.enable && config.services.garage.settings ? admin.api_bind_addr;
        message = "Kennel's Garage resource needs services.garage with settings.admin.api_bind_addr on this host";
      }
      {
        assertion = cfg.resources.postgres.enable -> config.services.postgresql.enable;
        message = "Kennel's PostgreSQL resource needs services.postgresql on this host";
      }
    ];

    # Skip kennel.slice units in switch-to-configuration's failed-unit sweep
    nixpkgs.overlays = [
      (_: prev: {
        switch-to-configuration-ng = prev.switch-to-configuration-ng.overrideAttrs (old: {
          postPatch = (old.postPatch or "") + ''
            substituteInPlace src/main.rs \
              --replace-fail 'for (unit, unit_state) in new_active_units {' 'for (unit, unit_state) in new_active_units { if unit.ends_with(".service") { let slice: Result<String, _> = unit_state.proxy.get("org.freedesktop.systemd1.Service", "Slice"); if matches!(slice, Ok(s) if s == "kennel.slice") { continue; } }'
          '';
        });
      })
    ];

    users = {
      users.${cfg.user} = mkMerge [
        (mkIf (cfg.user == "kennel") {
          isSystemUser = true;
          inherit (cfg) group;
          description = "Kennel service user";
        })
        (mkIf cfg.resources.valkey.enable { extraGroups = [ "redis-kennel" ]; })
      ];

      groups.${cfg.group} = mkIf (cfg.group == "kennel") { };
    };

    services.redis = mkIf cfg.resources.valkey.enable {
      package = mkDefault pkgs.valkey;
      servers.kennel.enable = true;
    };

    security.polkit.enable = true;
    security.polkit.extraConfig = ''
      polkit.addRule(function(action, subject) {
        if (action.id == "org.freedesktop.systemd1.manage-units" &&
            subject.user == "${cfg.user}") {
          return polkit.Result.YES;
        }
      });
    '';

    systemd = {
      slices.kennel = {
        description = "Kennel managed deployments";
      };

      services.kennel = {
        description = "Kennel deployment platform";
        # socket dirs in ReadWritePaths must exist before namespace setup
        after = [
          "network.target"
          "caddy.service"
        ]
        ++ optional cfg.resources.postgres.enable "postgresql.service"
        ++ optional cfg.resources.valkey.enable "redis-kennel.service";
        wants = [ "caddy.service" ];
        wantedBy = [ "multi-user.target" ];

        path = [
          cfg.nixPackage
        ]
        ++ optional cfg.resources.postgres.enable pkgs.postgresql
        ++ optional cfg.resources.valkey.enable pkgs.valkey;

        environment = {
          HOME = "/var/lib/kennel";
          RUST_LOG = "info,sqlx=warn";
          DATABASE_PATH = "/var/lib/kennel/kennel.db";
          API_PORT = toString cfg.api.port;
          EPHEMERAL_DOMAIN = cfg.domains.ephemeral;
          WEBHOOK_SECRET_FILE = cfg.webhookSecretFile;
          NIXBOT_TOKEN_FILE = cfg.nixbotTokenFile;
          FORGEJO_API_URL = cfg.forgejo.apiUrl;
          FORGEJO_API_TOKEN_FILE = cfg.forgejo.apiTokenFile;
        }
        // optionalAttrs cfg.resources.postgres.enable {
          POSTGRES_SOCKET_DIR = postgresSocketDir;
        }
        // optionalAttrs cfg.resources.valkey.enable {
          VALKEY_SOCKET_PATH = valkeySocket;
        }
        // optionalAttrs cfg.resources.garage.enable {
          GARAGE_ADMIN_ENDPOINT = garageAdminEndpoint;
          GARAGE_S3_ENDPOINT = cfg.resources.garage.s3Endpoint;
        }
        // optionalAttrs cfg.secrets.enable {
          VAULT_ENDPOINT = cfg.secrets.vaultEndpoint;
        }
        // optionalAttrs (cfg.domains.cloudflare.tunnelId != null && cfg.domains.cloudflare.zones != { }) {
          CLOUDFLARE_ZONES_JSON = builtins.toJSON cfg.domains.cloudflare.zones;
          KENNEL_TUNNEL_ID = cfg.domains.cloudflare.tunnelId;
        }
        // optionalAttrs (cfg.customDomainsFile != null) {
          CUSTOM_DOMAINS_FILE = cfg.customDomainsFile;
        }
        // optionalAttrs (cfg.grafanaUrl != null) {
          GRAFANA_URL = cfg.grafanaUrl;
        };

        serviceConfig = {
          User = cfg.user;
          Group = cfg.group;
          ExecStart = "${cfg.package}/bin/kennel";
          Restart = "on-failure";
          RestartSec = 5;

          NoNewPrivileges = true;
          PrivateTmp = true;
          ProtectSystem = "strict";
          ProtectHome = true;
          ReadWritePaths = [
            "/var/lib/kennel"
            "/run/kennel"
            "/nix/var/nix/gcroots/kennel"
          ]
          ++ optional cfg.resources.postgres.enable postgresSocketDir
          ++ optional cfg.resources.valkey.enable (dirOf valkeySocket);

          Delegate = "yes";

          EnvironmentFile = optional (cfg.environmentFile != null) cfg.environmentFile;
        };
      };

      tmpfiles.rules = [
        "d /var/lib/kennel 0755 ${cfg.user} ${cfg.group} -"
        "d /var/lib/kennel/sites 0755 ${cfg.user} ${cfg.group} -"
        "d /nix/var/nix/gcroots/kennel 0755 ${cfg.user} ${cfg.group} -"
        "d /var/lib/kennel/logs 0755 ${cfg.user} ${cfg.group} -"
        "d /run/kennel 0755 ${cfg.user} ${cfg.group} -"
      ];
    };

    services.caddy = {
      enable = true;
      globalConfig = ''
        # TODO: https://git.cmu.dev/ScottyLabs/infrastructure/issues/47
        servers {
          protocols h1 h2
        }
      '';
      virtualHosts.${cfg.domain}.extraConfig = ''
        reverse_proxy 127.0.0.1:${toString cfg.api.port}
      '';
    };

    # re-query substituters when retrying a fetch
    nix.settings.narinfo-cache-negative-ttl = mkDefault 0;
  };
}
