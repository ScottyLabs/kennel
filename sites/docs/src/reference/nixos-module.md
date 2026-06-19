# NixOS Module

The kennel NixOS module configures the kennel service, Caddy, systemd integration, and resource provisioning on a NixOS host.

```nix
{ kennel, ... }:
{
  imports = [ kennel.nixosModules.default ];

  services.kennel = {
    enable = true;
    package = kennel.packages.x86_64-linux.kennel;
    devenvPackage = kennel.packages.x86_64-linux.devenv;
    webhookSecretFile = config.age.secrets.kennel-webhook.path;
    environmentFile = config.age.secrets.kennel.path;

    domains = {
      ephemeral = "scottylabs.net";
      cloudflare.zones."scottylabs.org" = "<zone-id>";
    };

    resources.postgres = {
      enable = true;
      socketDir = "/run/postgresql";
    };

    secrets = {
      enable = true;
      vaultEndpoint = "https://secrets2.scottylabs.org";
    };
  };
}
```

## Options

### `services.kennel.enable`

Enable the kennel deployment platform.

Type: `bool`, default: `false`

### `services.kennel.package`

The kennel package to use.

Type: `package`

### `services.kennel.devenvPackage`

The devenv package. The build worker uses `devenv build` to evaluate project kennel configs from their `devenv.nix`.

Type: `package`

### `services.kennel.environmentFile`

Path to an environment file containing secrets like `VAULT_TOKEN`, `CACHIX_AUTH_TOKEN`, and `GARAGE_ADMIN_TOKEN`. Loaded by systemd before the service starts.

Type: `nullOr path`, default: `null`

### `services.kennel.api.host` / `services.kennel.api.port`

API server bind address and port.

Type: `str` / `port`, defaults: `"0.0.0.0"` / `3000`

### `services.kennel.webhookSecretFile`

Path to a file containing the HMAC secret used to verify all incoming webhooks. This is a single secret shared across all projects, provisioned by governance.

Type: `path`

### `services.kennel.domains.ephemeral`

Base domain for auto-generated deployment URLs. A wildcard DNS record should point `*.{domain}` to the kennel server.

Type: `str`, default: `"scottylabs.net"`

### `services.kennel.domains.cloudflare.zones`

Map of domain names to Cloudflare zone IDs. When this is non-empty, `publicIp` is set, and the `CLOUDFLARE_API_TOKEN` env var is provided (typically via the `environmentFile` secret), kennel automatically manages A records for any custom domain whose suffix matches one of the configured zones. The most specific zone wins for nested domains.

The token must have `Zone:DNS:Edit` permission on the zones listed.

Records are upserted on deploy and on each reconciliation pass (so they self-heal if pruned externally). Records are deleted only when kennel tears the deployment down, which happens in three cases:

1. The branch backing the deployment is deleted from the source repo (push event with deleted=true on that ref).
1. The deployment is associated with a pull request and that pull request is closed.
1. The deployment is on a `dev` or `preview` branch and exceeds `DEPLOYMENT_EXPIRY_DAYS` since its last update during a reconciliation pass.

Production deployments are not subject to expiry, so a record for a production custom domain stays in place until the project's main branch is deleted or the deployment row is removed manually.

Type: `attrsOf str`, default: `{}`

### `services.kennel.domains.cloudflare.publicIp`

Public IPv4 used as the content of the A records that kennel creates for custom domains. Required to enable DNS automation.

Type: `nullOr str`, default: `null`

### `services.kennel.domain`

Public domain for the kennel API and webhook endpoint. The module configures a Caddy virtualhost with automatic TLS for this domain, reverse-proxying to the API server.

Type: `str`, default: `"kennel.scottylabs.org"`

### `services.kennel.caddy.adminUrl`

Caddy admin API URL.

Type: `str`, default: `"http://localhost:2019"`

### `services.kennel.customDomainsFile`

Writes the custom domains of all live deployments to this path, one per line, refreshed on each reconciliation pass. Unset disables it.

Type: `nullOr str`, default: `null`

### `services.kennel.builder.maxConcurrentBuilds`

Maximum number of concurrent nix builds.

Type: `int`, default: `2`

### `services.kennel.builder.workDir`

Build working directory.

Type: `path`, default: `"/var/lib/kennel/builds"`

### `services.kennel.builder.cachix.enable` / `services.kennel.builder.cachix.cacheName`

Enable pushing build artifacts to a Cachix binary cache.

### `services.kennel.resources.postgres`

Enable PostgreSQL resource provisioning. Kennel creates a database per deployment using the specified socket directory for peer authentication.

- `enable` (`bool`, default: `false`)
- `socketDir` (`path`, default: `"/run/postgresql"`)

### `services.kennel.resources.valkey`

Enable Valkey resource provisioning. Kennel allocates a DB number per deployment from the shared instance.

- `enable` (`bool`, default: `false`)
- `socketPath` (`path`, default: `"/run/valkey/valkey.sock"`)

### `services.kennel.resources.garage`

Enable Garage S3 resource provisioning. Kennel creates a bucket and API key per deployment. Requires `GARAGE_ADMIN_TOKEN` in the environment file.

- `enable` (`bool`, default: `false`)
- `adminEndpoint` (`str`, default: `"http://localhost:3903"`)
- `s3Endpoint` (`str`, default: `"http://localhost:3900"`)

### `services.kennel.secrets`

Enable secretspec/OpenBao secret resolution at deploy time.

- `enable` (`bool`, default: `false`)
- `vaultEndpoint` (`str`, default: `"https://secrets2.scottylabs.org"`)

### `services.kennel.forgejo`

Forgejo API access for posting deployment status comments on pull requests. Required.

- `apiUrl` (`str`, default: `"https://codeberg.org/api/v1"`) -- Forgejo API base URL
- `apiTokenFile` (`path`, required) -- path to a file containing an API token with the `write:issue` scope. Kennel uses this to post and update a sticky comment on each PR listing its deployment URLs, and to mark the comment torn down when the PR is closed.

## What the module configures

- A systemd service for kennel with `Delegate=yes` for cgroup v2 access
- A polkit rule allowing the kennel user to create transient systemd units via D-Bus
- A `kennel.slice` for all managed deployment units
- A Caddy virtualhost for the kennel domain with automatic TLS, plus the admin API for dynamic route management
- tmpfiles rules for `/var/lib/kennel` subdirectories
- Firewall rules for ports 80 and 443
- Cachix binary cache substituter
