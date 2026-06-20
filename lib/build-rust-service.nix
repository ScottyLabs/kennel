# Builds a rust crate with crane, caching deps separately from the build
{ pkgs, crane }:

{
  src,
  pname ? null,
  version ? null,
  nativeBuildInputs ? [ ],
  buildInputs ? [ ],
  buildArgs ? { },
}:

let
  craneLib = crane.mkLib pkgs;
  fromToml = craneLib.crateNameFromCargoToml { cargoToml = src + "/Cargo.toml"; };

  commonArgs = {
    src = craneLib.cleanCargoSource src;
    pname = if pname != null then pname else fromToml.pname;
    version = if version != null then version else fromToml.version;
    strictDeps = true;
    inherit nativeBuildInputs buildInputs;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; doCheck = false; } // buildArgs)
