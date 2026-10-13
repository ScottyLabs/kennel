{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.haskell;

  # Keep in sync with build-haskell-service.nix
  hset = pkgs.haskell.packages.ghc914;

  extendedSet = hset.extend (
    final: _prev: lib.mapAttrs (name: src: final.callCabal2nix name src { }) cfg.localPackages
  );

  localDrvs = lib.mapAttrs (name: _src: extendedSet.${name}) cfg.localPackages;

  cabalDeps =
    drv:
    drv.getCabalDeps.libraryHaskellDepends
    ++ drv.getCabalDeps.executableHaskellDepends
    ++ drv.getCabalDeps.testHaskellDepends;

  depDrvs = lib.subtractLists (builtins.attrValues localDrvs) (
    lib.unique (lib.concatMap cabalDeps (builtins.attrValues localDrvs))
  );

  ghcEnv = if cfg.localPackages == { } then hset.ghc else hset.ghcWithPackages (_: depDrvs);
in
{
  options.scottylabs.haskell = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable the Haskell toolchain. Adds GHC, cabal-install,
        [HLS](https://github.com/haskell/haskell-language-server),
        [hlint](https://github.com/ndmitchell/hlint), and
        [fourmolu](https://github.com/fourmolu/fourmolu). Runs hlint on every
        commit.
      '';
    };

    localPackages = lib.mkOption {
      type = lib.types.attrsOf lib.types.path;
      default = { };
      description = ''
        The project's cabal packages, package name to source directory. Use the
        same directories as the `buildHaskellService` flake package.
      '';
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [
      ghcEnv
      pkgs.cabal-install
      hset.haskell-language-server
      pkgs.hlint
    ];

    treefmt.config.programs.fourmolu.enable = true;

    git-hooks.hooks.hlint.enable = true;
  };
}
