# kennel

Kennel is the deployment platform for [ScottyLabs](https://scottylabs.org). On every push it builds the project with Nix, runs its services as systemd units and static sites through [Caddy](https://caddyserver.com), provisions per-deployment resources, resolves secrets from [OpenBao](https://openbao.org), manages DNS through [Cloudflare](https://www.cloudflare.com), and serves it over HTTPS.

Every branch and open pull request gets its own deployment at `{project}-{branch}.scottylabs.net`, redeployed on every push and torn down when the branch or PR closes.

A project's `devenv.nix`, built on the shared [ScottyLabs devenv module](https://codeberg.org/scottylabs/devenv), defines its local development environment, its CI, and its production deployment. The daemon and the module are versioned together.

Internally, kennel is a single daemon that takes git webhooks, builds, deploys, and reconciles running state against declared intent. It keeps intent and build history in SQLite and leaves runtime state to systemd, Caddy, and Nix.

Kennel can also publish the hosts of its live deployments to a file. [ricochet](https://codeberg.org/anish/ricochet), a stateless OAuth2 callback relay, reads that file as its allowlist, so ephemeral previews can complete logins that identity providers won't issue wildcard redirect URIs for.

## Layout

- `crates/kennel`: the daemon
- `crates/kennel-config`: shared types and the devenv config contract
- `crates/kennel-provision`: resource provisioning (PostgreSQL, Valkey, Garage)
- `crates/entity`, `crates/migration`: SQLite schema and SeaORM entities
- `nixos/`: NixOS module to run it on a host
- `sites/docs`: documentation (mdBook)

## License

Licensed under the [GNU Affero General Public License v3.0](LICENSE).
