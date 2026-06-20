# The terminal interface

`navi tui` opens a terminal interface for managing a fleet interactively. It is
built for the case where you have more nodes than you can track from a stream of
log lines.

Start it with:

    navi tui

## Node view

Nodes are shown in a hierarchy that you can organise by category, environment,
or hostgroup. This lets you collapse a large fleet down to the part you care
about and act on a whole group at once.

## Monitoring

The interface shows live state for the fleet. You can watch logs as they
arrive, see RAM usage, and follow active tasks across every node, rather than
reading a single combined output stream.

## Acting on nodes

Selection in the node view drives actions. You can pick specific nodes or a
group and then deploy them, run garbage collection, or apply locally, without
leaving the interface. This is the same set of operations the command line
exposes, driven by selection instead of flags.

## Inspection

For any node you can view its metadata: its address, its tags, and the git
revision it is running against. This makes it quick to see which machines are
behind, without running a separate query for each one.

## Logs

The interface aggregates logs from local and remote operations and lets you
filter them. When a deploy touches many nodes, this is how you find the one that
failed.
