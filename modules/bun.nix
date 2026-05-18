{ pkgs, lib, config, ... }:

let
  cfg = config.scottylabs.bun;
in
{
  options.scottylabs.bun = {
    enable = lib.mkEnableOption "Bun/JavaScript development toolchain";
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = with pkgs; [ bun ];

    treefmt.config.programs.oxfmt.enable = true;
    treefmt.config.settings.formatter.oxfmt.options = [
      "--config"
      (toString (pkgs.writeText "oxfmtrc.json" (builtins.toJSON {
        ignorePatterns = [ "**/*.md" ];
        tabWidth = 4;
        useTabs = false;
        overrides = [{
          files = [ "**/*.yaml" "**/*.yml" "**/*.html" ];
          options.tabWidth = 2;
        }];
      })))
    ];

    git-hooks.hooks.oxlint = {
      enable = true;
      settings.fix = [ "safe" "suggestions" "dangerously" ];
    };
  };
}
