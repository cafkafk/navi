{
  flake.lib.makeHive = rawHive:
    let
      naviOptions = import ../../src/nix/hive/options.nix;
      naviModules = import ../../src/nix/hive/modules.nix;
    in
    import ../../src/nix/hive/eval.nix {
      inherit rawHive naviOptions naviModules;
      hermetic = true;
    };
}
