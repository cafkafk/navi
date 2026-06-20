# The full lifecycle

Navi's reason to exist is that it drives the whole infrastructure lifecycle from
one declarative source. A single Nix evaluation describes a fleet, and a single
command takes that fleet from nothing to running.

The lifecycle Navi covers is:

1. Cloud provisioning, creating the machines through Terraform.
2. OS installation, putting NixOS onto the new machines with nixos-anywhere.
3. Disk decryption, unlocking encrypted volumes such as ZFS in initrd.
4. Secret delivery, uploading keys to each node.
5. DNS, registering and updating records through supported providers.
6. Configuration switching, activating the target NixOS configuration.

Without Navi, these are separate tools stitched together by hand. A typical flow
is Terraform to create the machine, nixos-anywhere to install onto it, a secrets
mechanism to deliver keys, a DNS tool or console to publish records, and Colmena
to switch the configuration, each with its own state and its own runbook step.

With Navi, the same flow is one Nix-evaluated workflow. Provisioning a whole
tenant from scratch is:

    navi provision --on <tenant>-*

This is the part of Navi that does not exist in Colmena. Colmena begins once a
machine is already running and reachable. Navi begins before the machine exists.

## Why one evaluation matters

Because the whole lifecycle is evaluated from the same Hive, later stages can use
the outputs of earlier ones directly. An address that provisioning assigns
becomes a fact that installation and switching read, without you copying it
between tools. The fleet has one description, so there is one place to change and
one source of truth for what every machine should be.

## What it scales to

The same command shape that deploys one node deploys a whole class of them,
across both cloud and bare metal. Selecting more nodes widens an operation
without adding steps to it. For a concrete fleet built this way, see
[A production deployment](../example.md).

The chapters that follow break the lifecycle into its parts. Provisioning covers
creating machines and installing onto them. Applying covers the configuration
switch that Navi inherits from Colmena. The terminal interface and the daemon
cover operating a fleet of this size day to day.
