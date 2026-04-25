use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use clap::Args;
use tokio::process::Command;

use crate::error::{NaviError, NaviResult};
use crate::nix::{
    deployment::TargetNode, hive::Hive, node_filter::NodeFilterOpts, MetaConfig, NodeName,
    ProvisionerConfig, ProvisionerType,
};
use crate::terraform::TerraformExecutor;
use crate::util::{CommandExt, CommandExecution, confirm_action};

#[derive(Debug, Args)]
#[command(name = "provision", about = "Provision infrastructure for nodes")]
pub struct Opts {
    /// List available provisioners
    #[arg(long)]
    pub list: bool,

    /// Explicitly select a provisioner to run
    #[arg(conflicts_with = "on")]
    pub provisioner: Option<String>,

    /// Destroy and recreate the infrastructure
    #[arg(long)]
    pub reprovision: bool,

    /// Unlock the disk after deployment
    #[arg(long)]
    pub unlock: bool,

    /// Update the local Terraform lock file from the sandbox state
    #[arg(long, num_args(0..=1), default_missing_value = ".")]
    pub update_tf_lock: Option<String>,

    /// Skip the OS installation step (nixos-anywhere), only provision infrastructure and update facts
    #[arg(long)]
    pub skip_install: bool,

    /// IP address for bare-metal provisioning (skips interactive prompt)
    #[arg(long)]
    pub ip: Option<String>,

    #[command(flatten)]
    pub node_filter: NodeFilterOpts,
}


pub async fn run(hive: Hive, opts: Opts) -> NaviResult<()> {
    let meta = hive.get_meta_config().await?;

    // Derive facts if configured
    if !meta.facts.derive.is_empty() {
        tracing::info!("Deriving {} facts before provisioning...", meta.facts.derive.len());
        crate::command::facts::derive(&hive, meta.facts.derive.clone()).await?;
    }

    if opts.list {
        list_provisioners(meta.provisioners.as_ref());
        return Ok(());
    }

    let targets = hive
        .select_nodes(opts.node_filter.on.clone(), None, false)
        .await?;

    let names_to_run = resolve_provisioners_to_run(&opts, &targets)?;

    let all_provisioners =
        meta.provisioners
            .as_ref()
            .ok_or_else(|| NaviError::DeploymentError {
                message: "No provisioners defined in meta.provisioners".to_string(),
            })?;

    for name in names_to_run {
        let config = all_provisioners
            .get(&name)
            .ok_or_else(|| NaviError::DeploymentError {
                message: format!("Provisioner '{}' not found in config", name),
            })?;

        // Derive provisioner-specific facts
        if !config.derive.is_empty() {
            tracing::info!("Deriving {} facts for provisioner '{}'...", config.derive.len(), name);
            crate::command::facts::derive(&hive, config.derive.clone()).await?;
        }

        tracing::info!("Running provisioner: {}", name);

        run_provisioner(&hive, &name, config, &targets, &opts, &meta).await?;
    }

    Ok(())
}

fn list_provisioners(provisioners: Option<&HashMap<String, ProvisionerConfig>>) {
    if let Some(provisioners) = provisioners {
        println!("Available provisioners:");
        for name in provisioners.keys() {
            println!("- {}", name);
        }
    } else {
        println!("No provisioners defined in meta.provisioners");
    }
}

fn resolve_provisioners_to_run(
    opts: &Opts,
    targets: &HashMap<NodeName, TargetNode>,
) -> NaviResult<HashSet<String>> {
    if let Some(name) = &opts.provisioner {
        let mut s = HashSet::new();
        s.insert(name.clone());
        Ok(s)
    } else {
        let mut s = HashSet::new();
        for (name, target) in targets {
            if let Some(p) = &target.config.provisioner {
                s.insert(p.clone());
            } else {
                tracing::debug!("Node {} has no provisioner configured", name.as_str());
            }
        }

        if s.is_empty() {
            eprintln!("No provisioners found for the selected nodes.");
            eprintln!(
                "Hint: You haven't assigned a provisioner to any node in your hive.nix/flake.nix."
            );
            eprintln!("      You can run a specific provisioner using: navi provision --provisioner <name>");
            eprintln!("      Available provisioners can be listed with: navi provision --list");
            tracing::warn!("No provisioners found for the selected nodes.");
        }
        Ok(s)
    }
}

async fn run_provisioner(
    hive: &Hive,
    name: &str,
    config: &ProvisionerConfig,
    targets: &HashMap<NodeName, TargetNode>,
    opts: &Opts,
    meta: &MetaConfig,
) -> NaviResult<()> {
    match config.kind {
        ProvisionerType::Command => run_command_provisioner(config).await,
        ProvisionerType::FlakeApp => run_flake_app_provisioner(config).await,
        ProvisionerType::Terranix => {
            run_terranix_provisioner(hive, name, config, targets, opts, meta).await
        }
        ProvisionerType::BareMetal => {
            run_bare_metal_provisioner(hive, name, config, targets, opts, meta).await
        }
    }
}

async fn run_command_provisioner(config: &ProvisionerConfig) -> NaviResult<()> {
    let cmd_str = config.command.as_ref().ok_or(NaviError::DeploymentError {
        message: "Provisioner type is 'command' but no command specified".to_string(),
    })?;

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(cmd_str);
    cmd.passthrough().await?;
    Ok(())
}

async fn run_flake_app_provisioner(config: &ProvisionerConfig) -> NaviResult<()> {
    let app_name = config.app.as_ref().ok_or(NaviError::DeploymentError {
        message: "Provisioner type is 'flake-app' but no app specified".to_string(),
    })?;

    tracing::info!(
        "Filesystem flake app execution not yet fully implemented, running `nix run`..."
    );

    let mut cmd = Command::new("nix");
    cmd.arg("run").arg(format!(".#{}", app_name));
    cmd.passthrough().await?;
    Ok(())
}

async fn run_terranix_provisioner(
    hive: &Hive,
    name: &str,
    config: &ProvisionerConfig,
    targets: &HashMap<NodeName, TargetNode>,
    opts: &Opts,
    meta: &MetaConfig,
) -> NaviResult<()> {
    let config_drv_path = config
        .configuration
        .as_ref()
        .ok_or(NaviError::DeploymentError {
            message: "Provisioner type is 'terranix' but no configuration specified".to_string(),
        })?;

    tracing::info!("Ensuring configuration is built: {}", config_drv_path);

    // Ensure the config is realized
    let mut build_cmd = Command::new("nix-store");
    build_cmd.arg("--realize").arg(config_drv_path);

    let config_out_path =
        build_cmd
            .capture_store_path()
            .await
            .map_err(|e| NaviError::DeploymentError {
                message: format!("Failed to realize Terranix configuration: {}", e),
            })?;

    // Prepare workspace
    let work_dir = std::path::Path::new(".navi").join("provision").join(name);
    std::fs::create_dir_all(&work_dir).map_err(|e| NaviError::IoContext {
        error: e,
        context: format!("creating directory {:?}", work_dir),
    })?;

    // Symlink config.tf.json
    let link_path = work_dir.join("config.tf.json");
    if link_path.is_symlink() || link_path.exists() {
        std::fs::remove_file(&link_path).map_err(|e| NaviError::IoContext {
            error: e,
            context: format!("removing existing file {:?}", link_path),
        })?;
    }
    std::os::unix::fs::symlink(config_out_path.as_path(), &link_path).map_err(|e| {
        NaviError::IoContext {
            error: e,
            context: format!("symlinking {:?} -> {:?}", config_out_path, link_path),
        }
    })?;

    tracing::info!("Workspace prepared at {:?}", work_dir);

    // Inject provider configuration if missing (e.g. for Porkbun forks)
    if let Some(registrants) = &meta.registrants {
        inject_registrant_providers(&work_dir, registrants).await?;
    }

    // Initialize Terraform
    let mut executor = TerraformExecutor::new(work_dir.clone());

    // Sync Lockfile to Sandbox (Inject)
    if let Some(target_path_str) = &opts.update_tf_lock {
        let mut source_path = PathBuf::from(target_path_str);
        if source_path.is_dir() {
            source_path.push(".terraform.lock.hcl");
        }

        if source_path.exists() {
            tracing::info!(
                "Seeding sandbox with existing lockfile from {:?}",
                source_path
            );
            let sandbox_lock = work_dir.join(".terraform.lock.hcl");
            if let Err(e) = std::fs::copy(&source_path, &sandbox_lock) {
                tracing::warn!("Failed to seed sandbox with lockfile: {}", e);
            }
        }
    }

    executor.init().await?;

    // Sync Lockfile Back to Repo (Extract)
    if let Some(target_path_str) = &opts.update_tf_lock {
        tracing::info!("Syncing Terraform lock file back to repo...");
        let source_lock = work_dir.join(".terraform.lock.hcl");

        if source_lock.exists() {
            let mut target_path = PathBuf::from(target_path_str);
            if target_path.is_dir() {
                target_path.push(".terraform.lock.hcl");
            }

            match std::fs::copy(&source_lock, &target_path) {
                Ok(_) => tracing::info!("Successfully updated lock file at {:?}", target_path),
                Err(e) => tracing::error!("Failed to update lock file at {:?}: {}", target_path, e),
            }
        } else {
            tracing::warn!("No .terraform.lock.hcl generated in sandbox. Nothing to update.");
        }
    }

    if opts.reprovision {
        handle_reprovision(&mut executor, name).await?;
    }

    // Collect variables from registrants if enabled
    let mut extra_vars = HashMap::new();
    if let Some(registrants) = &meta.registrants {
        // Read config to check declared variables
        let defined_vars = get_defined_variables(&work_dir).await?;

        // Collect Porkbun vars
        for (account_name, account) in &registrants.porkbun {
            if account.terraform_secrets {
                if !defined_vars.contains(&account.api_key_variable) {
                    tracing::debug!("Skipping Porkbun api_key for account '{}' (variable '{}' not defined)", account_name, account.api_key_variable);
                } else {
                    tracing::info!(
                        "Fetching credentials for Porkbun account '{}'...",
                        account_name
                    );

                    let api_key = fetch_credential_output(&account.api_key_command).await?;
                    // Check for collision
                    if extra_vars.contains_key(&account.api_key_variable) {
                        tracing::warn!(
                            "Duplicate Terraform variable '{}' from Porkbun account '{}'. Overwriting.",
                            account.api_key_variable,
                            account_name
                        );
                    }
                    extra_vars.insert(account.api_key_variable.clone(), api_key);
                }

                if !defined_vars.contains(&account.secret_key_variable) {
                     tracing::debug!("Skipping Porkbun secret_key for account '{}' (variable '{}' not defined)", account_name, account.secret_key_variable);
                } else {
                    let secret_key = fetch_credential_output(&account.secret_api_key_command).await?;
                     if extra_vars.contains_key(&account.secret_key_variable) {
                        tracing::warn!(
                            "Duplicate Terraform variable '{}' from Porkbun account '{}'. Overwriting.",
                            account.secret_key_variable,
                            account_name
                        );
                    }
                    extra_vars.insert(account.secret_key_variable.clone(), secret_key);
                }
            }
        }
    }

    // Plan and Apply
    executor.plan_and_apply(targets, &extra_vars).await?;

    // Capture Facts
    if meta.facts.enable {
        tracing::info!("Capturing Terraform outputs to facts...");
        capture_facts(executor.tf_bin(), &work_dir, &meta.facts.dir_name, name).await?;
    }

    // Handle NixOS Anywhere
    if !opts.skip_install {
        if let Some(na_config) = &config.nixos_anywhere {
            if na_config.enable {
                tracing::info!("Running nixos-anywhere...");

                // We need the outputs locally
                let mut output_cmd = Command::new(executor.tf_bin());
                output_cmd.current_dir(&work_dir).arg("output").arg("-json");
                let output_json: serde_json::Value = output_cmd.capture_json().await?;

                // Determine relevant nodes
                let relevant_nodes: Vec<&str> = targets
                    .iter()
                    .filter(|(_, target)| target.config.provisioner.as_deref() == Some(name))
                    .map(|(n, _)| n.as_str())
                    .collect();

                crate::nix::nixos_anywhere::run(
                    hive,
                    targets,
                    na_config,
                    &output_json,
                    opts.unlock,
                    Some(relevant_nodes),
                )
                .await?;
            }
        }
    } else {
        tracing::info!("Skipping install step as requested.");
    }

    Ok(())
}

/// Bare-metal provisioner: resolves node IPs interactively or via `--ip`,
/// writes them as facts, then optionally runs nixos-anywhere.
async fn run_bare_metal_provisioner(
    hive: &Hive,
    name: &str,
    config: &ProvisionerConfig,
    targets: &HashMap<NodeName, TargetNode>,
    opts: &Opts,
    meta: &MetaConfig,
) -> NaviResult<()> {
    let facts_dir = Path::new(&meta.facts.dir_name).join(name);

    // Load existing facts if present
    let mut existing_outputs: serde_json::Value = if facts_dir.join("outputs.json").exists() {
        let content = tokio::fs::read_to_string(facts_dir.join("outputs.json"))
            .await
            .map_err(|e| NaviError::IoContext {
                error: e,
                context: format!("reading existing facts from {:?}", facts_dir),
            })?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Determine relevant nodes for this provisioner
    let relevant_nodes: Vec<(&NodeName, &TargetNode)> = targets
        .iter()
        .filter(|(_, target)| target.config.provisioner.as_deref() == Some(name))
        .collect();

    if relevant_nodes.is_empty() {
        tracing::warn!("No nodes assigned to bare-metal provisioner '{}'.", name);
        return Ok(());
    }

    // Resolve IP for each node
    for (node_name, _target) in &relevant_nodes {
        let ip_key = format!("{}_ip", node_name.as_str().replace('-', "_"));

        // Check if IP already exists in facts
        let existing_ip = existing_outputs
            .get(&ip_key)
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let ip = if opts.reprovision || existing_ip.is_none() {
            // Need to get the IP: from --ip flag or interactively
            if let Some(ip) = &opts.ip {
                tracing::info!(
                    "[bare-metal] Using provided IP for {}: {}",
                    node_name.as_str(),
                    ip
                );
                ip.clone()
            } else if let Some(existing) = &existing_ip {
                if !opts.reprovision {
                    tracing::info!(
                        "[bare-metal] Using existing IP for {}: {}",
                        node_name.as_str(),
                        existing
                    );
                    existing.clone()
                } else {
                    // Reprovision: prompt with existing as hint
                    prompt_for_ip(node_name.as_str(), Some(existing))?
                }
            } else {
                // No existing IP, no flag: prompt
                prompt_for_ip(node_name.as_str(), None)?
            }
        } else {
            let ip = existing_ip.unwrap();
            tracing::info!(
                "[bare-metal] Using existing IP for {}: {}",
                node_name.as_str(),
                ip
            );
            ip
        };

        // Write/update the fact for this node
        existing_outputs[&ip_key] = serde_json::json!({
            "sensitive": false,
            "type": "string",
            "value": ip
        });
    }

    // Persist facts
    if meta.facts.enable {
        write_bare_metal_facts(&facts_dir, &existing_outputs)?;
        tracing::info!("Facts saved to {:?}", facts_dir);
    }

    // Handle NixOS Anywhere
    if !opts.skip_install {
        if let Some(na_config) = &config.nixos_anywhere {
            if na_config.enable {
                tracing::info!("Running nixos-anywhere for bare-metal nodes...");

                let node_names: Vec<&str> = relevant_nodes
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect();

                crate::nix::nixos_anywhere::run(
                    hive,
                    targets,
                    na_config,
                    &existing_outputs,
                    opts.unlock,
                    Some(node_names),
                )
                .await?;
            }
        }
    } else {
        tracing::info!("Skipping install step as requested.");
    }

    Ok(())
}

/// Prompts the user for an IP address interactively.
fn prompt_for_ip(node_name: &str, existing: Option<&str>) -> NaviResult<String> {
    if let Some(existing) = existing {
        eprint!("Enter IP address for {} [{}]: ", node_name, existing);
    } else {
        eprint!("Enter IP address for {}: ", node_name);
    }

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| NaviError::IoError { error: e })?;

    let trimmed = input.trim();
    if trimmed.is_empty() {
        if let Some(existing) = existing {
            Ok(existing.to_string())
        } else {
            Err(NaviError::DeploymentError {
                message: format!("No IP address provided for node '{}'", node_name),
            })
        }
    } else {
        Ok(trimmed.to_string())
    }
}

/// Writes bare-metal facts (IP mappings) in the same format as Terraform outputs.
pub fn write_bare_metal_facts(
    facts_dir: &Path,
    outputs: &serde_json::Value,
) -> NaviResult<()> {
    // Create directory
    std::fs::create_dir_all(facts_dir).map_err(|e| NaviError::IoContext {
        error: e,
        context: format!("creating facts directory {:?}", facts_dir),
    })?;

    // Write outputs.json
    let json_path = facts_dir.join("outputs.json");
    let json_content =
        serde_json::to_string_pretty(outputs).expect("Failed to serialize outputs");
    std::fs::write(&json_path, json_content).map_err(|e| NaviError::IoContext {
        error: e,
        context: format!("writing facts to {:?}", json_path),
    })?;

    // Write default.nix (same format as Terraform facts)
    let nix_path = facts_dir.join("default.nix");
    let nix_content = r#"let
  raw = builtins.fromJSON (builtins.readFile ./outputs.json);
in
  builtins.mapAttrs (n: v: v.value) raw
"#;
    std::fs::write(&nix_path, nix_content).map_err(|e| NaviError::IoContext {
        error: e,
        context: format!("writing nix facts to {:?}", nix_path),
    })?;

    Ok(())
}

async fn inject_registrant_providers(
    work_dir: &Path,
    registrants: &crate::nix::RegistrantsConfig,
) -> NaviResult<()> {
    // We only handle Porkbun for now as it's the one with known forks requiring explicit config
    if registrants.porkbun.is_empty() {
        return Ok(());
    }

    // Use the first available account to determine variable names
    // (Navi currently overwrites variables if multiple accounts exist, so this matches existing logic)
    let account = registrants.porkbun.values().next().unwrap();
    let api_var = &account.api_key_variable;
    let secret_var = &account.secret_key_variable;

    let config_path = work_dir.join("config.tf.json");
    if !config_path.exists() {
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|e| NaviError::IoContext {
            error: e,
            context: format!("reading config file {:?}", config_path),
        })?;

    let mut json: serde_json::Value =
        serde_json::from_str(&content).map_err(|_| NaviError::DeploymentError {
            message: "Invalid config.tf.json".into(),
        })?;

    let mut modified = false;
    let mut aliases_to_inject = Vec::new();

    // Detect if we have a Porkbun provider in required_providers
    if let Some(req) = json
        .get("terraform")
        .and_then(|t| t.get("required_providers"))
        .and_then(|r| r.as_object())
    {
        for (alias, val) in req {
            // Check if this is a porkbun provider
            let is_porkbun = if let Some(src) = val.as_str() {
                src.ends_with("/porkbun") || alias.contains("porkbun")
            } else if let Some(src) = val.get("source").and_then(|s| s.as_str()) {
                src.ends_with("/porkbun") || alias.contains("porkbun")
            } else {
                false
            };

            if is_porkbun {
                aliases_to_inject.push(alias.clone());
            }
        }
    }

    for alias in aliases_to_inject {
        // Check if a provider block exists for this alias
        let has_config = json.get("provider").and_then(|p| p.get(&alias)).is_some();

        if !has_config {
            tracing::info!(
                "Injecting missing provider configuration for '{}'...",
                alias
            );

            // Create configuration block
            let config = serde_json::json!({
                "api_key": format!("${{var.{}}}", api_var),
                "secret_api_key": format!("${{var.{}}}", secret_var)
            });

            // Ensure "provider" object exists
            if !json.get("provider").is_some() {
                json["provider"] = serde_json::json!({});
            }

            if let Some(providers) = json.get_mut("provider").and_then(|p| p.as_object_mut()) {
                providers.insert(alias.clone(), config);
                modified = true;
            }
        }
    }

    if modified {
        // If config.tf.json is a symlink (it is), we must remove it before writing
        if config_path.is_symlink() || config_path.exists() {
            std::fs::remove_file(&config_path).map_err(|e| NaviError::IoContext {
                error: e,
                context: format!("removing config file {:?}", config_path),
            })?;
        }

        let new_content =
            serde_json::to_string_pretty(&json).expect("Failed to serialize modified config");
        std::fs::write(&config_path, new_content).map_err(|e| NaviError::IoContext {
            error: e,
            context: format!("writing config file {:?}", config_path),
        })?;
    }

    Ok(())
}

async fn handle_reprovision(executor: &mut TerraformExecutor, name: &str) -> NaviResult<()> {
    eprintln!("\n\u{001b}[33mWARNING: You have requested to REPROVISION.\u{001b}[0m");
    eprintln!(
        "This will DESTROY all resources managed by provisioner '{}' and then recreate them.",
        name
    );

    if !confirm_action("Are you sure you want to continue?")? {
        eprintln!("Cancelled reprovisioning.");
        return Ok(());
    }

    if let Err(e) = executor.destroy().await {
        eprintln!("\n\u{001b}[33mWARNING: Terraform destroy failed.\u{001b}[0m");
        eprintln!("This often occurs if resources have 'deletion_protection' enabled (e.g. Databases).");
        eprintln!("Specific error: {}", e);
        eprintln!("\nHowever, dependent resources (like VMs) are likely already destroyed.");
        
        if confirm_action("Do you want to ignore this error and proceed with Apply (recreation)?")? {
            tracing::info!("Proceeding with Apply/Recreation despite destroy failures...");
            return Ok(());
        }
        return Err(e);
    }
    Ok(())
}

async fn fetch_credential_output(command: &str) -> NaviResult<String> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    let output = cmd.capture_output().await?;
    Ok(output.trim().to_string())
}

async fn capture_facts(
    tf_bin: &str,
    work_dir: &Path,
    facts_dir_name: &str,
    provisioner_name: &str,
) -> NaviResult<()> {
    // Run terraform output -json
    let mut cmd = Command::new(tf_bin);
    cmd.current_dir(work_dir).arg("output").arg("-json");
    let output_json: serde_json::Value = cmd.capture_json().await?;

    // Determine target directory: <current_dir>/<facts_dir_name>/<provisioner_name>
    let target_dir = Path::new(facts_dir_name).join(provisioner_name);

    // Create directory
    std::fs::create_dir_all(&target_dir).map_err(|e| NaviError::IoContext {
        error: e,
        context: format!("creating facts directory {:?}", target_dir),
    })?;

    // Write outputs.json
    let json_path = target_dir.join("outputs.json");
    let json_content =
        serde_json::to_string_pretty(&output_json).expect("Failed to serialize outputs");
    std::fs::write(&json_path, json_content).map_err(|e| NaviError::IoContext {
        error: e,
        context: format!("writing facts to {:?}", json_path),
    })?;

    // Write default.nix
    let nix_path = target_dir.join("default.nix");
    let nix_content = r#"let
  raw = builtins.fromJSON (builtins.readFile ./outputs.json);
in
  builtins.mapAttrs (n: v: v.value) raw
"#;
    std::fs::write(&nix_path, nix_content).map_err(|e| NaviError::IoContext {
        error: e,
        context: format!("writing nix facts to {:?}", nix_path),
    })?;

    tracing::info!("Facts saved to {:?}", target_dir);

    Ok(())
}

async fn get_defined_variables(work_dir: &Path) -> NaviResult<HashSet<String>> {
    let config_path = work_dir.join("config.tf.json");
    if !config_path.exists() {
        return Ok(HashSet::new());
    }

    let content = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|e| NaviError::IoContext {
            error: e,
            context: format!("reading config file {:?}", config_path),
        })?;

    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|_| NaviError::DeploymentError {
            message: "Invalid config.tf.json".into(),
        })?;

    let mut defined = HashSet::new();

    // Check "variable" block
    if let Some(vars) = json.get("variable").and_then(|v| v.as_object()) {
        for key in vars.keys() {
            defined.insert(key.clone());
        }
    }

    Ok(defined)
}
