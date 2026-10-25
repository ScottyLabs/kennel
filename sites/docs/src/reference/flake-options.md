# Flake Options

These options are provided by the [shared ScottyLabs flake module](https://git.cmu.dev/ScottyLabs/kennel). Import it in your `flake.nix` and set them inside `perSystem`:

```nix
imports = [ inputs.scottylabs.flakeModules.default ];
```

{{#include generated/flake-options.md}}
