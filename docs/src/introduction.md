# Introduction

Navi is a deployment tool for NixOS. It builds system configurations from a
single Hive expression and applies them across a fleet of machines.

Navi is a fork of Colmena. It keeps full compatibility with standard Hive
configurations and adds three things on top: a persistent daemon that owns
connections and task queues, integrated infrastructure provisioning through
Terranix and Terraform, and a terminal interface for managing large fleets.

This manual is split into three parts. The tutorial takes you from an empty
directory to a deployed node. The guides cover individual features in more
depth. The reference documents every command and configuration option.

If you have used Colmena before, most of what you know carries over. The
chapters on the daemon, provisioning, and the terminal interface describe the
parts that are new.

## Status

Navi is in early development. Commands, configuration formats, and this manual
can change between releases without notice. Do not use it with production
credentials or on multi-user systems yet, because credential handling is not
hardened.

## How to read the command examples

Commands are shown without a shell prompt:

    navi apply --on web-01

Output is shown indented below the command when it matters. Where a command
needs a Hive that the previous chapter built, the chapter says so at the top.
