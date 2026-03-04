{
  lib,
  stdenv,
  craneLib,
  installShellFiles,
  nix-eval-jobs,
  makeBinaryWrapper,
  nixos-anywhere,
}:

let
  fs = lib.fileset;
  root = ../../.;
  srcIgnored = fs.unions [
    (root + "/.github")
    (root + "/renovate.json")

    (root + "/integration-tests")

    (root + "/nix")
  ];
  srcFiles = fs.difference root srcIgnored;

  # Common arguments used for both dependency building and the final package
  commonArgs = {
    pname = "navi";
    version = "0.0.0-pre";

    src = fs.toSource {
      inherit root;
      fileset = srcFiles;
    };

    strictDeps = true;

    nativeBuildInputs = [ installShellFiles makeBinaryWrapper ];

    buildInputs = [ nix-eval-jobs ];

    NIX_EVAL_JOBS = "${nix-eval-jobs}/bin/nix-eval-jobs";

    preBuild = ''
      if [[ -z "$NIX_EVAL_JOBS" ]]; then
        unset NIX_EVAL_JOBS
      fi
    '';

    # Crane specific: Disable tests during dep build if they require source code
    # But usually good to keep them enabled if possible.
    # We'll stick to original config where doCheck = false
    doCheck = false;
  };

  # Build the cargo artifacts (dependencies) separately
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

in craneLib.buildPackage (commonArgs // {
  inherit cargoArtifacts;

  postInstall = ''
    ${lib.optionalString (stdenv.hostPlatform == stdenv.buildPlatform) ''
      installShellCompletion --cmd navi \
        --bash <($out/bin/navi gen-completions bash) \
        --zsh <($out/bin/navi gen-completions zsh) \
        --fish <($out/bin/navi gen-completions fish)
    ''}

    wrapProgram $out/bin/navi \
      --prefix PATH : ${lib.makeBinPath [ nixos-anywhere ]}
  '';

  passthru = {
    # We guarantee CLI and Nix API stability for the same minor version
    apiVersion = builtins.concatStringsSep "." (lib.take 2 (lib.splitString "." commonArgs.version));
  };

  meta = with lib; {
    description = "A simple, stateless NixOS deployment tool";
    license = licenses.mit;
    platforms = platforms.linux ++ platforms.darwin;
    mainProgram = "navi";
  };
})
