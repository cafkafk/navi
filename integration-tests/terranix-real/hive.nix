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

    # A terranix provisioner whose configuration is the config.tf.json rendered
    # by tofunix at build time. The store path is substituted in by the test
    # script. Navi realizes it, then drives OpenTofu against it.
    provisioners.tf = {
      type = "terranix";
      configuration = "@terranixConfig@";
    };
  };

  deployer = tools.getStandaloneConfigFor "deployer";
}
