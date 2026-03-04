use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

use crate::command::tui::model::{App, MenuAction};

pub fn draw_menu(f: &mut Frame, app: &mut App) {
    let area = f.size();

    // Centered rect
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(area);

    let popup_layout_h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(popup_layout[1]);

    let rect = popup_layout_h[1];

    let items: Vec<ListItem> = app
        .get_current_menu_items()
        .iter()
        .map(|i| {
            let prefix = match i.checked {
                Some(true) => "[x] ",
                Some(false) => "[ ] ",
                None => match i.action {
                    MenuAction::Navigate(_) => "> ",
                    _ => "  ",
                },
            };

            ListItem::new(format!("{}{}", prefix, i.label))
        })
        .collect();

    let title = match app.menu_state.active_menu_id {
        crate::command::tui::model::MenuId::Main => "Main Menu",
        crate::command::tui::model::MenuId::DeploySettings => "Deploy Settings",
        crate::command::tui::model::MenuId::NodeContext => "Node Operations",
        crate::command::tui::model::MenuId::GarbageCollectInterval => "GC Interval",
    };

    let items_list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", title)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(Some(app.menu_state.selected_idx));

    f.render_widget(Clear, rect); // Clear underneath
    f.render_stateful_widget(items_list, rect, &mut state);
}
