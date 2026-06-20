# The Hive

A Hive is the configuration that describes your fleet. It is a Nix attribute set
where most attributes are nodes, and a few reserved attributes configure Navi
itself.

Create a file named `hive.nix`:

    {
      meta = {
        nixpkgs = import <nixpkgs> { system = "x86_64-linux"; };
      };

      defaults = { pkgs, ... }: {
        environment.systemPackages = [ pkgs.vim pkgs.curl ];
      };

      web-01 = { name, nodes, pkgs, ... }: {
        deployment.targetHost = "web-01.example.com";

        services.nginx.enable = true;
        system.stateVersion = "24.11";
      };
    }

## Reserved attributes

Two attribute names are reserved and are not treated as nodes.

`meta` configures the Hive as a whole. The most important field is `nixpkgs`,
which selects the package set every node is built against.

`defaults` is a NixOS module merged into every node. Put configuration that all
machines share here, such as common packages, users, or SSH settings.

## Nodes

Every other top-level attribute is a node, and its value is a NixOS module. The
node name is the attribute name. Inside a node, the `deployment` options control
how Navi connects and applies the configuration. The rest is ordinary NixOS.

The most common deployment option is `deployment.targetHost`, which sets the
address Navi connects to. If you omit it, Navi uses the node name as the host.

## Flake-based Hives

A Hive can also live inside a flake under the `navi` output, which lets you pin
nixpkgs and share modules with the rest of your configuration. Navi searches
upward from the working directory for a `flake.nix` or `hive.nix` unless you
pass `--config`.

The next chapter deploys this Hive.
