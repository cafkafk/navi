use crate::command::tui::view::quotes::LAIN_QUOTES;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, style::Color, Terminal};
use tokio::sync::{mpsc, Semaphore};

use crate::error::NaviResult;
use crate::job::JobMonitor;
use crate::nix::evaluator::{DrvSetEvaluator, NixEvalJobs};
use crate::nix::host::{Host, Local as LocalHost};
use crate::nix::{Hive, NixFlags, NodeConfig, NodeName};
use crate::progress::Message;
use crate::registrants::DomainInfo;
use arboard::Clipboard;

use crate::command::tui::events::AppEvent;
use crate::command::tui::input::handle_input;
use crate::command::tui::logging::init_tui_logging;
use crate::command::tui::model::{
    build_tree, App, AppMode, NodeState, ProvenanceStatus, RegistrantTreeItem, TreeCache,
}; // added TreeCache
use crate::command::tui::view::draw;
use crate::command::tui::Opts;
use crate::daemon::client::DaemonClient;
use crate::daemon::protocol::{DaemonEvent, Request, Response};

fn get_cache_path(context_dir: &std::path::Path) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let mut path = std::path::PathBuf::from(home);
    path.push(".cache");
    path.push("navi");

    for component in context_dir.components() {
        if let std::path::Component::Normal(c) = component {
            path.push(c);
        }
    }
    // Fix: create parent
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).ok();
    }
    path.push("tree.json");
    path
}

// Extracted run loop that takes an App instance.
// This allows both the normal `run` function and the `run_demo` function to reuse the UI logic.
pub async fn run_with_app(mut app: App) -> NaviResult<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Communication channel
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    // Init logging (Bridge tracing -> AppEvent::Progress) for Demo mode logs
    let (log_bridge_tx, mut log_bridge_rx) = mpsc::unbounded_channel::<Message>();
    init_tui_logging(log_bridge_tx.clone());

    let main_tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(msg) = log_bridge_rx.recv().await {
            let _ = main_tx_clone.send(AppEvent::Progress(msg));
        }
    });

    run_ui_loop(app, terminal, tx, rx, None, crate::nix::HivePath::Legacy(std::path::PathBuf::from(".")), None, vec!["region".to_string(), "role".to_string()], log_bridge_tx, 1).await
}

pub async fn run(mut hive: Hive, opts: Opts) -> NaviResult<()> {
    // For TUI, we default to enabling Daemon.
    let env_no_daemon = std::env::var("NAVI_NO_DAEMON").is_ok();

    // Attempt to connect to daemon
    let client = if !env_no_daemon {
        match DaemonClient::connect(true).await {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!(
                    "Failed to connect to daemon (and failed to auto-start): {}",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    // Enable show trace for better error reporting in TUI
    hive.set_show_trace(true);

    // Explicitly start daemon task to ensure it's alive while TUI is initializing
    if let Some(c) = &client {
        let _ = c.send(Request::GetState).await;
    }

    let hive = Arc::new(hive);
    let parallel = opts.parallel;

    // Cache Check
    let context_dir = hive
        .context_dir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let cache_path = get_cache_path(&context_dir);

    // Try load cache
    let cached_data = if let Ok(content) = tokio::fs::read_to_string(&cache_path).await {
        if let Ok(mut cache) = serde_json::from_str::<TreeCache>(&content) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            // 10 minutes cache validity for provenance
            if now.saturating_sub(cache.timestamp) > 600 {
                cache.provenance.clear();
            }
            Some(cache)
        } else {
            None
        }
    } else {
        None
    };

    let (
        nodes,
        initial_configs,
        tree,
        mut hierarchy,
        meta_config,
        from_cache,
        cached_states,
        cached_updates,
        cached_provenance,
    ) = if let Some(cache) = cached_data {
        let default_hierarchy = vec![
            "category".to_string(),
            "environment".to_string(),
            "hostgroup".to_string(),
        ];
        (
            cache.nodes,
            cache.configs,
            cache.tree,
            default_hierarchy,
            None::<crate::nix::MetaConfig>,
            true,
            cache.node_states,
            cache.node_last_updated,
            cache.provenance,
        )
    } else {
        let n = hive.node_names().await?;
        let m = hive.get_meta_config().await?.clone();
        let default_hierarchy = vec![
            "category".to_string(),
            "environment".to_string(),
            "hostgroup".to_string(),
        ];
        let h = m.hierarchy.as_ref().unwrap_or(&default_hierarchy).clone();
        let t = build_tree(&n, &HashMap::new(), &h);
        (
            n,
            HashMap::new(),
            t,
            h,
            Some(m),
            false,
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
        )
    };

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Detect Background Color (OSC 11)
    let mut term_bg = None;
    {
        use std::io::Write;
        let mut stdout = std::io::stdout();
        // Sending OSC 11 to query background color
        if stdout.write_all(b"\x1b]11;?\x07").is_ok() {
            let _ = stdout.flush();

            let mut buffer = String::new();
            let start = std::time::Instant::now();

            // Wait up to 150ms for response
            while start.elapsed() < Duration::from_millis(150) {
                if event::poll(Duration::from_millis(10)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        match key.code {
                            event::KeyCode::Char(c) => buffer.push(c),
                            event::KeyCode::Esc => buffer.push_str("\x1b"),
                            _ => {}
                        }
                    }
                }
                if buffer.ends_with('\x07') || buffer.len() > 30 {
                    break;
                }
            }

            if let Some(start_idx) = buffer.find("rgb:") {
                // Format: ...rgb:RRRR/GGGG/BBBB...
                let content = &buffer[start_idx + 4..]
                    .trim_end_matches('\x07')
                    .trim_end_matches('\x1b')
                    .trim_end_matches('\\');
                let parts: Vec<&str> = content.split('/').collect();
                if parts.len() == 3 {
                    let parse_channel = |s: &str| -> u8 {
                        let slice = if s.len() >= 2 { &s[0..2] } else { s };
                        u8::from_str_radix(slice, 16).unwrap_or(0)
                    };

                    let r = parse_channel(parts[0]);
                    let g = parse_channel(parts[1]);
                    let b = parse_channel(parts[2]);
                    term_bg = Some(Color::Rgb(r, g, b));
                }
            }
        }
    }

    // Fallback: Check COLORFGBG
    if term_bg.is_none() {
        if let Ok(v) = std::env::var("COLORFGBG") {
            let parts: Vec<&str> = v.split(';').collect();
            if parts.len() >= 2 {
                let bg = parts.last().unwrap().trim();
                if matches!(
                    bg,
                    "7" | "15" | "231" | "252" | "253" | "254" | "255" | "white" | "White"
                ) {
                    term_bg = Some(Color::Rgb(255, 255, 255));
                }
            }
        }
    }

    // Clipboard Actor
    let (clipboard_tx, mut clipboard_rx) = mpsc::unbounded_channel::<String>();

    // Spawn dedicated thread for clipboard persistence (critical for X11)
    std::thread::spawn(move || {
        // Initialize clipboard context once
        let mut clipboard_ctx = match Clipboard::new() {
            Ok(ctx) => Some(ctx),
            Err(_) => None,
        };

        while let Some(text) = clipboard_rx.blocking_recv() {
            // Try arboard first
            let mut success = false;

            // Re-init if needed
            if clipboard_ctx.is_none() {
                clipboard_ctx = Clipboard::new().ok();
            }

            if let Some(ref mut ctx) = clipboard_ctx {
                if ctx.set_text(text.clone()).is_ok() {
                    success = true;
                }
            }

            // Fallback to external commands if arboard failed (NixOS runtime lib issues)
            if !success {
                use std::io::Write;
                // Try wl-copy (Wayland)
                let mut child = std::process::Command::new("wl-copy")
                    .stdin(std::process::Stdio::piped())
                    .spawn();
                if child.is_err() {
                    // Try xclip (X11)
                    child = std::process::Command::new("xclip")
                        .args(["-selection", "clipboard"])
                        .stdin(std::process::Stdio::piped())
                        .spawn();
                }
                if child.is_err() {
                    // Try xsel (X11)
                    child = std::process::Command::new("xsel")
                        .args(["-b", "-i"])
                        .stdin(std::process::Stdio::piped())
                        .spawn();
                }

                if let Ok(mut c) = child {
                    if let Some(mut stdin) = c.stdin.take() {
                        let _ = stdin.write_all(text.as_bytes());
                    }
                    let _ = c.wait(); // Background thread so this wait is safe/fine
                }
            }
        }
    });

    // Get git status
    let git_rev = match std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => "unknown".to_string(),
    };

    let registrants_config = meta_config.as_ref().and_then(|m| m.registrants.clone());

    let mut app = App::new(
        nodes.clone(),
        initial_configs,
        tree,
        clipboard_tx,
        git_rev,
        registrants_config,
        term_bg,
    );

    // Set Daemon client
    app.daemon = client.clone();
    app.use_daemon = client.is_some();

    if from_cache {
        app.tree_loading = true;
        app.node_states = cached_states;
        app.node_last_updated = cached_updates;
        app.remote_provenance = cached_provenance;
    }
    app.list_state.select(Some(0));

    // Init loading state
    if !from_cache {
        for node in &nodes {
            app.node_states.insert(node.clone(), NodeState::Loading);
        }
    }

    // Communication channel
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    // Init logging (Bridge tracing -> AppEvent::Progress)
    let (log_bridge_tx, mut log_bridge_rx) = mpsc::unbounded_channel::<Message>();
    init_tui_logging(log_bridge_tx.clone());

    // Bridge log messages to main loop
    let main_tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(msg) = log_bridge_rx.recv().await {
            let _ = main_tx_clone.send(AppEvent::Progress(msg));
        }
    });

    let hive_path = hive.path().clone();

    run_ui_loop(app, terminal, tx, rx, Some(hive), hive_path, meta_config, hierarchy, log_bridge_tx, parallel).await
}

async fn run_ui_loop(
    mut app: App,
    mut terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    tx: mpsc::UnboundedSender<AppEvent>,
    mut rx: mpsc::UnboundedReceiver<AppEvent>,
    hive: Option<Arc<Hive>>,
    hive_path: crate::nix::HivePath,
    meta_config: Option<crate::nix::MetaConfig>,
    mut hierarchy: Vec<String>,
    log_bridge_tx: mpsc::UnboundedSender<Message>,
    parallel: usize,
) -> NaviResult<()> {
    // Bridge Daemon events
    if let Some(c) = &app.daemon {
        let tx_daemon = tx.clone();
        let client_ws = c.clone();
        tokio::spawn(async move {
            while let Some(response) = client_ws.next_response().await {
                match response {
                    Response::Event(event) => match event {
                        DaemonEvent::Log(msg) => {
                            let _ = tx_daemon.send(AppEvent::Progress(Message::Print(
                                crate::progress::Line::new(crate::job::JobId::new(), msg),
                            )));
                        }
                        DaemonEvent::NodeStateChanged(name, state) => {
                            let _ = tx_daemon.send(AppEvent::NodeStateChanged(name, state));
                        }
                        DaemonEvent::TaskStarted(uuid, desc) => {
                            let _ = tx_daemon.send(AppEvent::TaskStarted(uuid, desc));
                        }
                        DaemonEvent::TaskFinished(uuid) => {
                            let _ = tx_daemon.send(AppEvent::TaskFinished(uuid));
                        }
                        DaemonEvent::DiffComputed(output) => {
                            let _ = tx_daemon.send(AppEvent::DiffComputed(output));
                        }
                        DaemonEvent::NodeLog(name, msg) => {
                            let _ = tx_daemon.send(AppEvent::Progress(Message::Print(
                                crate::progress::Line::new(crate::job::JobId::new(), msg)
                                    .label(name.as_str().to_string()),
                            )));
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        });
    }

    // Start Registrants Fetcher
    if let Some(meta) = &meta_config {
        if let Some(reg_config) = &meta.registrants {
            let reg_tx = tx.clone();
            let config = reg_config.clone();
            tokio::spawn(async move {
                let (domains, errors) = crate::registrants::fetch_all(&config).await;
                let _ = reg_tx.send(AppEvent::RegistrantsLoaded(domains, errors));
            });
        }
    }

    // --- Parallel Metadata Fetcher (Batched Nix-Eval-Jobs) ---
    if let Some(hive_ref) = &hive {
        let fetch_tx = tx.clone();
        let log_bridge_tx_for_monitor = log_bridge_tx.clone();
        let hive_task = hive_ref.clone();
        let nodes_initial = app.all_nodes.clone();
        let meta_initial = meta_config.clone();

        tokio::spawn(async move {
            let (fresh_nodes, _fresh_meta) = if let Some(m) = meta_initial {
                (nodes_initial, m)
            } else {
                let n = hive_task.node_names().await.expect("Failed refetch");
                let m = hive_task
                    .get_meta_config()
                    .await
                    .expect("Failed refetch")
                    .clone();
                let _ = fetch_tx.send(AppEvent::MetaLoaded(m.clone()));
                (n, m)
            };

            let chunk_size = 5;
            let chunks: Vec<Vec<NodeName>> =
                fresh_nodes.chunks(chunk_size).map(|c| c.to_vec()).collect();
            let hive_arc = hive_task; // Reuse

            // Create JobMonitor to capture nix-eval-jobs logs
            let (monitor, meta_job) = JobMonitor::new(Some(log_bridge_tx_for_monitor));

            let eval_tx = fetch_tx.clone();

            // Spawn monitor loop (it will push Messages to log_bridge_tx matching logs)
            let _monitor_task = tokio::spawn(async move {
                let _ = monitor.run_until_completion().await;
            });

            // Run in job context
            let run_future = meta_job.run(|root_job| async move {
                let mut evaluator = NixEvalJobs::default();
                evaluator.set_eval_limit(parallel);
                evaluator.set_job(root_job.clone());

                let expr_res = hive_arc.eval_selected_config_chunks_expr(chunks);
                let expr = match expr_res {
                    Ok(e) => e,
                    Err(e) => return Err(e),
                };

                let mut flags = hive_arc.nix_flags();
                flags.set_impure(true); // Allow local flakes

                match evaluator.evaluate(&expr, flags).await {
                    Ok(mut stream) => {
                        use futures::StreamExt;
                        while let Some(result) = stream.next().await {
                            match result {
                                Ok(attr_out) => {
                                    let drv_path = attr_out.drv_path();
                                    let drv_path_str = drv_path.display().to_string();
                                    let tx_inner = eval_tx.clone();

                                    tokio::spawn(async move {
                                        let build_res = tokio::process::Command::new("nix-store")
                                            .arg("-r")
                                            .arg(&drv_path_str)
                                            .output()
                                            .await;

                                        if let Ok(output) = build_res {
                                            if output.status.success() {
                                                let out_path =
                                                    String::from_utf8_lossy(&output.stdout)
                                                        .trim()
                                                        .to_string();
                                                if let Ok(json) =
                                                    tokio::fs::read_to_string(&out_path).await
                                                {
                                                    if let Ok(configs) = serde_json::from_str::<
                                                        HashMap<NodeName, NodeConfig>,
                                                    >(
                                                        &json
                                                    ) {
                                                        for (name, config) in configs {
                                                            let _ = tx_inner.send(
                                                                AppEvent::ConfigLoaded(
                                                                    name, config,
                                                                ),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    });
                                }
                                Err(e) => {
                                    let _ = eval_tx.send(AppEvent::Progress(Message::PrintMeta(
                                        crate::progress::Line::new(
                                            crate::job::JobId::new(),
                                            format!("Eval attribute error: {:?}", e),
                                        )
                                        .style(crate::progress::LineStyle::Failure)
                                        .label("System".to_string()),
                                    )));
                                }
                            }
                        }
                    }
                    Err(e) => return Err(e),
                }
                Ok(())
            });

            if let Err(e) = run_future.await {
                let _ = fetch_tx.send(AppEvent::Progress(Message::PrintMeta(
                    crate::progress::Line::new(
                        crate::job::JobId::new(),
                        format!("Eval Job Failed: {}", e),
                    )
                    .style(crate::progress::LineStyle::Failure)
                    .label("System".to_string()),
                )));
            }

            let _ = fetch_tx.send(AppEvent::TreeRebuild);
        });
    }

    // Semaphore for provenance fetching (SSH)
    let prov_semaphore = Arc::new(Semaphore::new(10));

    // RAM Monitor

    let ram_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            if let Ok(contents) = tokio::fs::read_to_string("/proc/meminfo").await {
                let mut total = 0;
                let mut available = 0;

                for line in contents.lines() {
                    if let Some(rest) = line.strip_prefix("MemTotal:") {
                        if let Some(val) = rest.trim().split_whitespace().next() {
                            if let Ok(v) = val.parse::<u64>() {
                                total = v * 1024; // kB to bytes
                            }
                        }
                    }
                    if let Some(rest) = line.strip_prefix("MemAvailable:") {
                        if let Some(val) = rest.trim().split_whitespace().next() {
                            if let Ok(v) = val.parse::<u64>() {
                                available = v * 1024;
                            }
                        }
                    }
                }

                if total > 0 {
                    let used = total.saturating_sub(available);
                    let _ = ram_tx.send(AppEvent::RamUpdate(used, total));
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    loop {
        // Game of Life Tick
        if app.mode == AppMode::Boot {
            let size = terminal.size().unwrap_or_default();
            app.game_of_life
                .resize(size.width as usize, size.height as usize);

            // Animation / Reset Cycle: 700 ticks (approx 10-12s depending on loop)
            // 0-500: Normal GoL (approx 8s)
            // 500-700: Black out + Text Animation (approx 3s)
            // 700 -> 0: GoL Restarts
            let cycle_len = 700;
            let cycle_pos = app.tick_count % cycle_len;
            let cycle_idx = app.tick_count / cycle_len;

            // Reset loop after Cycle 5 extended sequence
            // 5 cycles * 700 = 3500
            // Cycle 5 Hold (20s) = 1200
            // Cycle 5 Fade (10s) = 600
            // Exit Anim (~13s) = 800 (approx safe buffer)
            // Total ~ 6100
            if app.tick_count > 6100 {
                app.tick_count = 0;
                app.game_of_life.reset();
            }

            // At start of animation phase (500), we trigger a reset for when it comes back at 700/0.
            if cycle_pos == 500 {
                app.game_of_life.reset();
            }

            // Game of Life simulation
            // Slow down the simulation slightly relative to UI loop
            if app.tick_count % 2 == 0 {
                app.game_of_life.tick();
            }

            // Wired State Update (Cycle 5+)
            if cycle_idx >= 5 {
                let mut rng = rand::rng();
                // Spawn new phantoms occasionally
                if rng.random_bool(0.15) {
                    // 15% chance per tick
                    let text_idx = rng.random_range(0..LAIN_QUOTES.len());
                    let text = LAIN_QUOTES[text_idx];
                    let text_len = text.len() as u16;

                    let max_x = size.width.saturating_sub(text_len).max(1);
                    let x = rng.random_range(0..max_x);
                    let y = rng.random_range(0..size.height);

                    let lifetime = rng.random_range(60..180); // 1-3 seconds

                    app.wired_state
                        .phantoms
                        .push(crate::command::tui::model::WiredPhantom {
                            text_idx,
                            x,
                            y,
                            spawn_tick: app.tick_count,
                            lifetime,
                        });
                }

                let current_tick = app.tick_count;
                app.wired_state.phantoms.retain(|p| {
                    let elapsed = current_tick.saturating_sub(p.spawn_tick);
                    elapsed < p.lifetime
                });
            }
        }

        app.tick_count = app.tick_count.wrapping_add(1);
        terminal.draw(|f| {
            draw(f, &mut app);
        })?;

        // Event Loop
        if event::poll(Duration::from_millis(16))? {
            let evt = event::read()?;
            match evt {
                Event::Mouse(m) => {
                    // Tab Bar Click
                    if m.kind == MouseEventKind::Down(event::MouseButton::Left) && m.row == 0 {
                        if m.column < 11 {
                            app.mode = AppMode::Normal;
                        } else if m.column < 26 {
                            if app.registrants_config.is_some() {
                                app.mode = AppMode::Registrants;
                            }
                        } else if m.column < 36 {
                            app.mode = AppMode::Logs;
                        }
                    }

                    // Determine log pane area for scroll/hit-testing
                    let log_rect = if app.mode == AppMode::Logs {
                        let size = terminal.size().ok().unwrap_or_default();
                        // Content area is row 1 to height-2 (top bar + bottom bar)
                        Some(ratatui::layout::Rect::new(
                            0,
                            1,
                            size.width,
                            size.height.saturating_sub(2),
                        ))
                    } else if app.log_popup_open {
                        let size = terminal.size().ok().unwrap_or_default();
                        let content_area = ratatui::layout::Rect::new(
                            0,
                            1,
                            size.width,
                            size.height.saturating_sub(2),
                        );

                        // Duplicated centered_rect logic (75%)
                        let percent_x = 75;
                        let percent_y = 75;

                        let popup_layout = ratatui::layout::Layout::default()
                            .direction(ratatui::layout::Direction::Vertical)
                            .constraints([
                                ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
                                ratatui::layout::Constraint::Percentage(percent_y),
                                ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
                            ])
                            .split(content_area);

                        let popup_rect = ratatui::layout::Layout::default()
                            .direction(ratatui::layout::Direction::Horizontal)
                            .constraints([
                                ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
                                ratatui::layout::Constraint::Percentage(percent_x),
                                ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
                            ])
                            .split(popup_layout[1])[1];

                        Some(popup_rect)
                    } else {
                        None
                    };

                    match m.kind {
                        MouseEventKind::Down(event::MouseButton::Left) => {
                            // Only select nodes if NOT clicking inside log popup
                            let mut clicked_log = false;
                            if let Some(r) = log_rect {
                                if m.column >= r.x
                                    && m.column < r.x + r.width
                                    && m.row >= r.y
                                    && m.row < r.y + r.height
                                {
                                    clicked_log = true;
                                }
                            }

                            if !clicked_log && !app.log_popup_open {
                                app.selection_start = Some((m.column, m.row));
                                app.selection_end = Some((m.column, m.row));
                            }
                        }
                        MouseEventKind::Drag(event::MouseButton::Left) => {
                            if app.selection_start.is_some() {
                                app.selection_end = Some((m.column, m.row));

                                if let Some(r) = log_rect {
                                    // Autoscroll if dragging past log pane vertical bounds
                                    if m.row < r.y {
                                        app.log_scroll_offset =
                                            app.log_scroll_offset.saturating_add(1);
                                    } else if m.row > (r.y + r.height) {
                                        app.log_scroll_offset =
                                            app.log_scroll_offset.saturating_sub(1);
                                    }
                                }
                            }
                        }
                        MouseEventKind::Up(event::MouseButton::Left) => {
                            if let (Some(start), Some(end)) =
                                (app.selection_start, app.selection_end)
                            {
                                app.selection_start = None;
                                app.selection_end = None;

                                // Trigger copy on next frame (where buffer is valid)
                                if start != end {
                                    app.copy_region = Some((start, end));
                                }
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            if let Some(r) = log_rect {
                                if m.column >= r.x
                                    && m.column < r.x + r.width
                                    && m.row >= r.y
                                    && m.row < r.y + r.height
                                {
                                    app.log_scroll_offset = app.log_scroll_offset.saturating_sub(3);
                                    if app.log_scroll_offset == 0 {
                                        app.log_stick_to_bottom = true;
                                    }
                                }
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if let Some(r) = log_rect {
                                if m.column >= r.x
                                    && m.column < r.x + r.width
                                    && m.row >= r.y
                                    && m.row < r.y + r.height
                                {
                                    app.log_scroll_offset = app.log_scroll_offset.saturating_add(3);
                                    app.log_stick_to_bottom = false;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Event::Key(key) => {
                    if app.mode == AppMode::Boot {
                        // Any key exits boot mode
                        app.mode = AppMode::Normal;
                    } else {
                        handle_input(&mut app, key, &tx, &hive_path, parallel).await;
                        if app.should_quit {
                            break;
                        }
                    }
                }
                _ => {} // Fallback for other events
            }
        }

        // Handle messages
        while let Ok(msg) = rx.try_recv() {
            match msg {
                AppEvent::Progress(p_msg) => {
                    match p_msg {
                        Message::Print(line) | Message::PrintMeta(line) => {
                            let text = line.text.clone();
                            let mut matched_node = None;

                            // Check if label matches a known node
                            if !line.label.is_empty() {
                                if let Ok(node_name) = NodeName::new(line.label.clone()) {
                                    // Verify it's in our node list to avoid creating garbage entries
                                    if app.all_nodes.contains(&node_name) {
                                        matched_node = Some(node_name);
                                    }
                                }
                            }

                            if let Some(ref node) = matched_node {
                                // Node-specific Log
                                app.node_logs
                                    .entry(node.clone())
                                    .or_default()
                                    .push(text.clone());
                            } else {
                                // System / Global Log
                                let formatted = format!("{} | {}", line.label, text);
                                app.logs.push(formatted);
                            }

                            if let Some(node) = matched_node {
                                // Infer Short Status
                                let short_status = if text.contains("copying path")
                                    || text.contains("copying closure")
                                {
                                    Some("Copying...")
                                } else if text.contains("building") {
                                    Some("Building...")
                                } else if text.contains("switching to configuration") {
                                    Some("Activating...")
                                } else if text.contains("waiting for lock") {
                                    Some("Waiting...")
                                } else if text.contains("evalulating") {
                                    Some("Evaluating...")
                                } else {
                                    None
                                };

                                let current_state =
                                    app.node_states.get(&node).unwrap_or(&NodeState::Idle);

                                use crate::progress::LineStyle;

                                let new_state = match line.style {
                                    LineStyle::Success | LineStyle::SuccessNoop => {
                                        NodeState::Success(text)
                                    }
                                    LineStyle::Failure => NodeState::Failed(text),
                                    _ => {
                                        // Running state management
                                        if let Some(s) = short_status {
                                            NodeState::Running(s.to_string())
                                        } else {
                                            // Provide stable state if no new short status found
                                            match current_state {
                                                NodeState::Running(prev) => {
                                                    NodeState::Running(prev.clone())
                                                }
                                                // If transitioning from Idle/Success/Failed to Running without specific status
                                                _ => NodeState::Running("Running...".to_string()),
                                            }
                                        }
                                    }
                                };

                                match &new_state {
                                    NodeState::Success(_) | NodeState::Failed(_) => {
                                        app.node_last_updated.insert(
                                            node.clone(),
                                            std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs(),
                                        );
                                        let _ = tx.send(AppEvent::SaveCache);
                                    }
                                    _ => {}
                                }
                                app.node_states.insert(node, new_state);
                            }
                        }
                        Message::Complete => {
                            app.logs.push("Deployment completed.".to_string());
                        }
                        _ => {}
                    }
                }
                AppEvent::TaskStarted(uuid, description) => {
                    app.active_tasks.insert(uuid, description);
                }
                AppEvent::TaskFinished(uuid) => {
                    app.active_tasks.remove(&uuid);
                }
                AppEvent::NodeStateChanged(name, state) => {
                    app.node_states.insert(name, state);
                }
                AppEvent::ConfigLoaded(name, config) => {
                    app.node_configs.insert(name.clone(), config.clone());

                    // Preserve existing state if it provides history (Success/Failed)
                    // Only transition Loading -> Idle
                    app.node_states
                        .entry(name.clone())
                        .and_modify(|s| {
                            if matches!(s, NodeState::Loading) {
                                *s = NodeState::Idle;
                            }
                        })
                        .or_insert(NodeState::Idle);

                    app.tree = build_tree(&app.all_nodes, &app.node_configs, &hierarchy);

                    // Spawn provenance fetcher
                    let tx_prov = tx.clone();
                    let prov_sem = prov_semaphore.clone();

                    tokio::spawn(async move {
                        let mut host: Box<dyn Host> = if let Some(ssh) = config.to_ssh_host() {
                            ssh.upcast()
                        } else {
                            LocalHost::new(NixFlags::default()).upcast()
                        };

                        loop {
                            let status = {
                                let _permit = prov_sem.acquire().await.unwrap();
                                match host.fetch_provenance().await {
                                    Ok(Some(p)) => ProvenanceStatus::Verified(p),
                                    Ok(None) => ProvenanceStatus::NoData,
                                    Err(e) => ProvenanceStatus::Error(e.to_string()),
                                }
                            };

                            if tx_prov
                                .send(AppEvent::ProvenanceLoaded(name.clone(), status.clone()))
                                .is_err()
                            {
                                break;
                            }

                            let sleep_time = match status {
                                ProvenanceStatus::Error(_) => Duration::from_secs(10),
                                _ => Duration::from_secs(60),
                            };

                            tokio::time::sleep(sleep_time).await;
                        }
                    });
                }
                AppEvent::ProvenanceLoaded(name, status) => {
                    app.remote_provenance.insert(name, status);
                    let _ = tx.send(AppEvent::SaveCache);
                }
                AppEvent::TreeRebuild => {
                    app.tree = build_tree(&app.all_nodes, &app.node_configs, &hierarchy);
                    let _ = tx.send(AppEvent::SaveCache);
                    app.tree_loading = false;
                }
                AppEvent::SaveCache => {
                    if let Some(hive_ref) = &hive {
                        // Save Cache
                        let cache = TreeCache {
                            nodes: app.all_nodes.clone(),
                            configs: app.node_configs.clone(),
                            tree: app.tree.clone(),
                            node_states: app.node_states.clone(),
                            node_last_updated: app.node_last_updated.clone(),
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            provenance: app.remote_provenance.clone(),
                        };

                        let context_dir = hive_ref
                            .context_dir()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

                        tokio::spawn(async move {
                            if let Ok(json) = serde_json::to_string(&cache) {
                                let cache_path = get_cache_path(&context_dir);
                                if let Some(p) = cache_path.parent() {
                                    tokio::fs::create_dir_all(p).await.ok();
                                }

                                // Atomic write: write to unique tmp file then rename
                                let tmp_name = format!("tree.json.tmp.{}", Uuid::new_v4());
                                let tmp_path = cache_path.with_file_name(tmp_name);

                                if tokio::fs::write(&tmp_path, json).await.is_ok() {
                                    tokio::fs::rename(tmp_path, cache_path).await.ok();
                                }
                            }
                        });
                    }
                }
                AppEvent::MetaLoaded(m) => {
                    let default = vec![
                        "category".to_string(),
                        "environment".to_string(),
                        "hostgroup".to_string(),
                    ];
                    hierarchy = m.hierarchy.as_ref().unwrap_or(&default).clone();

                    if let Some(reg_config) = m.registrants {
                        app.registrants_config = Some(reg_config.clone());
                        if app.registrants_loading {
                            let reg_tx = tx.clone();
                            tokio::spawn(async move {
                                let (domains, errors) =
                                    crate::registrants::fetch_all(&reg_config).await;
                                let _ = reg_tx.send(AppEvent::RegistrantsLoaded(domains, errors));
                            });
                        }
                    } else {
                        app.registrants_loading = false;
                    }
                }
                AppEvent::DiffComputed(output) => {
                    app.diff_output = Some(output);
                    app.diff_scroll_offset = 0;
                }
                AppEvent::Ssh(node_name) => {
                    if let Some(config) = app.node_configs.get(&node_name) {
                        let host = config.target_host.as_deref().unwrap_or("localhost");
                        let user = config.target_user.as_deref().unwrap_or("root");
                        let target = if host == "localhost" {
                            "localhost".to_string()
                        } else {
                            format!("{}@{}", user, host)
                        };

                        // Suspend TUI
                        let _ = disable_raw_mode();
                        let mut stdout = std::io::stdout();
                        let _ = execute!(stdout, LeaveAlternateScreen, DisableMouseCapture);
                        let _ = terminal.show_cursor();

                        println!("Navi: Connecting to {}...", target);

                        let mut cmd = std::process::Command::new("ssh");
                        cmd.arg("-t");

                        if let Some(ssh_host) = config.to_ssh_host() {
                            let options = ssh_host.ssh_options();
                            // Filter out -T (disable pseudo-tty) since we want an interactive TTY
                            let interactive_options = options
                                .into_iter()
                                .filter(|o| o != "-T")
                                .collect::<Vec<_>>();
                            cmd.args(interactive_options);
                        }

                        let status = cmd.arg(&target).status();

                        // Restore TUI
                        let _ = enable_raw_mode();
                        let mut stdout = std::io::stdout();
                        let _ = execute!(stdout, EnterAlternateScreen, EnableMouseCapture);
                        let _ = terminal.hide_cursor();
                        terminal.clear()?;

                        if let Err(e) = status {
                            app.flash_message =
                                Some((format!("SSH Failed: {}", e), std::time::Instant::now()));
                        }
                    }
                }
                AppEvent::RamUpdate(used, total) => {
                    app.ram_usage = (used, total);
                }
                AppEvent::RegistrantsLoaded(domains, errors) => {
                    app.registrant_domains = domains.clone();
                    app.registrants_loading = false;

                    // Build Tree
                    let mut tree = Vec::new();
                    use std::collections::BTreeMap;

                    // Group by Provider -> Account
                    let mut providers: BTreeMap<String, BTreeMap<String, Vec<DomainInfo>>> =
                        BTreeMap::new();

                    for d in domains {
                        providers
                            .entry(d.provider.clone())
                            .or_default()
                            .entry(d.account.clone())
                            .or_default()
                            .push(d);
                    }

                    for (prov_name, accounts) in providers {
                        let mut account_items = Vec::new();
                        for (acc_name, acc_domains) in accounts {
                            let mut domain_items = Vec::new();
                            for d in acc_domains {
                                let mut children = Vec::new();
                                if d.provider == "Porkbun" {
                                    children.push(RegistrantTreeItem::RecordGroup {
                                        name: "DNS Records".to_string(),
                                        domain_info: d.clone(),
                                        children: vec![RegistrantTreeItem::Message {
                                            text: "Loading...".to_string(),
                                        }],
                                        collapsed: true,
                                        loaded: false,
                                    });
                                    children.push(RegistrantTreeItem::RecordGroup {
                                        name: "Glue Records".to_string(),
                                        domain_info: d.clone(),
                                        children: vec![RegistrantTreeItem::Message {
                                            text: "Loading...".to_string(),
                                        }],
                                        collapsed: true,
                                        loaded: false,
                                    });
                                }

                                domain_items.push(RegistrantTreeItem::Domain {
                                    info: d,
                                    children,
                                    collapsed: true,
                                });
                            }
                            account_items.push(RegistrantTreeItem::Account {
                                name: acc_name,
                                children: domain_items,
                                collapsed: false,
                            });
                        }
                        tree.push(RegistrantTreeItem::Provider {
                            name: prov_name,
                            children: account_items,
                            collapsed: false,
                        });
                    }

                    app.registrants_tree = tree;

                    for e in errors {
                        app.logs.push(format!("[Registrants Error] {}", e));
                    }
                    if !app.logs.is_empty() {
                        app.flash_message = Some((
                            "Fetch errors. Check Logs [L]".to_string(),
                            std::time::Instant::now(),
                        ));
                    }
                }
                AppEvent::FetchRegistrantRecords(provider, account, domain, rec_type) => {
                    // Logic to fetch records
                    if let Some(meta) = &meta_config {
                        if let Some(reg_config) = &meta.registrants {
                            let mut credentials = None;
                            if provider == "Porkbun" {
                                if let Some(acc) = reg_config.porkbun.get(&account) {
                                    // Fetch creds
                                    let mut api_key = String::new();
                                    let mut secret = String::new();

                                    let parts1: Vec<&str> =
                                        acc.api_key_command.split_whitespace().collect();
                                    if !parts1.is_empty() {
                                        if let Ok(o) = std::process::Command::new(parts1[0])
                                            .args(&parts1[1..])
                                            .output()
                                        {
                                            if o.status.success() {
                                                api_key = String::from_utf8_lossy(&o.stdout)
                                                    .trim()
                                                    .to_string();
                                            }
                                        }
                                    }

                                    let parts2: Vec<&str> =
                                        acc.secret_api_key_command.split_whitespace().collect();
                                    if !parts2.is_empty() {
                                        if let Ok(o) = std::process::Command::new(parts2[0])
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
                                        credentials = Some(serde_json::json!({
                                            "apikey": api_key,
                                            "secretapikey": secret
                                        }));
                                    }
                                }
                            }

                            if let Some(creds) = credentials {
                                let tx_inner = tx.clone();
                                let domain_clone = domain.clone();
                                let type_clone = rec_type.clone();

                                tokio::spawn(async move {
                                    if type_clone == "DNS" {
                                        match crate::registrants::porkbun::fetch_dns_records(
                                            &domain_clone,
                                            &creds,
                                        )
                                        .await
                                        {
                                            Ok(recs) => {
                                                let _ = tx_inner.send(
                                                    AppEvent::RegistrantRecordsLoaded {
                                                        domain: domain_clone,
                                                        record_type: "DNS".to_string(),
                                                        dns_records: Some(recs),
                                                        glue_records: None,
                                                        error: None,
                                                    },
                                                );
                                            }
                                            Err(e) => {
                                                let _ = tx_inner.send(
                                                    AppEvent::RegistrantRecordsLoaded {
                                                        domain: domain_clone,
                                                        record_type: "DNS".to_string(),
                                                        dns_records: None,
                                                        glue_records: None,
                                                        error: Some(e.to_string()),
                                                    },
                                                );
                                            }
                                        }
                                    } else if type_clone == "Glue" {
                                        match crate::registrants::porkbun::fetch_glue_records(
                                            &domain_clone,
                                            &creds,
                                        )
                                        .await
                                        {
                                            Ok(recs) => {
                                                let _ = tx_inner.send(
                                                    AppEvent::RegistrantRecordsLoaded {
                                                        domain: domain_clone,
                                                        record_type: "Glue".to_string(),
                                                        dns_records: None,
                                                        glue_records: Some(recs),
                                                        error: None,
                                                    },
                                                );
                                            }
                                            Err(e) => {
                                                let _ = tx_inner.send(
                                                    AppEvent::RegistrantRecordsLoaded {
                                                        domain: domain_clone,
                                                        record_type: "Glue".to_string(),
                                                        dns_records: None,
                                                        glue_records: None,
                                                        error: Some(e.to_string()),
                                                    },
                                                );
                                            }
                                        }
                                    }
                                });
                            } else {
                                app.flash_message = Some((
                                    "Failed to get credentials".to_string(),
                                    std::time::Instant::now(),
                                ));
                            }
                        }
                    }
                }
                AppEvent::RegistrantRecordsLoaded {
                    domain,
                    record_type,
                    dns_records,
                    glue_records,
                    error,
                } => {
                    // Update tree
                    let new_items = if let Some(err) = error {
                        vec![RegistrantTreeItem::Message {
                            text: format!("Error: {}", err),
                        }]
                    } else if let Some(dns) = dns_records {
                        if dns.is_empty() {
                            vec![RegistrantTreeItem::Message {
                                text: "No records found.".to_string(),
                            }]
                        } else {
                            dns.into_iter()
                                .map(|r| RegistrantTreeItem::DnsRecord { record: r })
                                .collect()
                        }
                    } else if let Some(glue) = glue_records {
                        if glue.is_empty() {
                            vec![RegistrantTreeItem::Message {
                                text: "No glue records found.".to_string(),
                            }]
                        } else {
                            glue.into_iter()
                                .map(|r| RegistrantTreeItem::GlueRecord { record: r })
                                .collect()
                        }
                    } else {
                        vec![RegistrantTreeItem::Message {
                            text: "No data".to_string(),
                        }]
                    };

                    update_domain_records(
                        &mut app.registrants_tree,
                        &domain,
                        &record_type,
                        new_items,
                    );
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Typing Animation
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let text1_words = ["Close", " the", " world..."];
    let text2_words = ["...Open", " the", " Next"];

    // Write first part
    print!("\n");
    for word in text1_words {
        print!("{}", word);
        stdout.flush()?;
        std::thread::sleep(Duration::from_millis(150));
    }
    print!("\n          ");
    stdout.flush()?;

    // Type "...Open the Next" + Cursor
    for word in text2_words {
        print!("{}", word);
        stdout.flush()?;
        std::thread::sleep(Duration::from_millis(150));
    }

    std::thread::sleep(Duration::from_millis(500));

    // Delete/Backspace
    // "Open the Next" is 13 chars
    use crossterm::cursor;
    for _ in 0..13 {
        execute!(stdout, cursor::MoveLeft(1))?;
        print!(" ");
        execute!(stdout, cursor::MoveLeft(1))?;
        stdout.flush()?;
        std::thread::sleep(Duration::from_millis(30));
    }

    // Type correct text
    let correct = "txEn eht nepO";
    for c in correct.chars() {
        print!("{}", c);
        stdout.flush()?;
        if c == ' ' {
            std::thread::sleep(Duration::from_millis(150));
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    print!("\n\n");

    Ok(())
}

fn update_domain_records(

    tree: &mut Vec<RegistrantTreeItem>,
    domain: &str,
    rec_type: &str,
    new_items: Vec<RegistrantTreeItem>,
) {
    for item in tree.iter_mut() {
        match item {
            RegistrantTreeItem::Provider { children, .. }
            | RegistrantTreeItem::Account { children, .. } => {
                update_domain_records(children, domain, rec_type, new_items.clone());
            }
            RegistrantTreeItem::Domain { info, children, .. } => {
                if info.domain == domain {
                    for child in children.iter_mut() {
                        if let RegistrantTreeItem::RecordGroup {
                            name,
                            children: grp_children,
                            loaded,
                            ..
                        } = child
                        {
                            if (rec_type == "DNS" && name == "DNS Records")
                                || (rec_type == "Glue" && name == "Glue Records")
                            {
                                *grp_children = new_items.clone();
                                *loaded = true;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
