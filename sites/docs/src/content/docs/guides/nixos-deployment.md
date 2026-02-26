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
    
    projects.myapp = {
      repoUrl = "https://github.com/user/myapp";
      repoType = "github";
      webhookSecretFile = "/run/secrets/myapp-webhook";
    };
  };
}
```

This minimal configuration:

- Creates a PostgreSQL database automatically
- Runs Kennel on port 80 (HTTP)
- Exposes the API on port 3000
- Allows 2 concurrent builds
- Uses default directory locations
- Configures one project named "myapp"

## Full Configuration Example

```nix
{
  services.kennel = {
    enable = true;

    projects = {
      kennel = {
        repoUrl = "https://codeberg.org/ScottyLabs/kennel";
        repoType = "forgejo";
        webhookSecretFile = "/run/secrets/kennel-webhook";
      };
      website = {
        repoUrl = "https://codeberg.org/ScottyLabs/website";
        repoType = "forgejo";
        webhookSecretFile = "/run/secrets/website-webhook";
      };
    };

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

    dns = {
      enable = true;
      provider = "cloudflare";
      
      cloudflare = {
        apiTokenFile = "/run/secrets/cloudflare-api-token";
        zones = {
          "scottylabs.org" = "abc123def456";
        };
      };
      
      serverIpv4 = "1.2.3.4";
      serverIpv6 = "2001:db8::1";
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

### TLS Troubleshooting

For detailed TLS troubleshooting, see the [Troubleshooting Guide](./troubleshooting.md#tls-certificate-acquisition-failed).

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

## Project Configuration

Projects are configured declaratively in the NixOS module. Each project represents a Git repository to deploy.

```nix
{
  services.kennel.projects = {
    myapp = {
      repoUrl = "https://github.com/user/myapp";
      repoType = "github";  # or "forgejo"
      webhookSecretFile = "/run/secrets/myapp-webhook";
      defaultBranch = "main";  # optional, defaults to "main"
    };
    
    another-app = {
      repoUrl = "https://codeberg.org/user/another-app";
      repoType = "forgejo";
      webhookSecretFile = "/run/secrets/another-app-webhook";
    };
  };
}
```

On startup, Kennel syncs these projects to the database. When you add a project, it's automatically configured. When you remove a project from the configuration, it's removed from Kennel (and associated wildcard DNS is cleaned up).

### Webhook Secrets

Each project requires a webhook secret for verifying webhook requests from Forgejo or GitHub. Store these in files:

```bash
echo "your-secret-here" > /run/secrets/myapp-webhook
chmod 400 /run/secrets/myapp-webhook
```

Use the same secret when configuring the webhook in your Git repository.

## DNS Management

Kennel can automatically manage DNS records via Cloudflare. DNS uses **wildcard records per project** - when a project is configured, Kennel creates `*.project.basedomain.com` pointing to your server.

### DNS Configuration

```nix
{
  services.kennel = {
    enable = true;
    router.baseDomain = "example.com";
    
    projects.myapp = {
      repoUrl = "https://github.com/user/myapp";
      repoType = "github";
      webhookSecretFile = "/run/secrets/myapp-webhook";
    };
    
    dns = {
      enable = true;
      provider = "cloudflare";
      
      cloudflare = {
        apiTokenFile = "/run/secrets/cloudflare-api-token";
        zones = {
          "example.com" = "your-zone-id";
        };
      };
      
      serverIpv4 = "1.2.3.4";
      serverIpv6 = "2001:db8::1";
    };
  };
}
```

The Cloudflare API token needs DNS edit permissions. Create it at [Cloudflare Dashboard](https://dash.cloudflare.com/profile/api-tokens).

### How DNS Works

Kennel creates **one wildcard DNS record per project**:

```
*.myapp.example.com -> 1.2.3.4 (A record)
*.myapp.example.com -> 2001:db8::1 (AAAA record)
```

This allows all branch deployments to resolve automatically:
- `api-main.myapp.example.com` -> resolves via wildcard
- `api-feature-x.myapp.example.com` -> resolves via wildcard
- `web-main.myapp.example.com` -> resolves via wildcard

No individual DNS records are created per deployment. The wildcard covers all branches and services.

When you remove a project from the configuration, Kennel automatically deletes the wildcard DNS records.

### Custom Domain DNS

Custom domains specified in `kennel.toml` require individual DNS records because they fall outside the wildcard pattern:

```toml
[services.api]
custom_domain = "api.mycompany.com"

[static_sites.web]
custom_domain = "mycompany.com"
```

**Behavior:**
- **Auto-generated domain**: `api-main.myapp.example.com` uses wildcard DNS (no individual record)
- **Custom domain**: `api.mycompany.com` gets individual A and AAAA records created

When deploying a service or static site with a custom domain:
1. Kennel creates A and AAAA records pointing to the server IP
2. Records are stored in the database linked to the deployment
3. When the deployment is torn down, DNS records are automatically deleted

**Important:** The custom domain must be in a DNS zone configured in `services.kennel.dns.cloudflare.zones`. Kennel cannot create DNS records for domains in zones it doesn't manage.

### Multiple Zones

Kennel supports multiple DNS zones for different base domains:

```nix
{
  services.kennel.dns.cloudflare.zones = {
    "example.com" = "zone-id-1";
    "another-domain.org" = "zone-id-2";
  };
}
```

Projects are assigned to zones based on the `router.baseDomain` setting.

### Cloudflare Proxy

DNS records are created with `proxied: true`, enabling Cloudflare's CDN and DDoS protection.

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

For deployment and operational issues, see the [Troubleshooting Guide](./troubleshooting.md).
