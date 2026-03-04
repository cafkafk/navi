use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::UnboundedSender;

use crate::nix::hive::HivePath;

use super::{
    events::AppEvent,
    execution::{start_deployment, start_diff, start_garbage_collection},
    model::{App, AppMode, MenuAction, NodeState, SettingId, TreeItem},
};

pub async fn handle_input(
    app: &mut App,
    key_event: KeyEvent,
    tx: &UnboundedSender<AppEvent>,
    hive_path: &HivePath,
    parallel: usize,
) {
    let key_code = key_event.code;

    // --- Exit Confirmation Popup ---
    if app.show_exit_confirmation {
        match key_code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('n') => {
                app.show_exit_confirmation = false;
            }
            KeyCode::Left
            | KeyCode::Char('h')
            | KeyCode::Right
            | KeyCode::Char('l')
            | KeyCode::Tab => {
                app.exit_popup_selection = !app.exit_popup_selection;
            }
            KeyCode::Enter => {
                if app.exit_popup_selection {
                    app.should_quit = true;
                } else {
                    app.show_exit_confirmation = false;
                }
            }
            KeyCode::Char('y') => {
                app.should_quit = true;
            }
            _ => {}
        }
        return;
    }

    // --- Log Level Popup Interaction ---
    if app.log_level_popup_open {
        match key_code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('E') => {
                app.log_level_popup_open = false;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.log_level_idx = (app.log_level_idx + 1) % 4; // 4 levels
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if app.log_level_idx == 0 {
                    app.log_level_idx = 3;
                } else {
                    app.log_level_idx -= 1;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let levels = vec!["ERROR", "WARN", "INFO", "DEBUG"];
                if let Some(l) = levels.get(app.log_level_idx) {
                    if app.log_levels.contains(*l) {
                        app.log_levels.remove(*l);
                    } else {
                        app.log_levels.insert(l.to_string());
                    }
                }
            }
            _ => {}
        }
        return;
    }

    // --- Input Buffers (Search / Filter) ---
    if app.log_search_active || app.log_filter_active {
        match key_code {
            KeyCode::Esc => {
                app.log_search_active = false;
                app.log_filter_active = false;
                app.log_input_buffer.clear();
            }
            KeyCode::Enter => {
                if app.log_search_active {
                    if !app.log_input_buffer.is_empty() {
                        app.log_search_query = Some(app.log_input_buffer.clone());
                    } else {
                        app.log_search_query = None;
                    }
                    app.log_search_active = false;
                } else {
                    if !app.log_input_buffer.is_empty() {
                        app.log_filter_query = Some(app.log_input_buffer.clone());
                        app.log_stick_to_bottom = true;
                        app.log_scroll_offset = 0;
                    } else {
                        app.log_filter_query = None;
                    }
                    app.log_filter_active = false;
                }
                app.log_input_buffer.clear();
            }
            KeyCode::Backspace => {
                app.log_input_buffer.pop();
            }
            KeyCode::Char(c) => {
                app.log_input_buffer.push(c);
            }
            _ => {}
        }
        return;
    }

    // --- Yank Chord Handling ---
    if app.yank_pending {
        match key_code {
            KeyCode::Esc => {
                app.yank_pending = false;
                app.yank_count_buffer.clear();
            }
            KeyCode::Char(c) if c.is_digit(10) => {
                app.yank_count_buffer.push(c);
            }
            KeyCode::Char('G') => {
                // Yank all
                // Get current log context
                // For now, simpler: just yank global logs buffer
                let text = app.logs.join("\n");
                let _ = app.clipboard_tx.send(text);
                app.flash_message =
                    Some(("All logs copied!".to_string(), std::time::Instant::now()));
                app.yank_pending = false;
                app.yank_count_buffer.clear();
            }
            KeyCode::Char('j') => {
                // Yank last N lines
                let count: usize = app.yank_count_buffer.parse().unwrap_or(1);
                let len = app.logs.len();
                let start = len.saturating_sub(count);
                let text = app.logs[start..].join("\n");
                let _ = app.clipboard_tx.send(text);
                app.flash_message = Some((
                    format!("Copied last {} lines.", count),
                    std::time::Instant::now(),
                ));
                app.yank_pending = false;
                app.yank_count_buffer.clear();
            }
            _ => {
                app.yank_pending = false;
                app.yank_count_buffer.clear();
            }
        }
        // Don't process other keys while pending yank
        return;
    }

    // Toggle Menu
    if key_code == KeyCode::Char('p') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        if app.mode == AppMode::Menu {
            app.mode = AppMode::Normal;
        } else {
            app.mode = AppMode::Menu;
            app.menu_state = crate::command::tui::model::MenuState::default();
        }
        return;
    }

    // Toggle Log Popup (L)
    if key_code == KeyCode::Char('L') && !key_event.modifiers.contains(KeyModifiers::CONTROL) {
        if app.log_popup_open {
            app.log_popup_open = false;
        } else {
            app.log_popup_open = true;
            if !app.view_global_logs {
                let flat = App::flatten_tree(&app.tree);
                if let Some(idx) = app.list_state.selected() {
                    if let Some(item) = flat.get(idx) {
                        if let TreeItem::Node { name } = item.item {
                            app.log_focus_node = Some(name.clone());
                        }
                    }
                }
            }
        }
        return;
    }

    // Clear / Reset Logs (Ctrl-L)
    if key_code == KeyCode::Char('l') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        // "Scroll to bottom and put last log line at top" -> effectively clear
        // screen We'll simulate this by setting the offset to 0 (bottom) and
        // stick-to-bottom.
        app.log_scroll_offset = 0;
        app.log_stick_to_bottom = true;
        return;
    }

    // Determine current backend based on settings
    let current_daemon = if app.use_daemon {
        app.daemon.as_ref()
    } else {
        None
    };

    if app.mode == AppMode::Menu {
        match key_code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left | KeyCode::Char('h') => {
                // Go back or close
                if let Some((parent_id, parent_idx)) = app.menu_state.navigation_stack.pop() {
                    app.menu_state.active_menu_id = parent_id;
                    app.menu_state.selected_idx = parent_idx;
                } else {
                    app.mode = AppMode::Normal;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let items = app.get_current_menu_items();
                if !items.is_empty() {
                    app.menu_state.selected_idx = (app.menu_state.selected_idx + 1) % items.len();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let items = app.get_current_menu_items();
                if !items.is_empty() {
                    if app.menu_state.selected_idx == 0 {
                        app.menu_state.selected_idx = items.len() - 1;
                    } else {
                        app.menu_state.selected_idx -= 1;
                    }
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ') | KeyCode::Char('l') => {
                let items = app.get_current_menu_items();
                if let Some(item) = items.get(app.menu_state.selected_idx) {
                    match &item.action {
                        MenuAction::Navigate(target_id) => {
                            app.menu_state
                                .navigation_stack
                                .push((app.menu_state.active_menu_id, app.menu_state.selected_idx));
                            app.menu_state.active_menu_id = *target_id;
                            app.menu_state.selected_idx = 0;
                        }
                        MenuAction::Toggle(setting_id) => match setting_id {
                            SettingId::Reboot => {
                                app.deploy_settings.reboot = !app.deploy_settings.reboot
                            }
                            SettingId::InstallBootloader => {
                                app.deploy_settings.install_bootloader =
                                    !app.deploy_settings.install_bootloader
                            }
                            SettingId::NoKeys => {
                                app.deploy_settings.no_keys = !app.deploy_settings.no_keys
                            }
                            SettingId::NoSubstitute => {
                                app.deploy_settings.no_substitute =
                                    !app.deploy_settings.no_substitute
                            }
                            SettingId::NoGzip => {
                                app.deploy_settings.no_gzip = !app.deploy_settings.no_gzip
                            }
                            SettingId::BuildOnTarget => {
                                app.deploy_settings.build_on_target =
                                    !app.deploy_settings.build_on_target
                            }
                            SettingId::ForceReplaceUnknownProfiles => {
                                app.deploy_settings.force_replace_unknown_profiles =
                                    !app.deploy_settings.force_replace_unknown_profiles
                            }
                            SettingId::KeepResult => {
                                app.deploy_settings.keep_result = !app.deploy_settings.keep_result
                            }
                            SettingId::UseDaemon => {
                                if app.daemon.is_some() {
                                    app.use_daemon = !app.use_daemon;
                                } else {
                                    if app.daemon.is_some() {
                                        app.use_daemon = !app.use_daemon;
                                    } else {
                                        app.use_daemon = false;
                                        app.flash_message = Some((
                                            "Daemon not connected.".to_string(),
                                            std::time::Instant::now(),
                                        ));
                                    }
                                }
                            }
                        },
                        MenuAction::Execute(action) => {
                            app.mode = AppMode::Normal;

                            // Determine targets
                            let mut targets = Vec::new();
                            if !app.selected.is_empty() {
                                targets = app.selected.iter().cloned().collect();
                            } else {
                                let flat = App::flatten_tree(&app.tree);
                                if let Some(idx) = app.list_state.selected() {
                                    if let Some(item) = flat.get(idx) {
                                        let mut leaves = Vec::new();
                                        App::collect_leaves(item.item, &mut leaves);
                                        targets = leaves;
                                    }
                                }
                            }

                            if targets.is_empty() {
                                return;
                            }

                            // Filter busy
                            let busy_nodes: Vec<_> = targets
                                .iter()
                                .filter(|n| {
                                    if let Some(state) = app.node_states.get(n) {
                                        matches!(state, NodeState::Running(_))
                                    } else {
                                        false
                                    }
                                })
                                .cloned()
                                .collect();

                            if !busy_nodes.is_empty() {
                                app.flash_message = Some((
                                    format!("Skipping {} busy nodes.", busy_nodes.len()),
                                    std::time::Instant::now(),
                                ));
                                targets.retain(|n| !busy_nodes.contains(n));
                            }

                            if targets.is_empty() {
                                app.flash_message = Some((
                                    "All selected nodes are busy.".to_string(),
                                    std::time::Instant::now(),
                                ));
                                return;
                            }

                            use crate::command::tui::model::LocalAction;
                            match action {
                                LocalAction::Deploy => {
                                    app.log_popup_open = true;
                                    app.view_global_logs = false;
                                    app.logs.push(format!(
                                        "Starting deployment for {} nodes...",
                                        targets.len()
                                    ));
                                    for node in &targets {
                                        app.node_states.insert(
                                            node.clone(),
                                            NodeState::Running("Queued...".to_string()),
                                        );
                                    }
                                    start_deployment(
                                        hive_path.clone(),
                                        targets,
                                        app.deploy_settings.clone(),
                                        parallel,
                                        tx.clone(),
                                        current_daemon,
                                    )
                                    .await;
                                }
                                LocalAction::Diff => {
                                    if let Some(node) = targets.first() {
                                        app.mode = AppMode::Diff;
                                        app.diff_output = Some("Loading...".to_string());
                                        app.diff_scroll_offset = 0;
                                        start_diff(
                                            hive_path.clone(),
                                            node.clone(),
                                            tx.clone(),
                                            current_daemon,
                                        )
                                        .await;
                                    }
                                }
                                LocalAction::Ssh => {
                                    if let Some(node) = targets.first() {
                                        let _ = tx.send(AppEvent::Ssh(node.clone()));
                                    }
                                }
                                LocalAction::Gc(interval) => {
                                    app.log_popup_open = true;
                                    app.view_global_logs = true;
                                    for node in &targets {
                                        app.node_states.insert(
                                            node.clone(),
                                            NodeState::Running("GC Queued...".to_string()),
                                        );
                                    }
                                    start_garbage_collection(
                                        hive_path.clone(),
                                        targets,
                                        interval.clone(),
                                        tx.clone(),
                                        current_daemon,
                                    )
                                    .await;
                                }
                                LocalAction::Inspect => {
                                    app.inspector_open = true;
                                }
                            }
                        }
                        MenuAction::Close => {
                            app.mode = AppMode::Normal;
                        }
                    }
                }
            }
            _ => {}
        }
        return;
    }

    match key_code {
        KeyCode::PageUp => {
            if app.mode == AppMode::Diff {
                app.diff_scroll_offset = app.diff_scroll_offset.saturating_sub(10);
            } else if app.mode == AppMode::Logs || app.log_popup_open {
                app.log_scroll_offset = app.log_scroll_offset.saturating_add(10);
                app.log_stick_to_bottom = false;
            }
            app.pending_g = false;
        }
        KeyCode::PageDown => {
            if app.mode == AppMode::Diff {
                app.diff_scroll_offset = app.diff_scroll_offset.saturating_add(10);
            } else if app.mode == AppMode::Logs || app.log_popup_open {
                app.log_scroll_offset = app.log_scroll_offset.saturating_sub(10);
                if app.log_scroll_offset == 0 {
                    app.log_stick_to_bottom = true;
                }
            }
            app.pending_g = false;
        }
        KeyCode::Home => {
            if app.mode == AppMode::Diff {
                app.diff_scroll_offset = 0;
            } else if app.mode == AppMode::Registrants {
                app.registrants_state.select(Some(0));
            } else if app.mode == AppMode::Logs || app.log_popup_open {
                app.log_scroll_offset = usize::MAX;
                app.log_stick_to_bottom = false;
            } else {
                app.log_scroll_offset = usize::MAX;
                app.log_stick_to_bottom = false;
            }
            app.pending_g = false;
        }
        KeyCode::End => {
            if app.mode == AppMode::Diff {
                app.diff_scroll_offset = usize::MAX;
            } else if app.mode == AppMode::Logs || app.log_popup_open {
                app.log_scroll_offset = 0;
                app.log_stick_to_bottom = true;
            } else {
                app.log_scroll_offset = 0;
                app.log_stick_to_bottom = true;
            }
            app.pending_g = false;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.pending_g = false;
            match app.mode {
                AppMode::Diff => {
                    app.diff_scroll_offset = app.diff_scroll_offset.saturating_add(1);
                }
                AppMode::Logs => {
                    app.log_scroll_offset = app.log_scroll_offset.saturating_sub(1);
                    if app.log_scroll_offset == 0 {
                        app.log_stick_to_bottom = true;
                    }
                }
                AppMode::Normal if app.log_popup_open => {
                    app.log_scroll_offset = app.log_scroll_offset.saturating_sub(1);
                    if app.log_scroll_offset == 0 {
                        app.log_stick_to_bottom = true;
                    }
                }
                AppMode::Registrants => {
                    let flat = App::flatten_registrants(&app.registrants_tree);
                    let max = flat.len();
                    if max > 0 {
                        let i = match app.registrants_state.selected() {
                            Some(i) => {
                                if i >= max - 1 {
                                    0
                                } else {
                                    i + 1
                                }
                            }
                            None => 0,
                        };
                        app.registrants_state.select(Some(i));
                    }
                }
                _ => {
                    let flat_len = App::flatten_tree(&app.tree).len();
                    let i = match app.list_state.selected() {
                        Some(i) => {
                            if i >= flat_len - 1 {
                                0
                            } else {
                                i + 1
                            }
                        }
                        None => 0,
                    };
                    app.list_state.select(Some(i));
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.pending_g = false;
            match app.mode {
                AppMode::Diff => {
                    app.diff_scroll_offset = app.diff_scroll_offset.saturating_sub(1);
                }
                AppMode::Logs => {
                    app.log_scroll_offset = app.log_scroll_offset.saturating_add(1);
                    app.log_stick_to_bottom = false;
                }
                AppMode::Normal if app.log_popup_open => {
                    app.log_scroll_offset = app.log_scroll_offset.saturating_add(1);
                    app.log_stick_to_bottom = false;
                }
                AppMode::Registrants => {
                    let flat = App::flatten_registrants(&app.registrants_tree);
                    let max = flat.len();
                    if max > 0 {
                        let i = match app.registrants_state.selected() {
                            Some(i) => {
                                if i == 0 {
                                    max - 1
                                } else {
                                    i - 1
                                }
                            }
                            None => 0,
                        };
                        app.registrants_state.select(Some(i));
                    }
                }
                _ => {
                    let flat_len = App::flatten_tree(&app.tree).len();
                    let i = match app.list_state.selected() {
                        Some(i) => {
                            if i == 0 {
                                flat_len - 1
                            } else {
                                i - 1
                            }
                        }
                        None => 0,
                    };
                    app.list_state.select(Some(i));
                }
            }
        }
        KeyCode::Char('/') => {
            if app.mode == AppMode::Logs || app.log_popup_open {
                app.log_search_active = true;
            }
        }
        KeyCode::Char('g') => {
            if app.pending_g {
                // gg
                match app.mode {
                    AppMode::Diff => {
                        app.diff_scroll_offset = 0;
                    }
                    AppMode::Registrants => {
                        app.registrants_state.select(Some(0));
                    }
                    AppMode::Logs => {
                        app.log_scroll_offset = usize::MAX;
                        app.log_stick_to_bottom = false;
                    }
                    AppMode::Normal => {
                        if app.log_popup_open {
                            app.log_scroll_offset = usize::MAX;
                            app.log_stick_to_bottom = false;
                        } else {
                            app.list_state.select(Some(0));
                        }
                    }
                    _ => {
                        app.list_state.select(Some(0));
                    }
                }
                app.pending_g = false;
            } else {
                app.pending_g = true;
            }
        }
        KeyCode::Char('R') => {
            if app.mode == AppMode::Logs || app.log_popup_open {
                app.log_filter_active = true;
            }
        }
        KeyCode::Char('E') => {
            if app.mode == AppMode::Logs || app.log_popup_open {
                app.log_level_popup_open = !app.log_level_popup_open;
            }
        }
        KeyCode::Char('G') => {
            app.pending_g = false;
            match app.mode {
                AppMode::Diff => {
                    app.diff_scroll_offset = usize::MAX;
                }
                AppMode::Registrants => {
                    let flat = App::flatten_registrants(&app.registrants_tree);
                    if !flat.is_empty() {
                        app.registrants_state.select(Some(flat.len() - 1));
                    }
                }
                AppMode::Logs => {
                    app.log_scroll_offset = 0;
                    app.log_stick_to_bottom = true;
                }
                AppMode::Normal => {
                    if app.log_popup_open {
                        app.log_scroll_offset = 0;
                        app.log_stick_to_bottom = true;
                    } else {
                        let flat_len = App::flatten_tree(&app.tree).len();
                        app.list_state.select(Some(flat_len - 1));
                    }
                }
                _ => {
                    let flat_len = App::flatten_tree(&app.tree).len();
                    app.list_state.select(Some(flat_len - 1));
                }
            }
        }
        KeyCode::Char('s') => {
            if app.mode == AppMode::Logs || app.log_popup_open {
                app.log_wrap = !app.log_wrap;
                app.flash_message = Some((
                    format!("Log Wrap: {}", app.log_wrap),
                    std::time::Instant::now(),
                ));
            } else {
                let flat = App::flatten_tree(&app.tree);
                if let Some(idx) = app.list_state.selected() {
                    if let Some(item) = flat.get(idx) {
                        let target_node = match &item.item {
                            TreeItem::Node { name } => Some(name.clone()),
                            _ => None,
                        };

                        if let Some(node) = target_node {
                            let _ = tx.send(AppEvent::Ssh(node));
                        }
                    }
                }
            }
        }
        KeyCode::Char('S') => {
            if app.mode == AppMode::Logs || app.log_popup_open {
                app.log_wrap = !app.log_wrap;
                app.flash_message = Some((
                    format!("Log Wrap: {}", app.log_wrap),
                    std::time::Instant::now(),
                ));
            }
        }
        KeyCode::Char('y') => {
            // Start yank chord
            if app.mode == AppMode::Logs || app.log_popup_open {
                app.yank_pending = true;
            } else {
                if app.mode == AppMode::Registrants {
                    if let Some(idx) = app.registrants_state.selected() {
                        let flat = App::flatten_registrants(&app.registrants_tree);
                        if let Some(item) = flat.get(idx) {
                            let target_info = match item {
                                crate::command::tui::model::RegistrantTreeItem::Domain {
                                    info,
                                    ..
                                } => Some(info.clone()),
                                crate::command::tui::model::RegistrantTreeItem::RecordGroup {
                                    domain_info,
                                    ..
                                } => Some(domain_info.clone()),
                                _ => None,
                            };
                            if let Some(domain_info) = target_info {
                                if let Some(config) = &app.registrants_config {
                                    let mut found = false;
                                    if domain_info.provider == "Porkbun" {
                                        if let Some(acc) = config.porkbun.get(&domain_info.account)
                                        {
                                            use std::process::Command;
                                            let mut api_key = String::new();
                                            let mut secret = String::new();
                                            let parts1: Vec<&str> =
                                                acc.api_key_command.split_whitespace().collect();
                                            if !parts1.is_empty() {
                                                if let Ok(o) = Command::new(parts1[0])
                                                    .args(&parts1[1..])
                                                    .output()
                                                {
                                                    if o.status.success() {
                                                        api_key =
                                                            String::from_utf8_lossy(&o.stdout)
                                                                .trim()
                                                                .to_string();
                                                    }
                                                }
                                            }
                                            let parts2: Vec<&str> = acc
                                                .secret_api_key_command
                                                .split_whitespace()
                                                .collect();
                                            if !parts2.is_empty() {
                                                if let Ok(o) = Command::new(parts2[0])
                                                    .args(&parts2[1..])
                                                    .output()
                                                {
                                                    if o.status.success() {
                                                        secret = String::from_utf8_lossy(&o.stdout)
                                                            .trim()
                                                            .to_string();
                                                    }
                                                }
                                            }
                                            if !api_key.is_empty() && !secret.is_empty() {
                                                let json_obj = serde_json::json!({ "apikey": api_key, "secretapikey": secret });
                                                let _ = app.clipboard_tx.send(json_obj.to_string());
                                                app.flash_message = Some((
                                                    "Porkbun credentials copied!".to_string(),
                                                    std::time::Instant::now(),
                                                ));
                                                found = true;
                                            }
                                        }
                                    } else if domain_info.provider == "Namecheap" {
                                        if let Some(acc) =
                                            config.namecheap.get(&domain_info.account)
                                        {
                                            use std::process::Command;
                                            let mut user = String::new();
                                            let mut key = String::new();
                                            let parts1: Vec<&str> =
                                                acc.user_command.split_whitespace().collect();
                                            if !parts1.is_empty() {
                                                if let Ok(o) = Command::new(parts1[0])
                                                    .args(&parts1[1..])
                                                    .output()
                                                {
                                                    if o.status.success() {
                                                        user = String::from_utf8_lossy(&o.stdout)
                                                            .trim()
                                                            .to_string();
                                                    }
                                                }
                                            }
                                            let parts2: Vec<&str> =
                                                acc.api_key_command.split_whitespace().collect();
                                            if !parts2.is_empty() {
                                                if let Ok(o) = Command::new(parts2[0])
                                                    .args(&parts2[1..])
                                                    .output()
                                                {
                                                    if o.status.success() {
                                                        key = String::from_utf8_lossy(&o.stdout)
                                                            .trim()
                                                            .to_string();
                                                    }
                                                }
                                            }
                                            if !user.is_empty() && !key.is_empty() {
                                                let json_obj =
                                                    serde_json::json!({ "user": user, "key": key });
                                                let _ = app.clipboard_tx.send(json_obj.to_string());
                                                app.flash_message = Some((
                                                    "Namecheap credentials copied!".to_string(),
                                                    std::time::Instant::now(),
                                                ));
                                                found = true;
                                            }
                                        }
                                    }
                                    if !found {
                                        app.flash_message = Some((
                                            "Failed to fetch keys.".to_string(),
                                            std::time::Instant::now(),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        KeyCode::Tab => {
            match app.mode {
                AppMode::Normal | AppMode::Visual | AppMode::Diff => {
                    if app.registrants_config.is_some() || !app.registrants_tree.is_empty() {
                        app.mode = AppMode::Registrants;
                        if app.registrants_state.selected().is_none() {
                            app.registrants_state.select(Some(0));
                        }
                    } else {
                        app.mode = AppMode::Logs;
                    }
                }
                AppMode::Registrants => {
                    app.mode = AppMode::Logs;
                }
                AppMode::Logs => {
                    app.mode = AppMode::Normal;
                }
                _ => {
                    app.mode = AppMode::Normal;
                }
            }
            app.pending_g = false;
        }
        KeyCode::Char('1') => {
            app.mode = AppMode::Normal;
            app.pending_g = false;
        }
        KeyCode::Char('2') => {
            if app.registrants_config.is_some() || !app.registrants_tree.is_empty() {
                app.mode = AppMode::Registrants;
                if app.registrants_state.selected().is_none() {
                    app.registrants_state.select(Some(0));
                }
            }
            app.pending_g = false;
        }
        KeyCode::Char('3') => {
            app.mode = AppMode::Logs;
            app.pending_g = false;
        }
        KeyCode::Char('i') => {
            app.inspector_open = !app.inspector_open;
        }
        KeyCode::Char('q') => {
            if app.log_popup_open {
                app.log_popup_open = false;
            } else if app.mode == AppMode::Logs {
                app.mode = AppMode::Normal;
            } else if app.mode == AppMode::Visual {
                app.mode = AppMode::Normal;
                app.visual_anchor = None;
            } else if app.mode != AppMode::Menu {
                // Trigger exit confirmation
                app.show_exit_confirmation = true;
            }
            app.pending_g = false;
        }
        KeyCode::Esc => {
            if app.show_exit_confirmation {
                app.show_exit_confirmation = false;
            } else if app.log_popup_open {
                app.log_popup_open = false;
            } else if app.mode == AppMode::Logs {
                app.mode = AppMode::Normal;
            } else if app.mode == AppMode::Diff {
                app.mode = AppMode::Normal;
                app.diff_output = None;
            } else {
                app.mode = AppMode::Normal;
                app.visual_anchor = None;
                app.pending_g = false;
            }
        }
        KeyCode::Enter => {
            if app.mode == AppMode::Registrants {
                if let Some(idx) = app.registrants_state.selected() {
                    let flat = App::flatten_registrants(&app.registrants_tree);
                    if let Some(item) = flat.get(idx) {
                        match item {
                            crate::command::tui::model::RegistrantTreeItem::RecordGroup {
                                name,
                                collapsed,
                                loaded,
                                domain_info,
                                ..
                            } => {
                                if *collapsed && !*loaded {
                                    let rec_type =
                                        if name == "DNS Records" { "DNS" } else { "Glue" };
                                    let _ = tx.send(AppEvent::FetchRegistrantRecords(
                                        domain_info.provider.clone(),
                                        domain_info.account.clone(),
                                        domain_info.domain.clone(),
                                        rec_type.to_string(),
                                    ));
                                }
                                app.toggle_registrant_collapse(idx);
                            }
                            crate::command::tui::model::RegistrantTreeItem::Domain { .. }
                            | crate::command::tui::model::RegistrantTreeItem::Provider { .. }
                            | crate::command::tui::model::RegistrantTreeItem::Account { .. } => {
                                app.toggle_registrant_collapse(idx);
                            }
                            _ => {}
                        }
                    }
                }
            } else {
                app.mode = AppMode::Menu;
                app.menu_state = crate::command::tui::model::MenuState::default();
                app.menu_state.active_menu_id = crate::command::tui::model::MenuId::NodeContext;
            }
        }
        KeyCode::Char(' ') => {
            app.pending_g = false;
            match app.mode {
                AppMode::Normal => {
                    if let Some(i) = app.list_state.selected() {
                        let flat = App::flatten_tree(&app.tree);
                        if let Some(flat_item) = flat.get(i) {
                            match flat_item.item {
                                TreeItem::Node { name } => {
                                    let name_clone = name.clone();
                                    if app.selected.contains(&name_clone) {
                                        app.selected.remove(&name_clone);
                                    } else {
                                        app.selected.insert(name_clone);
                                    }
                                }
                                TreeItem::Group { .. } => {
                                    let mut leaves = Vec::new();
                                    App::collect_leaves(flat_item.item, &mut leaves);
                                    let all_selected =
                                        leaves.iter().all(|n| app.selected.contains(n));
                                    for node in leaves {
                                        if all_selected {
                                            app.selected.remove(&node);
                                        } else {
                                            app.selected.insert(node);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                AppMode::Visual => {
                    if let (Some(anchor), Some(current)) =
                        (app.visual_anchor, app.list_state.selected())
                    {
                        let start = std::cmp::min(anchor, current);
                        let end = std::cmp::max(anchor, current);
                        let flat = App::flatten_tree(&app.tree);
                        let mut distinct_updates = std::collections::HashSet::new();

                        for i in start..=end {
                            if let Some(flat_item) = flat.get(i) {
                                let mut leaves = Vec::new();
                                App::collect_leaves(flat_item.item, &mut leaves);
                                for leaf in leaves {
                                    distinct_updates.insert(leaf);
                                }
                            }
                        }

                        let any_unselected =
                            distinct_updates.iter().any(|n| !app.selected.contains(n));

                        for node in distinct_updates {
                            if any_unselected {
                                app.selected.insert(node);
                            } else {
                                app.selected.remove(&node);
                            }
                        }

                        app.mode = AppMode::Normal;
                        app.visual_anchor = None;
                    }
                }
                _ => {}
            }
        }
        KeyCode::Char('a') => {
            for name in &app.all_nodes {
                app.selected.insert(name.clone());
            }
            app.pending_g = false;
        }
        KeyCode::Char('n') | KeyCode::Char('x') => {
            app.selected.clear();
            app.pending_g = false;
        }
        KeyCode::Char('d') => {
            if !app.selected.is_empty() {
                let mut targets: Vec<_> = app.selected.iter().cloned().collect();
                let busy_nodes: Vec<_> = targets
                    .iter()
                    .filter(|n| {
                        if let Some(state) = app.node_states.get(n) {
                            matches!(state, NodeState::Running(_))
                        } else {
                            false
                        }
                    })
                    .cloned()
                    .collect();

                if !busy_nodes.is_empty() {
                    app.flash_message = Some((
                        format!("Skipping {} busy nodes.", busy_nodes.len()),
                        std::time::Instant::now(),
                    ));
                    targets.retain(|n| !busy_nodes.contains(n));
                }

                if !targets.is_empty() {
                    app.view_global_logs = false;
                    app.logs.push(format!(
                        "Starting deployment for {} nodes...",
                        targets.len()
                    ));
                    for node in &targets {
                        app.node_states
                            .insert(node.clone(), NodeState::Running("Queued...".to_string()));
                    }
                    start_deployment(
                        hive_path.clone(),
                        targets,
                        app.deploy_settings.clone(),
                        parallel,
                        tx.clone(),
                        current_daemon,
                    )
                    .await;
                }
            }
        }
        _ => {
            if app.pending_g {
                app.pending_g = false;
            }
        }
    }
}
