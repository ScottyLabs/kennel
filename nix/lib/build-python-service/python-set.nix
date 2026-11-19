# Loads a uv workspace and its locked package set via uv2nix
{
  pkgs,
  uv2nix,
  pyproject-nix,
  pyproject-build-systems,
}:

{
  src,
  python ? pkgs.python3,
  sourcePreference ? "wheel",
  # Applied last, to supply system libs or build tools uv2nix can't infer
  overrides ? (_final: _prev: { }),
}:

let
  inherit (pkgs) lib;

  workspace = uv2nix.lib.workspace.loadWorkspace { workspaceRoot = src; };
  overlay = workspace.mkPyprojectOverlay { inherit sourcePreference; };

  # The wheel overlay avoids building the bootstrap build systems from source
  buildSystems =
    if sourcePreference == "wheel" then
      pyproject-build-systems.overlays.wheel
    else
      pyproject-build-systems.overlays.default;
in
{
  inherit workspace;

  pythonSet = (pkgs.callPackage pyproject-nix.build.packages { inherit python; }).overrideScope (
    lib.composeManyExtensions [
      buildSystems
      overlay
      overrides
    ]
  );
}
