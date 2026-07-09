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

{{#include generated/nixos-module.md}}

## What the module configures

- A systemd service for kennel with `Delegate=yes` for cgroup v2 access
- A polkit rule allowing the kennel user to create transient systemd units via D-Bus
- A `kennel.slice` for all managed deployment units
- A Caddy virtualhost for the kennel domain with automatic TLS, plus the admin API for dynamic route management
- tmpfiles rules for `/var/lib/kennel` subdirectories
- Firewall rules for ports 80 and 443
- Cachix binary cache substituter
