# A tofunix (terranix fork) module. tofunix renders this to config.tf.json,
# which Navi's terranix provisioner feeds to OpenTofu.
#
# It deliberately uses no third-party provider: the VM has no network, and
# providers would need an offline mirror. Outputs alone exercise the full Navi
# path (realize the config, init, plan, apply, capture outputs as facts) with a
# real OpenTofu run. The computed output proves OpenTofu actually evaluated the
# configuration rather than echoing a constant.
{ ... }:
{
  output = {
    greeting = {
      value = "hello-from-tofunix";
    };

    computed = {
      value = "result-\${1 + 1}";
    };
  };
}
