{
  pkgs,
}:

let
  tools = pkgs.callPackage ../tools.nix {
    targets = [ ];
    prebuiltTarget = null;
  };

  # Render the tofunix module to config.tf.json at build time. The result is a
  # store path that the deployer realizes through Navi's terranix provisioner.
  terranixConfig = pkgs._inputs.tofunix.lib.terranixConfiguration {
    system = pkgs.stdenv.hostPlatform.system;
    inherit pkgs;
    modules = [ ./terraform.nix ];
  };
in
tools.runTest {
  name = "navi-terranix-real";

  nodes.deployer = {
    # OpenTofu is the binary Navi drives (NAVI_TERRAFORM_BINARY defaults to
    # "tofu"), and the rendered config must be in the store since the VM has no
    # network.
    environment.systemPackages = [ pkgs.opentofu ];
    virtualisation.additionalPaths = [ terranixConfig ];
  };

  navi.test = {
    bundle = ./.;
    testScript =
      ''
        deployer.succeed("sed -i 's @terranixConfig@ ${terranixConfig} g' /tmp/bundle/hive.nix")

        navi = "${tools.naviExec}"
      ''
      + builtins.readFile ./test-script.py;
  };
}
