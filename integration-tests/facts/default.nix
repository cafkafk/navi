{
  pkgs,
}:

let
  tools = pkgs.callPackage ../tools.nix {
    targets = [ ];
    prebuiltTarget = null;
  };

  # From integration-tests/nixpkgs.nix
  naviFlakeInputs = pkgs._inputs;

  # Store paths of every flake input AND its transitive inputs. `navi facts
  # derive` shells out to `nix eval`/`nix build`, which instantiate navi's
  # flake and its whole locked input closure (flake-parts pulls in
  # nix-community/nixpkgs.lib, etc.). The deployer has no network, so the
  # entire closure must already be in the store. Dedupe by path to terminate on
  # `follows` cycles. (Mirrors the flakes test.)
  naviFlakeInputClosure =
    let
      go =
        seen: input:
        if !(input ? outPath) || builtins.elem input.outPath seen then
          seen
        else
          let
            seen' = seen ++ [ input.outPath ];
            children = if input ? inputs then builtins.attrValues input.inputs else [ ];
          in
          builtins.foldl' go seen' children;
    in
    builtins.foldl' go [ ] (builtins.attrValues naviFlakeInputs);

  # `navi facts derive` builds each derivation in the test flake's `facts`
  # output. Those are plain JSON `writeText`s, but building one pulls in the
  # whole stdenv bootstrap (binutils, etc.), which is not in the offline VM
  # store and cannot be fetched (no network, substituters disabled). Pre-stage
  # the *realized* fact outputs so the in-VM `nix build` finds them already
  # built. These must stay byte-for-byte in sync with ./flake.nix (same
  # nixpkgs, names, and content) so the store paths match.
  factPkgs = import pkgs._inputs.nixpkgs { system = "x86_64-linux"; };
  factOutputs = [
    (factPkgs.writeText "greeting.json" (builtins.toJSON { hello = "world"; }))
    (factPkgs.writeText "answer.json" (builtins.toJSON { value = 42; }))
  ];
in
tools.runTest {
  name = "navi-facts";

  nodes.deployer = {
    virtualisation.additionalPaths = naviFlakeInputClosure ++ factOutputs;

    # The VM has no network. Inputs are pinned by navi's lockfile and staged
    # above, so disable the global flake registry to stop Nix from reaching for
    # channels.nixos.org while locking and evaluating. Enabling the flakes
    # feature is required for `navi facts derive` (which shells out to
    # nix-command/flakes) and for the flake-registry setting to be accepted.
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
        deployer.succeed("sed -i 's @navi@ path:${pkgs._inputs.self.outPath} g' /tmp/bundle/flake.nix")

        navi = "${tools.naviExec}"
      ''
      + builtins.readFile ./test-script.py;
  };
}
