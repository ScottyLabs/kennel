{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.haskell;

  # TODO: ghc914 once hydra caches its set
  # newest fully binary-cached set, keep in sync with build-haskell-service.nix
  hset = pkgs.haskell.packages.ghc912;

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
        Enable the Haskell development toolchain. Adds GHC, cabal-install,
        [HLS](https://github.com/haskell/haskell-language-server), and
        [hlint](https://github.com/ndmitchell/hlint), with
        [fourmolu](https://github.com/fourmolu/fourmolu) formatting via
        treefmt. Library dependencies come pre-built from the binary-cached
        nixpkgs package set, so `cabal build` compiles only the project's own
        modules. Runs hlint on every commit.
      '';
    };

    localPackages = lib.mkOption {
      type = lib.types.attrsOf lib.types.path;
      default = { };
      description = ''
        The project's cabal packages, package name to source directory. The
        dependency set is read from each package's `.cabal` file via
        `callCabal2nix` and provided through `ghcWithPackages`. List the same
        directories a `buildHaskellService` flake package names, so the shell
        and deployment builds resolve identical dependencies.
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
