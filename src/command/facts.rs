use clap::{Args, Subcommand};
use glob::Pattern;
use tokio::fs;
use tokio::process::Command;
use crate::error::{NaviError, NaviResult};
use crate::nix::{Hive, HivePath};

#[derive(Debug, Args)]
pub struct Opts {
    #[command(subcommand)]
    pub command: FactsCommand,
}

#[derive(Debug, Subcommand)]
pub enum FactsCommand {
    /// Derive facts from the Nix configuration (Pre-computation)
    Derive(DeriveOpts),
}

#[derive(Debug, Args)]
pub struct DeriveOpts {
    /// Filter facts to generate (glob patterns)
    pub filters: Vec<String>,
}

pub async fn run(hive: Hive, opts: Opts) -> NaviResult<()> {
    match opts.command {
        FactsCommand::Derive(derive_opts) => derive(&hive, derive_opts.filters).await,
    }
}

pub async fn derive(hive: &Hive, filters: Vec<String>) -> NaviResult<()> {
    let meta = hive.get_meta_config().await?;

    // Determine context directory
    let context_dir = hive.context_dir()
        .ok_or_else(|| NaviError::DeploymentError { message: "Context directory not found. Facts generation requires a local context.".to_string() })?;

    // Target directory: <context>/facts/derived
    let facts_dir = context_dir.join(&meta.facts.dir_name).join("derived");

    if !facts_dir.exists() {
        fs::create_dir_all(&facts_dir).await.map_err(|e| NaviError::IoContext {
            error: e,
            context: format!("creating facts directory {:?}", facts_dir),
        })?;
    }

    // Identify Flake URI
    let flake_uri = match hive.path() {
        HivePath::Flake(flake) => flake.uri().to_string(),
        _ => return Err(NaviError::DeploymentError { message: "Facts are only supported with Flakes".to_string() }),
    };

    tracing::info!("Discovering available facts...");

    // List facts using nix eval
    let mut cmd = Command::new("nix");
    cmd.arg("eval");
    cmd.args(["--json", "--apply", "builtins.attrNames"]);
    cmd.args(["--extra-experimental-features", "nix-command flakes"]);

    // Add hive flags (impure, show-trace, etc.)
    cmd.args(hive.nix_flags().to_args());

    cmd.arg(format!("{}#facts", flake_uri));

    let output = cmd.output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // If facts attribute doesn't exist, it might not be an error, just no facts defined.
        if stderr.contains("attribute 'facts' missing") {
            tracing::warn!("No 'facts' output defined in the flake.");
            return Ok(());
        }
        return Err(NaviError::DeploymentError { message: format!("Failed to list facts: {}", stderr) });
    }

    let facts: Vec<String> = serde_json::from_slice(&output.stdout)
        .map_err(|e| NaviError::DeploymentError { message: format!("Failed to parse facts list: {}", e) })?;

    // Filter facts
    let patterns = if filters.is_empty() {
        vec![Pattern::new("*").unwrap()]
    } else {
        filters.iter()
            .map(|f| Pattern::new(f).map_err(|e| NaviError::DeploymentError { message: format!("Invalid glob pattern: {}", e) }))
            .collect::<Result<Vec<_>, _>>()?
    };

    let selected_facts: Vec<&String> = facts.iter()
        .filter(|fact| patterns.iter().any(|p| p.matches(fact)))
        .collect();

    if selected_facts.is_empty() {
        if !filters.is_empty() {
            tracing::warn!("No facts matched the provided filters.");
        } else {
            tracing::info!("No facts found to derive.");
        }
        return Ok(());
    }

    tracing::info!("Deriving {} facts...", selected_facts.len());

    // Build facts
    for fact in selected_facts {
        tracing::info!("  -> {}", fact);
        let target_path = facts_dir.join(format!("{}.json", fact));

        // Create parent directories if fact name implies hierarchy (e.g. "group/fact")
        if let Some(parent) = target_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await.map_err(|e| NaviError::IoContext {
                    error: e,
                    context: format!("creating parent directory for fact {:?}", parent),
                })?;
            }
        }

        // Remove existing link/file to avoid conflicts
        if target_path.exists() || target_path.is_symlink() {
            fs::remove_file(&target_path).await.map_err(|e| NaviError::IoContext {
               error: e,
               context: format!("removing existing fact file {:?}", target_path),
           })?;
       }

        let mut build_cmd = Command::new("nix");
        build_cmd.arg("build");
        build_cmd.args(["--extra-experimental-features", "nix-command flakes"]);
        build_cmd.args(hive.nix_flags().to_args());

        // --out-link expects a path.
        build_cmd.arg("--out-link");
        build_cmd.arg(&target_path);

        build_cmd.arg(format!("{}#facts.\"{}\"", flake_uri, fact));

        let output = build_cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NaviError::DeploymentError { message: format!("Failed to derive fact '{}': {}", fact, stderr) });
        }
    }

    tracing::info!("Successfully derived facts in {:?}", facts_dir);
    Ok(())
}
