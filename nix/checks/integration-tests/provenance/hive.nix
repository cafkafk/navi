let
  tools = import ./tools.nix {
    insideVm = true;
    targets = [ "alpha" ];
  };
in
{
  meta = {
    nixpkgs = tools.pkgs;
  };

  defaults = {
    environment.etc."deployment".text = "FIRST";
  };

  deployer = tools.getStandaloneConfigFor "deployer";
  alpha = tools.getStandaloneConfigFor "alpha";
}
