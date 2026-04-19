{ config, lib, pkgs, ... }:

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
      description = "The devenv package for evaluating project configs";
    };

    webhookSecretFile = mkOption {
      type = types.path;
      description = "Path to file containing the webhook HMAC secret shared across all projects";
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
        description = "Base domain for auto-generated deployment URLs";
      };

      cloudflare = {
        zones = mkOption {
          type = types.attrsOf types.str;
          default = { };
          description = "Map of domain names to Cloudflare zone IDs for custom domain DNS";
        };

        apiTokenFile = mkOption {
          type = types.nullOr types.path;
          default = null;
          description = "Path to file containing Cloudflare API token";
        };
      };
    };

    domain = mkOption {
      type = types.str;
      default = "kennel.scottylabs.org";
      description = "Public domain for the kennel API and webhook endpoint";
    };

    caddy.adminUrl = mkOption {
      type = types.str;
      default = "http://localhost:2019";
      description = "Caddy admin API URL";
    };

    builder = {
      maxConcurrentBuilds = mkOption {
        type = types.int;
        default = 2;
        description = "Maximum concurrent builds";
      };

      workDir = mkOption {
        type = types.path;
        default = "/var/lib/kennel/builds";
        description = "Build working directory";
      };

      cachix = {
        enable = mkEnableOption "Cachix binary cache push";

        cacheName = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = "Cachix cache name";
        };
      };
    };

    resources = {
      postgres = {
        enable = mkEnableOption "PostgreSQL resource provisioning";
        socketDir = mkOption {
          type = types.path;
          default = "/run/postgresql";
          description = "PostgreSQL Unix socket directory";
        };
      };

      valkey = {
        enable = mkEnableOption "Valkey resource provisioning";
        socketPath = mkOption {
          type = types.path;
          default = "/run/valkey/valkey.sock";
          description = "Valkey Unix socket path";
        };
      };

      garage = {
        enable = mkEnableOption "Garage S3 resource provisioning";
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
      enable = mkEnableOption "secretspec/OpenBao secret resolution";
      vaultEndpoint = mkOption {
        type = types.str;
        default = "vault://secrets2.scottylabs.org/secret";
        description = "secretspec provider URI for OpenBao/Vault";
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
      description = "Path to environment file with secrets";
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
      description = "Kennel service user";
    };

    users.groups.${cfg.group} = mkIf (cfg.group == "kennel") { };

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
      after = [ "network.target" "caddy.service" ];
      wants = [ "caddy.service" ];
      wantedBy = [ "multi-user.target" ];

      path = [ cfg.devenvPackage ] ++ (with pkgs; [ git nix cachix ]);

      environment = {
        HOME = "/var/lib/kennel";
        RUST_LOG = "info";
        DATABASE_PATH = "/var/lib/kennel/kennel.db";
        API_HOST = cfg.api.host;
        API_PORT = toString cfg.api.port;
        EPHEMERAL_DOMAIN = cfg.domains.ephemeral;
        CADDY_ADMIN_URL = cfg.caddy.adminUrl;
        MAX_CONCURRENT_BUILDS = toString cfg.builder.maxConcurrentBuilds;
        WORK_DIR = cfg.builder.workDir;
        WEBHOOK_SECRET_FILE = cfg.webhookSecretFile;
      } // optionalAttrs cfg.builder.cachix.enable {
        CACHIX_CACHE_NAME = cfg.builder.cachix.cacheName;
      } // optionalAttrs cfg.resources.postgres.enable {
        POSTGRES_SOCKET_DIR = cfg.resources.postgres.socketDir;
      } // optionalAttrs cfg.resources.valkey.enable {
        VALKEY_SOCKET_PATH = cfg.resources.valkey.socketPath;
      } // optionalAttrs cfg.resources.garage.enable {
        GARAGE_ADMIN_ENDPOINT = cfg.resources.garage.adminEndpoint;
        GARAGE_S3_ENDPOINT = cfg.resources.garage.s3Endpoint;
      } // optionalAttrs cfg.secrets.enable {
        VAULT_ENDPOINT = cfg.secrets.vaultEndpoint;
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
        ] ++ optional cfg.resources.postgres.enable cfg.resources.postgres.socketDir
        ++ optional cfg.resources.valkey.enable (dirOf cfg.resources.valkey.socketPath);

        Delegate = "yes";

        EnvironmentFile = optional (cfg.environmentFile != null) cfg.environmentFile;
      };
    };

    systemd.tmpfiles.rules = [
      "d /var/lib/kennel 0755 ${cfg.user} ${cfg.group} -"
      "d /var/lib/kennel/builds 0755 ${cfg.user} ${cfg.group} -"
      "d /var/lib/kennel/sites 0755 ${cfg.user} ${cfg.group} -"
      "d /var/lib/kennel/logs 0755 ${cfg.user} ${cfg.group} -"
      "d /run/kennel 0755 ${cfg.user} ${cfg.group} -"
    ];

    services.caddy = {
      enable = true;
      globalConfig = ''
        on_demand_tls {
          ask http://localhost:${toString cfg.api.port}/internal/caddy/check-domain
        }
      '';
      virtualHosts.${cfg.domain}.extraConfig = ''
        reverse_proxy localhost:${toString cfg.api.port}
      '';
    };

    networking.firewall = {
      allowedTCPPorts = [ 443 80 ];
    };

    nix.settings = {
      extra-substituters = [ "https://scottylabs.cachix.org" ];
      extra-trusted-public-keys = [
        "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
      ];
    };
  };
}
