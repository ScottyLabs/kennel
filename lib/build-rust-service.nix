# Builds a rust crate with crane, caching deps separately from the build
{ pkgs, crane }:

{
  src,
  pname ? null,
  version ? null,
  paths ? null,
  nativeBuildInputs ? [ ],
  buildInputs ? [ ],
  buildArgs ? { },
}:

let
  craneLib = crane.mkLib pkgs;
  fromToml = craneLib.crateNameFromCargoToml { cargoToml = src + "/Cargo.toml"; };

  # scope src to just these workspace members, plus the manifests they need
  scopedSrc =
    if paths == null then
      src
    else
      pkgs.lib.fileset.toSource {
        root = src;
        fileset = pkgs.lib.fileset.unions (
          [
            (src + "/Cargo.toml")
            (src + "/Cargo.lock")
          ]
          ++ map (p: src + "/${p}") paths
        );
      };

  commonArgs = {
    # keep secretspec.toml in the source for secretspec_derive's build-time read
    src = pkgs.lib.cleanSourceWith {
      src = scopedSrc;
      filter = path: type: craneLib.filterCargoSources path type || baseNameOf path == "secretspec.toml";
    };
    pname = if pname != null then pname else fromToml.pname;
    version = if version != null then version else fromToml.version;
    strictDeps = true;
    inherit nativeBuildInputs buildInputs;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (
  commonArgs
  // {
    inherit cargoArtifacts;
    doCheck = false;
  }
  // buildArgs
)
