{ pkgs, src }:

let
  inherit (pkgs) lib;

  manifest = builtins.fromTOML (builtins.readFile (src + "/Cargo.toml"));
  projectLicense = manifest.workspace.package.license or manifest.package.license or "";
  splitStrings = sep: s: builtins.filter builtins.isString (builtins.split sep s);
  # the SPDX ids in the project's own license expression, minus WITH exceptions
  projectLicenseIds = lib.subtractLists [ "" "OR" "AND" ] (
    splitStrings "[ ()/]+" (lib.concatStrings (splitStrings " WITH [^ )]+" projectLicense))
  );

  hasLicense = prefix: builtins.any (lib.hasPrefix prefix) projectLicenseIds;
  # licenses a GPL-3.0 work can include
  gpl3Compatible = [
    "GPL-2.0-or-later"
    "GPL-3.0-only"
    "GPL-3.0-or-later"
    "LGPL-2.1-only"
    "LGPL-2.1-or-later"
    "LGPL-3.0-only"
    "LGPL-3.0-or-later"
  ];
  copyleftCompatible =
    if hasLicense "AGPL-3.0" then
      gpl3Compatible
      ++ [
        "AGPL-3.0-only"
        "AGPL-3.0-or-later"
      ]
    else if hasLicense "GPL-3.0" then
      gpl3Compatible
    else
      [ ];

  defaults = builtins.fromTOML (builtins.readFile ./deny.toml);
in
(pkgs.formats.toml { }).generate "deny.toml" (
  lib.recursiveUpdate defaults {
    licenses.allow = lib.unique (defaults.licenses.allow ++ projectLicenseIds ++ copyleftCompatible);
  }
)
