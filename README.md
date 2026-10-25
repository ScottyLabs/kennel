<div align="center">
  <h1>
    <pre>kennel</pre>
  </h1>
</div>

Kennel is the deployment platform for [ScottyLabs](https://scottylabs.org). On every push it deploys what [nixbot](https://github.com/Mic92/nixbot) built and pushed to the binary cache, runs its services as systemd units and static sites through [Caddy](https://caddyserver.com), provisions per-deployment resources, resolves secrets from [OpenBao](https://openbao.org), manages DNS through [Cloudflare](https://www.cloudflare.com), and serves it over HTTPS.

Every branch and open pull request gets its own deployment at `{project}-{branch}.scottylabs.net`, redeployed on every push and torn down when the branch or PR closes.

A project's `flake.nix`, built on the shared flake-parts module in this repo (`nix/flake-module`), defines its dev shell, its local services, its checks, and the services and sites kennel deploys. The daemon and the module are versioned together.

Internally, kennel is a single daemon that takes git webhooks and nixbot's build reports, substitutes each commit's artifact from the binary cache, deploys it, and reconciles running state against declared intent. It keeps intent in SQLite and leaves runtime state to systemd, Caddy, and Nix.

Kennel can also publish the hosts of its live deployments to a file. [ricochet](https://codeberg.org/anish/ricochet), a stateless OAuth2 callback relay, reads that file as its allowlist, so ephemeral previews can complete logins that identity providers won't issue wildcard redirect URIs for.

## Layout

- `crates/kennel`: the daemon
- `crates/kennel-config`: shared types and the `kennel.json` contract
- `crates/kennel-provision`: resource provisioning (PostgreSQL, SQLite, Valkey, Garage)
- `crates/entity`, `crates/migration`: SQLite schema and SeaORM entities
- `nix/flake-module`: the flake-parts module projects build on
- `nix/lib`: the build helpers projects get as the `helpers` module argument
- `nix/nixos.nix`: NixOS module to run the daemon on a host
- `sites/docs`: documentation (mdBook)

Adding options, checks, or build helpers is documented in [`nix/README.md`](nix/README.md).

## License

Licensed under the [GNU Affero General Public License v3.0](LICENSE).
