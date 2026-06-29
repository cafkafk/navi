{ pkgs }:

let
  tools = pkgs.callPackage ../tools.nix {
    targets = [ ];
    prebuiltTarget = "deployer";
    extraDeployerConfig = {
      users.users.navi = {
        isNormalUser = true;
        extraGroups = [ "wheel" ];
      };
      security.sudo.wheelNeedsPassword = false;
    };
  };
in
tools.runTest {
  name = "navi-apply-local";

  navi.test = {
    bundle = ./.;

    testScript = ''
      deployer.succeed("cd /tmp/bundle && sudo -u navi ${tools.naviExec} apply-local --sudo")
      deployer.succeed("grep SUCCESS /etc/deployment")
      deployer.succeed("grep SECRET /run/keys/key-text")
    '';
  };
}
