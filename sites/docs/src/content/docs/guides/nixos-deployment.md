---
title: NixOS Deployment
description: Deploy Kennel on NixOS using the declarative module
---

Kennel provides a NixOS module for declarative deployment and configuration.

## Prerequisites

- NixOS system
- Kennel package available in your nixpkgs or as a flake

## Basic Configuration

Add to your NixOS configuration:

```nix
{
  services.kennel = {
    enable = true;
    router.baseDomain = "example.com";
  };
}
```

This minimal configuration:

- Creates a PostgreSQL database automatically
- Runs Kennel on port 80 (HTTP)
- Exposes the API on port 3000
- Allows 2 concurrent builds
- Uses default directory locations

## Full Configuration Example

```nix
{
  services.kennel = {
    enable = true;

    router = {
      baseDomain = "scottylabs.org";
      address = "0.0.0.0:80";
      
      tls = {
        enable = true;
        email = "admin@scottylabs.org";
        staging = false;
      };
    };

    api = {
      host = "0.0.0.0";
      port = 3000;
    };

    database = {
      createLocally = true;
      name = "kennel";
      user = "kennel";
      host = "/run/postgresql";
      port = 5432;
    };

    builder = {
      maxConcurrentBuilds = 4;
      workDir = "/var/lib/kennel/builds";
      
      cachix = {
        enable = true;
        cacheName = "scottylabs";
        authTokenFile = "/run/secrets/cachix-auth-token";
      };
    };

    cleanup.interval = 600;

    user = "kennel";
    group = "kennel";
  };
}
```

## TLS/HTTPS Configuration

Enable TLS with Let's Encrypt:

```nix
{
  services.kennel = {
    enable = true;
    router = {
      baseDomain = "example.com";
      tls = {
        enable = true;
        email = "admin@example.com";
      };
    };
  };
}
```

The module:

- Automatically obtains certificates from Let's Encrypt
- Opens port 443 in the firewall
- Handles certificate renewal
- Stores certificates in `/var/lib/kennel/acme`

For testing, use the staging environment:

```nix
{
  services.kennel.router.tls = {
    enable = true;
    email = "admin@example.com";
    staging = true;
  };
}
```

Staging certificates won't be trusted by browsers but avoid rate limits during testing.

## Database Configuration

### Local PostgreSQL (Default)

The module creates a local PostgreSQL database automatically:

```nix
{
  services.kennel.database = {
    createLocally = true;
    name = "kennel";
    user = "kennel";
  };
}
```

The service connects via Unix socket at `/run/postgresql`.

### External PostgreSQL

To use an external database:

```nix
{
  services.kennel.database = {
    createLocally = false;
    host = "db.example.com";
    port = 5432;
    name = "kennel";
    user = "kennel";
  };
}
```

You'll need to configure authentication separately (password in environment file, etc.).

## Cachix Integration

Push build artifacts to Cachix:

```nix
{
  services.kennel.builder.cachix = {
    enable = true;
    cacheName = "your-cache";
    authTokenFile = "/run/secrets/cachix-auth-token";
  };
}
```

The auth token file should contain:

```
CACHIX_AUTH_TOKEN=your-token-here
```

The module automatically configures NixOS to use the ScottyLabs Cachix cache for faster builds.

## Directory Structure

The module creates these directories automatically:

- `/var/lib/kennel/builds` -> Build working directories
- `/var/lib/kennel/sites` -> Static site deployments
- `/var/lib/kennel/logs` -> Service logs
- `/var/lib/kennel/services` -> Service working directories
- `/var/lib/kennel/acme` -> TLS certificates (mode 0700)
- `/run/kennel/secrets` -> Runtime secrets (mode 0700)

All directories are owned by the configured user and group (default: `kennel:kennel`).

## Firewall

The module automatically opens required ports:

- API port (default: 3000)
- Router port 80 (HTTP) or 443 (HTTPS with TLS)

## Security

The systemd service includes hardening:

- `NoNewPrivileges=true` -> Cannot gain privileges
- `PrivateTmp=true` -> Isolated `/tmp`
- `ProtectSystem=strict` -> Read-only filesystem except allowed paths
- `ProtectHome=true` -> No access to home directories
- `ReadWritePaths` -> Limited to `/var/lib/kennel`, `/run/kennel`, `/etc/systemd/system`
- `CAP_NET_BIND_SERVICE` -> Only capability granted (for binding to port 80/443)

## Service Management

Kennel runs as a systemd service:

```bash
# Check status
systemctl status kennel

# View logs
journalctl -u kennel -f

# Restart service
systemctl restart kennel

# Stop service
systemctl stop kennel
```

## Customization

### Custom User/Group

```nix
{
  services.kennel = {
    enable = true;
    user = "myuser";
    group = "mygroup";
  };
  
  users.users.myuser = {
    isSystemUser = true;
    group = "mygroup";
  };
  
  users.groups.mygroup = {};
}
```

### Build Concurrency

Adjust based on available CPU cores:

```nix
{
  services.kennel.builder.maxConcurrentBuilds = 8;
}
```

### Cleanup Interval

Change how often expired deployments are checked:

```nix
{
  services.kennel.cleanup.interval = 300;  # 5 minutes
}
```

## Troubleshooting

### Service fails to start

Check logs:

```bash
journalctl -u kennel -n 50
```

Common issues:

- Database connection failed -> Check PostgreSQL is running
- Permission denied -> Check directory ownership
- Port already in use -> Check if another service is using port 80/443

### Database connection issues

Verify PostgreSQL is running:

```bash
systemctl status postgresql
```

Check database exists:

```bash
sudo -u postgres psql -c '\l' | grep kennel
```

### TLS certificate issues

View ACME logs in journal:

```bash
journalctl -u kennel | grep -i acme
```

Ensure DNS points to your server before enabling TLS.

### Port binding issues

If port 80 or 443 is already in use:

```bash
sudo lsof -i :80
sudo lsof -i :443
```

You can change the router port:

```nix
{
  services.kennel.router.address = "0.0.0.0:8080";
}
```
