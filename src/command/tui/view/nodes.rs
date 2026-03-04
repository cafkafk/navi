use ratatui::{
    style::Color,
    style::Modifier,
    style::Style,
    widgets::{List, ListItem},
    Frame,
};

use crate::command::tui::model::{App, AppMode, ProvenanceStatus, TreeItem};
use crate::nix::NodeState;

pub fn draw_nodes(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let flat_items = App::flatten_tree(&app.tree);
    let current_idx = app.list_state.selected().unwrap_or(0);

    let items: Vec<ListItem> = flat_items
        .iter()
        .enumerate()
        .map(|(i, flat)| {
            // Tree indentation logic (purely visual)
            let branch = if flat.last_in_group {
                "└── "
            } else {
                "├── "
            };

            // Build the prefix string
            let mut indent = String::new();
            if flat.depth > 0 {
                // For levels 0..depth-1, we cheat and print empty space because we lack ancestral data.
                // This avoids the "diagonal lines" visual noise.
                indent.push_str(&"    ".repeat(flat.depth - 1));
                indent.push_str(branch);
            }

            // Colors
            let mut style = Style::default();

            // Construct the content string
            let content_str = match flat.item {
                TreeItem::Group {
                    name, collapsed, ..
                } => {
                    let icon = if *collapsed { "▸" } else { "▾" };
                    let mut leaves = Vec::new();
                    App::collect_leaves(flat.item, &mut leaves);
                    let total = leaves.len();
                    let selected_count = leaves.iter().filter(|n| app.selected.contains(n)).count();

                    let mut outdated_count = 0;
                    for node in &leaves {
                        if let Some(ProvenanceStatus::Verified(prov)) =
                            app.remote_provenance.get(node)
                        {
                            if prov.commit != app.git_rev {
                                outdated_count += 1;
                            }
                        }
                    }

                    let stats_str = if outdated_count > 0 {
                        format!(" ({} outdated!)", outdated_count)
                    } else {
                        "".to_string()
                    };

                    let check = if selected_count == 0 {
                        "[ ]"
                    } else if selected_count == total {
                        "[x]"
                    } else {
                        "[-]"
                    };
                    format!("{} {} {} ({}){}", icon, check, name, total, stats_str)
                }
                TreeItem::Node { name } => {
                    let state = app.node_states.get(name).unwrap_or(&NodeState::Idle);
                    style = state_to_style(state);

                    let check = if let NodeState::Running(_) = state {
                        let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
                        let idx = (app.tick_count as usize / 4) % spinner.len();
                        format!("[{}]", spinner[idx])
                    } else if app.selected.contains(name) {
                        "[x]".to_string()
                    } else {
                        "[ ]".to_string()
                    };

                    let state_text = if matches!(state, NodeState::Idle) {
                        if let Some(status) = app.remote_provenance.get(name) {
                            match status {
                                ProvenanceStatus::Verified(prov) => {
                                    if prov.commit == app.git_rev {
                                        style = Style::default().fg(Color::Green);
                                        "✔ Synced".to_string()
                                    } else {
                                        style = Style::default().fg(Color::Red);
                                        "✘ Outdated".to_string()
                                    }
                                }
                                ProvenanceStatus::NoData => {
                                    style = Style::default().fg(Color::DarkGray);
                                    "? No Metadata".to_string()
                                }
                                ProvenanceStatus::Error(_e) => {
                                    style = Style::default().fg(Color::Magenta);
                                    "! Unreachable".to_string()
                                }
                            }
                        } else {
                            "⟳ Checking...".to_string()
                        }
                    } else if let NodeState::Running(msg) = state {
                        msg.clone()
                    } else {
                        state.as_str().to_string()
                    };

                    format!("{}  {}  {}", check, name.as_str(), state_text)
                }
            };

            // Selection Highlight
            if app.mode == AppMode::Visual {
                if let Some(anchor) = app.visual_anchor {
                    let start = std::cmp::min(anchor, current_idx);
                    let end = std::cmp::max(anchor, current_idx);
                    if i >= start && i <= end {
                        style = style.bg(Color::DarkGray);
                    }
                }
            }

            let mut spans = Vec::new();
            if !indent.is_empty() {
                spans.push(ratatui::text::Span::styled(
                    indent,
                    Style::default().fg(Color::DarkGray),
                ));
            }
            spans.push(ratatui::text::Span::styled(content_str, style));

            ListItem::new(ratatui::text::Line::from(spans))
        })
        .collect();

    let nodes_list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    f.render_stateful_widget(nodes_list, area, &mut app.list_state);
}

fn state_to_style(state: &NodeState) -> Style {
    match state {
        NodeState::Idle => Style::default(),
        NodeState::Loading => Style::default().fg(Color::Gray),
        NodeState::Running(_) => Style::default().fg(Color::Yellow),
        NodeState::Success(_) => Style::default().fg(Color::Green),
        NodeState::Failed(_) => Style::default().fg(Color::Red),
    }
}
