# Only used for --legacy-flake-eval
{
  description = "Internal Navi expressions (deprecated)";

  inputs = {
    hive.url = "%hive%";
  };

  outputs = { self, hive }: {
    processFlake = let
      compatibleSchema = "v0.5";

      # Evaluates a raw hive.
      #
      # This uses the `navi` output.
      evalHive = rawFlake: import ./eval.nix {
        inherit rawFlake;
        hermetic = true;
        naviOptions = import ./options.nix;
        naviModules = import ./modules.nix;
      };

      # Uses an already-evaluated hive.
      #
      # This uses the `naviHive` output.
      checkPreparedHive = hiveOutput:
        if !(hiveOutput ? __schema) then
          throw ''
            The naviHive output does not contain a valid evaluated hive.

            Hint: Use `navi.lib.makeHive`.
          ''
        else if hiveOutput.__schema != compatibleSchema then
          throw ''
            The naviHive output (schema ${hiveOutput.__schema}) isn't compatible with this version of Navi.

            Hint: Use the same version of Navi as in the Flake input.
          ''
        else hiveOutput;
    in
      if hive.outputs ? naviHive then checkPreparedHive hive.outputs.naviHive
      else evalHive hive;
  };
}
