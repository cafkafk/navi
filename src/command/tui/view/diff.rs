use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Paragraph, Wrap},
    Frame,
};

use crate::command::tui::model::App;

pub fn draw_diff(f: &mut Frame, app: &mut App, area: Rect) {
    let diff_text = app.diff_output.as_deref().unwrap_or("Calculating diff...");

    // Simple colorizing based on lines
    // Red for removals, Green for additions
    use ratatui::text::{Line, Span};

    let lines: Vec<Line> = diff_text
        .lines()
        .map(|l| {
            let style = if l.starts_with('+') || l.contains(" > ") {
                Style::default().fg(Color::Green)
            } else if l.starts_with('-') || l.contains(" < ") {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            };
            Line::from(Span::styled(l, style))
        })
        .collect();

    let block = Block::default(); // Empty block or just no block

    // Calculate Scroll
    let inner_height = area.height as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(inner_height);

    if app.diff_scroll_offset > max_scroll {
        app.diff_scroll_offset = max_scroll;
    }

    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((app.diff_scroll_offset as u16, 0));

    f.render_widget(p, area);
}
