{ pkgs }:

let
  tools = pkgs.callPackage ../tools.nix {
    targets = [ "alpha" ];
  };
in
tools.runTest {
  name = "navi-allow-apply-all";

  navi.test = {
    bundle = ./.;

    testScript = ''
      logs = deployer.fail("cd /tmp/bundle && run-copy-stderr ${tools.naviExec} apply")

      assert "No node filter" in logs

      deployer.succeed("cd /tmp/bundle && run-copy-stderr ${tools.naviExec} apply --on @target")
    '';
  };
}
