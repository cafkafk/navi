{
  stdenvNoCC,
  mdbook,
  navi,
}:

stdenvNoCC.mkDerivation {
  pname = "navi-manual";
  version = "unstable";

  src = ../../docs;

  nativeBuildInputs = [
    mdbook
    navi
  ];

  # Regenerate the command-line reference from the navi binary so the
  # published manual cannot drift from the actual commands and flags.
  buildPhase = ''
    runHook preBuild
    navi gen-manual > src/reference/cli.md
    mdbook build --dest-dir ./book
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out
    cp -r ./book/* $out/
    runHook postInstall
  '';
}
