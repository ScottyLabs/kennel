{
  config,
  lib,
  pkgs,
  ...
}:

with lib;

let
  cfg = config.services.kennel;
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

    api = {
      host = mkOption {
        type = types.str;
        default = "0.0.0.0";
        description = "API server bind address";
      };

      port = mkOption {
        type = types.port;
        default = 3000;
        description = "API server port";
      };
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

    caddy.adminUrl = mkOption {
      type = types.str;
      default = "http://localhost:2019";
      description = "Caddy admin API URL";
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
            creates a database per deployment using the specified socket
            directory for peer authentication.
          '';
        };
        socketDir = mkOption {
          type = types.path;
          default = "/run/postgresql";
          description = "PostgreSQL Unix socket directory";
        };
      };

      valkey = {
        enable = mkOption {
          type = types.bool;
          default = false;
          description = ''
            Whether to enable Valkey resource provisioning. Kennel allocates
            a DB number per deployment from the shared instance.
          '';
        };
        socketPath = mkOption {
          type = types.path;
          default = "/run/valkey/valkey.sock";
          description = "Valkey Unix socket path";
        };
      };

      garage = {
        enable = mkOption {
          type = types.bool;
          default = false;
          description = ''
            Whether to enable Garage S3 resource provisioning. Kennel creates
            a bucket and API key per deployment. Requires
            `GARAGE_ADMIN_TOKEN` in the environment file.
          '';
        };
        adminEndpoint = mkOption {
          type = types.str;
          default = "http://localhost:3903";
          description = "Garage admin API endpoint";
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
    ];

    users = {
      users.${cfg.user} = mkIf (cfg.user == "kennel") {
        isSystemUser = true;
        inherit (cfg) group;
        description = "Kennel service user";
      };

      groups.${cfg.group} = mkIf (cfg.group == "kennel") { };
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
        ++ optional (
          cfg.resources.postgres.enable && config.services.postgresql.enable
        ) "postgresql.service"
        ++ optionals cfg.resources.valkey.enable (
          mapAttrsToList (name: _: "redis-${name}.service") (
            filterAttrs (
              _: s: s.enable && s.unixSocket == cfg.resources.valkey.socketPath
            ) config.services.redis.servers
          )
        );
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
          API_HOST = cfg.api.host;
          API_PORT = toString cfg.api.port;
          EPHEMERAL_DOMAIN = cfg.domains.ephemeral;
          CADDY_ADMIN_URL = cfg.caddy.adminUrl;
          WEBHOOK_SECRET_FILE = cfg.webhookSecretFile;
          NIXBOT_TOKEN_FILE = cfg.nixbotTokenFile;
          FORGEJO_API_URL = cfg.forgejo.apiUrl;
          FORGEJO_API_TOKEN_FILE = cfg.forgejo.apiTokenFile;
        }
        // optionalAttrs cfg.resources.postgres.enable {
          POSTGRES_SOCKET_DIR = cfg.resources.postgres.socketDir;
        }
        // optionalAttrs cfg.resources.valkey.enable {
          VALKEY_SOCKET_PATH = cfg.resources.valkey.socketPath;
        }
        // optionalAttrs cfg.resources.garage.enable {
          GARAGE_ADMIN_ENDPOINT = cfg.resources.garage.adminEndpoint;
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
          ++ optional cfg.resources.postgres.enable cfg.resources.postgres.socketDir
          ++ optional cfg.resources.valkey.enable (dirOf cfg.resources.valkey.socketPath);

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
        reverse_proxy localhost:${toString cfg.api.port}
      '';
    };

    nix.settings = {
      extra-substituters = [ "https://scottylabs.cachix.org" ];
      # Lets kennel substitute artifacts as an untrusted user without --accept-flake-config
      trusted-substituters = [ "https://scottylabs.cachix.org" ];
      extra-trusted-public-keys = [
        "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
      ];
      # re-query cachix when retrying a fetch
      narinfo-cache-negative-ttl = mkDefault 0;
    };
  };
}
