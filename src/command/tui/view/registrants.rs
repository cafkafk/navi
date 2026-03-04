use crate::command::tui::model::{App, RegistrantTreeItem};
use ratatui::{
    style::{Color, Modifier, Style},
    widgets::{List, ListItem},
    Frame,
};

pub fn draw_registrants(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    if app.registrants_loading {
        let text = ratatui::widgets::Paragraph::new("Loading domains...")
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(text, area);
        return;
    }

    if app.registrants_tree.is_empty() {
        let msg = if app.registrants_config.is_none() {
            "No registrants configured in meta.registrants."
        } else {
            "No domains found."
        };
        let text =
            ratatui::widgets::Paragraph::new(msg).alignment(ratatui::layout::Alignment::Center);
        f.render_widget(text, area);
        return;
    }

    let items = render_tree_items(&app.registrants_tree, 0);
    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut app.registrants_state);
}

fn render_tree_items(items: &Vec<RegistrantTreeItem>, depth: usize) -> Vec<ListItem<'static>> {
    let mut out = Vec::new();
    let indent = "  ".repeat(depth);

    for item in items {
        match item {
            RegistrantTreeItem::Provider {
                name,
                children,
                collapsed,
            } => {
                let arrow = if *collapsed { "▶ " } else { "▼ " };
                let content = format!("{}{}{}", indent, arrow, name);
                out.push(
                    ListItem::new(content).style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                );

                if !*collapsed {
                    out.append(&mut render_tree_items(children, depth + 1));
                }
            }
            RegistrantTreeItem::Account {
                name,
                children,
                collapsed,
            } => {
                let arrow = if *collapsed { "▶ " } else { "▼ " };
                let content = format!("{}{}{}", indent, arrow, name);
                out.push(ListItem::new(content).style(Style::default().fg(Color::Yellow)));

                if !*collapsed {
                    out.append(&mut render_tree_items(children, depth + 1));
                }
            }
            RegistrantTreeItem::Domain {
                info,
                children,
                collapsed,
            } => {
                let arrow = if *collapsed { "▶ " } else { "▼ " };
                let expiry = info.expiry_date.as_deref().unwrap_or("N/A");
                // let auto = if info.auto_renew { "Auto" } else { "Manual" };
                let content = format!("{}{}{}", indent, arrow, info.domain);

                // Add details in gray
                let line = ratatui::text::Line::from(vec![
                    ratatui::text::Span::raw(content),
                    ratatui::text::Span::styled(
                        format!(" (Exp: {})", expiry),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);

                out.push(ListItem::new(line));

                if !*collapsed {
                    out.append(&mut render_tree_items(children, depth + 1));
                }
            }
            RegistrantTreeItem::RecordGroup {
                name,
                children,
                collapsed,
                ..
            } => {
                let arrow = if *collapsed { "▶ " } else { "▼ " };
                let content = format!("{}{}{}", indent, arrow, name);
                out.push(ListItem::new(content).style(Style::default().fg(Color::Magenta)));

                if !*collapsed {
                    out.append(&mut render_tree_items(children, depth + 1));
                }
            }
            RegistrantTreeItem::DnsRecord { record } => {
                let content = format!(
                    "{}  {} {} {} (TTL: {})",
                    indent, record.record_type, record.name, record.content, record.ttl
                );
                out.push(ListItem::new(content).style(Style::default().fg(Color::White)));
            }
            RegistrantTreeItem::GlueRecord { record } => {
                let v4 = record.ips.v4.join(", ");
                let v6 = record.ips.v6.join(", ");
                let content = format!("{}  {} [v4: {}] [v6: {}]", indent, record.host, v4, v6);
                out.push(ListItem::new(content).style(Style::default().fg(Color::White)));
            }
            RegistrantTreeItem::Message { text } => {
                let content = format!("{}  {}", indent, text);
                out.push(
                    ListItem::new(content).style(
                        Style::default()
                            .fg(Color::Gray)
                            .add_modifier(Modifier::ITALIC),
                    ),
                );
            }
        }
    }
    out
}
