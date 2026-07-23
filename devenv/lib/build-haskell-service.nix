# Builds a Haskell service with callCabal2nix, every dep from the binary-cached nixpkgs set
{ pkgs }:

{
  # Package name to build, must be a key of localPackages
  pname,
  # The project's cabal packages, package name -> source directory
  localPackages,
  # Final overlay over the package set, e.g. to jailbreak or pin a dependency
  overrides ? (_final: _prev: { }),
}:

let
  inherit (pkgs) lib;

  localOverlay =
    final: _prev: builtins.mapAttrs (name: src: final.callCabal2nix name src { }) localPackages;

  # TODO: ghc914 once hydra caches its set
  # Newest fully binary-cached set, keep in sync with modules/haskell.nix
  hset = pkgs.haskell.packages.ghc912.extend (lib.composeExtensions localOverlay overrides);
in
pkgs.haskell.lib.justStaticExecutables hset.${pname}
