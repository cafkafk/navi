# Setup is injected above by the Navi test harness, including substitution of the
# @nixpkgs@ and @navi@ flake inputs.
#
# `navi facts derive` reads the flake's `facts` output, builds each fact, and
# links the result into facts/derived/<name>.json. This test checks that the
# files appear with the right contents and that glob filters select a subset.

import json

with subtest("Lock the flake"):
    deployer.succeed(
        "cd /tmp/bundle && "
        "nix --extra-experimental-features 'nix-command flakes' flake lock"
    )

with subtest("Deriving all facts writes them to facts/derived"):
    deployer.succeed(f"cd /tmp/bundle && {navi} facts derive")

    greeting = deployer.succeed("cat /tmp/bundle/facts/derived/greeting.json")
    answer = deployer.succeed("cat /tmp/bundle/facts/derived/answer.json")

    assert json.loads(greeting) == {"hello": "world"}, greeting
    assert json.loads(answer) == {"value": 42}, answer

with subtest("A glob filter derives only the matching facts"):
    deployer.succeed("rm -rf /tmp/bundle/facts/derived")
    deployer.succeed(f"cd /tmp/bundle && {navi} facts derive 'greet*'")

    deployer.succeed("test -f /tmp/bundle/facts/derived/greeting.json")
    deployer.fail("test -f /tmp/bundle/facts/derived/answer.json")
