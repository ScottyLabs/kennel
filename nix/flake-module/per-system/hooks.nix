{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs;
in
{
  options.scottylabs.hooksCheck = {
    skip = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      internal = true;
      description = "Hook ids the `hooks` check skips because another check already runs them.";
    };

    setup = lib.mkOption {
      type = lib.types.lines;
      default = "";
      internal = true;
      description = ''
        Shell run in the `hooks` check before the hooks, from the project root,
        to stage the dependencies the hooks read without the network.
      '';
    };

    nativeBuildInputs = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      internal = true;
      description = "Extra build tools for the `hooks` check.";
    };

    buildInputs = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      internal = true;
      description = "Extra libraries for the `hooks` check.";
    };
  };

  config = {
    treefmt = {
      projectRootFile = "flake.nix";
      # the treefmt hook runs inside the hooks check
      flakeCheck = false;
      programs = {
        nixfmt.enable = true;
        mdformat.enable = true;
        taplo.enable = true;
        yamlfmt = {
          enable = true;
          settings.formatter.retain_line_breaks_single = true;
        };
      };
    };

    # pre-commit hooks in the build sandbox
    checks.hooks = config.pre-commit.settings.run.overrideAttrs (old: {
      name = "${cfg.project.name}-hooks";
      nativeBuildInputs = old.nativeBuildInputs ++ cfg.hooksCheck.nativeBuildInputs;
      buildInputs = old.buildInputs ++ cfg.hooksCheck.buildInputs;
      SKIP = lib.concatStringsSep "," cfg.hooksCheck.skip;
      preConfigure = cfg.hooksCheck.setup;
    });

    pre-commit = {
      check.enable = false;

      settings = {
        package = pkgs.prek;

        hooks = {
          treefmt.enable = true;
          statix.enable = true;
          deadnix.enable = true;
          gitleaks.enable = true;
          editorconfig-checker.enable = cfg.editorconfig.enable;
          commitizen.enable = cfg.conventionalCommits.enable;

          # TODO: https://github.com/cachix/git-hooks.nix/issues/728
          taplo-check = {
            enable = true;
            name = "taplo-check";
            entry = "${pkgs.taplo}/bin/taplo check";
            types = [ "toml" ];
            pass_filenames = true;
            language = "system";
          };

          block-ai-coauthors = {
            enable = true;
            name = "block-ai-coauthors";
            entry = "${pkgs.writeShellScript "block-ai-coauthors" ''
              if grep -iqE '^[[:space:]]*co-authored-by:.*(claude|cursor|copilot|codex)' "$1"; then
                echo "AI tool co-author trailers are not allowed. Remove the Co-authored-by line." >&2
                exit 1
              fi
            ''}";
            stages = [ "commit-msg" ];
          };

          block-ai-slop = {
            enable = true;
            name = "block-ai-slop";
            entry = "${pkgs.writeShellScript "block-ai-slop" ''
              [ "$#" -eq 0 ] && exit 0
              if ${pkgs.ripgrep}/bin/rg --color=never -n '[\x{2012}-\x{2015}\x{2018}\x{2019}\x{201A}\x{201C}\x{201D}\x{201E}\x{2026}\x{2190}-\x{2194}\x{21D0}\x{21D2}\x{21D4}\x{279C}\x{27A1}\x{2713}\x{2714}\x{2705}\x{2611}\x{2717}\x{2718}\x{2715}\x{2716}\x{274C}]' "$@"; then
                echo "Non-ASCII characters are not allowed (em and en dashes, smart quotes, ellipsis, arrows, check and x marks). Replace them with plain ASCII." >&2
                exit 1
              fi
            ''}";
            types = [ "text" ];
            language = "system";
          };
        };
      };
    };

    # .git/hooks call prek by store path
    scottylabs.shell.hook = ''
      ${config.pre-commit.installationScript}
      mkdir -p "$SCOTTYLABS_STATE/gcroots"
      nix-store --add-root "$SCOTTYLABS_STATE/gcroots/prek" --realise ${config.pre-commit.settings.package} >/dev/null
    '';
  };
}
