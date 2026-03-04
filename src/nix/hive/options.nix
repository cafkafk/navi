with builtins;
rec {
  keyType =
    {
      lib,
      name,
      config,
      ...
    }:
    let
      inherit (lib) types;
    in
    {
      options = {
        name = lib.mkOption {
          description = ''
            File name of the key.
          '';
          default = name;
          type = types.str;
        };

        text = lib.mkOption {
          description = ''
            Content of the key.
            One of `text`, `keyCommand` and `keyFile` must be set.
          '';
          default = null;
          type = types.nullOr types.str;
        };
        keyFile = lib.mkOption {
          description = ''
            Path of the local file to read the key from.
            One of `text`, `keyCommand` and `keyFile` must be set.
          '';
          default = null;
          apply = value: if value == null then null else toString value;
          type = types.nullOr types.path;
        };
        keyCommand = lib.mkOption {
          description = ''
            Command to run to generate the key.
            One of `text`, `keyCommand` and `keyFile` must be set.
          '';
          default = null;
          type =
            let
              nonEmptyList = types.addCheck (types.listOf types.str) (l: length l > 0);
            in
            types.nullOr nonEmptyList;
        };
        destDir = lib.mkOption {
          description = ''
            Destination directory on the host.
          '';
          default = "/run/keys";
          type = types.path;
        };
        path = lib.mkOption {
          description = ''
            Full path to the destination.
          '';
          default = "${config.destDir}/${config.name}";
          type = types.path;
          internal = true;
        };
        user = lib.mkOption {
          description = ''
            The group that will own the file.
          '';
          default = "root";
          type = types.str;
        };
        group = lib.mkOption {
          description = ''
            The group that will own the file.
          '';
          default = "root";
          type = types.str;
        };
        permissions = lib.mkOption {
          description = ''
            Permissions to set for the file.
          '';
          default = "0600";
          type = types.str;
        };
        uploadAt = lib.mkOption {
          description = ''
            When to upload the keys.

            - pre-activation (default): Upload the keys before activating the new system profile.
            - post-activation: Upload the keys after successfully activating the new system profile.

            For `navi upload-keys`, all keys are uploaded at the same time regardless of the configuration here.
          '';
          default = "pre-activation";
          type = types.enum [
            "pre-activation"
            "post-activation"
          ];
        };
      };
    };

  # Navi-specific options
  #
  # Largely compatible with NixOps/Morph.
  deploymentOptions =
    { name, lib, ... }:
    let
      inherit (lib) types;
    in
    {
      options = {
        deployment = {
          targetHost = lib.mkOption {
            description = ''
              The target SSH node for deployment.

              By default, the node's attribute name will be used.
              If set to null, only local deployment will be supported.
            '';
            type = types.nullOr types.str;
            default = name;
          };
          targetPort = lib.mkOption {
            description = ''
              The target SSH port for deployment.

              By default, the port is the standard port (22) or taken
              from your ssh_config.
            '';
            type = types.nullOr types.ints.unsigned;
            default = null;
          };
          targetUser = lib.mkOption {
            description = ''
              The user to use to log into the remote node. If set to null, the
              target user will not be specified in SSH invocations.
            '';
            type = types.nullOr types.str;
            default = "root";
          };
          allowLocalDeployment = lib.mkOption {
            description = ''
              Allow the configuration to be applied locally on the host running
              Navi.

              For local deployment to work, all of the following must be true:
              - The node must be running NixOS.
              - The node must have deployment.allowLocalDeployment set to true.
              - The node's networking.hostName must match the hostname.

              To apply the configurations locally, run `navi apply-local`.
              You can also set deployment.targetHost to null if the nost is not
              accessible over SSH (only local deployment will be possible).
            '';
            type = types.bool;
            default = false;
          };
          buildOnTarget = lib.mkOption {
            description = ''
              Whether to build the system profiles on the target node itself.

              When enabled, Navi will copy the derivation to the target
              node and initiate the build there. This avoids copying back the
              build results involved with the native distributed build
              feature. Furthermore, the `build` goal will be equivalent to
              the `push` goal. Since builds happen on the target node, the
              results are automatically "pushed" and won't exist in the local
              Nix store.

              You can temporarily override per-node settings by passing
              `--build-on-target` (enable for all nodes) or
              `--no-build-on-target` (disable for all nodes) on the command
              line.
            '';
            type = types.bool;
            default = false;
          };
          tags = lib.mkOption {
            description = ''
              A list of tags for the node.

              Can be used to select a group of nodes for deployment.
            '';
            type = types.listOf types.str;
            default = [ ];
          };
          keys = lib.mkOption {
            description = ''
              A set of secrets to be deployed to the node.

              Secrets are transferred to the node out-of-band and
              never ends up in the Nix store.
            '';
            type = types.attrsOf (types.submodule keyType);
            default = { };
          };
          replaceUnknownProfiles = lib.mkOption {
            description = ''
              Allow a configuration to be applied to a host running a profile we
              have no knowledge of. By setting this option to false, you reduce
              the likelyhood of rolling back changes made via another Navi user.

              Unknown profiles are usually the result of either:
              - The node had a profile applied, locally or by another Navi.
              - The host running Navi garbage-collecting the profile.

              To force profile replacement on all targeted nodes during apply,
              use the flag `--force-replace-unknown-profiles`.
            '';
            type = types.bool;
            default = true;
          };
          privilegeEscalationCommand = lib.mkOption {
            description = ''
              Command to use to elevate privileges when activating the new profiles on SSH hosts.

              This is used on SSH hosts when `deployment.targetUser` is not `root`.
              The user must be allowed to use the command non-interactively.
            '';
            type = types.listOf types.str;
            default = [
              "sudo"
              "-H"
              "--"
            ];
          };
          sshOptions = lib.mkOption {
            description = ''
              Extra SSH options to pass to the SSH command.
            '';
            type = types.listOf types.str;
            default = [ ];
          };
          provisioner = lib.mkOption {
            description = ''
              The name of the provisioner to use for this node.
            '';
            type = types.nullOr types.str;
            default = null;
          };
          providers = lib.mkOption {
            description = ''
              Cloud provider settings.
            '';
            default = { };
            type = types.submodule {
              options = {
                gcp = lib.mkOption {
                  description = "Google Cloud Platform configuration.";
                  default = null;
                  type = types.nullOr (
                    types.submodule {
                      options = {
                        project = lib.mkOption {
                          type = types.nullOr types.str;
                          default = null;
                          description = "The project ID.";
                        };
                        zone = lib.mkOption {
                          type = types.nullOr types.str;
                          default = null;
                          description = "The zone.";
                        };
                        iap = lib.mkOption {
                          type = types.bool;
                          default = true;
                          description = "Whether to use Identity-Aware Proxy (IAP) for SSH.";
                        };
                      };
                    }
                  );
                };
              };
            };
          };
          unlock = lib.mkOption {
            description = "Disk unlocking configuration.";
            default = { };
            type = types.submodule (
              { config, ... }:
              {

                options = {
                  enable = lib.mkOption {
                    type = types.bool;
                    default = false;
                    description = "Whether to enable disk unlocking support for this node.";
                  };
                  port = lib.mkOption {
                    type = types.int;
                    default = 2222;
                    description = "SSH port to connect to (defaults to 2222 for initrd).";
                  };
                  host = lib.mkOption {
                    type = types.nullOr types.str;
                    default = null;
                    description = "Override the address/hostname to connect to.";
                  };
                  forceHwLink = lib.mkOption {
                    type = types.bool;
                    default = false;
                    description = "Whether to force the connection through a physical interface (enp* or wlp*), bypassing Tailscale. Uses explicit routing table lookup to pick the best metric.";
                  };
                  interfaces = lib.mkOption {
                    type = types.nullOr (types.listOf types.str);
                    default = null;
                    description = "Explicit list of allowed interfaces to bind to (e.g. eth0). Mutually exclusive with forceHwLink.";
                  };
                  user = lib.mkOption {
                    type = types.nullOr types.str;
                    default = null;
                    description = "Override the SSH username.";
                  };
                  passwordCommand = lib.mkOption {
                    type = types.nullOr types.str;
                    default = null;
                    description = "Local command to retrieve the password (e.g., 'pass my-host').";
                  };
                  ignoreSshConfig = lib.mkOption {
                    type = types.bool;
                    default = false;
                    description = "Whether to ignore the local SSH configuration file (useful for bypassing VPNs/Tailscale).";
                  };
                  ignoreHostKeyCheck = lib.mkOption {
                    type = types.bool;
                    default = false;
                    description = "Whether to ignore strict host key checking (useful if initrd uses ephemeral keys).";
                  };
                  sshOptions = lib.mkOption {
                    type = types.listOf types.str;
                    default = [ ];
                    description = "Extra SSH options to use when connecting to the initrd.";
                  };
                  remoteCommand = lib.mkOption {
                    type = types.str;
                    default = "zpool import -a; zfs load-key -a && (killall zfs || true)";
                    description = "Command to run on the remote host to unlock.";
                  };
                };
              }
            );
          };
        };
      };
    };

    # Hive-wide options
      metaOptions =
        { lib, ... }:
        let
          inherit (lib) types;
        in
        {
          options = {
            name = lib.mkOption {
              description = ''
                The name of the configuration.
              '';
              type = types.str;
              default = "hive";
            };
            description = lib.mkOption {
              description = ''
                A short description for the configuration.
              '';
              type = types.str;
              default = "A Navi Hive";
            };
            nixpkgs = lib.mkOption {
              description = ''
                The pinned Nixpkgs package set. Accepts one of the following:

                - A path to a Nixpkgs checkout
                - The Nixpkgs lambda (e.g., import <nixpkgs>)
                - An initialized Nixpkgs attribute set

                This option must be specified when using Flakes.
              '';
              type = types.unspecified;
              default = null;
            };
            nodeNixpkgs = lib.mkOption {
              description = ''
                Node-specific Nixpkgs pins.
              '';
              type = types.attrsOf types.unspecified;
              default = { };
            };
            nodeSpecialArgs = lib.mkOption {
              description = ''
                Node-specific special args.
              '';
              type = types.attrsOf types.unspecified;
              default = { };
            };
            machinesFile = lib.mkOption {
              description = ''
                Use the machines listed in this file when building this hive configuration.

                If your Navi host has nix configured to allow for remote builds
                (for nix-daemon, your user being included in trusted-users)
                you can set a machines file that will be passed to the underlying
                nix-store command during derivation realization as a builders option.
                For example, if you support multiple orginizations each with their own
                build machine(s) you can ensure that builds only take place on your
                local machine and/or the machines specified in this file.

                See https://nixos.org/manual/nix/stable/advanced-topics/distributed-builds
                for the machine specification format.

                This option is ignored when builds are initiated on the remote nodes
                themselves via `deployment.buildOnTarget` or `--build-on-target`. To
                still use the Nix distributed build functionality, configure the
                builders on the target nodes with `nix.buildMachines`.
              '';
              default = null;
              apply = value: if value == null then null else toString value;
              type = types.nullOr types.path;
            };
            specialArgs = lib.mkOption {
              description = ''
                A set of special arguments to be passed to NixOS modules.

                This will be merged into the `specialArgs` used to evaluate
                the NixOS configurations.
              '';
              default = { };
              type = types.attrsOf types.unspecified;
            };
            allowApplyAll = lib.mkOption {
              description = ''
                Whether to allow deployments without a node filter set.

                If set to false, a node filter must be specified with `--on` when
                deploying.

                It helps prevent accidental deployments to the entire cluster
                when tags are used (e.g., `@production` and `@staging`).
              '';
              default = true;
              type = types.bool;
            };
            provisioners = lib.mkOption {
              description = ''
                A set of provisioners that can be used to deploy infrastructure.
              '';
              type = types.attrs;
              default = { };
            };
            registrants = lib.mkOption {
              description = "Domain registrar configurations.";
              default = null;
              type = types.nullOr (
                types.submodule {
                  options = {
                    porkbun = lib.mkOption {
                      description = "Porkbun accounts.";
                      default = { };
                      type = types.attrsOf (
                        types.submodule {
                          options = {
                            apiKeyCommand = lib.mkOption {
                              type = types.str;
                              description = "Command that outputs the API Key.";
                            };
                            secretApiKeyCommand = lib.mkOption {
                              type = types.str;
                              description = "Command that outputs the Secret API Key.";
                            };
                            terraformSecrets = lib.mkOption {
                              type = types.bool;
                              default = true;
                              description = "Inject these credentials as Terraform variables.";
                            };
                            apiKeyVariable = lib.mkOption {
                              type = types.str;
                              default = "porkbun_api_key";
                              description = "Terraform variable name for the API Key.";
                            };
                            secretKeyVariable = lib.mkOption {
                              type = types.str;
                              default = "porkbun_secret_api_key";
                              description = "Terraform variable name for the Secret Key.";
                            };
                          };
                        }
                      );
                    };
                    namecheap = lib.mkOption {
                      description = "Namecheap accounts.";
                      default = { };
                      type = types.attrsOf (
                        types.submodule {
                          options = {
                            apiKeyCommand = lib.mkOption {
                              type = types.str;
                              description = "Command that outputs the API Key.";
                            };
                            userCommand = lib.mkOption {
                              type = types.str;
                              description = "Command that outputs the API User.";
                            };
                          };
                        }
                      );
                    };
                  };
                }
              );
            };
            facts = lib.mkOption {
              description = "Configuration for persistent facts (Terraform outputs).";
              default = { };
              type = types.submodule {
                options = {
                  enable = lib.mkOption {
                    type = types.bool;
                    default = true;
                    description = "Whether to capture Terraform outputs to the facts directory.";
                  };
                  dirName = lib.mkOption {
                    type = types.str;
                    default = "facts";
                    description = "The directory name within the repository to store facts.";
                  };
                };
              };
            };
          };
        };
}
