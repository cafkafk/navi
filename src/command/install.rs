use std::collections::HashMap;
use std::path::Path;

use clap::Args;
use tokio::process::Command;

use crate::error::{NaviError, NaviResult};
use crate::nix::{
    deployment::TargetNode, hive::Hive, node_filter::NodeFilterOpts, NodeName, Ssh,
};
use crate::terraform::TerraformExecutor;
use crate::util::{CommandExt, confirm_action};

#[derive(Debug, Args)]
#[command(name = "install", about = "Install NixOS on target nodes using nixos-anywhere")]
pub struct Opts {
    #[command(flatten)]
    pub node_filter: NodeFilterOpts,

    /// Explicitly select a provisioner to run
    #[arg(long, conflicts_with = "on")]
    pub provisioner: Option<String>,

    /// Force installation even if provenance file exists
    #[arg(long)]
    pub force: bool,

    /// Unlock the disk after installation
    #[arg(long)]
    pub unlock: bool,

    /// List selected nodes without installing
    #[arg(long)]
    pub list: bool,

    /// Destroy and recreate the specific VM before installing
    #[arg(long, alias = "reinstall")]
    pub reinstall: bool,
}

pub async fn run(hive: Hive, opts: Opts) -> NaviResult<()> {
    let meta = hive.get_meta_config().await?;

    // 1. Resolve Targets
    let mut targets = hive
        .select_nodes(opts.node_filter.on.clone(), None, false)
        .await?;

    if let Some(prov_name) = &opts.provisioner {
        targets.retain(|_, target| target.config.provisioner.as_deref() == Some(prov_name));

        if targets.is_empty() {
            tracing::warn!("No nodes found for provisioner '{}'", prov_name);
            return Ok(());
        }
    }

    if opts.list {
        if targets.is_empty() {
            println!("No nodes selected.");
        } else {
            println!("Selected nodes for installation:");
            let mut names: Vec<_> = targets.keys().map(|n| n.as_str()).collect();
            names.sort();
            for name in names {
                println!("- {}", name);
            }
        }
        return Ok(());
    }

    if targets.is_empty() {
        tracing::warn!("No nodes selected for installation.");
        return Ok(());
    }

    // 2. Group Nodes by Provisioner
    // We need to know which provisioner manages which node to find the correct Terraform workspace/outputs.
    // If a node has no provisioner, we can't easily find its IP (unless we add a manual verify later),
    // so we skip it or warn.
    let mut provisioner_groups: HashMap<String, Vec<&str>> = HashMap::new();
    let mut no_provisioner_nodes: Vec<&str> = Vec::new();

    for (name, target) in &targets {
        if let Some(prov) = &target.config.provisioner {
            provisioner_groups
                .entry(prov.clone())
                .or_default()
                .push(name.as_str());
        } else {
            no_provisioner_nodes.push(name.as_str());
        }
    }

    if !no_provisioner_nodes.is_empty() {
        tracing::warn!(
            "The following nodes have no provisioner configured and will be skipped: {:?}",
            no_provisioner_nodes
        );
    }

    let all_provisioners = meta.provisioners.as_ref().ok_or_else(|| NaviError::DeploymentError {
        message: "No provisioners defined in meta.provisioners".to_string(),
    })?;

    // 3. Process each group
    for (prov_name, nodes) in provisioner_groups {
        let prov_config = all_provisioners.get(&prov_name).ok_or_else(|| {
            NaviError::DeploymentError {
                message: format!("Provisioner '{}' not found in config", prov_name),
            }
        })?;

        // Only Terranix and BareMetal provisioners support installation
        match prov_config.kind {
            crate::nix::ProvisionerType::Terranix | crate::nix::ProvisionerType::BareMetal => {}
            _ => {
                tracing::info!(
                    "Skipping provisioner '{}' (type {:?}) — installation not supported.",
                    prov_name,
                    prov_config.kind
                );
                continue;
            }
        }

        // Check if nixos-anywhere is enabled for this provisioner
        let na_config = if let Some(c) = &prov_config.nixos_anywhere {
            if !c.enable {
                tracing::info!(
                    "nixos-anywhere is disabled for provisioner '{}'. Skipping.",
                    prov_name
                );
                continue;
            }
            c
        } else {
            tracing::info!(
                "nixos-anywhere is not configured for provisioner '{}'. Skipping.",
                prov_name
            );
            continue;
        };

        tracing::info!(
            "Processing group for provisioner '{}': {:?}",
            prov_name,
            nodes
        );

        let facts_dir = Path::new(&meta.facts.dir_name).join(&prov_name);
        
        // 3a. Handle Reinstall Logic (Infrastructure Recreation) — Terranix only
        if opts.reinstall && prov_config.kind == crate::nix::ProvisionerType::Terranix {
            tracing::info!("Reinstall requested. Checking for resources to destroy and recreate...");
            
            // Reconstruct the workspace path
            let work_dir = Path::new(".navi").join("provision").join(&prov_name);
            if work_dir.exists() {
                let executor = TerraformExecutor::new(work_dir.clone());
                
                let mut changes_made = false;
                for node_name in &nodes {
                    if let Ok(Some(addr)) = executor.find_resource_address_for_node(node_name).await {
                        tracing::info!("Found resource for node {}: {}", node_name, addr);
                        tracing::warn!("RECREATING infrastructure for node {}", node_name);
                        
                        eprintln!("\n[33mTargeting Terraform resource: {}[0m", addr);
                        if confirm_action(&format!("Are you sure you want to destroy and recreate this resource for node '{}'?", node_name))? {
                            match executor.replace_resource(&addr).await {
                                Ok(_) => {
                                    tracing::info!("Successfully recreated infrastructure for {}", node_name);
                                    changes_made = true;
                                }
                                Err(e) => {
                                    tracing::error!("Failed to recreate infrastructure for {}: {}", node_name, e);
                                    return Err(e);
                                }
                            }
                        } else {
                            tracing::info!("Skipping recreation of {} by user request.", node_name);
                        }
                    } else {
                        tracing::warn!("Could not find Terraform resource for node {}. Skipping recreation step.", node_name);
                    }
                }

                if changes_made {
                    // Update outputs.json because IPs changed
                    tracing::info!("Infrastructure changed. Refreshing facts/outputs...");
                    capture_facts(executor.tf_bin(), &work_dir, &meta.facts.dir_name, &prov_name).await?;
                }
            } else {
                tracing::warn!("Provisioner workspace not found at {:?}. Cannot perform reinstall actions.", work_dir);
            }
        } else if opts.reinstall && prov_config.kind == crate::nix::ProvisionerType::BareMetal {
            tracing::info!("Reinstall requested for bare-metal provisioner '{}'. No infrastructure to recreate.", prov_name);
        }

        // 4. Load Outputs (Cache or Live)
        // We do this AFTER the potentially reinstall logic, so we have the new values.
        let outputs_json = load_outputs(&facts_dir, &prov_name).await?;

        // 5. Filter Nodes by Provenance
        let mut final_nodes: Vec<&str> = Vec::new();
        for node_name in nodes {
            // If reinstall or force is used, skip provenance check
            if opts.force || opts.reinstall {
                final_nodes.push(node_name);
                continue;
            }

            if should_install_node(node_name, &outputs_json, &targets).await? {
                final_nodes.push(node_name);
            } else {
                tracing::info!(
                    "Node '{}' already appears to be installed (provenance found). Use --force or --reinstall to bypass.",
                    node_name
                );
            }
        }

        if final_nodes.is_empty() {
            tracing::info!("No nodes to install for provisioner '{}'.", prov_name);
            continue;
        }

        // 6. Run NixOS Anywhere
        crate::nix::nixos_anywhere::run(
            &hive,
            &targets,
            na_config,
            &outputs_json,
            opts.unlock,
            Some(final_nodes),
        )
        .await?;
    }

    Ok(())
}

/// Loads outputs.json from facts dir, or falls back to `terraform output -json` in the workspace.
async fn load_outputs(facts_dir: &Path, prov_name: &str) -> NaviResult<serde_json::Value> {
    let json_path = facts_dir.join("outputs.json");
    if json_path.exists() {
        tracing::info!("Loading cached outputs from {:?}", json_path);
        let content = tokio::fs::read_to_string(&json_path)
            .await
            .map_err(|e| NaviError::IoError { error: e })?;
        let json: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| NaviError::DeploymentError {
                message: "Invalid facts/outputs.json".into(),
            })?;
        return Ok(json);
    }

    tracing::info!(
        "Cached outputs not found at {:?}. Attempting to read from Terraform workspace.",
        json_path
    );

    let work_dir = Path::new(".navi").join("provision").join(prov_name);
    if !work_dir.exists() {
        return Err(NaviError::DeploymentError {
            message: format!(
                "Provisioner workspace not found at {:?}. Run `navi provision` first or ensure facts are present.",
                work_dir
            ),
        });
    }

    let tf_bin = std::env::var("NAVI_TERRAFORM_BINARY").unwrap_or_else(|_| "tofu".to_string());
    let mut cmd = Command::new(tf_bin);
    cmd.current_dir(&work_dir).arg("output").arg("-json");

    let json: serde_json::Value = cmd.capture_json().await?;
    Ok(json)
}

// TODO: This is duplicated from provision.rs - should move to terraform.rs or util
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
    std::fs::create_dir_all(&target_dir).map_err(|e| NaviError::IoError { error: e })?;

    // Write outputs.json
    let json_path = target_dir.join("outputs.json");
    let json_content =
        serde_json::to_string_pretty(&output_json).expect("Failed to serialize outputs");
    std::fs::write(&json_path, json_content).map_err(|e| NaviError::IoError { error: e })?;

    // Write default.nix
    let nix_path = target_dir.join("default.nix");
    let nix_content = r#"let
  raw = builtins.fromJSON (builtins.readFile ./outputs.json);
in
  builtins.mapAttrs (n: v: v.value) raw
"#;
    std::fs::write(&nix_path, nix_content).map_err(|e| NaviError::IoError { error: e })?;

    tracing::info!("Facts saved to {:?}", target_dir);

    Ok(())
}

/// Verification step to check if a node is already installed.
/// It uses the same IP resolution logic as run() to find the host,
/// then attempts to SSH in and read /etc/navi/provenance.json.
async fn should_install_node(
    node: &str,
    outputs: &serde_json::Value,
    targets: &HashMap<NodeName, TargetNode>,
) -> NaviResult<bool> {
    let ip_key = format!("{}_ip", node.replace("-", "_"));
    let ip_val = outputs.get(&ip_key).and_then(|v| v.get("value")).and_then(|v| v.as_str());

    let ip = match ip_val {
        Some(i) => i.to_string(),
        None => {
            tracing::warn!("Could not find IP for {} in outputs. Assuming not installed (or manual intervention needed).", node);
            // If we can't find the IP, we can't check provenance.
            // We'll return true to let nixos-anywhere attempt (it will likely fail or user knows what they are doing).
            // Actually, if we can't find the IP, nixos-anywhere will also fail to find it.
            return Ok(true);
        }
    };

    let target_node = targets.get(&NodeName::new(node.to_string())?);
    
    // Determine connection details
    let (ssh_host, _use_iap) = if let Some(target) = target_node {
        match target.config.get_provider() {
            crate::nix::host::Provider::Gcp { iap, .. } if iap => {
                let hostname = target
                    .config
                    .target_host
                    .clone()
                    .unwrap_or_else(|| node.to_string());
                (hostname, true)
            }
            _ => (ip.clone(), false),
        }
    } else {
        (ip.clone(), false)
    };

    let mut host = if let Some(target) = target_node {
        let mut h = target.config.to_ssh_host().unwrap_or_else(|| Ssh::new(Some("root".to_string()), ssh_host.clone()));
        h.set_override_address(ssh_host);
        h.set_provider(target.config.get_provider());
        h
    } else {
        Ssh::new(Some("root".to_string()), ssh_host)
    };

    // We do a quick check. provenance file is usually at /etc/navi/provenance.json
    // We reuse the Host trait's ability internally, but Ssh struct specifically implements fetch_provenance logic
    // which tries to read that file.
    
    // However, the `Host` trait is in `crate::nix::host`, and `Ssh` implements it.
    // Let's use `fetch_provenance`.
    
    // We need to set up options to avoid hanging if the host is down (which means not installed maybe?)
    let mut opts = host.ssh_options();
    opts.extend([
        "-o".to_string(), "ConnectTimeout=5".to_string(),
        "-o".to_string(), "ConnectionAttempts=1".to_string(),
        "-o".to_string(), "StrictHostKeyChecking=no".to_string(), // Don't fail on new host keys
    ]);
    host.set_extra_ssh_options(opts);

    // Using fetch_provenance from the Host trait
    use crate::nix::Host;
    match host.fetch_provenance().await {
        Ok(Some(_)) => Ok(false), // Provenance exists -> Already installed
        Ok(None) => Ok(true),     // Provenance missing -> Needs install
        Err(_) => {
            // Connection failed or similar. Likely not installed or network issue.
            // We assume it needs install.
            tracing::debug!("Could not connect to {} to check provenance. Assuming clean.", node);
            Ok(true)
        }
    }
}
