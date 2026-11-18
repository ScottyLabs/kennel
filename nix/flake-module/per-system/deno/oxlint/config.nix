{
  lib,
  writeText,
  anti-slop,
}:

let
  # anti-slop rules are named after their files
  antiSlopRules =
    plugin: dir:
    lib.mapAttrs' (file: _: lib.nameValuePair "${plugin}/${lib.removeSuffix ".ts" file}" "error") (
      lib.filterAttrs (file: _: !lib.hasSuffix ".test.ts" file) (
        builtins.readDir "${anti-slop}/src/${dir}"
      )
    );
in
writeText "oxlintrc.json" (
  builtins.toJSON {
    extends = [ "./node_modules/@interfere/landmines/configs/recommended.json" ];
    jsPlugins = [
      {
        name = "anti-slop";
        specifier = "./anti-slop/index.ts";
      }
      {
        name = "anti-slop-effect";
        specifier = "./anti-slop/effect/index.ts";
      }
    ];
    rules =
      antiSlopRules "anti-slop" "rules"
      // antiSlopRules "anti-slop-effect" "effect/rules"
      // {
        "landmines-style/no-comments" = "off";
      };
  }
)
