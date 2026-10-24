# Regression guard for the semgrep-core patchelf workaround in
# ../modules/security.nix.
#
# patchelf 0.19.2 aborts in `shiftFile` ("Assertion `splitIndex == -1' failed",
# builder exit 134) when it rewrites the ~250MB prebuilt ELF semgrep-core
# unpacks from its PyPI wheel. semgrep-core is a build input of the semgrep
# Python package, so that failure takes the whole devenv shell down, on
# x86_64-linux and aarch64-linux alike.
# https://github.com/NixOS/nixpkgs/issues/577387
#
# Consumers evaluate this module against their own nixpkgs, so point the test
# at the rev you care about rather than only kennel's pin:
#
#   nix eval --impure --expr 'import ./nix/tests/semgrep-patchelf.nix {
#     pkgs = import (builtins.getFlake "github:nixos/nixpkgs/<rev>") {
#       system = "x86_64-linux";
#     };
#   }'
{ pkgs }:
let
  inherit (pkgs) lib;

  # Versions observed to corrupt or abort on semgrep-core's binary. Extend this
  # rather than deleting the workaround if another bad release ships.
  brokenPatchelfVersions = [ "0.19.2" ];

  security = import ../modules/security.nix {
    inherit pkgs lib;
    config.scottylabs = {
      enable = true;
      security = {
        osvScanner.enable = false;
        semgrep.enable = true;
        vulnix.enable = false;
        vulnix.whitelist = null;
      };
    };
  };

  semgrep = lib.findFirst (
    p: lib.hasInfix "semgrep" (p.name or "")
  ) null security.config.content.packages;

  patchelfUsed = lib.findFirst (
    d: lib.hasInfix "patchelf" (d.name or "")
  ) null semgrep.semgrep-core.nativeBuildInputs;

  # Darwin's semgrep-core takes no patchelf at all, so "none" is a pass there.
  patchelfVersion = if patchelfUsed == null then "none" else patchelfUsed.version;
in
assert lib.assertMsg (semgrep != null) "security.nix stopped putting semgrep on PATH";
assert lib.assertMsg (
  !lib.elem patchelfVersion brokenPatchelfVersions
) "semgrep-core would be built with patchelf ${patchelfVersion}, which aborts on its prebuilt ELF";
pkgs.emptyFile
