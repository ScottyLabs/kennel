{ lib, config, ... }:

{
  checks = {
    devShell = config.devShells.default;
  }
  // lib.concatMapAttrs (
    name: target:
    lib.mapAttrs' (check: drv: lib.nameValuePair "${name}-${check}" drv) (
      target.package.passthru.checks or { }
    )
  ) (config.services // config.sites);

  # Run on demand with `nix build .#<service>-mutants`
  packages = lib.concatMapAttrs (
    name: target:
    lib.optionalAttrs (target.package.passthru ? mutants) {
      "${name}-mutants" = target.package.passthru.mutants;
    }
  ) config.services;
}
