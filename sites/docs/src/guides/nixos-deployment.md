# NixOS Deployment

Kennel provides a NixOS module for declarative deployment and configuration.

## Basic Configuration

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

    resources = {
      postgres = {
        enable = true;
        socketDir = "/run/postgresql";
      };
      valkey = {
        enable = true;
        socketPath = "/run/valkey/valkey.sock";
        maxDatabases = 32;
      };
      garage = {
        enable = true;
        adminEndpoint = "http://localhost:3903";
        s3Endpoint = "http://localhost:3900";
        adminTokenFile = "/run/secrets/garage-admin-token";
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

```nix
{
  services.kennel.router.tls = {
    enable = true;
    email = "admin@example.com";
  };
}
```

The module automatically obtains certificates from Let's Encrypt, opens port 443 in the firewall, handles certificate renewal, and stores certificates in `/var/lib/kennel/acme`.

For testing, use `staging = true` to avoid Let's Encrypt rate limits.

## Database Configuration

### Local PostgreSQL (Default)

```nix
{
  services.kennel.database = {
    createLocally = true;
    name = "kennel";
    user = "kennel";
  };
}
```

Connects via Unix socket at `/run/postgresql`.

### External PostgreSQL

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

## Project Configuration

Projects are configured declaratively. On startup, Kennel syncs these to the database.

```nix
{
  services.kennel.projects = {
    myapp = {
      repoUrl = "https://github.com/user/myapp";
      repoType = "github";
      webhookSecretFile = "/run/secrets/myapp-webhook";
      defaultBranch = "main";
    };
  };
}
```

### Webhook Secrets

Each project requires a webhook secret for verifying requests:

```bash
echo "your-secret-here" > /run/secrets/myapp-webhook
chmod 400 /run/secrets/myapp-webhook
```

Use the same secret when configuring the webhook in your Git repository.

## Resource Providers

Kennel provisions per-deployment infrastructure resources within shared host-level services.

### PostgreSQL

```nix
{
  services.kennel.resources.postgres = {
    enable = true;
    socketDir = "/run/postgresql";
  };
}
```

Creates a database per deployment (`kennel_{project}_{branch}`), authenticates via Unix socket peer auth. Injected as `DATABASE_URL`.

### Valkey

```nix
{
  services.kennel.resources.valkey = {
    enable = true;
    socketPath = "/run/valkey/valkey.sock";
    maxDatabases = 32;
  };
}
```

Allocates a DB number (0-31) per deployment from the shared Valkey instance. Injected as `VALKEY_URL`. Configure Valkey with `databases 32` to support the full range.

### Garage

```nix
{
  services.kennel.resources.garage = {
    enable = true;
    adminEndpoint = "http://localhost:3903";
    s3Endpoint = "http://localhost:3900";
    adminTokenFile = "/run/secrets/garage-admin-token";
  };
}
```

Creates an S3 bucket and scoped API key per deployment via the [Garage](https://garagehq.deuxfleurs.fr/) admin API. Injected as `S3_ENDPOINT`, `S3_BUCKET`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`.

## DNS Management

Kennel manages DNS records via Cloudflare using wildcard records per project.

```nix
{
  services.kennel.dns = {
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
}
```

Creates `*.myapp.example.com` wildcard records so all branch deployments resolve automatically. Custom domains get individual A/AAAA records.

## Directory Structure

Created automatically:

- `/var/lib/kennel/builds` -- build working directories
- `/var/lib/kennel/sites` -- static site deployments
- `/var/lib/kennel/logs` -- build logs
- `/var/lib/kennel/services` -- service working directories
- `/var/lib/kennel/acme` -- TLS certificates
- `/run/kennel/secrets` -- runtime secrets

## Security

The systemd service includes hardening:

- `NoNewPrivileges=true`
- `PrivateTmp=true`
- `ProtectSystem=strict`
- `ProtectHome=true`
- `ReadWritePaths` limited to `/var/lib/kennel`, `/run/kennel`
- `CAP_SETUID` and `CAP_SETGID` for user switching in the process supervisor
- `Delegate=yes` for cgroup v2 delegation (process resource isolation)

## Service Management

```bash
systemctl status kennel
journalctl -u kennel -f
systemctl restart kennel
```

## Troubleshooting

For deployment and operational issues, see the [Troubleshooting Guide](./troubleshooting.md).
