{
  pkgs,
}:

let
  tools = pkgs.callPackage ../tools.nix {
    targets = [ ];
    prebuiltTarget = null;
  };
in
tools.runTest {
  name = "navi-daemon";

  navi.test = {
    bundle = ./.;
    testScript =
      ''
        navi = "${tools.naviExec}"
      ''
      + builtins.readFile ./test-script.py;
  };
}
