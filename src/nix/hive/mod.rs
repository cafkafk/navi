mod assets;
mod expressions;
pub mod path;

#[cfg(test)]
mod tests;

pub use path::HivePath;

use std::collections::{HashMap, HashSet};
use std::convert::AsRef;
use std::path::{Path, PathBuf};

use tokio::process::Command;
use tokio::sync::OnceCell;
use validator::Validate;

use super::deployment::TargetNode;
use super::{
    Flake, MetaConfig, NixExpression, NixFlags, NodeConfig, NodeFilter, NodeName,
    ProfileDerivation, SerializedNixExpression, StorePath,
};
use crate::error::NaviResult;
use crate::job::JobHandle;
use crate::util::{CommandExecution, CommandExt};
use assets::Assets;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvaluationMethod {
    /// Use nix-instantiate and specify the entire Nix expression.
    ///
    /// This is the default method for non-flake configs. It's also used
    /// used for flakes with --legacy-flake-eval.
    ///
    /// For flakes, we use `builtins.getFlakes`. Pure evaluation no longer works
    /// with this method in Nix 2.21+.
    NixInstantiate,

    /// Use `nix eval --apply` on top of a flake.
    ///
    /// This is the default method for flakes.
    ///
    /// In this method, we can no longer pull in our bundled assets and
    /// the flake must expose a compatible `naviHive` output.
    DirectFlakeEval,
}

#[derive(Debug)]
pub struct Hive {
    /// Path to the hive.
    path: HivePath,

    /// Method to evaluate the hive with.
    evaluation_method: EvaluationMethod,

    /// Path to the context directory.
    ///
    /// Normally this is directory containing the "hive.nix"
    /// or "flake.nix".
    context_dir: Option<PathBuf>,

    /// Static files required to evaluate a Hive configuration.
    assets: Assets,

    /// Whether to pass --show-trace in Nix commands.
    show_trace: bool,

    /// Whether to pass --impure in Nix commands.
    impure: bool,

    /// Options to pass as --option name value.
    nix_options: HashMap<String, String>,

    meta_config: OnceCell<MetaConfig>,
}

struct NixInstantiate<'hive> {
    hive: &'hive Hive,
    expression: String,
}

/// An expression to evaluate the system profiles of selected nodes.
struct EvalSelectedExpression<'hive> {
    hive: &'hive Hive,
    nodes_expr: SerializedNixExpression,
}

/// An expression to evaluate the deployment config of selected nodes in chunks.
struct EvalSelectedConfigChunksExpression<'hive> {
    hive: &'hive Hive,
    chunks: Vec<Vec<NodeName>>,
}

impl Hive {
    pub async fn new(path: HivePath) -> NaviResult<Self> {
        let context_dir = path.context_dir();
        // TODO: Skip asset extraction for direct flake eval
        let assets = Assets::new(path.clone()).await?;

        let evaluation_method = if path.is_flake() {
            EvaluationMethod::DirectFlakeEval
        } else {
            EvaluationMethod::NixInstantiate
        };

        Ok(Self {
            path,
            evaluation_method,
            context_dir,
            assets,
            show_trace: false,
            impure: false,
            nix_options: HashMap::new(),
            meta_config: OnceCell::new(),
        })
    }

    pub fn context_dir(&self) -> Option<&Path> {
        self.context_dir.as_ref().map(|p| p.as_ref())
    }

    pub async fn get_meta_config(&self) -> NaviResult<&MetaConfig> {
        self.meta_config
            .get_or_try_init(|| async {
                self.nix_instantiate("hive.metaConfig")
                    .eval()
                    .capture_json()
                    .await
            })
            .await
    }

    pub fn set_evaluation_method(&mut self, method: EvaluationMethod) {
        if !self.is_flake() && method == EvaluationMethod::DirectFlakeEval {
            return;
        }

        self.evaluation_method = method;
    }

    pub fn set_show_trace(&mut self, value: bool) {
        self.show_trace = value;
    }

    pub fn set_impure(&mut self, impure: bool) {
        self.impure = impure;
    }

    pub fn add_nix_option(&mut self, name: String, value: String) {
        self.nix_options.insert(name, value);
    }

    /// Returns Nix options to set for this Hive.
    pub fn nix_flags(&self) -> NixFlags {
        let mut flags = NixFlags::default();
        flags.set_show_trace(self.show_trace);
        flags.set_pure_eval(self.path.is_flake());
        flags.set_impure(self.impure);
        flags.set_options(self.nix_options.clone());
        flags
    }

    /// Returns Nix flags to set for this Hive, with configured remote builders.
    pub async fn nix_flags_with_builders(&self) -> NaviResult<NixFlags> {
        let mut flags = self.nix_flags();

        if let Some(machines_file) = &self.get_meta_config().await?.machines_file {
            flags.set_builders(Some(format!("@{}", machines_file)));
        }

        Ok(flags)
    }

    /// Convenience wrapper to filter nodes for CLI actions.
    pub async fn select_nodes(
        &self,
        filter: Option<NodeFilter>,
        ssh_config: Option<PathBuf>,
        ssh_only: bool,
    ) -> NaviResult<HashMap<NodeName, TargetNode>> {
        let mut node_configs = None;

        tracing::info!("Enumerating nodes...");

        let all_nodes = self.node_names().await?;
        let selected_nodes = match filter {
            Some(filter) => {
                if filter.has_node_config_rules() {
                    tracing::debug!("Retrieving deployment info for all nodes...");

                    let all_node_configs = self.deployment_info().await?;
                    let filtered = filter
                        .filter_node_configs(all_node_configs.iter())
                        .into_iter()
                        .collect();

                    node_configs = Some(all_node_configs);

                    filtered
                } else {
                    filter.filter_node_names(&all_nodes)?.into_iter().collect()
                }
            }
            None => all_nodes.clone(),
        };

        let n_selected = selected_nodes.len();

        let mut node_configs = if let Some(configs) = node_configs {
            configs
        } else {
            tracing::debug!("Retrieving deployment info for selected nodes...");
            self.deployment_info_selected(&selected_nodes).await?
        };

        // Pre-resolve which provisioner names are bare-metal type, so we can
        // mark their SSH hosts to strip ProxyCommand from nix copy invocations
        let bare_metal_provisioners: HashSet<&str> = self
            .get_meta_config()
            .await
            .ok()
            .and_then(|meta| meta.provisioners.as_ref())
            .map(|provs| {
                provs
                    .iter()
                    .filter(|(_, c)| c.kind == crate::nix::ProvisionerType::BareMetal)
                    .map(|(name, _)| name.as_str())
                    .collect()
            })
            .unwrap_or_default();

        if !bare_metal_provisioners.is_empty() {
            tracing::debug!("Bare-metal provisioners: {:?}", bare_metal_provisioners);
        }

        let mut targets = HashMap::new();
        let mut n_ssh = 0;
        for node in selected_nodes.into_iter() {
            let config = node_configs.remove(&node).unwrap();

            let host = config.to_ssh_host().map(|mut host| {
                n_ssh += 1;

                if let Some(ssh_config) = &ssh_config {
                    host.set_ssh_config(ssh_config.clone());
                }

                if self.is_flake() {
                    host.set_use_nix3_copy(true);
                }

                let is_bare_metal = config
                    .provisioner
                    .as_deref()
                    .map_or(false, |p| bare_metal_provisioners.contains(p));

                if is_bare_metal {
                    tracing::debug!(
                        "Marking {} as bare-metal (provisioner: {:?})",
                        node.as_str(),
                        config.provisioner,
                    );
                    host.set_is_bare_metal(true);
                }

                host.upcast()
            });
            let ssh_host = host.is_some();
            let target = TargetNode::new(node.clone(), host, config);

            if !ssh_only || ssh_host {
                targets.insert(node, target);
            }
        }

        let skipped = n_selected - n_ssh;

        if targets.is_empty() {
            if skipped != 0 {
                tracing::warn!("No hosts selected.");
            } else {
                tracing::warn!("No hosts selected ({} skipped).", skipped);
            }
        } else if targets.len() == all_nodes.len() {
            tracing::info!("Selected all {} nodes.", targets.len());
        } else if !ssh_only || skipped == 0 {
            tracing::info!(
                "Selected {} out of {} hosts.",
                targets.len(),
                all_nodes.len()
            );
        } else {
            tracing::info!(
                "Selected {} out of {} hosts ({} skipped).",
                targets.len(),
                all_nodes.len(),
                skipped
            );
        }

        Ok(targets)
    }

    /// Returns a list of all node names.
    pub async fn node_names(&self) -> NaviResult<Vec<NodeName>> {
        self.nix_instantiate("attrNames hive.nodes")
            .eval()
            .capture_json()
            .await
    }

    /// Retrieve deployment info for all nodes.
    pub async fn deployment_info(&self) -> NaviResult<HashMap<NodeName, NodeConfig>> {
        let configs: HashMap<NodeName, NodeConfig> = self
            .nix_instantiate("hive.deploymentConfig")
            .eval_with_builders()
            .await?
            .capture_json()
            .await?;

        for config in configs.values() {
            config.validate()?;
            for key in config.keys.values() {
                key.validate()?;
            }
        }
        Ok(configs)
    }

    /// Retrieve deployment info for a single node.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub async fn deployment_info_single(&self, node: &NodeName) -> NaviResult<Option<NodeConfig>> {
        let expr = expressions::deployment_config_single(node.as_str());
        self.nix_instantiate(&expr)
            .eval_with_builders()
            .await?
            .capture_json()
            .await
    }

    /// Retrieve deployment info for a list of nodes.
    pub async fn deployment_info_selected(
        &self,
        nodes: &[NodeName],
    ) -> NaviResult<HashMap<NodeName, NodeConfig>> {
        let nodes_expr = SerializedNixExpression::new(nodes);

        let configs: HashMap<NodeName, NodeConfig> = self
            .nix_instantiate(&expressions::deployment_config_selected(
                &nodes_expr.expression(),
            ))
            .eval_with_builders()
            .await?
            .capture_json()
            .await?;

        for config in configs.values() {
            config.validate()?;
            for key in config.keys.values() {
                key.validate()?;
            }
        }

        Ok(configs)
    }

    /// Evaluates selected nodes.
    ///
    /// Evaluation may take up a lot of memory, so we make it possible
    /// to split up the evaluation process into chunks and run them
    /// concurrently with other processes (e.g., build and apply).
    pub async fn eval_selected(
        &self,
        nodes: &[NodeName],
        job: Option<JobHandle>,
    ) -> NaviResult<HashMap<NodeName, ProfileDerivation>> {
        let nodes_expr = SerializedNixExpression::new(nodes);

        let expr = expressions::eval_selected_drv_paths(&nodes_expr.expression());

        let command = self.nix_instantiate(&expr).eval_with_builders().await?;
        let mut execution = CommandExecution::new(command);
        execution.set_job(job);
        execution.set_hide_stdout(true);

        execution
            .capture_json::<HashMap<NodeName, StorePath>>()
            .await?
            .into_iter()
            .map(|(name, path)| {
                let path = path.into_derivation()?;
                Ok((name, path))
            })
            .collect()
    }

    /// Returns the expression to evaluate selected nodes.
    pub fn eval_selected_expr(&self, nodes: &[NodeName]) -> NaviResult<impl NixExpression + '_> {
        let nodes_expr = SerializedNixExpression::new(nodes);

        Ok(EvalSelectedExpression {
            hive: self,
            nodes_expr,
        })
    }

    /// Returns the expression to evaluate chunks of configs.
    pub fn eval_selected_config_chunks_expr(
        &self,
        chunks: Vec<Vec<NodeName>>,
    ) -> NaviResult<impl NixExpression + '_> {
        Ok(EvalSelectedConfigChunksExpression { hive: self, chunks })
    }

    /// Evaluates an expression using values from the configuration.
    pub async fn introspect(&self, expression: String, instantiate: bool) -> NaviResult<String> {
        if instantiate {
            let expression = expressions::introspect(&expression);
            self.nix_instantiate(&expression)
                .instantiate_with_builders()
                .await?
                .capture_output()
                .await
        } else {
            let expression = expressions::introspect_json(&expression);
            self.nix_instantiate(&expression)
                .eval_with_builders()
                .await?
                .capture_json()
                .await
        }
    }

    /// Returns the expression for a REPL session.
    pub fn get_repl_expression(&self) -> String {
        if self.is_flake() {
            let flake_uri = if let HivePath::Flake(flake) = self.path() {
                if let Some(dir) = flake.local_dir() {
                    dir.to_string_lossy().to_string()
                } else {
                    flake.uri().to_string()
                }
            } else {
                panic!("Hive thought it was a flake but path says otherwise");
            };

            return expressions::repl_flake(&flake_uri, &self.get_base_expression());
        }

        expressions::repl_legacy(&self.get_base_expression())
    }

    /// Returns the base expression from which the evaluated Hive can be used.
    fn get_base_expression(&self) -> String {
        match self.evaluation_method {
            EvaluationMethod::NixInstantiate => self.assets.get_base_expression(),
            EvaluationMethod::DirectFlakeEval => expressions::FLAKE_APPLY_SNIPPET.to_string(),
        }
    }

    /// Returns whether this Hive is a flake.
    fn is_flake(&self) -> bool {
        matches!(self.path(), HivePath::Flake(_))
    }

    fn nix_instantiate(&self, expression: &str) -> NixInstantiate {
        NixInstantiate::new(self, expression.to_owned())
    }

    pub fn path(&self) -> &HivePath {
        &self.path
    }
}

impl<'hive> NixInstantiate<'hive> {
    fn new(hive: &'hive Hive, expression: String) -> Self {
        Self { hive, expression }
    }

    fn instantiate(&self) -> Command {
        // TODO: Better error handling
        if self.hive.evaluation_method == EvaluationMethod::DirectFlakeEval {
            panic!("Instantiation is not supported with DirectFlakeEval");
        }

        let mut command = Command::new("nix-instantiate");

        if self.hive.is_flake() {
            command.args(["--extra-experimental-features", "flakes"]);
        }

        let mut full_expression = self.hive.get_base_expression();
        full_expression += &self.expression;

        command
            .arg("--no-gc-warning")
            .arg("-E")
            .arg(&full_expression);

        command
    }

    fn eval(self) -> Command {
        let flags = self.hive.nix_flags();

        match self.hive.evaluation_method {
            EvaluationMethod::NixInstantiate => {
                let mut command = self.instantiate();

                command
                    .arg("--eval")
                    .arg("--json")
                    .arg("--strict")
                    // Ensures the derivations are instantiated
                    // Required for system profile evaluation and IFD
                    .arg("--read-write-mode")
                    .args(flags.to_args());

                command
            }
            EvaluationMethod::DirectFlakeEval => {
                let mut command = Command::new("nix");
                let flake = if let HivePath::Flake(flake) = self.hive.path() {
                    flake
                } else {
                    panic!("The DirectFlakeEval evaluation method only support flakes");
                };

                let hive_installable = format!("{}#naviHive", flake.uri());

                let mut full_expression = self.hive.get_base_expression();
                full_expression += &self.expression;

                command
                    .arg("eval") // nix eval
                    .args(["--extra-experimental-features", "flakes nix-command"])
                    .arg(hive_installable)
                    .arg("--json")
                    .arg("--apply")
                    .arg(&full_expression)
                    .args(flags.to_args());

                command
            }
        }
    }

    async fn instantiate_with_builders(self) -> NaviResult<Command> {
        let flags = self.hive.nix_flags_with_builders().await?;
        let mut command = self.instantiate();

        command.args(flags.to_args());

        Ok(command)
    }

    async fn eval_with_builders(self) -> NaviResult<Command> {
        let flags = self.hive.nix_flags_with_builders().await?;
        let mut command = self.eval();

        command.args(flags.to_args());

        Ok(command)
    }
}

impl<'hive> NixExpression for EvalSelectedExpression<'hive> {
    fn expression(&self) -> String {
        expressions::eval_selected(
            &self.hive.get_base_expression(),
            &self.nodes_expr.expression(),
        )
    }

    fn requires_flakes(&self) -> bool {
        self.hive.is_flake()
    }
}

impl<'hive> NixExpression for EvalSelectedConfigChunksExpression<'hive> {
    fn expression(&self) -> String {
        if self.hive.is_flake() {
            let flake_uri = if let HivePath::Flake(flake) = self.hive.path() {
                if let Some(dir) = flake.local_dir() {
                    dir.to_string_lossy().to_string()
                } else {
                    flake.uri().to_string()
                }
            } else {
                panic!("Hive thought it was a flake but path says otherwise");
            };

            expressions::build_config_chunks(
                &self.hive.get_base_expression(),
                Some(&flake_uri),
                true,
                &self.chunks,
            )
        } else {
            expressions::build_config_chunks(
                &self.hive.get_base_expression(),
                None,
                false,
                &self.chunks,
            )
        }
    }

    fn requires_flakes(&self) -> bool {
        self.hive.is_flake()
    }
}
