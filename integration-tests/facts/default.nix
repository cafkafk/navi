{
  pkgs,
}:

let
  inherit (pkgs) lib;

  tools = pkgs.callPackage ../tools.nix {
    targets = [ ];
    prebuiltTarget = null;
  };

  # From integration-tests/nixpkgs.nix
  naviFlakeInputs = pkgs._inputs;
in
tools.runTest {
  name = "navi-facts";

  nodes.deployer = {
    virtualisation.additionalPaths = lib.mapAttrsToList (k: v: v.outPath) naviFlakeInputs;

    # The VM has no network. Every flake reference in this test is pinned via a
    # path or a follows, so disable the global flake registry to stop Nix from
    # reaching for channels.nixos.org while locking and evaluating. Enabling the
    # flakes feature in the config is required for the flake-registry setting to
    # be accepted at nix.conf build time.
    nix.settings.experimental-features = [
      "nix-command"
      "flakes"
    ];
    nix.settings.flake-registry = "";
  };

  navi.test = {
    bundle = ./.;

    testScript =
      ''
        deployer.succeed("sed -i 's @nixpkgs@ path:${pkgs._inputs.nixpkgs.outPath}?narHash=${pkgs._inputs.nixpkgs.narHash} g' /tmp/bundle/flake.nix")
        deployer.succeed("sed -i 's @navi@ path:${tools.navi.src} g' /tmp/bundle/flake.nix")

        navi = "${tools.naviExec}"
      ''
      + builtins.readFile ./test-script.py;
  };
}
