# Navi

Navi is in early development. Commands, configuration formats, and this README
can change between releases without notice. Treat everything here as provisional.

Navi is experimental and its credential handling is not yet hardened. Do not use
it with production API keys or on multi-user systems.

![navi demo picture](docs/readme/readme_header.png)

Navi is a deployment tool for NixOS, forked from Colmena. It keeps full
compatibility with standard Hive configurations and adds a persistent daemon
that owns connections and task queues, integrated infrastructure provisioning
through Terranix and Terraform, and a terminal interface for managing large
fleets.

## Overview

Navi introduces a client-server model on top of the Colmena workflow. A
background daemon manages deployment state and locks, so overlapping operations
against a shared fleet serialise instead of racing. Navi also bridges
provisioning and configuration by running Terraform as part of the deployment,
which lets a single flow create a machine and then deploy NixOS onto it.

## Features

The terminal interface, opened with `navi tui`, manages a fleet interactively.
It shows nodes in a hierarchy you can organise by category, environment, or
hostgroup, streams live logs, RAM usage, and active tasks, and lets you deploy,
garbage collect, or apply locally from the current selection. For any node it
shows metadata such as its address, tags, and git revision, and it aggregates
and filters logs from local and remote operations.

Infrastructure provisioning is integrated through Terranix. You define Terraform
resources alongside your nodes in the same Hive, and `navi provision` plans,
applies, and destroys them. Navi manages the Terraform lock file and state,
captures outputs as persistent facts, and can bootstrap fresh machines with
nixos-anywhere once they exist. Google Cloud Platform is supported natively,
including authentication and access through an Identity-Aware Proxy tunnel.

The deployment layer adds several capabilities beyond Colmena. The daemon allows
detached operations and prevents race conditions. The `navi disk-unlock` command
unlocks encrypted ZFS pools on a remote host, including over SSH in initrd. Every
deployment writes provenance metadata, namely the git commit, deployer identity,
and timestamp, to `/etc/navi/provenance.json` on the target. Navi compares local
closures against remote systems with `nvd` to show package-level diffs before
deploying, and it can fetch DNS and glue record status from providers such as
Porkbun and Namecheap.

## Documentation

The manual lives in `docs/` and is built with mdBook. To read it locally:

```bash
nix develop
mdbook serve docs
```

You can also build it as a Nix package, which regenerates the command-line
reference from the binary:

```bash
nix build .#manual
```

## Installation

Navi is a Nix flake. You can run it directly:

```bash
nix run github:cafkafk/navi
```

Or install it into your profile:

```bash
nix profile install github:cafkafk/navi
```

## Configuration

Navi uses the standard Hive format and adds a few `meta` options for its own
features.

You can define provisioners in `meta.provisioners` and assign them to nodes:

```nix
{
  meta = {
    nixpkgs = <nixpkgs>;

    provisioners.gcp = {
      type = "terranix";
      configuration = ./terraform/gcp.nix;
    };
  };

  defaults = {
    deployment.provisioner = "gcp";
  };

  # ... node definitions
}
```

You can configure remote unlocking for encrypted hosts:

```nix
{
  nodes.web-01 = {
    deployment.unlock = {
      enable = true;
      port = 2222; # Initrd SSH port
      user = "root";
      # Command to run on the remote host to unlock
      remoteCommand = "zfs load-key -a && killall zfs";
    };
    # ...
  };
}
```

## Usage

Navi keeps the standard Colmena CLI arguments for compatibility. Apply to every
node, or select nodes with `--on`:

```bash
# Apply configuration to all nodes
navi apply

# Apply to specific nodes
navi apply --on web-01,web-02
```

Launch the interactive dashboard:

```bash
navi tui
```

Manage infrastructure resources:

```bash
# Provision infrastructure for specific nodes
navi provision --on web-01

# Unlock disks on remote hosts, useful after a reboot
navi disk-unlock web-01

# Provision infrastructure without installing NixOS
navi provision --on web-01 --skip-install
```

Navi wraps the standard SSH client and adds fleet awareness:

```bash
# SSH into a node
navi ssh web-01

# Remove host keys for a specific node, for example after re-provisioning
navi ssh web-01 -R

# Remove host keys for a group of nodes
navi ssh -R --on "web-*"
```

Navi spawns the daemon automatically when it is needed, but you can manage it
yourself:

```bash
# Check daemon status
navi daemon status
```

## FAQ

### Why is this named navi?

It's not a blue alien, an e-sport team, a bike, or a companion in legend of
Zelda. It's a reference to the NAVI computer, the Knowledge Navigator, in
Serial Experiments Lain, that runs Copland OS (which in itself is a reference
to early apple computers).

Over the season, the machine evolves from a simple, portable device into a
house consuming system of tubes, wires, and fluids. Much like this program.

Here's a link to see it: [Navi Progression: Serial Experiments Lain (youtube)](https://www.youtube.com/watch?v=UdXHdAPHVWE&t=2s)

### Why not use a deployment tool as an input?

Deep native integration. If I'd just have consumed it indirectly, I'd not have
the nescesarry control over the internal. Further, I've always wanted my own
deployment tool, and I know that there is a lot to be gained here from a tight
integration.

Heck, I've pondered subsumming several other tools, but I've held off... for
now.

### Where is the documentation?

The manual is in `docs/` and is built with mdBook. See the documentation
section above for how to read or build it. The command-line reference is
generated from the binary, so it stays in sync with the actual commands.

The source code is still the most complete reference, and for anything the
manual does not yet cover, reading the source is the right move.

### Will you support xyz provider?

Maybe, but prerequisite to asking this, please provide me with the funding to
test against those providers.

### Why should I use this instead of xyz?

You really shouldn't. Please don't.


<br>
<br>
<br>
<br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br><br>

<!-- Software doesn't have to be boring. Let's make more things that aren't boring. Please. -->

<p align="center">
  <img src="https://fauux.neocities.org/wiredLogInNew_512px_06.gif" alt="Fauux's Copland OS logo art">
</p>
