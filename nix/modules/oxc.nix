{
  pkgs,
  lib,
  config,
  ...
}:

let
  inherit (config.scottylabs) deno node;
  react = (deno.enable && deno.react.enable) || (node.enable && node.react.enable);
  oxlintPlugins = [
    "oxc"
    "unicorn"
    "typescript"
  ]
  ++ lib.optionals react [
    "react"
    "jsx-a11y"
  ];
in
{
  # oxlint and oxfmt are shared by the deno and node toolchains
  config = lib.mkIf (config.scottylabs.enable && (deno.enable || node.enable)) {
    # oxlint prefers node_modules/.bin/tsgolint over PATH
    env.OXLINT_TSGOLINT_PATH = lib.getExe pkgs.tsgolint;

    treefmt.config.programs.oxfmt = {
      enable = true;
      excludes = [ "*.md" ]; # owned by mdformat
    };

    git-hooks.hooks.oxlint = {
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
        allow = [
          "prefer-readonly-parameter-types"
          # TODO comments are allowed
          "no-warning-comments"
        ]
        ++ lib.optionals react [
          # The automatic jsx runtime needs no react import
          "react-in-jsx-scope"
          "jsx-uses-react"
        ];
      };
    };
  };
}
