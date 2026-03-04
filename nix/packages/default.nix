{ inputs, ... }: 
{
  perSystem = { config, system, pkgs, ... }: 
  let
    craneLib = inputs.crane.mkLib pkgs;
  in {
    packages = {
      navi = pkgs.callPackage ./navi.nix { 
        inherit craneLib;
        nixos-anywhere = inputs.nixos-anywhere.packages.${system}.nixos-anywhere;
      };
      default = config.packages.navi;
    };
  };
}

