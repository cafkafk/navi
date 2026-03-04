{ inputs, ... }: 
{
  imports = [
    ./systems.nix
    ./packages/default.nix
    ./apps/default.nix
    ./devShells/default.nix
    ./checks/default.nix
    ./overlays/default.nix
    ./nixosModules/default.nix
    ./lib/default.nix
  ];
  
  _module.args.inputs = inputs;
}
