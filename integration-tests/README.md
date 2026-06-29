# Integration Tests

A set of integration tests using the NixOS test framework. Each test brings up
one or more virtual machines, runs Navi inside them, and asserts on the result.

To run a single test:

    nix build .#checks.x86_64-linux.<name>

To run the whole suite:

    nix flake check

The tests are exposed as flake checks (see `nix/checks/default.nix`), which is
how CI runs them. Building `integration-tests/default.nix` directly is not
supported on its own: it falls back to `nixpkgs.nix`, which expects a
`flake-compat.nix` and overlay wiring that only the flake provides.

## Harness

`tools.nix` builds the shared fixture: a `deployer` node with Navi and a
prebuilt system closure, and zero or more minimal `target` nodes. A test is a
directory containing:

- `default.nix`, which calls `tools.runTest` with the test name and a bundle.
- `hive.nix` (or `flake.nix` for flake tests), the configuration under test.
- `test-script.py`, the Python driver run by the NixOS test framework. The
  harness injects setup before it, including the `navi` binary path.

The deployer has no network access during the test, so anything a command needs
must be in the Nix store before the run. `tools.nix` arranges this for the
prebuilt target closure and for flake inputs.

## Tests inherited from Colmena

`apply`, `apply-local`, `build-on-target`, `exec`, `flakes`, `parallel`, and
`allow-apply-all` cover the deployment behaviour Navi shares with Colmena.

## Navi-specific tests

These cover features Navi adds on top of Colmena, without depending on any cloud:

- `provenance` switches a target and checks that `/etc/navi/provenance.json` is
  written with the expected fields and permissions.
- `daemon` starts the background daemon, waits for its socket, and checks that
  `navi daemon status` reports it running.
- `provision-command` uses a `command` provisioner to stand in for the cloud
  boundary, and checks that Navi resolves and runs it. Real provisioners talk to
  Terraform or to hardware; the `command` type lets the orchestration be tested
  in isolation.
- `terranix-real` drives the `terranix` provisioner end to end against a real
  OpenTofu. A tofunix module is rendered to config.tf.json at build time, and
  the test checks that Navi realizes it, runs init, plan, and apply, and
  captures the outputs as facts. It uses no third-party provider, so the
  OpenTofu run stays offline; the outputs alone exercise Navi's whole terranix
  path.
- `facts` derives facts from a flake's `facts` output. `navi facts derive`
  shells out to `nix eval` and `nix build`, so the test stages Navi's whole
  flake input closure into the store and pre-builds the fact outputs, letting
  the derivation run with no network in the VM.
