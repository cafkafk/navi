# Setup is injected above by the Navi test harness, including substitution of
# the rendered tofunix config path into hive.nix.
#
# This drives Navi's terranix provisioner end to end against a real OpenTofu:
# realize config.tf.json, init, plan, apply, and capture the outputs as facts.
# No third-party provider is used, so the run stays offline.

import json

with subtest("Provisioning runs OpenTofu through the terranix provisioner"):
    # printf answers Navi's single pre-apply confirmation prompt. A streaming
    # `yes` would be SIGPIPE-killed once Navi stops reading, and the test
    # driver's pipefail would then fail the command despite Navi succeeding.
    # --skip-install stops after facts are captured, since there is no machine
    # to install onto.
    deployer.succeed(
        "cd /tmp/bundle && "
        f"printf 'y\\n' | {navi} provision tf --skip-install 2>&1"
    )

with subtest("OpenTofu outputs are captured as facts"):
    raw = deployer.succeed("cat /tmp/bundle/facts/tf/outputs.json")
    data = json.loads(raw)

    assert data["greeting"]["value"] == "hello-from-tofunix", data
    # result-2 only appears if OpenTofu evaluated the configuration, rather than
    # the value being passed through verbatim.
    assert data["computed"]["value"] == "result-2", data
