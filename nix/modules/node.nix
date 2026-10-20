{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.node;
  store = "${config.devenv.root}/.devenv/state/pnpm";
  # Hooks start at the repo root; dirs without the script are skipped
  runScript =
    name: script:
    pkgs.writeShellScript "pnpm-${name}" (
      lib.concatMapStrings (dir: ''
        (cd ${lib.escapeShellArg dir} && pnpm run --if-present ${lib.escapeShellArg script}) || exit 1
      '') cfg.dirs
    );
in
{
  options.scottylabs.node = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable the Node.js toolchain. Adds [Node.js](https://nodejs.org),
        [pnpm](https://pnpm.io),
        [oxlint](https://oxc.rs/docs/guide/usage/linter.html), and
        [oxfmt](https://oxc.rs/docs/guide/usage/formatter). Runs oxlint and the
        `package.json` scripts named by `checkScript` and `testScript` in each
        of `dirs` on every commit.
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nodejs_24;
      defaultText = lib.literalExpression "pkgs.nodejs_24";
      description = ''
        Node.js package. Keep it in step with the `nodejs` passed to
        `buildPnpmTask`, so the shell and the build run the same runtime.
      '';
    };

    pnpm = lib.mkOption {
      type = lib.types.package;
      default = pkgs.pnpm;
      defaultText = lib.literalExpression "pkgs.pnpm";
      description = ''
        pnpm package. Pass the same package as `pnpm` to `buildPnpmTask`. A
        `packageManager` field in `package.json` naming another pnpm major
        makes the shell switch to that version while the Nix build keeps
        this one, so either drop the field or pick the matching package (for
        example `pkgs.pnpm_10`).
      '';
    };

    react.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Add the
        [`react`](https://oxc.rs/docs/guide/usage/linter/plugins#react-plugin)
        and
        [`jsx-a11y`](https://oxc.rs/docs/guide/usage/linter/plugins#jsx-a11y-plugin)
        plugins to oxlint.
      '';
    };

    dirs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "." ];
      description = ''
        Directories, relative to the project root, holding a pnpm project's
        `package.json` and `pnpm-lock.yaml` (for a workspace, its root).
        `pnpm install` and the commit hook scripts run in each.
      '';
    };

    checkScript = lib.mkOption {
      type = lib.types.str;
      default = "typecheck";
      description = ''
        `package.json` script in each of `dirs` that type-checks the project,
        run on every commit when present. In a workspace, have it fan out to
        the members (for example `pnpm -r typecheck` or `turbo typecheck`).
      '';
    };

    testScript = lib.mkOption {
      type = lib.types.str;
      default = "test";
      description = ''
        `package.json` script in each of `dirs` that runs the tests, run on
        every commit when present.
      '';
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [
      cfg.package
      cfg.pnpm
    ];

    # pnpm 10 reads npm_config_*, pnpm 11 and later read pnpm_config_*
    env.npm_config_store_dir = store;
    env.pnpm_config_store_dir = store;

    # pnpm installs every workspace member from its root
    enterShell = lib.concatMapStrings (dir: ''
      (cd ${lib.escapeShellArg dir} && pnpm install)
    '') cfg.dirs;

    git-hooks.hooks = {
      pnpm-check = {
        enable = true;
        name = "pnpm-check";
        entry = "${runScript "check" cfg.checkScript}";
        files = "\\.(ts|tsx|mts|cts|js|mjs|cjs|jsx|vue|svelte)$";
        pass_filenames = false;
        language = "system";
      };
      pnpm-test = {
        enable = true;
        name = "pnpm-test";
        entry = "${runScript "test" cfg.testScript}";
        files = "\\.(ts|tsx|mts|cts|js|mjs|cjs|jsx|vue|svelte)$";
        pass_filenames = false;
        language = "system";
      };
    };
  };
}
