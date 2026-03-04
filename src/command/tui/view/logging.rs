use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::command::tui::model::App;
use ansi_to_tui::IntoText;

pub fn draw_logs(f: &mut Frame, app: &mut App, area: Rect) {
    let mut title = "Logs (Global)".to_string();
    let mut log_lines: Vec<String> = Vec::new();

    // Determine Source
    if let Some(node) = &app.log_focus_node {
        title = format!("Logs: {}", node.as_str());
        if let Some(logs) = app.node_logs.get(node) {
            log_lines = logs.clone();
        }
    } else {
        // Fallback to Global
        log_lines = app.logs.clone();
    }

    // Level Filter
    if app.log_levels.len() < 4 {
        // If not all enabled
        log_lines.retain(|line| {
            let mut has_tag = false;
            if line.contains("ERROR") {
                if app.log_levels.contains("ERROR") {
                    return true;
                }
                has_tag = true;
            }
            if line.contains("WARN") {
                if app.log_levels.contains("WARN") {
                    return true;
                }
                has_tag = true;
            }
            if line.contains("INFO") {
                if app.log_levels.contains("INFO") {
                    return true;
                }
                has_tag = true;
            }
            if line.contains("DEBUG") {
                if app.log_levels.contains("DEBUG") {
                    return true;
                }
                has_tag = true;
            }
            // If has_tag is true here, it means it matched a tag but that tag wasn't enabled.
            // If no tag found, we keep it (system messages, etc)
            !has_tag
        });
    }

    // Filter Logic (In-Memory substring "Grep")
    if let Some(query) = &app.log_filter_query {
        title.push_str(&format!(" [Filter: '{}']", query));
        log_lines.retain(|line| line.contains(query));
    }

    // Limit buffer processing for performance
    let max_keep = 5000;
    let total_lines = log_lines.len();
    let start_idx = total_lines.saturating_sub(max_keep);
    let display_slice = &log_lines[start_idx..];

    // Process Lines (Highlighting & ANSI)
    let styled_lines: Vec<Line> = display_slice
        .iter()
        .map(|raw_line| {
            // 1. Search Highlighting (Highest Priority)
            //
            // NOTE: This logic operates on the raw string. If ANSI codes are
            // present, they are treated as text. This can break display if a
            // match cuts an escape code.  Ideally, we'd strip ANSI before
            // searching, or search inside parsed Text.
            if let Some(search) = &app.log_search_query {
                if !search.is_empty() && raw_line.contains(search) {
                    let mut spans = Vec::new();
                    let mut last_end = 0;

                    for (idx, matched) in raw_line.match_indices(search) {
                        if idx > last_end {
                            spans.push(Span::raw(raw_line[last_end..idx].to_string()));
                        }
                        spans.push(Span::styled(
                            matched.to_string(),
                            Style::default().bg(Color::Yellow).fg(Color::Black),
                        ));
                        last_end = idx + matched.len();
                    }
                    if last_end < raw_line.len() {
                        spans.push(Span::raw(raw_line[last_end..].to_string()));
                    }
                    return Line::from(spans);
                }
            }

            // 2. ANSI Parsing
            if let Ok(text) = raw_line.into_text() {
                if let Some(line) = text.lines.into_iter().next() {
                    return line;
                }
            }

            // 3. Fallback Heuristics
            // e.g. [ERROR] red
            if raw_line.contains("ERROR") || raw_line.contains("Wait") {
                if raw_line.contains("ERROR") {
                    return Line::styled(raw_line.as_str(), Style::default().fg(Color::Red));
                } else if raw_line.contains("WARN") {
                    return Line::styled(raw_line.as_str(), Style::default().fg(Color::Yellow));
                }
            }

            Line::from(raw_line.as_str())
        })
        .collect();

    // Scroll Logic
    let view_height = area.height.saturating_sub(2) as usize; // -2 for borders
    let content_len = styled_lines.len();
    // If content is shorter than height, max_scroll is 0.
    let max_scroll = content_len.saturating_sub(view_height);

    // Auto-Scroll Sticky Check
    if app.log_stick_to_bottom {
        app.log_scroll_offset = 0;
    }

    // Clamp
    if app.log_scroll_offset > max_scroll {
        app.log_scroll_offset = max_scroll;
    }

    // Convert offset (distance from bottom) to scroll_y (distance from top)
    // offset=0 => scroll_y = max_scroll (Bottom)
    let scroll_y = max_scroll.saturating_sub(app.log_scroll_offset);
    let scroll_x = app.log_scroll_horizontal as u16;

    app.log_inner_rect = Some(area);

    // Status Bar Info
    // 0 = bottom (100%), max = top (0%)
    let percent = if max_scroll > 0 {
        ((max_scroll - scroll_y) * 100) / max_scroll
    } else {
        100
    };

    let wrap_status = if app.log_wrap { "WRAP" } else { "NOWRAP" };
    let stick_status = if app.log_stick_to_bottom {
        "STICK"
    } else {
        "MANUAL"
    };

    let info = format!(" {}% | {} | {} ", percent, wrap_status, stick_status);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(
            Line::from(vec![Span::styled(
                info,
                Style::default().fg(Color::DarkGray),
            )])
            .alignment(ratatui::layout::Alignment::Right),
        );

    let mut paragraph = Paragraph::new(styled_lines)
        .block(block)
        .scroll((scroll_y as u16, scroll_x));

    if app.log_wrap {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }

    f.render_widget(paragraph, area);

    // Render Input Box Overlay (Left aligned at bottom)
    if app.log_search_active || app.log_filter_active {
        let prompt = if app.log_search_active {
            " /"
        } else {
            " Filter: "
        };
        let mut width = (prompt.len() + app.log_input_buffer.len() + 4) as u16;
        width = width.max(20).min(area.width - 4);

        // Position at bottom left of log pane, just above border
        let input_area = Rect::new(
            area.x + 2,
            (area.y + area.height).saturating_sub(3),
            width,
            3,
        );

        let input_block = Block::default()
            .borders(Borders::ALL)
            .style(Style::default().bg(Color::Blue).fg(Color::White));

        let input_text = format!("{}{}", prompt, app.log_input_buffer);
        let p = Paragraph::new(input_text).block(input_block);

        // Clear background to ensure legibility
        f.render_widget(ratatui::widgets::Clear, input_area);
        f.render_widget(p, input_area);
    }

    // Render Level Selector Popup (Top Right)
    if app.log_level_popup_open {
        let width = 20;
        let height = 6;
        let popup_area = Rect::new(
            (area.x + area.width).saturating_sub(width + 1),
            area.y + 1,
            width,
            height,
        );

        let levels = vec!["ERROR", "WARN", "INFO", "DEBUG"];
        let items: Vec<ratatui::text::Line> = levels
            .iter()
            .map(|&l| {
                let checked = app.log_levels.contains(l);
                let symbol = if checked { "[x]" } else { "[ ]" };
                let style = if checked {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                ratatui::text::Line::styled(format!("{} {}", symbol, l), style)
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .title("Levels")
            .style(Style::default().bg(Color::DarkGray));
        let p = Paragraph::new(items).block(block);

        f.render_widget(ratatui::widgets::Clear, popup_area);
        f.render_widget(p, popup_area);
    }
}
