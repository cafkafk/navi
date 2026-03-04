{ pkgs, ... }: {
  perSystem = { config, ... }: {
    apps = {
      navi = {
        type = "app";
        program = "${config.packages.navi}/bin/navi";
      };
    };
  };
}
