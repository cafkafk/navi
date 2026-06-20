# Applying configurations

`navi apply` is the core deployment command. It moves a set of nodes from their
current state to the configuration described in the Hive.

## The apply sequence

A full apply runs four stages for each node. It evaluates the node from the
Hive, builds the resulting system closure, copies that closure to the target,
and activates it. Each stage depends on the one before it, and a failure stops
that node without affecting the others.

The goal argument stops the sequence early. See the previous chapter for the
list of goals. Use `build` in continuous integration to verify that every node
still evaluates and builds, without deploying anything.

## Where the build happens

By default Navi builds on the machine that runs the command. You can build on
the target instead, which is useful when the target has resources the local
machine lacks, or when you want to avoid copying large closures over a slow
link. The relevant deployment options are documented in the Hive reference.

## Parallelism

Navi applies to multiple nodes at once. The daemon manages the connection pool
and task queue, so a large fleet deploys concurrently rather than one node at a
time. Concurrency limits keep the local machine and the network from being
overwhelmed.

## Local deployment

To apply the local machine's own configuration without SSH, use
`navi apply-local`. This is the right command for a machine that deploys itself,
such as a workstation or a bootstrap host.

## Rebooting

Pass `--reboot` to reboot each node after activation and wait for it to come
back before reporting success. This is how you confirm that a configuration
survives a real boot, not just a live activation.
