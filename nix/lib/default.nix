{
  pkgs,
  kennelInputs,
  rustCheckSetup,
  rustToolchain,
}:

{
  buildDenoTask = pkgs.callPackage ./build-deno-task { };

  buildRustService = import ./build-rust-service {
    inherit pkgs;
    inherit (kennelInputs) crane;
    checkSetup = rustCheckSetup;
    toolchain = rustToolchain;
  };

  buildPythonService = import ./build-python-service {
    inherit pkgs;
    inherit (kennelInputs) uv2nix pyproject-nix pyproject-build-systems;
  };

  buildZensical =
    {
      src,
      name ? "docs",
    }:
    pkgs.runCommand name { nativeBuildInputs = [ pkgs.zensical ]; } ''
      cp -r --no-preserve=mode ${src} project
      cd project
      zensical build
      cp -r site "$out"
    '';

  buildOptionsDoc = import ./options-doc.nix { inherit pkgs; };
}
