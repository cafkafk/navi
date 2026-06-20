{ pkgs, ... }: {
  perSystem = { config, pkgs, ... }: {
    apps = {
      navi = {
        type = "app";
        program = "${config.packages.navi}/bin/navi";
      };

      # Serve the mdBook manual locally with live reload, on
      # http://localhost:3000 by default. Run from the repository root:
      # `nix run .#serve-manual`. Extra arguments are passed through to
      # `mdbook serve`, e.g. `nix run .#serve-manual -- --port 8080`.
      serve-manual = {
        type = "app";
        program = "${pkgs.writeShellApplication {
          name = "serve-manual";
          runtimeInputs = [ pkgs.mdbook ];
          text = ''
            # Default to binding 127.0.0.1 (mdbook would otherwise pick
            # localhost, which can resolve to IPv6 only), but let the caller
            # override by passing their own --hostname.
            args=("$@")
            case " ''${args[*]} " in
              *" --hostname "*) ;;
              *) args=(--hostname 127.0.0.1 "''${args[@]}") ;;
            esac
            mdbook serve docs "''${args[@]}"
          '';
        }}/bin/serve-manual";
      };
    };
  };
}
