let
  tools = import ./tools.nix {
    insideVm = true;
    targets = [ "alpha" ];
  };
in
{
  meta = {
    nixpkgs = tools.pkgs;

    # A "command" provisioner runs an arbitrary shell command. Real deployments
    # use the "terranix" or "bareMetal" provisioners to talk to a cloud or to
    # physical hardware; here we stand in for that boundary with a command that
    # records that it ran. This keeps the test free of any cloud dependency
    # while still exercising Navi's provisioner resolution and execution.
    provisioners.local = {
      type = "command";
      command = "echo navi-provisioned > /tmp/provision-marker";
    };
  };

  deployer = tools.getStandaloneConfigFor "deployer";

  alpha = {
    imports = [
      (tools.getStandaloneConfigFor "alpha")
    ];

    deployment.provisioner = "local";
  };
}
