# Setup is injected above by the Navi test harness.
#
# This exercises the provisioner path without any cloud. The "local" provisioner
# is of type "command", so Navi resolves it, runs its shell command, and we
# assert the side effect. The deploy/install step is skipped, since this test is
# about provisioner resolution and execution, not about installing onto a fresh
# machine.

with subtest("The configured provisioner is listed"):
    out = deployer.succeed(f"cd /tmp/bundle && {navi} provision --list 2>&1")
    assert "local" in out, out

with subtest("There is no marker before provisioning"):
    deployer.succeed("test ! -e /tmp/provision-marker")

with subtest("Running the provisioner executes its command"):
    # The provisioner name is a positional argument; --skip-install keeps this
    # to provisioner resolution and execution, with no install step.
    deployer.succeed(
        f"cd /tmp/bundle && {navi} provision local --skip-install 2>&1"
    )
    marker = deployer.succeed("cat /tmp/provision-marker").strip()
    assert marker == "navi-provisioned", f"unexpected marker: {marker!r}"
