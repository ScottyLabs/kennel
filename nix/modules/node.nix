{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.node;
  store = "${config.devenv.root}/.devenv/state/pnpm";
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
        root `package.json` scripts named by `checkScript` and `testScript` on
        every commit.
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

    checkScript = lib.mkOption {
      type = lib.types.str;
      default = "typecheck";
      description = ''
        Root `package.json` script that type-checks the project, run on every
        commit when present. In a workspace, have it fan out to the members
        (for example `pnpm -r typecheck` or `turbo typecheck`).
      '';
    };

    testScript = lib.mkOption {
      type = lib.types.str;
      default = "test";
      description = ''
        Root `package.json` script that runs the tests, run on every commit
        when present.
      '';
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [
      cfg.package
      pkgs.pnpm
    ];

    # pnpm 10 reads npm_config_*, pnpm 11 and later read pnpm_config_*
    env.npm_config_store_dir = store;
    env.pnpm_config_store_dir = store;

    # pnpm installs every workspace member from the root
    enterShell = ''
      pnpm install
    '';

    git-hooks.hooks = {
      pnpm-check = {
        enable = true;
        name = "pnpm-check";
        entry = "pnpm run --if-present ${cfg.checkScript}";
        files = "\\.(ts|tsx|mts|cts|js|mjs|cjs|jsx|vue|svelte)$";
        pass_filenames = false;
        language = "system";
      };
      pnpm-test = {
        enable = true;
        name = "pnpm-test";
        entry = "pnpm run --if-present ${cfg.testScript}";
        files = "\\.(ts|tsx|mts|cts|js|mjs|cjs|jsx|vue|svelte)$";
        pass_filenames = false;
        language = "system";
      };
    };
  };
}
