# Introduction

Navi is a unified deployment tool for NixOS fleets at scale. It collapses the
full infrastructure lifecycle into a single declarative, Nix-evaluated workflow:
cloud provisioning, OS installation, secret delivery, DNS, disk decryption, and
configuration switching, all from one flake.

What otherwise takes a stack of separate tools, Terraform to create machines,
nixos-anywhere to install onto them, a secrets mechanism, a DNS tool, and
Colmena to switch the configuration, Navi expresses as one evaluation and drives
from one command. Bringing a group of machines from nothing to their target
configuration looks like:

    navi provision --on <selector>

Because the whole lifecycle is evaluated from the same Hive, the command that
acts on one node acts on a whole class of them with the same shape, on cloud or
bare metal. Selecting more nodes widens the operation without changing it.
Navi is built to hold a large, heterogeneous fleet in a single flake. For a
concrete picture of what that looks like, see
[A production deployment](example.md).

## What makes it different

Navi is a fork of Colmena and keeps full compatibility with standard Hive
configurations, so everything you can already express in Colmena still works.
What Navi adds is the rest of the lifecycle.

Colmena deploys configurations to machines that already exist. Navi also creates
those machines, installs NixOS onto them, unlocks their disks, manages their
DNS, and delivers their secrets. It does all of this from the same Nix
evaluation, rather than from a stack of separate tools wired together by hand.
One evaluation describes the whole fleet, and one command drives it from nothing
to running.

Two further capabilities exist to make this practical at scale. A terminal
interface, `navi tui`, manages large fleets interactively. A background daemon
owns connections and locks so that operations across many nodes run concurrently
without racing. These matter once a fleet is large, but they serve the workflow
above rather than being the point of it.

## How this manual is organised

The tutorial takes you from an empty directory to a deployed node. The guides
cover individual features in more depth, starting with the deployment and
provisioning workflow that is Navi's reason to exist. The reference documents
every command and configuration option, and its command-line section is
generated from the binary so it cannot drift.

## Status

Navi is in early development. Commands, configuration formats, and this manual
can change between releases without notice. Do not use it with production
credentials or on multi-user systems yet, because credential handling is not
hardened.

## How to read the command examples

Commands are shown without a shell prompt:

    navi apply --on web-01

Where a command needs a Hive that an earlier chapter built, the chapter says so
at the top.
