use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line as TextLine, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::command::tui::model::{App, ProvenanceStatus, TreeItem};

pub fn draw_inspector_popup(f: &mut Frame, app: &mut App) {
    // 1. Generate content matches
    let content_lines = get_inspector_content(app);

    if content_lines.is_empty() {
        return;
    }

    // 2. Calculate optimal width
    let screen_width = f.size().width;
    let max_width = (screen_width as f64 * 0.75) as u16;

    // Find longest line
    let content_max_len = content_lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
    let required_width = content_max_len + 4; // Borders + padding

    let actual_width = required_width.clamp(40, max_width);
    let estimated_height = (content_lines.len() + 2) as u16; // Borders
    let max_height = (f.size().height as f64 * 0.8) as u16;
    let actual_height = estimated_height.min(max_height);

    // Anchored Top-Right
    let area = Rect {
        x: f.size()
            .width
            .saturating_sub(actual_width)
            .saturating_sub(1), // -1 for right margin
        y: 1, // Below top bar
        width: actual_width,
        height: actual_height,
    };

    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Inspector ")
        .style(Style::default().bg(Color::Black));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let inspector = Paragraph::new(content_lines).wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(inspector, inner_area);
}

fn get_inspector_content(app: &App) -> Vec<TextLine> {
    let flat_items = App::flatten_tree(&app.tree);
    let current_idx = app.list_state.selected().unwrap_or(0);

    if let Some(flat) = flat_items.get(current_idx) {
        match flat.item {
            TreeItem::Node { name } => {
                if let Some(config) = app.node_configs.get(name) {
                    let host_display = if let Some(ref host) = config.target_host {
                        let user = config.target_user.as_deref().unwrap_or("root");
                        format!("{}@{}", user, host)
                    } else {
                        "Local".to_string()
                    };
                    let tags_str = config.tags().join(", ");

                    let prov_status = if let Some(status) = app.remote_provenance.get(name) {
                        match status {
                            ProvenanceStatus::Verified(prov) => {
                                let is_synced = prov.commit == app.git_rev;
                                (
                                    if is_synced {
                                        "Synced"
                                    } else {
                                        "Outdated/Diverged"
                                    },
                                    if is_synced { Color::Green } else { Color::Red },
                                    prov.commit.clone(),
                                    prov.deployed_by.clone(),
                                )
                            }
                            ProvenanceStatus::NoData => (
                                "Unknown (No Data)",
                                Color::Gray,
                                "???".to_string(),
                                "???".to_string(),
                            ),
                            ProvenanceStatus::Error(_) => (
                                "Connection Failed",
                                Color::Magenta,
                                "???".to_string(),
                                "???".to_string(),
                            ),
                        }
                    } else {
                        (
                            "Fetching...",
                            Color::Yellow,
                            "...".to_string(),
                            "...".to_string(),
                        )
                    };

                    let local_short_rev = app.git_rev.chars().take(7).collect::<String>();

                    vec![
                        TextLine::from(vec![
                            Span::styled(
                                "Node: ",
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(name.as_str()),
                        ]),
                        TextLine::from(""),
                        TextLine::from(vec![
                            Span::styled("Remote Status: ", Style::default().fg(Color::Cyan)),
                            Span::styled(prov_status.0, Style::default().fg(prov_status.1)),
                        ]),
                        TextLine::from(vec![
                            Span::styled("Remote Commit: ", Style::default().fg(Color::Cyan)),
                            Span::raw(if prov_status.2 == "..." {
                                "".to_string()
                            } else {
                                prov_status.2.chars().take(7).collect::<String>()
                            }),
                        ]),
                        TextLine::from(vec![
                            Span::styled("Deployed By: ", Style::default().fg(Color::Cyan)),
                            Span::raw(prov_status.3),
                        ]),
                        TextLine::from(""),
                        TextLine::from(vec![
                            Span::styled("Target Host: ", Style::default().fg(Color::Cyan)),
                            Span::raw(host_display),
                        ]),
                        TextLine::from(vec![
                            Span::styled("Target Port: ", Style::default().fg(Color::Cyan)),
                            Span::raw(
                                config
                                    .target_port
                                    .map(|p| p.to_string())
                                    .unwrap_or_else(|| "22".to_string()),
                            ),
                        ]),
                        TextLine::from(vec![
                            Span::styled("Build Location: ", Style::default().fg(Color::Cyan)),
                            Span::raw(if config.build_on_target() {
                                "On Target"
                            } else {
                                "Local / Builder"
                            }),
                        ]),
                        TextLine::from(vec![
                            Span::styled("Tags: ", Style::default().fg(Color::Cyan)),
                            Span::raw(if tags_str.is_empty() {
                                "None".to_string()
                            } else {
                                tags_str
                            }),
                        ]),
                        TextLine::from(""),
                        TextLine::from(vec![
                            Span::styled("Local Revision: ", Style::default().fg(Color::Magenta)),
                            Span::raw(local_short_rev),
                        ]),
                    ]
                } else {
                    vec![
                        TextLine::from(vec![
                            Span::styled(
                                "Node: ",
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(name.as_str()),
                        ]),
                        TextLine::from(""),
                        TextLine::from(vec![
                            Span::styled("Status: ", Style::default().fg(Color::Cyan)),
                            Span::styled("Loading metadata...", Style::default().fg(Color::Yellow)),
                        ]),
                    ]
                }
            }
            TreeItem::Group { name, .. } => {
                let mut leaves = Vec::new();
                App::collect_leaves(flat.item, &mut leaves);
                let total = leaves.len();
                let mut outdated = 0;
                for leaf in &leaves {
                    if let Some(ProvenanceStatus::Verified(prov)) = app.remote_provenance.get(leaf)
                    {
                        if prov.commit != app.git_rev {
                            outdated += 1;
                        }
                    }
                }
                let stats = if outdated > 0 {
                    format!(" ({} outdated!)", outdated)
                } else {
                    "".to_string()
                };
                vec![
                    TextLine::from(vec![
                        Span::styled(
                            "Group: ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(name),
                    ]),
                    TextLine::from(format!("Contains {} items{}", total, stats)),
                ]
            }
        }
    } else {
        vec![]
    }
}
