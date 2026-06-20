{
  pkgs ? import ./nixpkgs.nix,
  pkgsStable ? import ./nixpkgs-stable.nix,
}:

{
  apply = import ./apply { inherit pkgs; };
  apply-streaming = import ./apply {
    inherit pkgs;
    evaluator = "streaming";
  };
  apply-local = import ./apply-local { inherit pkgs; };
  build-on-target = import ./build-on-target { inherit pkgs; };
  exec = import ./exec { inherit pkgs; };

  flakes = import ./flakes {
    inherit pkgs;
  };
  flakes-impure = import ./flakes {
    inherit pkgs;
    pure = false;
  };
  #flakes-streaming = import ./flakes { inherit pkgs; evaluator = "streaming"; };

  parallel = import ./parallel { inherit pkgs; };

  allow-apply-all = import ./allow-apply-all { inherit pkgs; };

  apply-stable = import ./apply { pkgs = pkgsStable; };

  # Navi-specific feature tests
  provenance = import ./provenance { inherit pkgs; };
  daemon = import ./daemon { inherit pkgs; };
  provision-command = import ./provision-command { inherit pkgs; };

  # facts is intentionally not wired in yet. `navi facts derive` shells out to
  # `nix eval`/`nix build`, which re-instantiate Navi's whole flake input
  # closure from scratch; doing that with no network in the VM needs more
  # offline plumbing. The test under ./facts is a work in progress.
}
