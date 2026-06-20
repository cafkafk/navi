# Provisioning infrastructure

Navi can create the machines it deploys to, not just configure existing ones. It
integrates Terranix and Terraform so that infrastructure and NixOS
configuration live in the same Hive.

## The model

You declare infrastructure resources alongside your nodes. Navi renders the
Terranix expressions to Terraform, runs Terraform to create the resources, and
captures the outputs as facts. Those facts, such as a freshly assigned IP
address, become available to the rest of the deployment.

This closes the gap between provisioning and configuration. A single workflow
can stand up a cloud instance and then deploy a NixOS system onto it.

## Provisioning commands

`navi provision` drives the Terraform lifecycle. It plans, applies, and destroys
infrastructure, and it manages the Terraform lock file and state so that
concurrent runs do not corrupt each other.

## Bootstrapping fresh machines

After provisioning creates a machine, it is a bare host with no NixOS on it.
Navi integrates nixos-anywhere to install NixOS onto such a host over SSH,
turning a blank instance into a managed node in one flow.

The `navi install` command loads the captured outputs and bootstraps the
target. For bare-metal hosts it can resolve the address interactively or from a
flag rather than from Terraform outputs.

## Cloud integration

Navi has native support for Google Cloud Platform. It handles authentication and
can reach instances through an Identity-Aware Proxy tunnel, so you can deploy to
machines that have no public address.

## Facts

Outputs captured from Terraform are stored as facts and persist between runs.
Facts are how provisioning hands information to deployment. Inspect and manage
them with `navi facts`.
