use std::collections::HashMap;

use clap::Args;
use serde_json::json;

use crate::error::NaviResult;
use crate::nix::{hive::Hive, NodeConfig, NodeName, Provider};

#[derive(Debug, Args)]
#[command(name = "list", about = "List available hosts and their configuration")]
pub struct Opts {
    /// Output in JSON format
    #[arg(long, short = 'j')]
    pub json: bool,
}

pub async fn run(hive: Hive, opts: Opts) -> NaviResult<()> {
    // Evaluating the hive deployment config to get node information.
    let deployment_info = hive.deployment_info().await?;

    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&deployment_info).unwrap()
        );
    } else {
        // Collect and sort nodes
        let mut nodes: Vec<(NodeName, NodeConfig)> = deployment_info.into_iter().collect();
        nodes.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));

        if nodes.is_empty() {
            eprintln!("No nodes found in the configuration.");
            return Ok(());
        }

        // Calculate column widths
        let max_name_len = nodes
            .iter()
            .map(|(n, _)| n.len())
            .max()
            .unwrap_or(0)
            .max(4); // "NAME".len() is 4

        let max_host_len = nodes
            .iter()
            .map(|(_, c)| c.target_host.as_deref().map_or(0, |h| h.len()))
            .max()
            .unwrap_or(0)
            .max(11); // "TARGET HOST".len() is 11

        let max_prov_len = nodes
            .iter()
            .map(|(_, c)| c.provisioner.as_deref().map_or(0, |p| p.len()))
            .max()
            .unwrap_or(0)
            .max(11); // "PROVISIONER".len() is 11

        let max_link_len = nodes
            .iter()
            .map(|(_, c)| match c.get_provider() {
                Provider::Ssh => 3,                   // "SSH".len()
                Provider::Gcp { iap: true, .. } => 9, // "GCP (IAP)".len()
                Provider::Gcp { .. } => 12,           // "GCP (Direct)".len()
            })
            .max()
            .unwrap_or(0)
            .max(4); // "LINK".len() is 4

        // Print header
        println!(
            "{:<w_name$}  {:<w_host$}  {:<w_prov$}  {:<w_link$}  TAGS",
            "NAME",
            "TARGET HOST",
            "PROVISIONER",
            "LINK",
            w_name = max_name_len,
            w_host = max_host_len,
            w_prov = max_prov_len,
            w_link = max_link_len,
        );

        // Print rows
        for (name, config) in nodes {
            let host = config.target_host.as_deref().unwrap_or("-");
            let prov = config.provisioner.as_deref().unwrap_or("-");
            let link = match config.get_provider() {
                Provider::Ssh => "SSH",
                Provider::Gcp { iap: true, .. } => "GCP (IAP)",
                Provider::Gcp { .. } => "GCP (Direct)",
            };
            let tags = config.tags().join(", ");

            println!(
                "{:<w_name$}  {:<w_host$}  {:<w_prov$}  {:<w_link$}  {}",
                name.as_str(),
                host,
                prov,
                link,
                tags,
                w_name = max_name_len,
                w_host = max_host_len,
                w_prov = max_prov_len,
                w_link = max_link_len,
            );
        }
    }

    Ok(())
}
