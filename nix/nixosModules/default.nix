{
  flake.nixosModules = 
    let
      naviOptions = import ../../src/nix/hive/options.nix;
      naviModules = import ../../src/nix/hive/modules.nix;
    in {
      inherit (naviOptions) deploymentOptions metaOptions;
      inherit (naviModules) keyChownModule keyServiceModule assertionModule;
    };
}
