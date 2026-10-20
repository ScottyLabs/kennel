{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.deno;
in
{
  options.scottylabs.deno = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable the Deno toolchain. Adds [Deno](https://deno.com),
        [oxlint](https://oxc.rs/docs/guide/usage/linter.html), and
        [oxfmt](https://oxc.rs/docs/guide/usage/formatter). Runs oxlint,
        `deno check`, and `deno test` on every commit.
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
        Directories, relative to the project root, with their own `deno.json`
        or `package.json`. `deno install` runs in each on shell entry.
      '';
    };

    svelte = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Add the
          [`svelte-check`](https://github.com/sveltejs/language-tools/tree/master/packages/svelte-check)
          pre-commit hook and run `svelte-kit sync` in each of `dirs` on shell
          entry.
        '';
      };
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [ pkgs.deno ] ++ lib.optional cfg.svelte.enable pkgs.svelte-check;

    env.DENO_DIR = "${config.devenv.root}/.devenv/state/deno";

    enterShell = lib.concatMapStrings (dir: ''
      (cd ${lib.escapeShellArg dir} && deno install${lib.optionalString cfg.svelte.enable " && deno run -A npm:@sveltejs/kit/svelte-kit sync"})
    '') cfg.dirs;

    git-hooks.hooks = {
      svelte-check = lib.mkIf cfg.svelte.enable {
        enable = true;
        entry = "${pkgs.svelte-check}/bin/svelte-check";
        # svelte-check walks the whole project
        files = "\\.(svelte|ts|js|mts|cts|mjs|cjs|tsx|jsx)$";
        pass_filenames = false;
      };
      # svelte-check owns all typescript in svelte repos
      deno-check = lib.mkIf (!cfg.svelte.enable) {
        enable = true;
        name = "deno-check";
        # Checks the whole graph so module augmentations apply
        entry = "deno check .";
        files = "\\.(ts|tsx|mts|cts)$";
        pass_filenames = false;
        language = "system";
      };
      deno-test = {
        enable = true;
        name = "deno-test";
        # Tests exercise first-party code that reads env and files at import
        entry = "deno test -A --ignore=.devenv --permit-no-files";
        files = "\\.(ts|tsx|mts|cts|js|mjs|cjs|jsx)$";
        pass_filenames = false;
        language = "system";
      };
    };
  };
}
