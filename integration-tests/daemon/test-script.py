# Setup is injected above by the Navi test harness.
#
# The daemon is a background service that owns connections and locks. It listens
# on a Unix socket and answers status queries. This test exercises its lifecycle:
# status before start, starting it, and status once it is up.

with subtest("Status reports the daemon is not running before it starts"):
    out = deployer.succeed(f"cd /tmp/bundle && {navi} daemon status 2>&1 || true")
    assert "Daemon Status: Running" not in out, f"daemon unexpectedly running: {out}"

with subtest("The daemon starts and binds its socket"):
    # Run the daemon as a transient unit so it outlives the shell that starts
    # it. Backgrounding it inside a succeed() call would let it be reaped once
    # the call returns, leaving a stale socket behind.
    deployer.succeed(
        "systemd-run --unit=navi-daemon --collect "
        "--working-directory=/tmp/bundle "
        "--setenv=NIX_PATH=nixpkgs=/nixpkgs "
        f"{navi} daemon start"
    )
    deployer.wait_for_file("/tmp/navi.sock")

with subtest("Status reports the daemon is running once it is up"):
    deployer.wait_until_succeeds(
        f"cd /tmp/bundle && {navi} daemon status 2>&1 | grep -q 'Daemon Status: Running'"
    )
    out = deployer.succeed(f"cd /tmp/bundle && {navi} daemon status")
    assert "Active Tasks: 0" in out, out
