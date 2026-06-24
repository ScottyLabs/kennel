# Builds a uv project into a venv via uv2nix, every dep resolved as a Nix derivation
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
  # pick the locked dependency closure to install from the loaded workspace
  selectDeps ? (ws: ws.deps.default),
  # applied last, to supply system libs or build tools uv2nix can't infer
  overrides ? (_final: _prev: { }),
}:

let
  inherit (pkgs) lib;

  pname = (fromTOML (builtins.readFile (src + "/pyproject.toml"))).project.name;

  workspace = uv2nix.lib.workspace.loadWorkspace { workspaceRoot = src; };
  overlay = workspace.mkPyprojectOverlay { inherit sourcePreference; };

  # the wheel overlay avoids building the bootstrap build systems from source
  buildSystems =
    if sourcePreference == "wheel" then
      pyproject-build-systems.overlays.wheel
    else
      pyproject-build-systems.overlays.default;

  pythonSet = (pkgs.callPackage pyproject-nix.build.packages { inherit python; }).overrideScope (
    lib.composeManyExtensions [
      buildSystems
      overlay
      overrides
    ]
  );
in
pythonSet.mkVirtualEnv "${pname}-env" (selectDeps workspace)
