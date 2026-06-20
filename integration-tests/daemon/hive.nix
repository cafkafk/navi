let
  tools = import ./tools.nix {
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
