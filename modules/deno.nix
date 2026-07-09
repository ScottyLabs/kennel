{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.deno;
  oxlintPlugins = [
    "oxc"
    "unicorn"
    "typescript"
  ]
  ++ lib.optionals cfg.react.enable [
    "react"
    "jsx-a11y"
  ];
in
{
  options.scottylabs.deno = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable the Deno/JavaScript development toolchain. Adds
        [Deno](https://deno.com),
        [oxlint](https://oxc.rs/docs/guide/usage/linter.html) (denying the
        `correctness`, `suspicious`, `pedantic`, and `perf` categories),
        [oxfmt](https://oxc.rs/docs/guide/usage/formatter), and
        [tsgolint](https://github.com/typescript-eslint/tsgolint) on `PATH` for
        `oxlint --type-aware`. Runs oxlint, `deno check`, and `deno test` on
        every commit.
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

    svelte = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Add the
          [`svelte-check`](https://github.com/sveltejs/language-tools/tree/master/packages/svelte-check)
          pre-commit hook.
        '';
      };

      dir = lib.mkOption {
        type = lib.types.str;
        default = ".";
        description = ''
          The SvelteKit app directory, relative to the project root. When
          `svelte.enable` is set, `deno install` and `svelte-kit sync` run here
          on shell entry so `svelte-check` has `node_modules` and the generated
          `.svelte-kit` types available.
        '';
      };
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    # tsgolint on PATH so `oxlint --type-aware` finds it
    packages =
      with pkgs;
      [
        deno
        tsgolint
      ]
      ++ lib.optional cfg.svelte.enable svelte-check;

    env.DENO_DIR = "${config.devenv.root}/.devenv/state/deno";

    enterShell = lib.mkIf cfg.svelte.enable ''
      (cd ${cfg.svelte.dir} && deno install && deno run -A npm:@sveltejs/kit/svelte-kit sync)
    '';

    treefmt.config.programs.oxfmt = {
      enable = true;
      excludes = [ "*.md" ]; # owned by mdformat
    };

    git-hooks.hooks = {
      oxlint = {
        enable = true;
        settings = {
          plugins = oxlintPlugins;
          deny = [
            "correctness"
            "suspicious"
            "pedantic"
            "perf"
          ];
          typeAware = true;
          allow = [ "prefer-readonly-parameter-types" ];
        };
      };
      svelte-check = lib.mkIf cfg.svelte.enable {
        enable = true;
        entry = "${pkgs.svelte-check}/bin/svelte-check";
        # svelte-check walks the whole project; trigger on any source change
        files = "\\.(svelte|ts|js|mts|cts|mjs|cjs|tsx|jsx)$";
        pass_filenames = false;
      };
      deno-check = {
        enable = true;
        name = "deno-check";
        entry = "deno check";
        files = "\\.(ts|tsx|mts|cts)$";
        pass_filenames = true;
        language = "system";
      };
      deno-test = {
        enable = true;
        name = "deno-test";
        entry = "deno test --ignore=.devenv,.direnv --permit-no-files";
        files = "\\.(ts|tsx|mts|cts|js|mjs|cjs|jsx)$";
        pass_filenames = false;
        language = "system";
      };
    };
  };
}
