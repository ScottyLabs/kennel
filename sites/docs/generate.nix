# Mdbook source tree with the options references generated from module
# declarations, pulled into the reference pages via {{#include}}
{
  pkgs,
  helpers,
  inputs,
}:

let
  repoUrl = "https://git.cmu.dev/ScottyLabs/kennel/src/branch/main";

  flakeOptionsMd = helpers.buildOptionsDoc {
    module.imports = import ../../nix/flake-module/per-system inputs;
    subtree = options: {
      inherit (options) scottylabs services sites;
    };
    root = ../..;
    inherit repoUrl;
  };

  nixosOptionsMd = helpers.buildOptionsDoc {
    module = ../../nix/nixos.nix;
    subtree = options: options.services.kennel;
    root = ../..;
    inherit repoUrl;
  };
in
rec {
  # Unescape dots in generated headings (nixpkgs#224661)
  generated = pkgs.runCommand "kennel-docs-generated" { } ''
    mkdir $out
    sed 's/\\\././g' ${flakeOptionsMd} > $out/flake-options.md
    sed 's/\\\././g' ${nixosOptionsMd} > $out/nixos-module.md
  '';

  src = pkgs.runCommand "kennel-docs-src" { } ''
    cp -r ${./.} $out
    chmod -R u+w $out
    mkdir -p $out/src/reference/generated
    cp ${generated}/*.md $out/src/reference/generated/
  '';
}
