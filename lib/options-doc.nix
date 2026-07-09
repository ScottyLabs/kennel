# Renders a module set's option declarations to CommonMark for docs sites
{ pkgs }:

{
  module,
  # selects the options to document, e.g. options: options.scottylabs
  subtree,
  # rewrite declaration links under root to the repoUrl source browser
  root,
  repoUrl,
}:

let
  inherit (pkgs) lib;

  eval = lib.evalModules {
    modules = [
      module
      # tolerate config set on options declared elsewhere
      { config._module.check = false; }
    ];
    specialArgs = { inherit pkgs; };
  };

  rootStr = toString root;

  rewriteDecl =
    decl:
    let
      path = toString decl;
      rel = lib.removePrefix "${rootStr}/" path;
    in
    if lib.hasPrefix rootStr path then
      {
        url = "${repoUrl}/${rel}";
        name = rel;
      }
    else
      decl;

  doc = pkgs.nixosOptionsDoc {
    options = subtree eval.options;
    transformOptions = opt: opt // { declarations = map rewriteDecl opt.declarations; };
  };
in
doc.optionsCommonMark
