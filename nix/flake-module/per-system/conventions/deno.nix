{
  lib,
  cfg,
  exists,
  readJSON,
  files,
  ...
}:

let
  normalize = path: lib.removeSuffix "/" (lib.removeSuffix "/**" (lib.removePrefix "./" path));
  quoted = lib.concatMapStringsSep ", " (path: "\"${path}\"");

  oxlintConfigs = [
    ".oxlintrc.json"
    ".oxlintrc.jsonc"
    "oxlint.config.ts"
    "oxlint.config.mts"
  ];
  strayOxlint = lib.filter (
    file: file != ".oxlintrc.json" && lib.elem (baseNameOf file) oxlintConfigs
  ) files;

  kennelOxlint = "./.scottylabs/oxlint/oxlintrc.json";
  extends = map (path: "./${normalize path}") (
    lib.toList ((readJSON ".oxlintrc.json").extends or [ ])
  );

  denoConfig = lib.findFirst exists "deno.json" [
    "deno.json"
    "deno.jsonc"
  ];
  excludes = [ ".scottylabs" ] ++ lib.remove "." (map normalize cfg.deno.dirs);
  excluded = lib.optionals (exists denoConfig) (map normalize ((readJSON denoConfig).exclude or [ ]));
  unexcluded = lib.filter (
    dir: !lib.any (pattern: dir == pattern || lib.hasPrefix "${pattern}/" dir) excluded
  ) excludes;
in
lib.optionals cfg.deno.enable (
  map (file: "${file}: move these settings into \"overrides\" in the root .oxlintrc.json") strayOxlint
  ++ lib.optional (
    exists ".oxlintrc.json" && !lib.elem kennelOxlint extends
  ) ".oxlintrc.json: add \"${kennelOxlint}\" to \"extends\""
  ++ lib.optional (
    !cfg.deno.svelte.enable && unexcluded != [ ]
  ) "${denoConfig}: add ${quoted unexcluded} to \"exclude\""
)
