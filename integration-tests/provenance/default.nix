{
  pkgs,
}:

let
  tools = pkgs.callPackage ../tools.nix {
    targets = [ "alpha" ];
  };
in
tools.runTest {
  name = "navi-provenance";

  navi.test = {
    bundle = ./.;
    testScript =
      ''
        navi = "${tools.naviExec}"
      ''
      + builtins.readFile ./test-script.py;
  };
}
