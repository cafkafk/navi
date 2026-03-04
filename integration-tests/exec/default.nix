{ pkgs }:

let
  tools = pkgs.callPackage ../tools.nix { };
in
tools.runTest {
  name = "navi-exec";

  navi.test = {
    bundle = ./.;

    testScript = ''
      logs = deployer.succeed("cd /tmp/bundle && ${tools.naviExec} exec --on @target -- echo output from '$(hostname)' 2>&1")

      assert "output from alpha" in logs
      assert "output from beta" in logs
      assert "output from gamma" in logs
    '';
  };
}
