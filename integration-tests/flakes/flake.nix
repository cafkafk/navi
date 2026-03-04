{
  description = "A simple deployment";

  inputs = {
    nixpkgs.url = "@nixpkgs@";
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
    };
}
