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
  # Pick the locked dependency closure to install from the loaded workspace
  selectDeps ? (ws: ws.deps.default),
  # Applied last, to supply system libs or build tools uv2nix can't infer
  overrides ? (_final: _prev: { }),
}:

let
  pname = (fromTOML (builtins.readFile (src + "/pyproject.toml"))).project.name;

  inherit
    (import ./python-set.nix
      {
        inherit
          pkgs
          uv2nix
          pyproject-nix
          pyproject-build-systems
          ;
      }
      {
        inherit
          src
          python
          sourcePreference
          overrides
          ;
      }
    )
    workspace
    pythonSet
    ;
in
pythonSet.mkVirtualEnv "${pname}-env" (selectDeps workspace)
