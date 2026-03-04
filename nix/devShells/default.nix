{
  perSystem = { config, pkgs, ... }: {
    devShells.default = pkgs.mkShell {
    RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
    NIX_PATH = "nixpkgs=${pkgs.path}";

    inputsFrom = [
      config.packages.navi
    ];
    packages = with pkgs; [
      bashInteractive
      editorconfig-checker
      nixfmt-rfc-style
      clippy
      rust-analyzer
      cargo-outdated
      cargo-audit
      rustfmt
      python3
      python3Packages.flake8

      nix
    ];
  };
};
}
