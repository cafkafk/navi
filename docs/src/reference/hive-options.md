# Hive options

This chapter documents the reserved attributes of a Hive and the `deployment`
options available on each node. Everything not listed here is ordinary NixOS
configuration.

## meta

`meta` configures the Hive as a whole.

- `meta.nixpkgs` selects the package set nodes are built against. It is the one
  field most Hives must set.

## defaults

`defaults` is a NixOS module merged into every node. Use it for configuration
that all machines share.

## deployment

The `deployment` attribute set on a node controls how Navi connects to it and
applies its configuration.

- `deployment.targetHost` is the address Navi connects to. If unset, the node
  name is used as the host.
- `deployment.targetUser` is the user Navi connects as.
- `deployment.tags` is a list of tags used to select the node with `--on`.
- `deployment.buildOnTarget` builds the closure on the target instead of
  locally.

This list covers the common options. Run `navi eval` against a node to see the
full set of deployment values it resolves to.
