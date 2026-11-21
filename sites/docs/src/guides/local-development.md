# Local Development

## Installing Nix

Install [Lix](https://lix.systems/install/) with the ScottyLabs binary cache. Without it, `nix develop` builds ScottyLabs' own packages from source on your machine, which can take a long time.

```bash
curl -sSf -L https://install.lix.systems/lix | sh -s -- install \
  --extra-conf "extra-substituters = https://cache.scottylabs.org" \
  --extra-conf "extra-trusted-public-keys = deploy-01.scottylabs.org-1:TtFRYjsx76lUXkCDvobNkTFZGOeLZFKTzdKX9oyiCcY="
```

If Lix or Nix is already installed, add the same two settings to `/etc/nix/nix.conf` and restart the daemon with `sudo systemctl restart nix-daemon` on Linux or `sudo launchctl kickstart -k system/org.nixos.nix-daemon` on macOS.

The warning `ignoring the client-specified setting 'trusted-public-keys'` is harmless, since it only means Nix skipped the copy of the key in the project's `nixConfig`. If it's followed by `not signed by any of the keys in 'trusted-public-keys'`, the running daemon doesn't have the key. Check that both settings are in `/etc/nix/nix.conf` and restart the daemon with the command above, since it only reads the file at startup.

### Checking the install

```bash
nix --version                       # names Lix, not Nix
nix config show substituters        # lists https://cache.scottylabs.org
nix config show trusted-public-keys # lists deploy-01.scottylabs.org-1:...
```

If either setting is missing, the install flags above didn't land in `/etc/nix/nix.conf`. Add them there and restart the daemon. The symptom of a missing cache is a first `nix develop` that compiles a Rust toolchain instead of downloading one.

### `HTTP error 401` or `Bad credentials` from GitHub

Nix is sending a GitHub token that has expired or been revoked. GitHub rejects requests with a bad token outright, even for public repositories that would download fine without one. Find where the token is set with:

```bash
sudo grep -rs access-tokens ~/.config/nix/ /etc/nix/
echo "$NIX_CONFIG"
```

Replace the token with a fresh one, such as the output of `gh auth token`. Nix doesn't run commands in `nix.conf`, so paste the token itself. Deleting the `access-tokens` line also fixes the error, but without a token GitHub allows only 60 requests an hour, shared by everyone on the same IP address.

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

## The dev shell

Everything a project needs is in its dev shell: its language toolchains, `treefmt`, `prek`, `bao`, `secretspec`, [collar](#collar), and [pitchfork](#local-services) when the project runs local services. Nothing is installed globally, and two projects can pin different toolchain versions without colliding.

Enter it from inside the checkout:

```bash
git clone https://git.cmu.dev/ScottyLabs/<project>.git
cd <project>
nix develop
```

The shell is built for `x86_64-linux`, `aarch64-linux`, and `aarch64-darwin` on every push, so the first entry downloads it rather than building it, as long as the ScottyLabs cache is configured. Later entries are near-instant.

Run `nix develop` from the repository itself. The shell keeps per-project state in `.scottylabs` at the checkout root, such as the git hooks' store roots and the configs it links into the tree, so outside a checkout it refuses to start:

```
/home/you is not inside a git checkout, run nix develop from the project's repository
```

Entry also fails when the project breaks one of kennel's conventions, printing every violation at once, such as a `secretspec.toml` missing a profile header or a `flake.nix` that doesn't list the binary cache. Fix each line it prints and enter again.

On entry the shell installs the project's [git hooks](#git-hooks) and loads the project's secrets for the `dev` profile. The first time on a machine, [log in to OpenBao](./secrets.md#local-development) so there is a token to resolve them with, otherwise entry warns and leaves the secrets unset:

```bash
nix run git+https://git.cmu.dev/ScottyLabs/kennel#login
```

Projects that have not yet moved to the kennel flake module still use `devenv shell` instead of `nix develop`. Everything else on this page applies to both.

## collar

The dev shell includes [collar](https://docs.collar.scottylabs.org), the ScottyLabs command-line tool for pull requests, issues, CI, previews and project ownership. Its [getting started guide](https://docs.collar.scottylabs.org/getting-started/#sign-in) shows how to sign in and use it.

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
