# RFC 0005: NixOS Module

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-02-25
- **Updated:** 2026-02-25

## Overview

Provide a declarative NixOS module for deploying and managing Kennel, enabling configuration through NixOS options and automatic systemd service setup.

## Motivation

Currently, deploying Kennel requires manual setup: creating directories, configuring environment variables, managing systemd units, setting up PostgreSQL, and handling TLS certificates. This is error-prone and inconsistent across deployments.

A NixOS module provides:

- Declarative configuration via NixOS options
- Automatic directory creation with correct permissions
- Integrated systemd service management
- Built-in PostgreSQL setup
- ACME/TLS certificate management
- Consistent deployments across systems

## Goals

- Expose all Kennel configuration as NixOS options
- Automatically create required directories with correct ownership
- Generate and manage systemd service units
- Integrate with NixOS PostgreSQL module
- Support ACME/Let's Encrypt for TLS certificates
- Provide sensible defaults while allowing full customization

## Non-Goals

- Multi-instance deployments (running multiple Kennel instances on same host)
- Container or VM-based deployment (this is for bare NixOS)
- Cloud provider-specific configurations (AWS, GCP, etc.)
- Custom reverse proxy integration (nginx, Caddy) -- Kennel's built-in router is sufficient

## Detailed Design

### Module Location

Create `nixos/default.nix` as the primary module file, following NixPkgs module conventions.

### Configuration Options

```nix
services.kennel = {
  enable = mkEnableOption "Kennel deployment platform";

  package = mkOption {
    type = types.package;
    default = pkgs.kennel;
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
```

### Generated Systemd Service

```nix
systemd.services.kennel = {
  description = "Kennel deployment platform";
  after = [ "network.target" "postgresql.service" ];
  wantedBy = [ "multi-user.target" ];

  serviceConfig = {
    Type = "notify";
    User = cfg.user;
    Group = cfg.group;
    ExecStart = "${cfg.package}/bin/kennel";
    Restart = "on-failure";
    RestartSec = 5;

    # Security hardening
    NoNewPrivileges = true;
    PrivateTmp = true;
    ProtectSystem = "strict";
    ProtectHome = true;
    ReadWritePaths = [
      "/var/lib/kennel"
      "/run/kennel"
      "/etc/systemd/system"  # For creating deployment units
    ];

    # Environment
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
    ] ++ (optionals cfg.router.tls.enable [
      "ACME_EMAIL=${cfg.router.tls.email}"
      "ACME_STAGING=${if cfg.router.tls.staging then "true" else "false"}"
    ]) ++ (optionals cfg.builder.cachix.enable [
      "CACHIX_CACHE_NAME=${cfg.builder.cachix.cacheName}"
    ]);

    EnvironmentFile = optionals (cfg.builder.cachix.authTokenFile != null) [
      cfg.builder.cachix.authTokenFile
    ];

    # Capabilities for binding to port 80
    AmbientCapabilities = [ "CAP_NET_BIND_SERVICE" ];
    CapabilityBoundingSet = [ "CAP_NET_BIND_SERVICE" ];
  };
};
```

### Directory Management

```nix
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
```

### User and Group Creation

```nix
users.users.${cfg.user} = mkIf (cfg.user == "kennel") {
  isSystemUser = true;
  group = cfg.group;
  description = "Kennel service user";
};

users.groups.${cfg.group} = mkIf (cfg.group == "kennel") {};
```

### PostgreSQL Integration

```nix
services.postgresql = mkIf cfg.database.createLocally {
  enable = true;
  ensureDatabases = [ cfg.database.name ];
  ensureUsers = [{
    name = cfg.database.user;
    ensureDBOwnership = true;
  }];
};
```

### Firewall Configuration

```nix
networking.firewall = mkIf cfg.enable {
  allowedTCPPorts = [
    cfg.api.port
  ] ++ (optional cfg.router.tls.enable 443)
    ++ (optional (!cfg.router.tls.enable) 80);
};
```

### Binary Cache Configuration

```nix
nix.settings = mkIf cfg.enable {
  extra-substituters = [ "https://scottylabs.cachix.org" ];
  extra-trusted-public-keys = [
    "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
  ];
};
```

### Example Configuration

```nix
{
  services.kennel = {
    enable = true;

    router = {
      baseDomain = "scottylabs.org";
      tls = {
        enable = true;
        email = "admin@scottylabs.org";
      };
    };

    builder.cachix = {
      enable = true;
      cacheName = "kennel";
      authTokenFile = "/run/secrets/cachix-auth-token";
    };
  };
}
```

Minimal configuration (uses all defaults):

```nix
{
  services.kennel = {
    enable = true;
    router.baseDomain = "example.com";
  };
}
```

### Assertions and Warnings

```nix
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
];

warnings = optional (cfg.router.tls.staging) 
  "Kennel TLS is using Let's Encrypt staging environment. Certificates will not be trusted by browsers.";
```

## Alternatives Considered

### Single Monolithic Module vs Submodules

**Chosen:** Single module with nested options

**Alternative:** Split into separate modules (kennel-api, kennel-builder, kennel-router, kennel-deployer)

**Reasoning:** Kennel components are tightly coupled and must run together. Separate modules would add complexity without benefit. The nested option structure (`services.kennel.api.*`, `services.kennel.router.*`) provides sufficient organization.

### Manual systemd Units vs Generated

**Chosen:** Generate systemd unit from NixOS options

**Alternative:** Require users to write their own systemd units

**Reasoning:** Generated units ensure consistency and reduce configuration errors. The module can apply security hardening and correct dependencies automatically.

### Declarative Database vs Imperative Setup

**Chosen:** Integrate with NixOS PostgreSQL module for declarative database creation

**Alternative:** Require manual database setup

**Reasoning:** NixOS philosophy is declarative configuration. Automatic database creation with `ensureDatabases` and `ensureUsers` eliminates manual setup steps and ensures idempotent deployments.

### Environment Variables vs Config File

**Chosen:** Use environment variables for configuration

**Alternative:** Generate TOML/YAML config file

**Reasoning:** Kennel already uses environment variables. Adding config file support would require changes to the application. Environment variables integrate cleanly with systemd and are the current pattern.

## Open Questions

1. **Backup integration:** Should the module provide options for automated backups of `/var/lib/kennel` and the PostgreSQL database?

1. **Logging configuration:** Should there be options to configure log levels per-component (webhook, builder, deployer, router) or is global `RUST_LOG` sufficient?

1. **Port range configuration:** Should the service port allocation range (currently hardcoded 18000-19999) be configurable via NixOS options?

1. **Valkey integration:** Preview database feature uses Valkey (Redis). Should the module include optional Valkey service integration similar to PostgreSQL?

## Implementation Phases

### Core Module Structure

Create the basic module skeleton with option definitions and systemd service generation. Define all `services.kennel.*` options with types, defaults, and descriptions. Generate the main `systemd.services.kennel` unit with proper dependencies and environment variables. Implement basic assertions to validate required options.

### Security and Directory Setup

Configure security hardening for the systemd service and automatic directory creation. Use systemd tmpfiles rules to create `/var/lib/kennel/*` and `/run/kennel/*` directories with correct ownership. Apply systemd security options (`NoNewPrivileges`, `ProtectSystem`, `ReadWritePaths`). Create system user and group with appropriate permissions. Add `CAP_NET_BIND_SERVICE` capability for binding to privileged ports.

### Database Integration

Integrate with NixOS PostgreSQL module for automatic database provisioning. Use `services.postgresql.ensureDatabases` and `ensureUsers` when `database.createLocally = true`. Generate correct `DATABASE_URL` connection string for Unix socket or TCP connections. Add proper service ordering (`after = ["postgresql.service"]`).

### TLS and ACME Support

Add options for Let's Encrypt certificate management. Define `router.tls.*` options for email, staging mode, and enable flag. Configure environment variables for ACME client. Set up `/var/lib/kennel/acme` directory for certificate storage. Add firewall rules for ports 80 and 443 based on TLS configuration.

### Cachix Integration

Support optional Cachix binary cache push and consumption. Add `builder.cachix.*` options for cache name and auth token file. Use `EnvironmentFile` to securely load Cachix auth token. Add assertions to ensure required Cachix options are set when enabled. Automatically configure `nix.settings.extra-substituters` and `nix.settings.extra-trusted-public-keys` to use ScottyLabs Cachix cache (scottylabs.cachix.org) for faster builds.

### Documentation and Testing

Write comprehensive documentation and example configurations. Document all options in module comments. Provide example configurations for common scenarios (minimal, TLS-enabled, Cachix-enabled). Create NixOS VM tests to verify module functionality. Test database creation, systemd service startup, and directory permissions.
