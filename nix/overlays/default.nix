{ inputs, ... }:
{
  # NOTE: This overlay logic is complex and shared. We might want to centralize
  # it.  But for simplicity, we'll keep the logic that applies the overlay
  # inside the checking code or passed to packages as needed.
  #
  # For now, let's just make sure we export the overlay itself.
  flake.overlays.default = final: prev: {
    navi = final.callPackage ../packages/navi.nix {
      craneLib = inputs.crane.mkLib final;
    };
  };
}
