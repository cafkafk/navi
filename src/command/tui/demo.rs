use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use ratatui::style::Color;
use tokio::sync::mpsc;

use crate::command::tui::model::{App, AppMode, TreeItem, RegistrantTreeItem};
use crate::error::NaviResult;
use crate::nix::{NodeConfig, NodeName, NodeState};
use crate::registrants::DomainInfo;
use crate::registrants::porkbun::PorkbunDnsRecord;

pub async fn run_demo() -> NaviResult<()> {
    tracing::info!("Starting TUI in DEMO mode");

    let (clipboard_tx, _clipboard_rx) = mpsc::unbounded_channel();

    // 1. Build Fake Nodes & Hierarchy
    let mut nodes = Vec::new();
    let mut configs = HashMap::new();
    let mut node_states = HashMap::new();
    let mut logs = HashMap::new();

    let regions = vec!["us-east1", "us-west2", "eu-west1", "eu-central1", "asia-northeast1", "sa-east1"];
    let mut tree = Vec::new();

    let mut id_counter = 0;

    for region in regions {
        let mut region_children = Vec::new();
        
        let roles = vec![
            ("lb", 2),
            ("app", 8), 
            ("worker", 5),
            ("cache", 3),
            ("db", 3),
            ("queue", 2),
            ("monitoring", 1)
        ];

        for (role, count) in roles {
            let mut role_children = Vec::new();

            for i in 1..=count {
                id_counter += 1;
                let name = format!("{}-{}-{:02}", region, role, i);
                let node_name = NodeName::new(name.clone()).unwrap();
                
                nodes.push(node_name.clone());
                configs.insert(node_name.clone(), NodeConfig::default());

                // Fake States with variety
                let status = match id_counter % 10 {
                    0 => NodeState::Failed("SSH Timeout".to_string()),
                    1 => NodeState::Running("Copying closure (82%)...".to_string()),
                    2 => NodeState::Running("Building (eval)...".to_string()),
                    3 => NodeState::Running("Waiting for lock...".to_string()),
                    4 => NodeState::Running("Activating...".to_string()),
                    _ => NodeState::Success("Deployed".to_string()),
                };
                
                node_states.insert(node_name.clone(), status);

                // Fake Logs
                let mut node_logs = Vec::new();
                for j in 0..20 {
                    node_logs.push(format!("Nov 10 10:{:02}:{:02} systemd[1]: Service {} status check OK.", 10+j, j, role));
                    if id_counter % 7 == 0 {
                        node_logs.push(format!("Nov 10 10:{:02}:{:02} {}[{}]: WARN: Connection pool saturation at 85%.", 10+j, j+1, role, 1000+id_counter));
                    }
                }
                logs.insert(node_name.clone(), node_logs);

                role_children.push(TreeItem::Node { name: node_name });
            }

            region_children.push(TreeItem::Group {
                name: format!("{} ({})", role, count),
                children: role_children,
                collapsed: false, // Expand everything for the screenshot density
            });
        }

        tree.push(TreeItem::Group {
            name: format!("{} ({})", region, 24), // Total approx
            children: region_children,
            collapsed: false,
        });
    }

    // 2. Fake Registrants
    let companies = vec![
        ("google", "com"),
        ("alphabet", "xyz"),
        ("youtube", "com"),
        ("android", "com"),
        ("waymo", "com"),
        ("deepmind", "com"),
        ("calico", "com"),
        ("wing", "com"),
        ("verily", "com"),
        ("loon", "com"),
    ];

    let mut registrants_tree = Vec::new();
    let mut domains_vec = Vec::new();

    for (company, tld) in companies {
        let domain_str = format!("{}.{}", company, tld);
        let info = DomainInfo {
            domain: domain_str.clone(),
            auto_renew: true,
            expiry_date: Some("2030-01-01".to_string()),
            provider: "Porkbun".to_string(),
            account: "Main".to_string(),
        };
        domains_vec.push(info.clone());

        let mut dns_records = Vec::new();
        dns_records.push(RegistrantTreeItem::DnsRecord { 
            record: PorkbunDnsRecord {
                id: "1".to_string(),
                name: domain_str.clone(),
                record_type: "A".to_string(),
                content: "1.2.3.4".to_string(),
                ttl: "600".to_string(),
                prio: None,
                notes: None,
            }
        });
        dns_records.push(RegistrantTreeItem::DnsRecord { 
            record: PorkbunDnsRecord {
                id: "2".to_string(),
                name: format!("www.{}", domain_str),
                record_type: "CNAME".to_string(),
                content: domain_str.clone(),
                ttl: "600".to_string(),
                prio: None,
                notes: None,
            }
        });
        
        let children = vec![
            RegistrantTreeItem::RecordGroup {
                name: "DNS Records".to_string(),
                domain_info: info.clone(),
                children: dns_records,
                collapsed: false, // Expand for demo
                loaded: true,
            },
            RegistrantTreeItem::RecordGroup {
                name: "Glue Records".to_string(),
                domain_info: info.clone(),
                children: vec![RegistrantTreeItem::Message { text: "No glue records".to_string() }],
                collapsed: true,
                loaded: true,
            }
        ];

        registrants_tree.push(RegistrantTreeItem::Domain {
            info,
            children,
            collapsed: false,
        });
    }

    // Wrap in Provider/Account grouping for consistency with real view
    registrants_tree = vec![
        RegistrantTreeItem::Provider {
            name: "Porkbun".to_string(),
            collapsed: false,
            children: vec![
                RegistrantTreeItem::Account {
                    name: "Enterprise".to_string(),
                    collapsed: false,
                    children: registrants_tree,
                }
            ]
        }
    ];

    // 3. Init App
    let mut app = App::new(
        nodes,
        configs,
        tree,
        clipboard_tx,
        "demo-v9.0.0-rc1".to_string(), // Cool version
        None,
        None,
    );

    app.node_states = node_states;
    app.node_logs = logs;
    app.registrant_domains = domains_vec;
    app.registrants_tree = registrants_tree;
    app.registrants_loading = false;
    app.mode = AppMode::Boot; // Enable boot animation for demo effect (Cyberpunk style)

    // 4. Run Loop
    crate::command::tui::core::run::run_with_app(app).await
}
