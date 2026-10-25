# Local Development

## Local services

The dev shell includes [pitchfork](https://pitchfork.jdx.dev) when the project enables a local service such as `scottylabs.postgres`. Use it to start and stop those services:

```bash
pitchfork start --local # starts every service and waits on each one's readiness check
pitchfork status
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
    depends = [ "postgres" ];
  };
};
```

The [flake options](../reference/flake-options.md) list every field a process takes.

## OAuth relay

When developing the OAuth flow locally, enable the relay so the IdP redirects back through it the way it does in production:

```nix
scottylabs.ricochet = {
  enable = true;
  appUrl = "http://localhost:3000";
};
```

Governance already seeds the correct `OAUTH_RELAY_URL` for `profiles.dev`, so `enable` only runs Ricochet locally. `appUrl` is your service's local URL, the port your dev server listens on, exported as `APP_URL` so the relay can return to it. It only applies to development, since kennel sets `APP_URL` for deployed services (see [Runtime environment](./deploying.md#runtime-environment)), so leave `APP_URL` out of `secretspec.toml`.

## Scripts

`scottylabs.shell.scripts` defines commands available in the dev shell. Use them for project-specific development tasks such as database migrations or code generation:

```nix
scottylabs.shell.scripts = {
  migrate = "sea-orm-cli migrate up -d crates/migration";
  generate-api = "cd app && deno task generate-api";
};
```
