# Local Development

## Installing Nix

Install [Lix](https://lix.systems/install/) with the ScottyLabs binary cache. Without the cache, `nix develop` builds ScottyLabs' own packages from source on your machine, which can take a long time.

```bash
curl -sSf -L https://install.lix.systems/lix | sh -s -- install \
  --extra-conf "extra-substituters = https://scottylabs.cachix.org" \
  --extra-conf "extra-trusted-public-keys = scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
```

If Lix or Nix is already installed, add the same two settings to `/etc/nix/nix.conf` and restart the daemon with `sudo systemctl restart nix-daemon` on Linux or `sudo launchctl kickstart -k system/org.nixos.nix-daemon` on macOS.

### iets

[iets](https://github.com/numtide/iets) is a faster replacement for the `nix` command, including `nix develop`. For anything it doesn't support yet, it runs Lix instead.

You don't need iets, but if your machine runs NixOS or nix-darwin, you can install it with the configuration below.

On NixOS, add `github:numtide/iets` as the `iets` input of your system flake and enable its module:

```nix
{ inputs, pkgs, ... }:
{
  imports = [ inputs.iets.nixosModules.default ];
  iets.classic = {
    enable = true;
    nix = pkgs.lixPackageSets.stable.lix;
  };
}
```

On nix-darwin, set the Nix package to iets over Lix:

```nix
{ inputs, pkgs, ... }:
let
  iets = inputs.iets.packages.${pkgs.stdenv.hostPlatform.system}.default;
in
{
  nix.package = inputs.iets.lib.mkClassic pkgs iets pkgs.lixPackageSets.stable.lix;
}
```

## Local services

The dev shell includes [pitchfork](https://pitchfork.jdx.dev) when the project enables a local service such as `scottylabs.postgres`. Use it to start and stop those services:

```bash
pitchfork start --local # starts every service and waits on each one's readiness check
pitchfork status
pitchfork logs postgres # follows one service's output
pitchfork stop --local
```

To run your own processes the same way, such as a dev server or a worker, define them under `scottylabs.processes`:

```nix
scottylabs.processes = {
  web = {
    run = "cd sites/app && deno task dev";
    ready = "curl -sf http://localhost:5173";
  };
  worker = {
    run = "cargo run -p worker";
    depends = [ "postgres-setup" ];
  };
};
```

Each process runs from the project root with the dev shell's tools and variables, such as `DATABASE_URL`, but without your secrets. A process that needs them runs under `secretspec run`, for example `secretspec run -- cargo run -p worker`. The [flake options](../reference/flake-options.md) list every field a process takes.

## OAuth relay

When developing the OAuth flow locally, enable the relay so the IdP redirects back through it the way it does in production:

```nix
scottylabs.ricochet = {
  enable = true;
  appUrl = "http://localhost:3000";
};
```

[Governance](https://docs.governance.scottylabs.org/features/#oidc) already seeds the correct `OAUTH_RELAY_URL` for `profiles.dev`, so `enable` only runs Ricochet locally. `appUrl` is your service's local URL, the port your dev server listens on, exported as `APP_URL` so the relay can return to it. It only applies to development, since kennel sets `APP_URL` for deployed services (see [Runtime environment](./deploying.md#runtime-environment)), so leave `APP_URL` out of `secretspec.toml`.

## Scripts

`scottylabs.shell.scripts` defines commands available in the dev shell. Use them for project-specific development tasks such as database migrations or code generation:

```nix
scottylabs.shell.scripts = {
  migrate = "sea-orm-cli migrate up -d crates/migration";
  generate-api = "cd app && deno task generate-api";
};
```

## Git hooks

Entering the dev shell installs the project's git hooks, which run on every commit. They check formatting, run the linters and tests the project enables, look for leaked secrets, and require a [Conventional Commits](https://www.conventionalcommits.org/) message such as `fix: handle empty search results`. To run them against the whole repository without committing, use:

```bash
prek run --all-files
```
