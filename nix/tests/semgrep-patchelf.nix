{ pkgs }:
let
  inherit (pkgs) lib;

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

  patchelfVersion = if patchelfUsed == null then "none" else patchelfUsed.version;
in
assert lib.assertMsg (semgrep != null) "security.nix stopped putting semgrep on PATH";
assert lib.assertMsg (
  !lib.elem patchelfVersion brokenPatchelfVersions
) "semgrep-core would be built with patchelf ${patchelfVersion}, which aborts on its prebuilt ELF";
pkgs.emptyFile
