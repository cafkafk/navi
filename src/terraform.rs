use std::collections::HashMap;
use std::path::PathBuf;
use tokio::process::Command;

use crate::error::{NaviError, NaviResult};
use crate::nix::{deployment::TargetNode, NodeName};
use crate::util::{CommandExecution, CommandExt};

pub struct TerraformExecutor {
    work_dir: PathBuf,
    tf_bin: String,
}

impl TerraformExecutor {
    pub fn new(work_dir: PathBuf) -> Self {
        let tf_bin = std::env::var("NAVI_TERRAFORM_BINARY").unwrap_or_else(|_| "tofu".to_string());
        Self { work_dir, tf_bin }
    }

    pub fn tf_bin(&self) -> &str {
        &self.tf_bin
    }

    pub async fn init(&self) -> NaviResult<()> {
        tracing::info!("Initializing {}...", self.tf_bin);
        let mut init = Command::new(&self.tf_bin);
        init.current_dir(&self.work_dir).arg("init").arg("-upgrade");

        let mut exec = CommandExecution::new(init);

        if let Err(e) = exec.run().await {
            let (_, stderr) = exec.get_logs();
            if let Some(stderr) = stderr {
                if self.handle_init_error(stderr).await? {
                    tracing::info!("Retrying initialization after migration...");
                    let mut retry = Command::new(&self.tf_bin);
                    retry
                        .current_dir(&self.work_dir)
                        .arg("init")
                        .arg("-upgrade");
                    return retry.passthrough().await;
                }
            }
            // Re-print the error if we didn't handle it
            if !exec.get_logs().1.unwrap_or(&"".to_string()).is_empty() {
                eprintln!("{}", exec.get_logs().1.unwrap());
            }
            return Err(e);
        }
        Ok(())
    }

    async fn handle_init_error(&self, stderr: &str) -> NaviResult<bool> {
        // 0. Check for Backend configuration changed
        if stderr.contains("Backend configuration changed") {
            eprintln!("\n\x1b[33mBackend configuration change detected.\x1b[0m");
            eprintln!("The Terraform state backend has changed (e.g. bucket or key update).");
            
            if confirm_action("Do you want to migrate existing state to the new configuration? (Runs 'init -migrate-state')")? {
                tracing::info!("Running init -migrate-state...");
                let mut cmd = Command::new(&self.tf_bin);
                cmd.current_dir(&self.work_dir)
                   .arg("init")
                   .arg("-migrate-state");
                
                // We use passthrough to allow Terraform to ask interactive questions if needed (e.g. confirming copy)
                cmd.passthrough().await?;
                return Ok(true);
            } else if confirm_action("Do you want to reconfigure (ignoring old state)? (Runs 'init -reconfigure')")? {
                tracing::info!("Running init -reconfigure...");
                let mut cmd = Command::new(&self.tf_bin);
                cmd.current_dir(&self.work_dir)
                   .arg("init")
                   .arg("-reconfigure");
                cmd.passthrough().await?;
                return Ok(true);
            }
        }

        // 1. Check for specific "plugins not installed" error or other signs of provider mismatch
        // The error might look like: "registry.opentofu.org/vitvio/porkbun: there is no package for..."
        // Or "Error loading the state: Required plugins are not installed"

        let missing_providers = self.extract_missing_providers(stderr);
        if missing_providers.is_empty() {
            return Ok(false);
        }

        // 2. Read the current configuration to find available providers
        let configured_providers = self.get_configured_providers().await?;
        let mut recovered = false;

        for missing in missing_providers {
            let suffix = missing.split('/').last().unwrap_or(&missing);

            // Find candidates in config that end with the same name
            // e.g. missing "vitvio/porkbun", config has "kyswtn/porkbun"
            let candidates: Vec<&String> = configured_providers
                .iter()
                .filter(|p| p.ends_with(suffix))
                .collect();

            if candidates.len() == 1 {
                let replacement = candidates[0];
                if &missing != replacement {
                    eprintln!("\n[33mProvider mismatch detected.[0m");
                    eprintln!("The state/lockfile requires: {}", missing);
                    eprintln!("Configuration has:         {}", replacement);

                    let prompt = format!("Do you want to migrate state from '{}' to '{}'?\n(This will reset the sandbox lockfile)", missing, replacement);
                    if confirm_action(&prompt)? {
                        // 3. Delete lockfile to allow init to succeed with new config
                        let lock_file = self.work_dir.join(".terraform.lock.hcl");
                        if lock_file.exists() {
                            tracing::info!("Removing .terraform.lock.hcl to unblock init...");
                            std::fs::remove_file(&lock_file)
                                .map_err(|e| NaviError::IoError { error: e })?;
                        }

                        // 4. Run init (must succeed for replace-provider to work reliably if plugins check is needed)
                        tracing::info!("Running partial init...");
                        let mut partial_init = Command::new(&self.tf_bin);
                        partial_init
                            .current_dir(&self.work_dir)
                            .arg("init")
                            .arg("-upgrade");
                        partial_init.passthrough().await?;

                        // 5. Run replace-provider
                        tracing::info!("Migrating provider in state...");
                        self.run_replace_provider(&missing, replacement).await?;

                        recovered = true;
                    }
                }
            }
        }
        Ok(recovered)
    }

    fn extract_missing_providers(&self, stderr: &str) -> Vec<String> {
        let mut missing = Vec::new();
        for line in stderr.lines() {
            let line = line.trim();
            // Look for "- registry...: there is no package"
            if line.starts_with("- ")
                && (line.contains(": there is no package") || line.contains("cached in"))
            {
                if let Some(part) = line.split_whitespace().nth(1) {
                    let provider = part.trim_end_matches(':');
                    if provider.contains('/') {
                        missing.push(provider.to_string());
                    }
                }
            }
        }
        missing
    }

    async fn get_configured_providers(&self) -> NaviResult<Vec<String>> {
        let config_path = self.work_dir.join("config.tf.json");
        let content = tokio::fs::read_to_string(config_path)
            .await
            .map_err(|e| NaviError::IoError { error: e })?;

        let json: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| NaviError::DeploymentError {
                message: "Invalid config.tf.json".into(),
            })?;

        let mut providers = Vec::new();
        if let Some(req) = json
            .get("terraform")
            .and_then(|t| t.get("required_providers"))
            .and_then(|r| r.as_object())
        {
            for (_, val) in req {
                if let Some(src) = val.as_str() {
                    providers.push(src.to_string());
                } else if let Some(src) = val.get("source").and_then(|s| s.as_str()) {
                    providers.push(src.to_string());
                }
            }
        }
        Ok(providers)
    }

    async fn run_replace_provider(&self, from: &str, to: &str) -> NaviResult<()> {
        let mut cmd = Command::new(&self.tf_bin);
        cmd.current_dir(&self.work_dir).args([
            "state",
            "replace-provider",
            "-auto-approve",
            from,
            to,
        ]);
        cmd.passthrough().await
    }

    pub async fn destroy(&self) -> NaviResult<()> {
        loop {
            tracing::info!("Destroying infrastructure...");
            let mut destroy = Command::new(&self.tf_bin);
            destroy
                .current_dir(&self.work_dir)
                .arg("destroy")
                .arg("-auto-approve");

            let mut destroy_exec = CommandExecution::new(destroy);
            let result = destroy_exec.run().await;

            if let Err(NaviError::ChildFailure { exit_code: 1, .. }) = result {
                let (_, stderr) = destroy_exec.get_logs();
                if let Some(stderr) = stderr {
                    if stderr_contains_auth_error(&stderr) {
                        eprintln!("\n[33mAuthentication error detected during destroy.[0m");
                        if try_refresh_gcloud_auth().await? {
                            continue;
                        }
                    }
                }
                return result;
            }
            result?;
            break;
        }
        Ok(())
    }

    pub async fn plan_and_apply(
        &self,
        targets: &HashMap<NodeName, TargetNode>,
        extra_vars: &HashMap<String, String>,
    ) -> NaviResult<()> {
        // Plan loop
        loop {
            tracing::info!("Planning configuration...");
            let mut plan = Command::new(&self.tf_bin);
            plan.current_dir(&self.work_dir)
                .arg("plan")
                .arg("-out=tfplan");

            for (k, v) in extra_vars {
                plan.arg("-var");
                plan.arg(format!("{}={}", k, v));
            }

            let mut plan_exec = CommandExecution::new(plan);
            let result = plan_exec.run().await;

            if let Err(NaviError::ChildFailure { exit_code: 1, .. }) = result {
                let (_, stderr) = plan_exec.get_logs();
                if let Some(stderr) = stderr {
                    if stderr_contains_auth_error(&stderr) {
                        eprintln!("\n[33mAuthentication error detected during plan.[0m");
                        if try_refresh_gcloud_auth().await? {
                            continue;
                        }
                    }
                }
                return result;
            }
            result?;
            break;
        }

        eprintln!("\nDo you want to perform these actions?");
        if !confirm_action("Do you want to perform these actions?")? {
            eprintln!("Cancelled.");
            return Ok(());
        }

        // Apply loop
        loop {
            tracing::info!("Applying configuration...");
            let mut apply = Command::new(&self.tf_bin);
            apply.current_dir(&self.work_dir).arg("apply").arg("tfplan");

            let mut apply_exec = CommandExecution::new(apply);
            let result = apply_exec.run().await;

            if let Err(NaviError::ChildFailure { exit_code: 1, .. }) = result {
                let (_, stderr) = apply_exec.get_logs();
                if let Some(stderr) = stderr {
                    if stderr_contains_auth_error(&stderr) {
                        eprintln!("\n[33mAuthentication error detected during apply.[0m");
                        if try_refresh_gcloud_auth().await? {
                            continue;
                        }
                    }

                    if stderr.contains("requires stopping it")
                        && stderr.contains("allow_stopping_for_update")
                    {
                        if let Some(idx) = stderr.find("google_compute_instance.") {
                            let remainder = &stderr[idx + "google_compute_instance.".len()..];
                            let instance_name = remainder
                                .chars()
                                .take_while(|c| c.is_alphanumeric() || *c == '-')
                                .collect::<String>();

                            eprintln!("\n[33mInstance update requires cleanup/stop.[0m");
                            eprintln!(
                                "Terraform cannot update '{}' because it is running.",
                                instance_name
                            );
                            eprintln!("We can stop it for you to allow the update to proceed.");

                            let prompt =
                                format!("Do you want to stop instance '{}' now?", instance_name);
                            if confirm_action(&prompt)? {
                                if stop_instance_helper(targets, &instance_name).await? {
                                    continue;
                                }
                            }
                        }
                    }
                }
                return result;
            }
            result?;
            break;
        }
        Ok(())
    }

    /// Finds a resource address for a given node name.
    /// This is a heuristic that searches the state for a resource ending in `["<node_name>"]` or `.<node_name>`.
    pub async fn find_resource_address_for_node(&self, node: &str) -> NaviResult<Option<String>> {
        let mut cmd = Command::new(&self.tf_bin);
        cmd.current_dir(&self.work_dir).arg("state").arg("list");
        
        // We use capture_output to get stdout as string
        let output = cmd.capture_output().await?;
        
        let suffix_brackets = format!("[\"{}\"]", node);
        let suffix_dot = format!(".{}", node);
        
        // Terraform resources often use underscores where Nix uses dashes.
        let node_underscored = node.replace("-", "_");
        let suffix_brackets_u = format!("[\"{}\"]", node_underscored);
        let suffix_dot_u = format!(".{}", node_underscored);
        
        // Debug logging to help troubleshoot matching failures
        tracing::debug!("Looking for resource address for node '{}' in terraform state.", node);
        tracing::debug!("Candidates suffixes: '{}', '{}', '{}', '{}'", 
            suffix_brackets, suffix_dot, suffix_brackets_u, suffix_dot_u);

        for line in output.lines() {
            let line = line.trim();
            if line.ends_with(&suffix_brackets) 
                || line.ends_with(&suffix_dot)
                || line.ends_with(&suffix_brackets_u)
                || line.ends_with(&suffix_dot_u) 
            {
                return Ok(Some(line.to_string()));
            }
        }
        
        // If not found, dump state list for debugging if we are in verbose mode
        tracing::warn!("Failed to find resource for node '{}' in state.", node);
        tracing::warn!("State list output (first 20 lines):\n{}", output.lines().take(20).collect::<Vec<_>>().join("\n"));
        if output.lines().count() > 20 {
            tracing::warn!("... ({} more lines)", output.lines().count() - 20);
        }
        
        Ok(None)
    }

    /// Targeted replacement of a specific resource.
    /// Runs `tofu apply -replace=<addr> -auto-approve`, handling "already exists" conflicts by pre-destroying.
    pub async fn replace_resource(&self, address: &str) -> NaviResult<()> {
        // Step 1: Explicitly destroy the resource first to avoid "already exists" errors
        // during replacement if create_before_destroy is active (or similar conflicts).
        tracing::info!("Step 1/2: Destroying resource {}...", address);
        let mut destroy_cmd = Command::new(&self.tf_bin);
        destroy_cmd.current_dir(&self.work_dir)
           .arg("apply")
           .arg("-destroy")
           .arg(format!("-target={}", address))
           .arg("-auto-approve");
        
        // We log warnings but don't fail immediately if destroy fails, as it might already be gone.
        // But generally, we want to know.
        let status = destroy_cmd.status().await?;
        if !status.success() {
            tracing::warn!("Specific destroy failed or resource was already gone. Proceeding to create...");
        }

        // Step 2: Create the resource again (targeting it specifically to be safe/fast)
        tracing::info!("Step 2/2: Recreating resource {}...", address);
        let mut create_cmd = Command::new(&self.tf_bin);
        create_cmd.current_dir(&self.work_dir)
           .arg("apply")
           .arg(format!("-target={}", address))
           .arg("-auto-approve");

        create_cmd.passthrough().await
    }
}

fn stderr_contains_auth_error(stderr: &str) -> bool {
    stderr.contains("invalid_grant") || stderr.contains("invalid_rapt")
}

async fn try_refresh_gcloud_auth() -> NaviResult<bool> {
    eprintln!("It looks like your Google Cloud credentials have expired.");
    eprintln!("Run the following command to refresh them:\n");
    eprintln!("    gcloud auth application-default login\n");

    if confirm_action("Do you want us to run the auth command? If so, we can do that and apply it again for you with your consent.")? {
        let mut gcloud = Command::new("gcloud");
        gcloud.args(["auth", "application-default", "login"]);
        if gcloud.passthrough().await.is_ok() {
            tracing::info!("Authentication successful. Retrying...");
            return Ok(true);
        }
    }
    Ok(false)
}

fn confirm_action(prompt: &str) -> NaviResult<bool> {
    eprintln!("{} [y/N]", prompt);
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| NaviError::IoError { error: e })?;
    let t = input.trim().to_lowercase();
    Ok(t == "y" || t == "yes")
}

async fn stop_instance_helper(
    targets: &HashMap<NodeName, TargetNode>,
    instance_name: &str,
) -> NaviResult<bool> {
    use crate::nix::host::Provider;

    // Find zone for the node matching the instance name
    let target_node = targets
        .iter()
        .find(|(n, _)| n.as_str() == instance_name)
        .or_else(|| {
            // Fallback: if only one target, assume it's the one
            if targets.len() == 1 {
                targets.iter().next()
            } else {
                None
            }
        });

    if let Some((_, target)) = target_node {
        let zone = match target.config.get_provider() {
            Provider::Gcp { zone, .. } => zone,
            _ => None,
        };

        if let Some(z) = zone {
            tracing::info!("Stopping instance {} in zone {}...", instance_name, z);
            let mut gstop = Command::new("gcloud");
            gstop.args(["compute", "instances", "stop", instance_name, "--zone", &z]);

            if gstop.passthrough().await.is_ok() {
                tracing::info!("Instance stopped. Retrying apply...");
                return Ok(true);
            }
        } else {
            eprintln!(
                "Could not determine zone for instance {}. Please stop it manually.",
                instance_name
            );
        }
    } else {
        eprintln!(
            "Could not find configuration for instance {}. Please stop it manually.",
            instance_name
        );
    }
    Ok(false)
}
