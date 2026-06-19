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
    enable = lib.mkEnableOption "Deno/JavaScript development toolchain";
    react.enable = lib.mkEnableOption "Adds react + jsx-a11y plugins for oxlint";
    svelte.enable = lib.mkEnableOption "Adds svelte-check pre-commit hook";
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

    treefmt.config.programs.oxfmt = {
      enable = true;
      excludes = [ "*.md" ]; # owned by mdformat
    };

    git-hooks.hooks = {
      oxlint = {
        enable = true;
        settings = {
          plugins = oxlintPlugins;
          deny = [ "all" ];
        };
      };
      svelte-check = lib.mkIf cfg.svelte.enable {
        enable = true;
        entry = "${pkgs.svelte-check}/bin/svelte-check";
        # svelte-check walks the whole project; trigger on any source change
        files = "\\.(svelte|ts|js|mts|cts|mjs|cjs|tsx|jsx)$";
        pass_filenames = false;
      };
    };
  };
}
