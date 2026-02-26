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

    database = {
      host = mkOption {
        type = types.str;
        default = "/run/postgresql";
        description = "PostgreSQL host (Unix socket path or hostname)";
      };

      port = mkOption {
        type = types.port;
        default = 5432;
        description = "PostgreSQL port";
      };

      name = mkOption {
        type = types.str;
        default = "kennel";
        description = "Database name";
      };

      user = mkOption {
        type = types.str;
        default = "kennel";
        description = "Database user";
      };

      createLocally = mkOption {
        type = types.bool;
        default = true;
        description = "Whether to create database locally";
      };
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

    projects = mkOption {
      type = types.attrsOf (types.submodule {
        options = {
          repoUrl = mkOption {
            type = types.str;
            example = "https://codeberg.org/ScottyLabs/kennel";
            description = "Git repository URL";
          };

          repoType = mkOption {
            type = types.enum [ "forgejo" "github" ];
            default = "forgejo";
            description = "Repository type (forgejo or github)";
          };

          webhookSecretFile = mkOption {
            type = types.path;
            example = "/run/secrets/kennel-webhook-secret";
            description = "Path to file containing webhook secret";
          };

          defaultBranch = mkOption {
            type = types.str;
            default = "main";
            description = "Default branch name";
          };
        };
      });
      default = { };
      example = {
        kennel = {
          repoUrl = "https://codeberg.org/ScottyLabs/kennel";
          repoType = "forgejo";
          webhookSecretFile = "/run/secrets/kennel-webhook";
          defaultBranch = "main";
        };
      };
      description = "Projects to deploy with Kennel";
    };

    router = {
      address = mkOption {
        type = types.str;
        default = "0.0.0.0:80";
        description = "Router bind address";
      };

      baseDomain = mkOption {
        type = types.str;
        example = "scottylabs.org";
        description = "Base domain for auto-generated subdomains";
      };

      tls = {
        enable = mkEnableOption "TLS/ACME support";

        email = mkOption {
          type = types.str;
          default = "";
          example = "admin@scottylabs.org";
          description = "Email for Let's Encrypt account";
        };

        staging = mkOption {
          type = types.bool;
          default = false;
          description = "Use Let's Encrypt staging environment";
        };
      };
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
          example = "kennel";
          description = "Cachix cache name";
        };

        authTokenFile = mkOption {
          type = types.nullOr types.path;
          default = null;
          example = "/run/secrets/cachix-auth-token";
          description = "Path to file containing Cachix auth token";
        };
      };
    };

    cleanup = {
      interval = mkOption {
        type = types.int;
        default = 600;
        description = "Auto-expiry check interval in seconds";
      };
    };

    dns = {
      enable = mkEnableOption "Automatic DNS management";

      provider = mkOption {
        type = types.enum [ "cloudflare" ];
        default = "cloudflare";
        description = "DNS provider to use";
      };

      cloudflare = {
        apiTokenFile = mkOption {
          type = types.nullOr types.path;
          default = null;
          example = "/run/secrets/cloudflare-api-token";
          description = "Path to file containing Cloudflare API token";
        };

        zones = mkOption {
          type = types.attrsOf types.str;
          default = { };
          example = {
            "scottylabs.org" = "abc123def456";
            "example.com" = "xyz789ghi012";
          };
          description = "Map of domain names to Cloudflare zone IDs";
        };
      };

      serverIpv4 = mkOption {
        type = types.str;
        example = "1.2.3.4";
        description = "Server IPv4 address for DNS records";
      };

      serverIpv6 = mkOption {
        type = types.str;
        example = "2001:db8::1";
        description = "Server IPv6 address for DNS records";
      };
    };

    user = mkOption {
      type = types.str;
      default = "kennel";
      description = "User to run Kennel service as";
    };

    group = mkOption {
      type = types.str;
      default = "kennel";
      description = "Group to run Kennel service as";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.router.tls.enable -> cfg.router.tls.email != "";
        message = "services.kennel.router.tls.email must be set when TLS is enabled";
      }
      {
        assertion = cfg.builder.cachix.enable -> cfg.builder.cachix.cacheName != null;
        message = "services.kennel.builder.cachix.cacheName must be set when Cachix is enabled";
      }
      {
        assertion = cfg.builder.cachix.enable -> cfg.builder.cachix.authTokenFile != null;
        message = "services.kennel.builder.cachix.authTokenFile must be set when Cachix is enabled";
      }
      {
        assertion = cfg.dns.enable -> cfg.dns.cloudflare.apiTokenFile != null;
        message = "services.kennel.dns.cloudflare.apiTokenFile must be set when DNS is enabled";
      }
      {
        assertion = cfg.dns.enable -> cfg.dns.cloudflare.zones != { };
        message = "services.kennel.dns.cloudflare.zones must be configured when DNS is enabled";
      }
    ];

    warnings = optional cfg.router.tls.staging
      "Kennel TLS is using Let's Encrypt staging environment. Certificates will not be trusted by browsers.";

    users.users.${cfg.user} = mkIf (cfg.user == "kennel") {
      isSystemUser = true;
      group = cfg.group;
      description = "Kennel service user";
    };

    users.groups.${cfg.group} = mkIf (cfg.group == "kennel") { };

    services.postgresql = mkIf cfg.database.createLocally {
      enable = true;
      ensureDatabases = [ cfg.database.name ];
      ensureUsers = [{
        name = cfg.database.user;
        ensureDBOwnership = true;
      }];
    };

    systemd.services.kennel = {
      description = "Kennel deployment platform";
      after = [ "network.target" ] ++ optional cfg.database.createLocally "postgresql.service";
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        Type = "notify";
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
          "/etc/systemd/system"
        ];

        Environment = [
          "RUST_LOG=info"
          "DATABASE_URL=postgresql://${cfg.database.user}@${cfg.database.host}:${toString cfg.database.port}/${cfg.database.name}"
          "API_HOST=${cfg.api.host}"
          "API_PORT=${toString cfg.api.port}"
          "ROUTER_ADDR=${cfg.router.address}"
          "BASE_DOMAIN=${cfg.router.baseDomain}"
          "MAX_CONCURRENT_BUILDS=${toString cfg.builder.maxConcurrentBuilds}"
          "WORK_DIR=${cfg.builder.workDir}"
          "AUTO_EXPIRY_CHECK_INTERVAL_SECS=${toString cfg.cleanup.interval}"
        ] ++ optionals cfg.router.tls.enable [
          "ACME_EMAIL=${cfg.router.tls.email}"
          "ACME_STAGING=${if cfg.router.tls.staging then "true" else "false"}"
        ] ++ optionals cfg.builder.cachix.enable [
          "CACHIX_CACHE_NAME=${cfg.builder.cachix.cacheName}"
        ] ++ optionals cfg.dns.enable [
          "DNS_ENABLED=true"
          "DNS_PROVIDER=${cfg.dns.provider}"
          "CLOUDFLARE_ZONES=${concatStringsSep "," (mapAttrsToList (domain: zoneId: "${domain}:${zoneId}") cfg.dns.cloudflare.zones)}"
          "SERVER_IPV4=${cfg.dns.serverIpv4}"
          "SERVER_IPV6=${cfg.dns.serverIpv6}"
        ];

        EnvironmentFile =
          (optionals (cfg.builder.cachix.authTokenFile != null) [ cfg.builder.cachix.authTokenFile ])
          ++ (optionals cfg.dns.enable [ cfg.dns.cloudflare.apiTokenFile ]);

        AmbientCapabilities = [ "CAP_NET_BIND_SERVICE" ];
        CapabilityBoundingSet = [ "CAP_NET_BIND_SERVICE" ];
      };
    };

    # Create projects configuration file for Kennel to read on startup
    environment.etc."kennel/projects.json" = mkIf (cfg.projects != { }) {
      text = builtins.toJSON (mapAttrsToList
        (name: proj: {
          inherit name;
          repo_url = proj.repoUrl;
          repo_type = proj.repoType;
          webhook_secret_file = proj.webhookSecretFile;
          default_branch = proj.defaultBranch;
        })
        cfg.projects);
      mode = "0440";
      user = cfg.user;
      group = cfg.group;
    };

    systemd.tmpfiles.rules = [
      "d /var/lib/kennel 0755 ${cfg.user} ${cfg.group} -"
      "d /var/lib/kennel/builds 0755 ${cfg.user} ${cfg.group} -"
      "d /var/lib/kennel/sites 0755 ${cfg.user} ${cfg.group} -"
      "d /var/lib/kennel/logs 0755 ${cfg.user} ${cfg.group} -"
      "d /var/lib/kennel/services 0755 ${cfg.user} ${cfg.group} -"
      "d /var/lib/kennel/acme 0700 ${cfg.user} ${cfg.group} -"
      "d /run/kennel 0755 ${cfg.user} ${cfg.group} -"
      "d /run/kennel/secrets 0700 ${cfg.user} ${cfg.group} -"
    ];

    networking.firewall = {
      allowedTCPPorts = [ cfg.api.port ]
        ++ optional cfg.router.tls.enable 443
        ++ optional (!cfg.router.tls.enable) 80;
    };

    nix.settings = {
      extra-substituters = [ "https://scottylabs.cachix.org" ];
      extra-trusted-public-keys = [
        "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
      ];
    };
  };
}
