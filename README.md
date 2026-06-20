# Navi

Navi is in early development. Commands, configuration formats, and this README
can change between releases without notice. Treat everything here as provisional.

Navi is experimental and its credential handling is not yet hardened. Do not use
it with production API keys or on multi-user systems.

![navi demo picture](docs/readme/readme_header.png)

Navi is a unified deployment tool for NixOS fleets at scale. It collapses the
full infrastructure lifecycle into a single declarative, Nix-evaluated workflow:
cloud provisioning, OS installation, secret delivery, DNS, disk decryption, and
configuration switching, all from one flake.

## Overview

What otherwise takes a stack of separate tools, Terraform to create machines,
nixos-anywhere to install onto them, a secrets mechanism, a DNS tool, and Colmena
to switch the configuration, Navi expresses as one Nix evaluation and drives from
one command. Bringing a group of machines from nothing to their target
configuration looks like:

```bash
navi provision --on <selector>
```

Because the whole lifecycle is evaluated from the same Hive, the command that
acts on one node acts on a whole class of them with the same shape, on cloud or
bare metal. Selecting more nodes widens the operation without changing it.

Navi is a fork of Colmena and keeps full compatibility with standard Hive
configurations, so everything you can already express in Colmena still works.
The difference is the rest of the lifecycle. Colmena deploys to machines that
already exist. Navi also creates them, installs NixOS, unlocks their disks,
manages their DNS, and delivers their secrets, all from the same evaluation
rather than from separate tools wired together by hand.

## Features

Provisioning is the part that does not exist in Colmena. You declare
infrastructure alongside your nodes in the same Hive, and `navi provision` plans,
applies, and destroys it through Terranix and Terraform. Navi manages the
Terraform lock file and state, captures outputs as persistent facts that later
stages read, and bootstraps fresh machines with nixos-anywhere once they exist.
Google Cloud Platform is supported natively, including authentication and access
through an Identity-Aware Proxy tunnel.

The lifecycle reaches all the way onto encrypted, bare-metal hardware. The
`navi disk-unlock` command unlocks encrypted ZFS pools on a remote host,
including over SSH in initrd, so a machine that boots into an encrypted volume
can still be brought up as part of a deploy. Every deployment writes provenance
metadata, namely the git commit, deployer identity, and timestamp, to
`/etc/navi/provenance.json` on the target. Navi compares local closures against
remote systems with `nvd` to show package-level diffs before deploying, and it
can fetch DNS and glue record status from providers such as Porkbun and
Namecheap.

Two capabilities support operating a fleet of this size. The terminal interface,
opened with `navi tui`, shows nodes in a hierarchy you can organise by category,
environment, or hostgroup, streams live logs, RAM usage, and active tasks, and
lets you deploy, garbage collect, or apply locally from the current selection. A
background daemon owns connections and locks, so overlapping operations across
many nodes run concurrently without racing and can continue after the client
that started them exits.

## Example

To show what this looks like at scale rather than on one node, Navi is used to
run a heterogeneous production fleet from a single flake: around thirty GCP
virtual machines grouped across several tenants and environments, alongside
on-prem hardware on encrypted ZFS that is unlocked remotely as part of a deploy.

From that one description, operations that would otherwise be multi-step runbooks
reduce to a command each. Standing up a whole tenant from nothing, machines
created, NixOS installed, configuration switched, is `navi provision --on
<tenant>-*`. Rolling a change to an environment is `navi apply --on @staging`.
Unlocking an encrypted on-prem node so it can finish booting is
`navi disk-unlock <node>`. None of these does something separate tools could not,
but they come from one source of truth, and scaling up means selecting more
nodes, not adding more steps.

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
