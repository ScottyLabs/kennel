# Mdbook source tree with the options references generated from module
# declarations, pulled into the reference pages via {{#include}}
{ pkgs, scottylabs }:

let
  slib = scottylabs.mkLib pkgs;

  devenvOptionsMd = scottylabs.packages.${pkgs.stdenv.hostPlatform.system}.options-doc;

  nixosOptionsMd = slib.buildOptionsDoc {
    module = ../../nixos;
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
