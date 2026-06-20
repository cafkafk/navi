# A production deployment

This chapter sketches a real fleet managed with Navi. It is here to show what the
workflow looks like at scale, rather than on the single node the tutorial builds.

The fleet is heterogeneous and lives in one flake:

- Around thirty GCP virtual machines, grouped across several tenants and
  environments.
- On-prem hardware running on encrypted ZFS, unlocked remotely as part of a
  deploy.

From that single description, operations that would otherwise be multi-step
runbooks reduce to one command each.

## Standing up a tenant

Creating every machine for a tenant, installing NixOS onto them, and switching
them to their configuration:

    navi provision --on <tenant>-*

This is the step that replaces a sequence of Terraform, nixos-anywhere, and
Colmena runs. The machines do not exist when it starts and are serving their
configuration when it finishes.

## Rolling out a change

Once the machines exist, applying a configuration change to an environment:

    navi apply --on @staging

The selector picks nodes by name, tag, or group, so the same command targets one
machine, an environment, or the whole fleet.

## Bringing up encrypted hardware

The on-prem machines boot into an encrypted ZFS volume. Unlocking one remotely,
so it can finish booting and rejoin the fleet:

    navi disk-unlock <node>

## Watching the fleet

For day-to-day operation across this many nodes, the terminal interface shows the
fleet by tenant or environment, with live logs and task state:

    navi tui

## The point

None of these commands is doing something a pile of separate tools could not do.
What changes is that they come from one description and one tool. The fleet has a
single source of truth, and scaling it up is a matter of selecting more nodes,
not adding more steps.
