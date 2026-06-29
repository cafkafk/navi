let
  tools = import ./tools.nix {
    insideVm = true;
    targets = [ ];
    prebuiltTarget = "deployer";
  };
in
{
  meta = {
    nixpkgs = tools.pkgs;
  };

  deployer =
    { lib, ... }:
    {
      imports = [
        (tools.getStandaloneConfigFor "deployer")
      ];

      deployment = {
        allowLocalDeployment = true;
      };

      # Preserve the deploying user across activation. apply-local runs as the
      # `navi` user (see extraDeployerConfig in default.nix); if the deployed
      # configuration omitted this user, activation would delete it, and navi's
      # post-activation provenance write (which uses sudo) would then fail with
      # "sudo: you do not exist in the passwd database".
      users.users.navi = {
        isNormalUser = true;
        extraGroups = [ "wheel" ];
      };
      security.sudo.wheelNeedsPassword = false;

      environment.etc."deployment".text = "SUCCESS";

      # /run/keys/key-text
      deployment.keys."key-text".text = "SECRET";
    };
}
