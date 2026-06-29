{
  description = "Navi facts test";

  inputs = {
    nixpkgs.url = "@nixpkgs@";

    # navi is its real flake source (pkgs._inputs.self). The deployer has no
    # network, so navi's whole transitive input closure is staged into the
    # store by default.nix and locking resolves it from navi's own lockfile
    # pins. We deliberately do NOT use `follows` overrides here: overriding a
    # transitive input forces nix to re-resolve it at lock time, which fails
    # against the empty offline registry ("cannot find flake 'flake:flake-parts'").
    navi.url = "@navi@";
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
