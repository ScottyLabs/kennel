# Mdbook source tree with the options references generated from module
# declarations, pulled into the reference pages via {{#include}}
{
  pkgs,
  mkLib,
  devenvOptionsMd,
}:

let
  slib = mkLib pkgs;

  nixosOptionsMd = slib.buildOptionsDoc {
    module = ../../nix/nixos.nix;
    subtree = options: options.services.kennel;
    root = ../..;
    repoUrl = "https://codeberg.org/ScottyLabs/kennel/src/branch/main";
  };
in
rec {
  # Unescape dots in generated headings (nixpkgs#224661)
  generated = pkgs.runCommand "kennel-docs-generated" { } ''
    mkdir $out
    sed 's/\\\././g' ${devenvOptionsMd} > $out/devenv-options.md
    sed 's/\\\././g' ${nixosOptionsMd} > $out/nixos-module.md
  '';

  src = pkgs.runCommand "kennel-docs-src" { } ''
    cp -r ${./.} $out
    chmod -R u+w $out
    mkdir -p $out/src/reference/generated
    cp ${generated}/*.md $out/src/reference/generated/
  '';
}
