{
  pkgs,
  evaluator ? "chunked",
}:

let
  tools = pkgs.callPackage ../tools.nix { };
in
tools.runTest {
  name = "navi-apply-${evaluator}";

  navi.test = {
    bundle = ./.;
    testScript =
      ''
        navi = "${tools.naviExec}"
        evaluator = "${evaluator}"
      ''
      + builtins.readFile ./test-script.py;
  };
}
