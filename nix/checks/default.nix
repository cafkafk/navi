{ inputs, ... }: 
{
  perSystem = { system, pkgs, config, self, ... }: 
  let
      # Re-implementing the eval-jobs overlay logic
      _evalJobsOverlay = final: prev:
        let
          patched = prev.nix-eval-jobs.overrideAttrs (old: {
            version = old.version + "-navi";
            patches = (old.patches or [ ]) ++ [
              (
                if builtins.compareVersions old.version "2.25.0" >= 0 then
                  ./nix-eval-jobs-unstable.patch
                else
                  ./nix-eval-jobs-stable.patch
              )
            ];
            # To silence the warning
            __intentionallyOverridingVersion = true;
          });
        in
        {
          nix-eval-jobs = patched;
        };

      # Helper for creating the specific pkgs instances needed for tests
      inputsOverlay = final: prev: {
        _inputs = inputs;
      };

  in {
    checks = if pkgs.stdenv.isLinux then
      import ../../integration-tests {
        pkgs = import inputs.nixpkgs {
          inherit system;
          overlays = [
            inputs.self.overlays.default
            inputsOverlay
            _evalJobsOverlay
          ];
        };
        pkgsStable = import inputs.stable {
          inherit system;
          overlays = [
            inputs.self.overlays.default
            inputsOverlay
            _evalJobsOverlay
          ];
        };
      }
    else
      { };
  };
  
  flake.githubActions = inputs.nix-github-actions.lib.mkGithubMatrix {
    checks = {
      inherit (inputs.self.checks) x86_64-linux;
    };
  }; 
}
