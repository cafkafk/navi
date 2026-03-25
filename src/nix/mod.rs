use std::collections::HashMap;
use std::hash::Hash;
use std::ops::Deref;
use std::path::Path;

use serde::de;
use serde::{Deserialize, Deserializer, Serialize};
use validator::{Validate, ValidationError as ValidationErrorType};

use crate::error::{NaviError, NaviResult};

pub mod host;
pub use host::Provider;
pub use host::Ssh;
pub use host::{CopyDirection, CopyOptions, Host, RebootOptions};

pub mod hive;
pub use hive::{Hive, HivePath};

pub mod store;
pub use store::{BuildResult, StoreDerivation, StorePath};

pub mod key;
pub use key::Key;

pub mod profile;
pub use profile::{Profile, ProfileDerivation};

pub mod deployment;
pub use deployment::Goal;

pub mod node_state;
pub use node_state::NodeState;

pub mod info;
pub use info::NixCheck;

pub mod flake;
pub use flake::Flake;

pub mod node_filter;
pub use node_filter::NodeFilter;

pub mod provenance;
pub use provenance::Provenance;

pub mod evaluator;
pub mod nixos_anywhere;

pub mod expression;
pub use expression::{NixExpression, SerializedNixExpression};

pub const DEFAULT_KEXEC_URL_TEMPLATE: &str = "https://github.com/nix-community/nixos-images/releases/download/nixos-25.05/nixos-kexec-installer-noninteractive-{}.tar.gz";

/// Path to the main system profile.
pub const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";

/// Path to the system profile that's currently active.
pub const CURRENT_PROFILE: &str = "/run/current-system";

/// A node's attribute name.
#[derive(Serialize, Deserialize, Clone, Debug, Hash, Eq, PartialEq)]
#[serde(transparent)]
pub struct NodeName(#[serde(deserialize_with = "NodeName::deserialize")] String);

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProvidersConfig {
    pub gcp: Option<GcpConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GcpConfig {
    pub project: Option<String>,
    pub zone: Option<String>,
    #[serde(default = "default_true")]
    pub iap: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Validate, Deserialize, Serialize)]
pub struct NodeConfig {
    #[serde(rename = "targetHost")]
    pub target_host: Option<String>,

    #[serde(rename = "targetUser")]
    pub target_user: Option<String>,

    #[serde(rename = "targetPort")]
    pub target_port: Option<u16>,

    #[serde(rename = "allowLocalDeployment", default)]
    pub allow_local_deployment: bool,

    #[serde(rename = "buildOnTarget", default)]
    pub build_on_target: bool,

    #[serde(default)]
    pub tags: Vec<String>,

    #[serde(default)]
    pub providers: ProvidersConfig,

    #[serde(default)]
    pub unlock: DiskUnlockConfig,

    #[serde(rename = "replaceUnknownProfiles", default)]
    pub replace_unknown_profiles: bool,

    #[serde(rename = "privilegeEscalationCommand", default)]
    pub privilege_escalation_command: Vec<String>,

    #[serde(rename = "sshOptions", default)]
    pub extra_ssh_options: Vec<String>,

    #[serde(rename = "provisioner")]
    pub provisioner: Option<String>,

    #[validate(custom(function = "validate_keys"))]
    #[serde(default)]
    pub keys: HashMap<String, Key>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            target_host: None,
            target_user: None,
            target_port: None,
            allow_local_deployment: false,
            build_on_target: false,
            tags: Vec::new(),
            providers: ProvidersConfig { gcp: None },
            unlock: DiskUnlockConfig::default(),
            replace_unknown_profiles: false,
            privilege_escalation_command: Vec::new(),
            extra_ssh_options: Vec::new(),
            provisioner: None,
            keys: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Validate, Deserialize, Serialize)]
pub struct MetaConfig {
    #[serde(rename = "allowApplyAll")]
    pub allow_apply_all: bool,

    #[serde(rename = "machinesFile")]
    pub machines_file: Option<String>,

    #[serde(rename = "hierarchy")]
    pub hierarchy: Option<Vec<String>>,

    #[serde(rename = "provisioners")]
    pub provisioners: Option<HashMap<String, ProvisionerConfig>>,

    #[serde(rename = "registrants")]
    pub registrants: Option<RegistrantsConfig>,

    #[serde(default)]
    pub facts: FactsConfig,
}

#[derive(Debug, Clone, Validate, Deserialize, Serialize)]
pub struct FactsConfig {
    #[serde(default = "default_true")]
    pub enable: bool,

    #[serde(rename = "dirName", default = "default_facts_dir_name")]
    pub dir_name: String,

    #[serde(default)]
    pub derive: Vec<String>,
}

impl Default for FactsConfig {
    fn default() -> Self {
        Self {
            enable: default_true(),
            dir_name: default_facts_dir_name(),
            derive: Vec::new(),
        }
    }
}

fn default_facts_dir_name() -> String {
    "facts".to_string()
}

#[derive(Debug, Clone, Validate, Deserialize, Serialize)]
pub struct RegistrantsConfig {
    #[serde(default)]
    pub porkbun: HashMap<String, PorkbunAccount>,
    #[serde(default)]
    pub namecheap: HashMap<String, NamecheapAccount>,
}

#[derive(Debug, Clone, Validate, Deserialize, Serialize)]
pub struct NamecheapAccount {
    #[serde(rename = "apiKeyCommand")]
    pub api_key_command: String,
    #[serde(rename = "userCommand")]
    pub user_command: String,
}

#[derive(Debug, Clone, Validate, Deserialize, Serialize)]
pub struct PorkbunAccount {
    #[serde(rename = "apiKeyCommand")]
    pub api_key_command: String,
    #[serde(rename = "secretApiKeyCommand")]
    pub secret_api_key_command: String,

    #[serde(rename = "terraformSecrets", default = "default_true")]
    pub terraform_secrets: bool,

    #[serde(rename = "apiKeyVariable", default = "default_porkbun_api_var")]
    pub api_key_variable: String,

    #[serde(rename = "secretKeyVariable", default = "default_porkbun_secret_var")]
    pub secret_key_variable: String,
}

fn default_porkbun_api_var() -> String {
    "porkbun_api_key".to_string()
}

fn default_porkbun_secret_var() -> String {
    "porkbun_secret_api_key".to_string()
}

#[derive(Debug, Clone, Validate, Deserialize, Serialize)]
pub struct ProvisionerConfig {
    #[serde(rename = "type")]
    pub kind: ProvisionerType,
    pub command: Option<String>,
    pub app: Option<String>,
    pub configuration: Option<String>,
    #[serde(rename = "nixosAnywhere")]
    pub nixos_anywhere: Option<NixosAnywhereConfig>,

    #[serde(default)]
    pub derive: Vec<String>,
}

#[derive(Debug, Clone, Validate, Deserialize, Serialize)]
pub struct NixosAnywhereConfig {
    pub enable: bool,
    #[serde(rename = "sshUser")]
    pub ssh_user: Option<String>,
    #[serde(default)]
    pub unlock: bool,
    #[serde(rename = "extraArgs", default)]
    pub extra_args: Vec<String>,
    #[serde(rename = "downloadKexecLocally", default)]
    pub download_kexec_locally: bool,
    #[serde(rename = "kexecUrlTemplate", default)]
    pub kexec_url_template: Option<String>,
}

#[derive(Debug, Clone, Validate, Deserialize, Serialize)]
pub struct DiskUnlockConfig {
    #[serde(default)]
    pub enable: bool,

    #[serde(default = "default_unlock_port")]
    pub port: u16,

    pub host: Option<String>,

    #[serde(rename = "forceHwLink", default)]
    pub force_hw_link: bool,

    #[serde(default)]
    pub interfaces: Option<Vec<String>>,

    pub user: Option<String>,

    #[serde(rename = "passwordCommand")]
    pub password_command: Option<String>,

    #[serde(rename = "ignoreSshConfig", default)]
    pub ignore_ssh_config: bool,

    #[serde(rename = "ignoreHostKeyCheck", default)]
    pub ignore_host_key_check: bool,

    #[serde(rename = "sshOptions", default)]
    pub ssh_options: Vec<String>,

    #[serde(rename = "remoteCommand", default = "default_remote_unlock_cmd")]
    pub remote_command: String,
}

impl Default for DiskUnlockConfig {
    fn default() -> Self {
        Self {
            enable: false,
            port: default_unlock_port(),
            host: None,
            force_hw_link: false,
            interfaces: None,
            user: None,
            password_command: None,
            ignore_ssh_config: false,
            ignore_host_key_check: false,
            ssh_options: vec![],
            remote_command: default_remote_unlock_cmd(),
        }
    }
}

fn default_unlock_port() -> u16 {
    2222
}

fn default_remote_unlock_cmd() -> String {
    "zpool import -a; zfs load-key -a && (killall zfs || true)".to_string()
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProvisionerType {
    Command,
    FlakeApp,
    Terranix,
}

/// Nix CLI flags.
#[derive(Debug, Clone, Default)]
pub struct NixFlags {
    /// Whether to pass --show-trace.
    show_trace: bool,

    /// Whether to pass --pure-eval.
    pure_eval: bool,

    /// Whether to pass --impure.
    impure: bool,

    /// Designated builders.
    ///
    /// See <https://nixos.org/manual/nix/stable/advanced-topics/distributed-builds.html>.
    ///
    /// Valid examples:
    /// - `@/path/to/machines`
    /// - `builder@host.tld riscv64-linux /home/nix/.ssh/keys/builder.key 8 1 kvm`
    builders: Option<String>,

    /// Options to pass as --option name value.
    options: HashMap<String, String>,
}

impl NodeName {
    /// Returns the string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Creates a NodeName from a String.
    pub fn new(name: String) -> NaviResult<Self> {
        let validated = Self::validate(name)?;
        Ok(Self(validated))
    }

    /// Deserializes a potentially-invalid node name.
    fn deserialize<'de, D>(deserializer: D) -> Result<String, D::Error>
    where
        D: Deserializer<'de>,
    {
        use de::Error;
        String::deserialize(deserializer)
            .and_then(|s| Self::validate(s).map_err(|e| Error::custom(e.to_string())))
    }

    fn validate(s: String) -> NaviResult<String> {
        // FIXME: Elaborate
        if s.is_empty() {
            return Err(NaviError::EmptyNodeName);
        }

        Ok(s)
    }
}

impl Deref for NodeName {
    type Target = str;

    fn deref(&self) -> &str {
        self.0.as_str()
    }
}

impl NodeConfig {
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn get_provider(&self) -> Provider {
        if let Some(gcp) = &self.providers.gcp {
            return Provider::Gcp {
                project: gcp.project.clone(),
                zone: gcp.zone.clone(),
                iap: gcp.iap,
            };
        }

        let provider_tag = self.tags.iter().find(|t| t.starts_with("provider:"));

        match provider_tag.map(|s| s.as_str()) {
            Some("provider:gcp") => {
                let mut project = None;
                let mut zone = None;

                for tag in &self.tags {
                    if let Some(val) = tag.strip_prefix("gcp:project:") {
                        project = Some(val.to_string());
                    } else if let Some(val) = tag.strip_prefix("gcp:zone:") {
                        zone = Some(val.to_string());
                    }
                }

                Provider::Gcp {
                    project,
                    zone,
                    iap: true,
                }
            }
            _ => Provider::Ssh,
        }
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn allows_local_deployment(&self) -> bool {
        self.allow_local_deployment
    }

    pub fn privilege_escalation_command(&self) -> &Vec<String> {
        &self.privilege_escalation_command
    }

    pub fn build_on_target(&self) -> bool {
        self.build_on_target
    }
    pub fn set_build_on_target(&mut self, enable: bool) {
        self.build_on_target = enable;
    }

    pub fn to_ssh_host(&self) -> Option<Ssh> {
        self.target_host.as_ref().map(|target_host| {
            let mut host = Ssh::new(self.target_user.clone(), target_host.clone());
            host.set_privilege_escalation_command(self.privilege_escalation_command.clone());
            host.set_extra_ssh_options(self.extra_ssh_options.clone());
            host.set_provider(self.get_provider());

            if let Some(target_port) = self.target_port {
                host.set_port(target_port);
            }

            host
        })
    }
}

impl NixFlags {
    pub fn set_show_trace(&mut self, show_trace: bool) {
        self.show_trace = show_trace;
    }

    pub fn set_pure_eval(&mut self, pure_eval: bool) {
        self.pure_eval = pure_eval;
    }

    pub fn set_impure(&mut self, impure: bool) {
        self.impure = impure;
    }

    pub fn set_builders(&mut self, builders: Option<String>) {
        self.builders = builders;
    }

    pub fn set_options(&mut self, options: HashMap<String, String>) {
        self.options = options;
    }

    pub fn to_args(&self) -> Vec<String> {
        self.to_args_inner(false)
    }

    /// Returns arguments for `nix-store`.
    pub fn to_nix_store_args(&self) -> Vec<String> {
        self.to_args_inner(true)
    }

    fn to_args_inner(&self, nix_store: bool) -> Vec<String> {
        let mut args = Vec::new();

        if let Some(builders) = &self.builders {
            args.append(&mut vec![
                "--option".to_string(),
                "builders".to_string(),
                builders.clone(),
            ]);
        }

        if self.show_trace {
            args.push("--show-trace".to_string());
        }

        if self.pure_eval {
            args.push("--pure-eval".to_string());
        }

        // The `nix-store` command does not accept `--impure`
        // TODO: Not happy about this solution - Have a better Nix abstraction that hides
        // CLI details (e.g., nix3 CLI differences)
        if self.impure && !nix_store {
            args.push("--impure".to_string());
        }

        for (name, value) in self.options.iter() {
            args.push("--option".to_string());
            args.push(name.to_string());
            args.push(value.to_string());
        }

        args
    }
}

fn validate_keys(keys: &HashMap<String, Key>) -> Result<(), ValidationErrorType> {
    // Bad secret names:
    // - /etc/passwd
    // - ../../../../../etc/passwd

    for name in keys.keys() {
        let path = Path::new(name);
        if path.has_root() {
            return Err(ValidationErrorType::new(
                "Secret key name cannot be absolute",
            ));
        }

        if path.components().count() != 1 {
            return Err(ValidationErrorType::new(
                "Secret key name cannot contain path separators",
            ));
        }
    }
    Ok(())
}
