{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.deno;
  inherit (config.scottylabs) root;
  denoLib = pkgs.callPackage ../../lib/deno.nix { };

  oxlintPlugins = [
    "oxc"
    "unicorn"
    "typescript"
  ]
  ++ lib.optionals cfg.react.enable [
    "react"
    "jsx-a11y"
  ];

  # A workspace member without its own lock resolves from the root deno.lock
  lockfiles = lib.unique (
    lib.filter builtins.pathExists (
      map (
        dir:
        let
          own = root + "/${dir}/deno.lock";
        in
        if builtins.pathExists own then own else root + "/deno.lock"
      ) cfg.dirs
    )
  );

  caches = lib.imap0 (
    i: lockfile:
    denoLib.mkCache {
      name = "${config.scottylabs.project.name}-${toString i}";
      inherit lockfile;
    }
  ) lockfiles;

  # update deno.jsr.hash next to each staged deno.lock
  jsrHash = pkgs.writeShellApplication {
    name = "deno-jsr-hash";
    runtimeInputs = [
      denoLib.fetchJsr
      pkgs.jq
    ];
    text = ''
      status=0
      for lock in "$@"; do
        file="$(dirname "$lock")/deno.jsr.hash"
        if [ "$(jq '.jsr // {} | length' "$lock")" = 0 ]; then
          continue
        fi
        tmp=$(mktemp -d)
        fetch-jsr "$lock" "$tmp/cache"
        hash=$(nix --extra-experimental-features nix-command hash path "$tmp/cache")
        rm -rf "$tmp"
        if [ "$(cat "$file" 2>/dev/null)" != "$hash" ]; then
          echo "$hash" > "$file"
          echo "updated $file"
          status=1
        fi
      done
      exit "$status"
    '';
  };
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

    svelte.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Add the
        [`svelte-check`](https://github.com/sveltejs/language-tools/tree/master/packages/svelte-check)
        hook and run `svelte-kit sync` in each of `dirs` on shell entry, so each
        of `dirs` must be a SvelteKit project.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    scottylabs = {
      hooksCheck = {
        # needs network
        skip = [ "deno-jsr-hash" ];
        nativeBuildInputs = [
          pkgs.deno
        ]
        ++ lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.autoPatchelfHook;
        buildInputs = lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.stdenv.cc.cc.lib;
        setup = ''
          export DENO_DIR="$TMPDIR/deno"
          export OXLINT_TSGOLINT_PATH=${lib.getExe pkgs.tsgolint}
          mkdir -p "$DENO_DIR"
        ''
        + lib.concatMapStrings (cache: ''
          cp -rn ${cache}/. "$DENO_DIR/"
          chmod -R u+w "$DENO_DIR"
        '') caches
        + lib.concatMapStrings (dir: ''
          (
            cd ${lib.escapeShellArg dir}
            deno install --cached-only --frozen
            ${lib.optionalString cfg.svelte.enable "deno run -A --cached-only npm:@sveltejs/kit/svelte-kit sync"}
            ${denoLib.patchAddons}
          )
        '') cfg.dirs;
      };

      shell = {
        packages = [ pkgs.deno ] ++ lib.optional cfg.svelte.enable pkgs.svelte-check;

        # oxlint prefers node_modules/.bin/tsgolint over PATH
        env.OXLINT_TSGOLINT_PATH = lib.getExe pkgs.tsgolint;

        exports = ''
          export DENO_DIR="$SCOTTYLABS_STATE/deno"
        '';

        hook = lib.concatMapStrings (dir: ''
          (cd "$SCOTTYLABS_ROOT"/${lib.escapeShellArg dir} && deno install${lib.optionalString cfg.svelte.enable " && deno run -A npm:@sveltejs/kit/svelte-kit sync"})
        '') cfg.dirs;
      };
    };

    treefmt.programs.oxfmt = {
      enable = true;
      excludes = [ "*.md" ];
    };

    pre-commit.settings.hooks = {
      oxlint = {
        enable = true;
        args = [ "--no-error-on-unmatched-pattern" ];
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
            "no-warning-comments"
          ]
          ++ lib.optionals cfg.react.enable [
            "react-in-jsx-scope"
            "jsx-uses-react"
          ];
        };
      };
      svelte-check = lib.mkIf cfg.svelte.enable {
        enable = true;
        entry = lib.getExe pkgs.svelte-check;
        files = "\\.(svelte|ts|js|mts|cts|mjs|cjs|tsx|jsx)$";
        pass_filenames = false;
      };
      deno-check = lib.mkIf (!cfg.svelte.enable) {
        enable = true;
        name = "deno-check";
        entry = "deno check .";
        files = "\\.(ts|tsx|mts|cts)$";
        pass_filenames = false;
        language = "system";
      };
      deno-jsr-hash = {
        enable = true;
        name = "deno-jsr-hash";
        entry = lib.getExe jsrHash;
        files = "(^|/)deno\\.lock$";
      };
      deno-test = {
        enable = true;
        name = "deno-test";
        entry = "deno test -A --ignore=.scottylabs --permit-no-files";
        files = "\\.(ts|tsx|mts|cts|js|mjs|cjs|jsx)$";
        pass_filenames = false;
        language = "system";
      };
    };
  };
}
