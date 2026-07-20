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

    devenvPackage = mkOption {
      type = types.package;
      description = ''
        The devenv package. The build worker uses `devenv build` to evaluate
        project kennel configs from their `devenv.nix`.
      '';
    };

    webhookSecretFile = mkOption {
      type = types.path;
      description = ''
        Path to a file containing the HMAC secret used to verify all incoming
        webhooks. This is a single secret shared across all projects,
        provisioned by governance.
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
            Map of domain names to Cloudflare zone IDs. When this is
            non-empty, `publicIp` is set, and the `CLOUDFLARE_API_TOKEN`
            environment variable is provided (typically via the
            `environmentFile` secret), kennel automatically manages A records
            for any custom domain whose suffix matches one of the configured
            zones. The most specific zone wins for nested domains.

            The token must have `Zone:DNS:Edit` permission on the zones
            listed.

            Records are upserted on deploy and on each reconciliation pass
            (so they self-heal if pruned externally). Records are deleted
            only when kennel tears the deployment down, which happens in
            three cases:

            1. The branch backing the deployment is deleted from the source
               repo (push event with deleted=true on that ref).
            2. The deployment is associated with a pull request and that
               pull request is closed.
            3. The deployment is on a `dev` or `preview` branch and exceeds
               `DEPLOYMENT_EXPIRY_DAYS` since its last update during a
               reconciliation pass.

            Production deployments are not subject to expiry, so a record for
            a production custom domain stays in place until the project's
            main branch is deleted or the deployment row is removed manually.
          '';
        };

        publicIp = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = ''
            Public IPv4 used as the content of the A records that kennel
            creates for custom domains. Required to enable DNS automation.
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

    builder = {
      maxConcurrentBuilds = mkOption {
        type = types.int;
        default = 2;
        description = "Maximum number of concurrent nix builds";
      };

      workDir = mkOption {
        type = types.path;
        default = "/var/lib/kennel/builds";
        description = "Build working directory";
      };

      cachix = {
        enable = mkEnableOption "pushing build artifacts to a Cachix binary cache";

        cacheName = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = "Cachix cache name";
        };
      };
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
        default = "https://codeberg.org/api/v1";
        description = "Forgejo API base URL used to post PR deployment comments";
      };

      apiTokenFile = mkOption {
        type = types.path;
        description = ''
          Path to a file containing a Forgejo API token with the
          `write:issue` scope. Kennel uses it to post and update a sticky
          comment on each pull request listing its deployment URLs, and to
          mark the comment torn down when the pull request is closed.
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
        Path to an environment file containing secrets like `VAULT_TOKEN`,
        `CACHIX_AUTH_TOKEN`, and `GARAGE_ADMIN_TOKEN`. Loaded by systemd
        before the service starts.
      '';
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.builder.cachix.enable -> cfg.builder.cachix.cacheName != null;
        message = "services.kennel.builder.cachix.cacheName must be set when Cachix is enabled";
      }
      {
        assertion = cfg.resources.garage.enable -> cfg.environmentFile != null;
        message = "An environmentFile containing GARAGE_ADMIN_TOKEN is required when Garage is enabled";
      }
    ];

    users.users.${cfg.user} = mkIf (cfg.user == "kennel") {
      isSystemUser = true;
      group = cfg.group;
      extraGroups = [ "kennel-builds" ];
      description = "Kennel service user";
    };

    users.groups.${cfg.group} = mkIf (cfg.group == "kennel") { };

    # Shared group so the daemon can read back the build units' work dirs
    users.groups.kennel-builds = { };

    security.polkit.enable = true;
    security.polkit.extraConfig = ''
      polkit.addRule(function(action, subject) {
        if (action.id == "org.freedesktop.systemd1.manage-units" &&
            subject.user == "${cfg.user}") {
          return polkit.Result.YES;
        }
      });
    '';

    systemd.slices.kennel = {
      description = "Kennel managed deployments";
    };

    systemd.services.kennel = {
      description = "Kennel deployment platform";
      after = [
        "network.target"
        "caddy.service"
      ];
      wants = [ "caddy.service" ];
      wantedBy = [ "multi-user.target" ];

      path = [
        cfg.devenvPackage
      ]
      ++ (with pkgs; [
        git
        nix
        cachix
      ])
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
        MAX_CONCURRENT_BUILDS = toString cfg.builder.maxConcurrentBuilds;
        WORK_DIR = cfg.builder.workDir;
        WEBHOOK_SECRET_FILE = cfg.webhookSecretFile;
        FORGEJO_API_URL = cfg.forgejo.apiUrl;
        FORGEJO_API_TOKEN_FILE = cfg.forgejo.apiTokenFile;
      }
      // optionalAttrs cfg.builder.cachix.enable {
        CACHIX_CACHE_NAME = cfg.builder.cachix.cacheName;
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
      // optionalAttrs (cfg.domains.cloudflare.publicIp != null && cfg.domains.cloudflare.zones != { }) {
        CLOUDFLARE_ZONES_JSON = builtins.toJSON cfg.domains.cloudflare.zones;
        KENNEL_PUBLIC_IP = cfg.domains.cloudflare.publicIp;
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

    systemd.tmpfiles.rules = [
      "d /var/lib/kennel 0755 ${cfg.user} ${cfg.group} -"
      "d /var/lib/kennel/builds 2770 ${cfg.user} kennel-builds -"
      "d /var/lib/kennel/sites 0755 ${cfg.user} ${cfg.group} -"
      "d /nix/var/nix/gcroots/kennel 0755 ${cfg.user} ${cfg.group} -"
      "d /var/lib/kennel/logs 0755 ${cfg.user} ${cfg.group} -"
      "d /run/kennel 0755 ${cfg.user} ${cfg.group} -"
    ];

    services.caddy = {
      enable = true;
      globalConfig = ''
        # TODO: https://codeberg.org/ScottyLabs/infrastructure/issues/47
        servers {
          protocols h1 h2
        }

        on_demand_tls {
          ask http://localhost:${toString cfg.api.port}/internal/caddy/check-domain
        }
      '';
      virtualHosts.${cfg.domain}.extraConfig = ''
        reverse_proxy localhost:${toString cfg.api.port}
      '';
    };

    networking.firewall = {
      allowedTCPPorts = [
        443
        80
      ];
    };

    nix.settings = {
      extra-substituters = [ "https://scottylabs.cachix.org" ];
      # Lets the untrusted build units pull from the cache without --accept-flake-config
      trusted-substituters = [ "https://scottylabs.cachix.org" ];
      extra-trusted-public-keys = [
        "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
      ];
    };
  };
}
