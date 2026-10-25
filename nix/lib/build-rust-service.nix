{
  pkgs,
  crane,
}:

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
    # secretspec_derive reads secretspec.toml at build time
    src = pkgs.lib.cleanSourceWith {
      src = scopedSrc;
      filter = path: type: craneLib.filterCargoSources path type || baseNameOf path == "secretspec.toml";
    };
    pname = if pname != null then pname else fromToml.pname;
    version = if version != null then version else fromToml.version;
    strictDeps = true;
    inherit nativeBuildInputs buildInputs;
    cargoExtraArgs = buildArgs.cargoExtraArgs or "--locked";
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  package = craneLib.buildPackage (
    commonArgs
    // {
      inherit cargoArtifacts;
      doCheck = false;
      meta.mainProgram = commonArgs.pname;
    }
    // buildArgs
  );
in
package
// {
  passthru = (package.passthru or { }) // {
    checks = {
      clippy = craneLib.cargoClippy (
        commonArgs
        // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
        }
      );
      test = craneLib.cargoTest (commonArgs // { inherit cargoArtifacts; });
    };
  };
}
