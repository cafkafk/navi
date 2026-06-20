# Setup is injected above by the Navi test harness.
#
# Provenance metadata is written to /etc/navi/provenance.json on a target
# whenever a configuration is switched or booted. This test checks that the
# file lands on the target and carries the expected fields.

import json

with subtest("Switching the target writes provenance"):
    deployer.succeed(
        "cd /tmp/bundle && "
        f"{navi} apply switch --eval-node-limit 4 --on alpha"
    )
    alpha.succeed("grep FIRST /etc/deployment")

with subtest("Provenance metadata lands on the target as JSON"):
    raw = alpha.succeed("cat /etc/navi/provenance.json")
    data = json.loads(raw)

    for key in ("commit", "flake_uri", "timestamp", "deployed_by"):
        assert key in data, f"provenance missing key {key!r}; got {sorted(data)}"

    assert data["timestamp"].isdigit(), f"timestamp not numeric: {data['timestamp']!r}"

with subtest("Provenance file is owned by root and not world-writable"):
    perms = alpha.succeed("stat -c '%U %a' /etc/navi/provenance.json").strip()
    user, mode = perms.split()
    assert user == "root", f"provenance owned by {user}, expected root"
    assert mode[-1] not in ("2", "3", "6", "7"), f"provenance is world-writable: {mode}"
