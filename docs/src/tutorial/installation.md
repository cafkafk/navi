# Installation

Navi needs a working Nix installation with flakes enabled. It does not need to
be installed on the machines you deploy to. Those only need an SSH server and a
Nix daemon.

## Run without installing

You can run Navi straight from the flake:

    nix run github:cafkafk/navi -- --help

This is the quickest way to try a command without changing your environment.

## Add it to a flake

To pin Navi in a project, add it as an input and use the package it exposes:

    {
      inputs.navi.url = "github:cafkafk/navi";

      outputs = { self, nixpkgs, navi, ... }: {
        # navi.packages.<system>.navi
      };
    }

## Development shell

If you are working on Navi itself, the repository ships a development shell with
the Rust toolchain, Nix tooling, and mdBook for this manual:

    nix develop

From there, `cargo build` produces the binary and `mdbook serve docs` serves
this manual locally.

## Enabling flakes

If Nix reports that flakes are an experimental feature, add this to your Nix
configuration:

    experimental-features = nix-command flakes

The rest of the tutorial assumes flakes are enabled.
