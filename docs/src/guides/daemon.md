# The daemon

Navi runs a background daemon that owns connections, task queues, and
deployment state. Commands you run on the command line are clients that talk to
this daemon rather than doing the work in their own process.

## Why a daemon

Splitting the work from the client process buys three things. It holds a single
connection pool, so repeated commands against the same fleet reuse connections.
It serialises operations that must not overlap, which prevents two applies from
racing on the same node. And it lets long operations continue after the client
that started them exits, so a deploy survives a closed terminal.

## Managing the daemon

The daemon is controlled through the `navi daemon` subcommand. Use it to start
the daemon, check its status, and stop it.

A client starts the daemon automatically when one is not already running, so for
everyday use you rarely call these commands directly. Manage the daemon
explicitly when you want it to run as a system service, or when you are
debugging.

## State and locks

The daemon is the single owner of deployment locks. When an apply holds a lock
on a node, other operations against that node wait rather than colliding. This
is what makes it safe to run overlapping commands against a shared fleet.

Because the daemon holds this state, stopping it cancels in-flight operations.
Stop it only when no deploy is running, or accept that running operations are
aborted.
