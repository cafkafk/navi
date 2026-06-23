use std::collections::BTreeMap;

use clap::{Args, ValueEnum};
use console::Style;

use crate::error::NaviResult;
use crate::nix::{hive::Hive, NodeConfig, NodeName, Provider};

/// Key to group hosts by when rendering the human-readable list.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum GroupBy {
    /// One section per tag (hosts with several tags appear in each).
    Tag,
    /// One section per provisioner.
    Provisioner,
    /// One section per connection link (SSH, GCP, ...).
    Link,
}

#[derive(Debug, Args)]
#[command(name = "list", about = "List available hosts and their configuration")]
pub struct Opts {
    /// Output in JSON format
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Group hosts into sections by the given key
    #[arg(long, value_enum, value_name = "KEY")]
    pub group_by: Option<GroupBy>,
}

/// A single, pre-resolved row of the table.
struct Row {
    name: String,
    host: Option<String>,
    prov: Option<String>,
    provider: Provider,
    link: &'static str,
    tags: Vec<String>,
}

impl Row {
    fn from_node(name: &NodeName, config: &NodeConfig) -> Self {
        let provider = config.get_provider();
        Self {
            name: name.as_str().to_string(),
            host: config.target_host.clone(),
            prov: config.provisioner.clone(),
            link: link_label(&provider),
            provider,
            tags: config.tags().to_vec(),
        }
    }

    /// The keys this row belongs to for the given grouping.
    fn group_keys(&self, by: GroupBy) -> Vec<String> {
        match by {
            GroupBy::Tag => {
                if self.tags.is_empty() {
                    vec!["(untagged)".to_string()]
                } else {
                    self.tags.clone()
                }
            }
            GroupBy::Provisioner => {
                vec![self.prov.clone().unwrap_or_else(|| "(none)".to_string())]
            }
            GroupBy::Link => vec![self.link.to_string()],
        }
    }
}

/// Column widths shared across every section so columns stay aligned.
struct Widths {
    name: usize,
    host: usize,
    prov: usize,
    link: usize,
}

fn link_label(provider: &Provider) -> &'static str {
    match provider {
        Provider::Ssh => "SSH",
        Provider::Gcp { iap: true, .. } => "GCP (IAP)",
        Provider::Gcp { .. } => "GCP (Direct)",
    }
}

fn link_style(provider: &Provider) -> Style {
    match provider {
        Provider::Ssh => Style::new().cyan(),
        Provider::Gcp { iap: true, .. } => Style::new().green(),
        Provider::Gcp { .. } => Style::new().yellow(),
    }
}

fn pad(s: &str, w: usize) -> String {
    format!("{s:<w$}")
}

fn compute_widths(rows: &[Row]) -> Widths {
    Widths {
        name: rows.iter().map(|r| r.name.len()).max().unwrap_or(0).max(4), // "NAME"
        host: rows
            .iter()
            .map(|r| r.host.as_deref().map_or(1, str::len))
            .max()
            .unwrap_or(0)
            .max(11), // "TARGET HOST"
        prov: rows
            .iter()
            .map(|r| r.prov.as_deref().map_or(1, str::len))
            .max()
            .unwrap_or(0)
            .max(11), // "PROVISIONER"
        link: rows.iter().map(|r| r.link.len()).max().unwrap_or(0).max(4), // "LINK"
    }
}

fn print_header(w: &Widths) {
    let h = Style::new().bold();
    println!(
        "{}  {}  {}  {}  {}",
        h.apply_to(pad("NAME", w.name)),
        h.apply_to(pad("TARGET HOST", w.host)),
        h.apply_to(pad("PROVISIONER", w.prov)),
        h.apply_to(pad("LINK", w.link)),
        h.apply_to("TAGS"),
    );
}

fn print_row(row: &Row, w: &Widths) {
    let dim = Style::new().dim();

    let host = match &row.host {
        Some(h) => pad(h, w.host),
        None => dim.apply_to(pad("-", w.host)).to_string(),
    };
    let prov = match &row.prov {
        Some(p) => pad(p, w.prov),
        None => dim.apply_to(pad("-", w.prov)).to_string(),
    };
    let link = link_style(&row.provider)
        .apply_to(pad(row.link, w.link))
        .to_string();
    let tags = if row.tags.is_empty() {
        dim.apply_to("-").to_string()
    } else {
        dim.apply_to(row.tags.join(", ")).to_string()
    };

    println!("{}  {host}  {prov}  {link}  {tags}", pad(&row.name, w.name));
}

fn is_catchall(key: &str) -> bool {
    key == "(untagged)" || key == "(none)"
}

/// Orders sections alphabetically, but keeps catch-all buckets last.
fn section_order(a: &str, b: &str) -> std::cmp::Ordering {
    is_catchall(a).cmp(&is_catchall(b)).then_with(|| a.cmp(b))
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

pub async fn run(hive: Hive, opts: Opts) -> NaviResult<()> {
    // Evaluating the hive deployment config to get node information.
    let deployment_info = hive.deployment_info().await?;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&deployment_info).unwrap());
        return Ok(());
    }

    // Collect and sort nodes by name.
    let mut nodes: Vec<(NodeName, NodeConfig)> = deployment_info.into_iter().collect();
    nodes.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));

    if nodes.is_empty() {
        eprintln!("No nodes found in the configuration.");
        return Ok(());
    }

    let rows: Vec<Row> = nodes
        .iter()
        .map(|(name, config)| Row::from_node(name, config))
        .collect();
    let widths = compute_widths(&rows);
    let dim = Style::new().dim();

    match opts.group_by {
        None => {
            print_header(&widths);
            for row in &rows {
                print_row(row, &widths);
            }
            println!(
                "\n{}",
                dim.apply_to(format!("{} host{}", rows.len(), plural(rows.len())))
            );
        }
        Some(by) => {
            let mut groups: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
            for row in &rows {
                for key in row.group_keys(by) {
                    groups.entry(key).or_default().push(row);
                }
            }

            // Sort sections alphabetically, but push catch-all buckets last.
            let mut keys: Vec<&String> = groups.keys().collect();
            keys.sort_by(|a, b| section_order(a, b));

            let title = Style::new().bold().blue();
            print_header(&widths);
            for key in &keys {
                let section = &groups[*key];
                println!(
                    "\n{} {}",
                    title.apply_to(format!("▸ {key}")),
                    dim.apply_to(format!("({})", section.len())),
                );
                for row in section {
                    print_row(row, &widths);
                }
            }
            println!(
                "\n{}",
                dim.apply_to(format!(
                    "{} host{} in {} group{}",
                    rows.len(),
                    plural(rows.len()),
                    keys.len(),
                    plural(keys.len()),
                ))
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, host: Option<&str>, prov: Option<&str>, provider: Provider, tags: &[&str]) -> Row {
        let link = link_label(&provider);
        Row {
            name: name.to_string(),
            host: host.map(str::to_string),
            prov: prov.map(str::to_string),
            provider,
            link,
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn plural_suffix() {
        assert_eq!(plural(0), "s");
        assert_eq!(plural(1), "");
        assert_eq!(plural(2), "s");
    }

    #[test]
    fn widths_respect_header_minimums_when_data_is_short() {
        let rows = vec![row("a", Some("h"), Some("p"), Provider::Ssh, &["t"])];
        let w = compute_widths(&rows);
        assert_eq!(w.name, 4); // "NAME"
        assert_eq!(w.host, 11); // "TARGET HOST"
        assert_eq!(w.prov, 11); // "PROVISIONER"
        assert_eq!(w.link, 4); // "LINK"
    }

    #[test]
    fn widths_grow_to_fit_data() {
        let rows = vec![
            row("short", Some("h"), None, Provider::Ssh, &[]),
            row(
                "a-very-long-hostname-node",
                Some("some.long.target.host.example.com"),
                Some("baremetal-provisioner"),
                Provider::Gcp { project: None, zone: None, iap: false },
                &[],
            ),
        ];
        let w = compute_widths(&rows);
        assert_eq!(w.name, "a-very-long-hostname-node".len());
        assert_eq!(w.host, "some.long.target.host.example.com".len());
        assert_eq!(w.prov, "baremetal-provisioner".len());
        assert_eq!(w.link, "GCP (Direct)".len());
    }

    #[test]
    fn missing_host_counts_as_one_column_for_width() {
        // A node with no target host should not force the column past the header.
        let rows = vec![row("n", None, None, Provider::Ssh, &[])];
        let w = compute_widths(&rows);
        assert_eq!(w.host, 11);
    }

    #[test]
    fn group_keys_tag_lists_every_tag() {
        let r = row("n", None, None, Provider::Ssh, &["prod", "web"]);
        assert_eq!(r.group_keys(GroupBy::Tag), vec!["prod", "web"]);
    }

    #[test]
    fn group_keys_tag_falls_back_to_untagged() {
        let r = row("n", None, None, Provider::Ssh, &[]);
        assert_eq!(r.group_keys(GroupBy::Tag), vec!["(untagged)"]);
    }

    #[test]
    fn group_keys_provisioner_falls_back_to_none() {
        let with = row("n", None, Some("baremetal"), Provider::Ssh, &[]);
        let without = row("n", None, None, Provider::Ssh, &[]);
        assert_eq!(with.group_keys(GroupBy::Provisioner), vec!["baremetal"]);
        assert_eq!(without.group_keys(GroupBy::Provisioner), vec!["(none)"]);
    }

    #[test]
    fn group_keys_link_uses_link_label() {
        let ssh = row("n", None, None, Provider::Ssh, &[]);
        let iap = row("n", None, None, Provider::Gcp { project: None, zone: None, iap: true }, &[]);
        assert_eq!(ssh.group_keys(GroupBy::Link), vec!["SSH"]);
        assert_eq!(iap.group_keys(GroupBy::Link), vec!["GCP (IAP)"]);
    }

    #[test]
    fn sections_sort_catchall_last() {
        let mut keys = vec!["(untagged)", "web", "db", "(none)", "app"];
        keys.sort_by(|a, b| section_order(a, b));
        assert_eq!(keys, vec!["app", "db", "web", "(none)", "(untagged)"]);
    }
}
