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
}
