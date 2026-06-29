{ pkgs }:

let
  tools = import ./tools.nix {
    inherit pkgs;
    insideVm = true;
    targets = [ ];
    prebuiltTarget = null;
  };
in
{
  meta = {
    nixpkgs = tools.pkgs;
  };

  deployer = tools.getStandaloneConfigFor "deployer";
}
