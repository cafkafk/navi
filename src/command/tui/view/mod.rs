pub mod boot;
pub mod diff;
pub mod inspector;
pub mod logging;
pub mod menu;
pub mod nodes;
pub mod quotes;
pub mod registrants;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs},
    Frame,
};

use crate::command::tui::model::{App, AppMode};

use boot::draw_boot;
use diff::draw_diff;
use inspector::draw_inspector_popup;
use logging::draw_logs;
use menu::draw_menu;
use nodes::draw_nodes;
use registrants::draw_registrants;

pub fn draw(f: &mut Frame, app: &mut App) {
    if app.mode == AppMode::Boot {
        draw_boot(f, app);
        return;
    }

    // Master Layout: Top Bar (1), Content (Min 0), Bottom Bar (1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.size());

    let top_bar_area = chunks[0];
    let content_area = chunks[1];
    let bottom_bar_area = chunks[2];

    // --- Top Bar (Tabs) ---
    draw_top_bar(f, app, top_bar_area);

    // --- Content ---
    match app.mode {
        AppMode::Logs => {
            // Full screen logs (Tab 3)
            draw_logs(f, app, content_area);
        }
        AppMode::Registrants => {
            draw_registrants(f, app, content_area);
        }
        AppMode::Diff => {
            draw_diff(f, app, content_area);
        }
        _ => {
            // Normal / Visual / etc -> Draw Nodes
            draw_nodes(f, app, content_area);

            // Log Popup Overlay
            if app.log_popup_open {
                let popup_area = centered_rect(75, 75, content_area);
                f.render_widget(Clear, popup_area);
                draw_logs(f, app, popup_area);
            }
        }
    }

    // Overlay: Inspector Popup
    if app.inspector_open {
        draw_inspector_popup(f, app);
    }

    // Overlay: Menu
    if app.mode == AppMode::Menu {
        draw_menu(f, app);
    }

    // Overlay: Exit Confirmation
    if app.show_exit_confirmation {
        // Smaller fixed size popup
        let width = 44;
        let height = 7;
        let size = f.size();
        let area = Rect::new(
            size.width.saturating_sub(width) / 2,
            size.height.saturating_sub(height) / 2,
            width,
            height,
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                "Quit App",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Reset).fg(Color::Yellow)); // Soft yellow border

        let yes_style = if app.exit_popup_selection {
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };

        let no_style = if !app.exit_popup_selection {
            Style::default()
                .bg(Color::Red)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red)
        };

        let text = vec![
            Line::from(vec![Span::raw("")]),
            Line::from(vec![Span::raw("Are you sure you want to exit?")]),
            Line::from(vec![Span::raw("")]),
            Line::from(vec![
                Span::raw("      "), // spacer
                Span::styled(" [ Yes ] ", yes_style),
                Span::raw("      "), // spacer
                Span::styled(" [ No ] ", no_style),
            ]),
        ];

        let paragraph = Paragraph::new(text)
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);

        // Clear background for popup
        f.render_widget(Clear, area);
        f.render_widget(paragraph, area);
    }

    // --- Bottom Bar (Status) ---
    draw_bottom_bar(f, app, bottom_bar_area);
}

fn draw_top_bar(f: &mut Frame, app: &App, area: Rect) {
    let titles = vec![" [1] HOSTS ", " [2] REGISTRANTS ", " [3] LOGS "];

    let selected_index = match app.mode {
        AppMode::Registrants => 1,
        AppMode::Logs => 2,
        _ => 0,
    };

    let tabs = Tabs::new(titles)
        .select(selected_index)
        .style(Style::default().fg(Color::Gray)) // Inactive
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ) // Active: Standard Blue
        .divider(" ");

    // We render a standard background
    let bar_block = Block::default().style(Style::default().bg(Color::Reset));
    f.render_widget(bar_block, area);
    f.render_widget(tabs, area);
}

fn draw_bottom_bar(f: &mut Frame, app: &App, area: Rect) {
    // Left: Mode
    let mode_str = match app.mode {
        AppMode::Normal => " NORMAL ",
        AppMode::Visual => " VISUAL ",
        AppMode::Diff => " DIFF ",
        AppMode::Registrants => " REGISTRANTS ",
        AppMode::Logs => " LOGS ",
        AppMode::Menu => " MENU ",
        AppMode::Boot => " BOOT ",
    };

    let mode_style = match app.mode {
        AppMode::Visual => Style::default()
            .bg(Color::Yellow)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
        AppMode::Diff => Style::default()
            .bg(Color::Magenta)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
        AppMode::Logs => Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
        AppMode::Registrants => Style::default()
            .bg(Color::Cyan)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
        _ => {
            if app.log_popup_open {
                // Indicate popup mode
                Style::default()
                    .bg(Color::Green)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else if app.show_exit_confirmation {
                Style::default()
                    .bg(Color::Yellow)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            }
        }
    };

    // Override mode string if popup is open in Normal mode
    let mode_display = if app.show_exit_confirmation {
        " CONFIRM EXIT "
    } else if app.mode == AppMode::Normal && app.log_popup_open {
        " LOG POPUP "
    } else if app.tree_loading && app.mode == AppMode::Normal {
        " REFRESHING "
    } else {
        mode_str
    };

    // Right side: Daemon Status
    let daemon_status = if app.use_daemon {
        if app.daemon.is_some() {
            "D:ON"
        } else {
            "D:ERR"
        }
    } else {
        "D:OFF"
    };
    let daemon_color = if app.use_daemon && app.daemon.is_some() {
        Color::Green
    } else if app.use_daemon {
        Color::Red
    } else {
        Color::Gray
    };

    // Center: Message
    let msg = if let Some((text, _)) = &app.flash_message {
        text.clone()
    } else {
        match app.mode {
            AppMode::Visual => "Range Select. [Space] Toggle | [Esc] Cancel".to_string(),
            AppMode::Logs => {
                "/ Search | [R] Filter | [s] Wrap | [y] Yank | [Ctrl-L] Stick".to_string()
            }
            AppMode::Diff => "Scroll [j/k] | [Esc] Close".to_string(),
            _ => {
                if app.show_exit_confirmation {
                    "[Enter/y] Yes | [Esc/q/n] No".to_string()
                } else if app.log_popup_open {
                    "/ Search | [R] Filter | [s] Wrap | [y] Yank | [L] Close".to_string()
                } else if app.tree_loading {
                    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                    let step = (app.tick_count as usize / 2) % spinner.len();
                    format!("{} Refreshing...", spinner[step])
                } else {
                    "Ready. [Space] Select | [i] Inspect | [d] Deploy | [L] Logs Popup".to_string()
                }
            }
        }
    };

    // Right: Stats
    let (used, _total) = app.ram_usage;
    let used_gb = used as f64 / 1024.0 / 1024.0 / 1024.0;

    // Git
    let git_short = app.git_rev.chars().take(6).collect::<String>();

    // Tasks
    let tasks_str = if !app.active_tasks.is_empty() {
        format!("⚙ {}", app.active_tasks.len())
    } else {
        "".to_string()
    };

    let stats = format!("{}  MEM: {:.1}G  REV: {} ", tasks_str, used_gb, git_short);

    // Standard Terminal Colors (No blinding white)
    let bar_bg = Color::DarkGray;
    let bar_fg = Color::White;

    let spans = vec![
        Span::styled(mode_display, mode_style),
        Span::styled(" ", Style::default().bg(bar_bg)), // spacer
        Span::styled(msg, Style::default().bg(bar_bg).fg(bar_fg)),
    ];

    let left_line = Line::from(spans);

    let right_spans = vec![
        Span::styled(stats, Style::default().bg(bar_bg).fg(bar_fg)),
        Span::styled(" | ", Style::default().bg(bar_bg).fg(Color::DarkGray)),
        Span::styled(
            daemon_status,
            Style::default()
                .bg(bar_bg)
                .fg(daemon_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", Style::default().bg(bar_bg)),
    ];
    let right_line = Line::from(right_spans);

    let left_widget = Paragraph::new(left_line);
    let right_widget = Paragraph::new(right_line).alignment(ratatui::layout::Alignment::Right);

    // Background
    f.render_widget(Block::default().style(Style::default().bg(bar_bg)), area);

    f.render_widget(left_widget, area);
    f.render_widget(right_widget, area);
}

// Helper for centering a rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
