# Your first deploy

This chapter deploys the Hive from the previous chapter. It assumes you can
reach `web-01` over SSH and that your user can escalate to root there.

## Build before you deploy

Start by building the configuration without touching the target. This catches
evaluation and build errors early:

    navi apply build --on web-01

Navi evaluates the Hive, then builds the system closure for the selected node.
Nothing is copied or activated yet.

## Push and activate

When the build succeeds, apply the configuration:

    navi apply --on web-01

With no goal given, `apply` runs the full sequence. It builds the closure,
copies it to the target, and activates it as the new system generation. If
activation fails, the previous generation stays active, so a bad deploy does not
leave the machine in a broken state.

## Selecting nodes

The `--on` flag selects which nodes to act on. It accepts node names, tags, and
group expressions, so a single command can target one machine or a whole class
of them. With no `--on`, Navi acts on every node in the Hive.

    navi apply --on web-01,web-02
    navi apply --on @web

## Deployment goals

`apply` takes an optional goal that stops the sequence early:

- `build` evaluates and builds only.
- `push` also copies the closure to the target.
- `switch` activates immediately and makes the change persist across reboots.
- `boot` makes the change take effect on the next reboot only.

Running `apply` with no goal is equivalent to `switch`.

You now have a deployed node. The guides cover what happens behind these
commands and the features built on top of them.
