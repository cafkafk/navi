{
  description = "Navi facts test";

  inputs = {
    nixpkgs.url = "@nixpkgs@";

    navi.url = "@navi@";
    # `navi facts derive` shells out to `nix eval`, which instantiates this
    # flake and eagerly fetches the whole locked input closure of `navi`. The
    # VM has no network, so collapse navi's heavy transitive inputs onto inputs
    # that are already present. `navi.lib.makeHive`, all this test needs from
    # navi, does not use any of them.
    navi.inputs.nixpkgs.follows = "nixpkgs";
    navi.inputs.stable.follows = "nixpkgs";
    navi.inputs.nixos-anywhere.follows = "nixpkgs";
    navi.inputs.nix-github-actions.follows = "nixpkgs";
    # flake-parts and crane are kept real: navi's own flake uses
    # flake-parts.lib.mkFlake, and both are already present in the VM store.
    # flake-parts pulls nixpkgs-lib from github, so point it at the local
    # nixpkgs (which exposes lib) to keep the evaluation offline.
    navi.inputs.flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      navi,
    }:
    let
      pkgs = import nixpkgs {
        system = "x86_64-linux";
      };
    in
    {
      navi = import ./hive.nix { inherit pkgs; };
      naviHive = navi.lib.makeHive self.outputs.navi;

      # Each fact is a derivation that produces a JSON file. `navi facts derive`
      # discovers them via `builtins.attrNames`, builds each one, and links the
      # result into facts/derived/<name>.json.
      facts = {
        greeting = pkgs.writeText "greeting.json" (builtins.toJSON { hello = "world"; });
        answer = pkgs.writeText "answer.json" (builtins.toJSON { value = 42; });
      };
    };
}
