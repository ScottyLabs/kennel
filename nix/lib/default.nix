{ pkgs, kennelInputs }:

{
  buildDenoTask = pkgs.callPackage ./build-deno-task.nix { };

  buildRustService = import ./build-rust-service.nix {
    inherit pkgs;
    inherit (kennelInputs) crane;
  };

  buildHaskellService = import ./build-haskell-service.nix { inherit pkgs; };

  buildPythonService = import ./build-python-service.nix {
    inherit pkgs;
    inherit (kennelInputs) uv2nix pyproject-nix pyproject-build-systems;
  };

  buildMdbook =
    {
      src,
      name ? "docs",
    }:
    pkgs.runCommand name { nativeBuildInputs = [ pkgs.mdbook ]; } ''
      mdbook build ${src} --dest-dir "$out"
    '';

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
