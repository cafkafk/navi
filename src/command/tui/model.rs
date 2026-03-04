use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use ratatui::{layout::Rect, style::Color, widgets::ListState};
use serde::{Deserialize, Serialize}; // Add this
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::daemon::client::DaemonClient;
pub use crate::nix::NodeState;
use crate::nix::{NodeConfig, NodeName, Provenance, RegistrantsConfig};
use crate::registrants::porkbun::{PorkbunDnsRecord, PorkbunGlueRecord};
use crate::registrants::DomainInfo;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ProvenanceStatus {
    Verified(Provenance),
    NoData,
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppMode {
    Boot,
    Normal,
    Visual,
    Diff,
    Menu,
    Registrants,
    Logs,
}

#[derive(Clone, Debug)]
pub enum RegistrantTreeItem {
    Provider {
        name: String,
        children: Vec<RegistrantTreeItem>,
        collapsed: bool,
    },
    Account {
        name: String,
        children: Vec<RegistrantTreeItem>,
        collapsed: bool,
    },
    Domain {
        info: DomainInfo,
        children: Vec<RegistrantTreeItem>,
        collapsed: bool,
    },
    RecordGroup {
        name: String, // "DNS Records", "Glue Records"
        domain_info: DomainInfo,
        children: Vec<RegistrantTreeItem>,
        collapsed: bool,
        loaded: bool,
    },
    DnsRecord {
        record: PorkbunDnsRecord,
    },
    GlueRecord {
        record: PorkbunGlueRecord,
    },
    Message {
        text: String,
    },
}

#[derive(Clone, Debug)]
pub struct GameOfLife {
    pub cells: Vec<Vec<bool>>,
    pub width: usize,
    pub height: usize,
    pub ticks: u64,
}

impl Default for GameOfLife {
    fn default() -> Self {
        Self {
            cells: Vec::new(),
            width: 0,
            height: 0,
            ticks: 0,
        }
    }
}

impl GameOfLife {
    pub fn resize(&mut self, width: usize, height: usize) {
        if width == self.width && height == self.height {
            return;
        }

        let mut new_cells = vec![vec![false; width]; height];

        // Random init if empty or size changed significantly
        use rand::Rng;
        let mut rng = rand::rng();

        for y in 0..height {
            for x in 0..width {
                new_cells[y][x] = rng.random_bool(0.2);
            }
        }

        self.cells = new_cells;
        self.width = width;
        self.height = height;
    }

    pub fn reset(&mut self) {
        // Force re-initialization
        let width = self.width;
        let height = self.height;
        if width > 0 && height > 0 {
            use rand::Rng;
            let mut rng = rand::rng();
            for y in 0..height {
                for x in 0..width {
                    self.cells[y][x] = rng.random_bool(0.2);
                }
            }
        }
        self.ticks = 0;
    }

    pub fn tick(&mut self) {
        self.ticks += 1;
        if self.width == 0 || self.height == 0 {
            return;
        }

        let mut next_cells = self.cells.clone();

        // TODO: Parallel safe? We'll just simple iteration for now.
        for y in 0..self.height {
            for x in 0..self.width {
                let mut neighbors = 0;

                // 3x3 grid check
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }

                        let ny = (y as isize + dy).rem_euclid(self.height as isize) as usize;
                        let nx = (x as isize + dx).rem_euclid(self.width as isize) as usize;

                        if self.cells[ny][nx] {
                            neighbors += 1;
                        }
                    }
                }

                let alive = self.cells[y][x];
                if alive && (neighbors < 2 || neighbors > 3) {
                    next_cells[y][x] = false;
                } else if !alive && neighbors == 3 {
                    next_cells[y][x] = true;
                }
            }
        }

        self.cells = next_cells;
    }
}

#[derive(Clone, Debug)]
pub struct WiredPhantom {
    pub text_idx: usize,
    pub x: u16,
    pub y: u16,
    pub spawn_tick: u64,
    pub lifetime: u64,
}

#[derive(Clone, Debug)]
pub struct WiredState {
    pub phantoms: Vec<WiredPhantom>,
}

impl Default for WiredState {
    fn default() -> Self {
        Self {
            phantoms: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeploySettings {
    pub reboot: bool,
    pub install_bootloader: bool,
    pub no_keys: bool,
    pub no_substitute: bool,
    pub no_gzip: bool,
    pub build_on_target: bool,
    pub force_replace_unknown_profiles: bool,
    pub keep_result: bool,
}

impl Default for DeploySettings {
    fn default() -> Self {
        Self {
            reboot: false,
            install_bootloader: false,
            no_keys: false,
            no_substitute: false,
            no_gzip: false,
            build_on_target: false,
            force_replace_unknown_profiles: false,
            keep_result: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuId {
    Main,
    DeploySettings,
    NodeContext,
    GarbageCollectInterval,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SettingId {
    Reboot,
    InstallBootloader,
    NoKeys,
    NoSubstitute,
    NoGzip,
    BuildOnTarget,
    ForceReplaceUnknownProfiles,
    KeepResult,
    UseDaemon,
}

#[derive(Clone, Debug)]
pub enum LocalAction {
    Deploy,
    Diff,
    Ssh,
    Gc(Option<String>), // None = delete old, Some = delete-older-than X
    Inspect,
}

#[derive(Clone, Debug)]
pub enum MenuAction {
    Navigate(MenuId),
    Toggle(SettingId),
    Execute(LocalAction),
    Close,
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub label: String,
    pub action: MenuAction,
    pub checked: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct MenuState {
    pub active_menu_id: MenuId,
    pub selected_idx: usize,
    pub navigation_stack: Vec<(MenuId, usize)>, // History to navigate back
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            active_menu_id: MenuId::Main,
            selected_idx: 0,
            navigation_stack: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TreeItem {
    Group {
        name: String,
        children: Vec<TreeItem>,
        collapsed: bool,
    },
    Node {
        name: NodeName,
    },
}

#[derive(Clone, Debug)]
pub struct FlatItem<'a> {
    pub item: &'a TreeItem,
    pub depth: usize,
    pub index_in_tree: Vec<usize>, // Path to item in tree for modification
    pub last_in_group: bool,
}

pub struct App {
    pub tree: Vec<TreeItem>,
    pub node_configs: HashMap<NodeName, NodeConfig>,
    pub remote_provenance: HashMap<NodeName, ProvenanceStatus>,
    pub selected: HashSet<NodeName>,
    pub node_states: HashMap<NodeName, NodeState>,
    pub node_last_updated: HashMap<NodeName, u64>,
    pub node_logs: HashMap<NodeName, Vec<String>>,
    pub logs: Vec<String>,
    pub log_scroll_offset: usize,
    pub log_scroll_horizontal: usize,
    pub log_popup_open: bool,
    pub log_level_popup_open: bool,
    pub log_search_active: bool,
    pub log_filter_active: bool,
    pub log_input_buffer: String,
    pub log_search_query: Option<String>,
    pub log_filter_query: Option<String>,
    pub log_wrap: bool,
    pub log_stick_to_bottom: bool,
    pub log_focus_node: Option<NodeName>,
    pub log_levels: HashSet<String>, // "INFO", "WARN", "ERROR", "DEBUG"
    pub log_level_idx: usize,
    pub yank_pending: bool,
    pub yank_count_buffer: String,

    // Global Popups
    pub show_exit_confirmation: bool,
    pub exit_popup_selection: bool, // true = Yes, false = No
    pub should_quit: bool,

    pub log_inner_rect: Option<Rect>,
    pub selection_start: Option<(u16, u16)>,
    pub selection_end: Option<(u16, u16)>,
    pub view_global_logs: bool,
    pub tick_count: u64,
    pub list_state: ListState,
    pub registrants_state: ListState,
    pub active_tasks: HashMap<Uuid, String>,
    pub git_rev: String,

    // Vim-style state
    pub mode: AppMode,
    pub visual_anchor: Option<usize>,
    pub pending_g: bool,

    // All known nodes
    pub all_nodes: Vec<NodeName>,

    // Clipboard communication
    pub clipboard_tx: UnboundedSender<String>,
    pub flash_message: Option<(String, Instant)>,

    // Copy state
    pub copy_region: Option<((u16, u16), (u16, u16))>, // (start, end)

    // Diff state
    pub diff_output: Option<String>,
    pub diff_scroll_offset: usize,

    // System Monitor
    pub ram_usage: (u64, u64), // (used, total)

    // Menu / Settings
    pub deploy_settings: DeploySettings,
    pub menu_state: MenuState,

    // Boot Screen
    pub game_of_life: GameOfLife,
    pub wired_state: WiredState,

    // Registrants
    pub registrants_config: Option<RegistrantsConfig>,
    pub registrant_domains: Vec<DomainInfo>,
    pub registrants_tree: Vec<RegistrantTreeItem>,
    pub registrants_loading: bool,

    // Inspector
    pub inspector_open: bool,

    // Tree Loading State (Cache)
    pub tree_loading: bool,

    // Daemon
    pub daemon: Option<DaemonClient>,
    pub use_daemon: bool,

    // Terminal Theme
    pub terminal_bg: Option<Color>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TreeCache {
    pub nodes: Vec<NodeName>,
    pub configs: HashMap<NodeName, NodeConfig>,
    pub tree: Vec<TreeItem>,
    #[serde(default)]
    pub node_states: HashMap<NodeName, NodeState>,
    #[serde(default)]
    pub node_last_updated: HashMap<NodeName, u64>,
    #[serde(default)]
    pub timestamp: u64,
    #[serde(default)]
    pub provenance: HashMap<NodeName, ProvenanceStatus>,
}

impl App {
    pub fn new(
        nodes: Vec<NodeName>,
        node_configs: HashMap<NodeName, NodeConfig>,
        tree: Vec<TreeItem>,
        clipboard_tx: UnboundedSender<String>,
        git_rev: String,
        registrants_config: Option<RegistrantsConfig>,
        terminal_bg: Option<Color>,
    ) -> Self {
        Self {
            tree,
            node_configs,
            remote_provenance: HashMap::new(),
            selected: HashSet::new(), // Start with none selected
            node_states: HashMap::new(),
            node_last_updated: HashMap::new(),
            node_logs: HashMap::new(),
            logs: Vec::new(),
            log_scroll_offset: 0,
            log_scroll_horizontal: 0,
            log_popup_open: false,
            log_level_popup_open: false,
            log_search_active: false,
            log_filter_active: false,
            log_input_buffer: String::new(),
            log_search_query: None,
            log_filter_query: None,
            log_wrap: true, // Soft wrap default
            log_stick_to_bottom: true,
            log_focus_node: None,
            log_levels: ["INFO".to_string(), "WARN".to_string(), "ERROR".to_string()]
                .into_iter()
                .collect(),
            log_level_idx: 0,
            yank_pending: false,
            yank_count_buffer: String::new(),
            show_exit_confirmation: false,
            exit_popup_selection: true, // Default to Yes
            should_quit: false,
            log_inner_rect: None,
            selection_start: None,
            selection_end: None,
            view_global_logs: false,
            tick_count: 0,
            list_state: ratatui::widgets::ListState::default(),
            registrants_state: ratatui::widgets::ListState::default(),
            active_tasks: HashMap::new(),
            git_rev,
            mode: AppMode::Boot, // Start in Boot
            visual_anchor: None,
            pending_g: false,
            all_nodes: nodes,
            clipboard_tx,
            flash_message: None,
            copy_region: None,
            diff_output: None,
            diff_scroll_offset: 0,
            ram_usage: (0, 0),
            deploy_settings: DeploySettings::default(),
            menu_state: MenuState::default(),
            game_of_life: GameOfLife::default(),
            wired_state: WiredState::default(),
            registrants_config,
            registrant_domains: Vec::new(),
            registrants_tree: Vec::new(),
            registrants_loading: true,
            inspector_open: false,
            tree_loading: false,
            daemon: None,
            use_daemon: true,
            terminal_bg,
        }
    }

    // Static method avoids borrowing the whole App
    pub fn flatten_tree(tree: &Vec<TreeItem>) -> Vec<FlatItem> {
        let mut flat = Vec::new();
        let len = tree.len();
        for (i, item) in tree.iter().enumerate() {
            let is_last = i == len - 1;
            Self::flatten_recursive(item, 0, vec![i], is_last, &mut flat);
        }
        flat
    }

    fn flatten_recursive<'a>(
        item: &'a TreeItem,
        depth: usize,
        path: Vec<usize>,
        is_last: bool,
        out: &mut Vec<FlatItem<'a>>,
    ) {
        out.push(FlatItem {
            item,
            depth,
            index_in_tree: path.clone(),
            last_in_group: is_last,
        });

        if let TreeItem::Group {
            children,
            collapsed,
            ..
        } = item
        {
            if !*collapsed {
                let len = children.len();
                for (i, child) in children.iter().enumerate() {
                    let mut child_path = path.clone();
                    child_path.push(i);
                    let child_is_last = i == len - 1;
                    Self::flatten_recursive(child, depth + 1, child_path, child_is_last, out);
                }
            }
        }
    }

    pub fn toggle_group_collapse(&mut self, path: &[usize]) {
        Self::toggle_recursive(&mut self.tree, path);
    }

    fn toggle_recursive(items: &mut Vec<TreeItem>, path: &[usize]) {
        if let Some((&idx, rest)) = path.split_first() {
            if let Some(item) = items.get_mut(idx) {
                if rest.is_empty() {
                    // Reached target
                    if let TreeItem::Group { collapsed, .. } = item {
                        *collapsed = !*collapsed;
                    }
                } else {
                    // Descend
                    if let TreeItem::Group { children, .. } = item {
                        Self::toggle_recursive(children, rest);
                    }
                }
            }
        }
    }

    // Helper to get all leaf nodes under an item
    pub fn collect_leaves(item: &TreeItem, leaves: &mut Vec<NodeName>) {
        match item {
            TreeItem::Group { children, .. } => {
                for child in children {
                    Self::collect_leaves(child, leaves);
                }
            }
            TreeItem::Node { name } => {
                leaves.push(name.clone());
            }
        }
    }

    pub fn toggle_registrant_collapse(&mut self, idx: usize) {
        // Flatten tree to find item at visual index
        let mut count = 0;
        Self::toggle_registrant_recursive(&mut self.registrants_tree, idx, &mut count);
    }

    fn toggle_registrant_recursive(
        items: &mut Vec<RegistrantTreeItem>,
        target_idx: usize,
        current_idx: &mut usize,
    ) -> bool {
        for item in items.iter_mut() {
            if *current_idx == target_idx {
                match item {
                    RegistrantTreeItem::Provider { collapsed, .. }
                    | RegistrantTreeItem::Account { collapsed, .. }
                    | RegistrantTreeItem::Domain { collapsed, .. }
                    | RegistrantTreeItem::RecordGroup { collapsed, .. } => {
                        *collapsed = !*collapsed;
                    }
                    _ => {}
                }
                return true;
            }
            *current_idx += 1;

            match item {
                RegistrantTreeItem::Provider {
                    children,
                    collapsed,
                    ..
                }
                | RegistrantTreeItem::Account {
                    children,
                    collapsed,
                    ..
                }
                | RegistrantTreeItem::Domain {
                    children,
                    collapsed,
                    ..
                }
                | RegistrantTreeItem::RecordGroup {
                    children,
                    collapsed,
                    ..
                } => {
                    if !*collapsed {
                        if Self::toggle_registrant_recursive(children, target_idx, current_idx) {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub fn flatten_registrants(tree: &Vec<RegistrantTreeItem>) -> Vec<&RegistrantTreeItem> {
        let mut out = Vec::new();
        for item in tree {
            out.push(item);
            match item {
                RegistrantTreeItem::Provider {
                    children,
                    collapsed,
                    ..
                }
                | RegistrantTreeItem::Account {
                    children,
                    collapsed,
                    ..
                }
                | RegistrantTreeItem::Domain {
                    children,
                    collapsed,
                    ..
                }
                | RegistrantTreeItem::RecordGroup {
                    children,
                    collapsed,
                    ..
                } => {
                    if !*collapsed {
                        out.append(&mut Self::flatten_registrants(children));
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn get_current_menu_items(&self) -> Vec<MenuItem> {
        match self.menu_state.active_menu_id {
            MenuId::Main => vec![
                MenuItem {
                    label: "Inspector (i)".to_string(),
                    action: MenuAction::Execute(LocalAction::Inspect),
                    checked: None,
                },
                MenuItem {
                    label: "Options".to_string(),
                    action: MenuAction::Navigate(MenuId::DeploySettings),
                    checked: None,
                },
                MenuItem {
                    label: "Use Daemon".to_string(),
                    action: MenuAction::Toggle(SettingId::UseDaemon),
                    checked: Some(self.use_daemon),
                },
                MenuItem {
                    label: "Close Menu".to_string(),
                    action: MenuAction::Close,
                    checked: None,
                },
            ],
            MenuId::DeploySettings => vec![
                MenuItem {
                    label: "Reboot (--reboot)".to_string(),
                    action: MenuAction::Toggle(SettingId::Reboot),
                    checked: Some(self.deploy_settings.reboot),
                },
                MenuItem {
                    label: "Install Bootloader (--install-bootloader)".to_string(),
                    action: MenuAction::Toggle(SettingId::InstallBootloader),
                    checked: Some(self.deploy_settings.install_bootloader),
                },
                MenuItem {
                    label: "Skip Key Upload (--no-keys)".to_string(),
                    action: MenuAction::Toggle(SettingId::NoKeys),
                    checked: Some(self.deploy_settings.no_keys),
                },
                MenuItem {
                    label: "No Substitutes (--no-substitutes)".to_string(),
                    action: MenuAction::Toggle(SettingId::NoSubstitute),
                    checked: Some(self.deploy_settings.no_substitute),
                },
                MenuItem {
                    label: "No GZip (--no-gzip)".to_string(),
                    action: MenuAction::Toggle(SettingId::NoGzip),
                    checked: Some(self.deploy_settings.no_gzip),
                },
                MenuItem {
                    label: "Build on Target (--build-on-target)".to_string(),
                    action: MenuAction::Toggle(SettingId::BuildOnTarget),
                    checked: Some(self.deploy_settings.build_on_target),
                },
                MenuItem {
                    label: "Force Replace Unknown Profiles (--force-replace-unknown-profiles)"
                        .to_string(),
                    action: MenuAction::Toggle(SettingId::ForceReplaceUnknownProfiles),
                    checked: Some(self.deploy_settings.force_replace_unknown_profiles),
                },
                MenuItem {
                    label: "Keep Result / GC Roots (--keep-result)".to_string(),
                    action: MenuAction::Toggle(SettingId::KeepResult),
                    checked: Some(self.deploy_settings.keep_result),
                },
                MenuItem {
                    label: "Back".to_string(),
                    action: MenuAction::Navigate(MenuId::Main),
                    checked: None,
                },
            ],
            MenuId::NodeContext => {
                let mut items = vec![MenuItem {
                    label: "Deploy".to_string(),
                    action: MenuAction::Execute(LocalAction::Deploy),
                    checked: None,
                }];

                // Diff only works on single selection
                let selected_count = if !self.selected.is_empty() {
                    self.selected.len()
                } else {
                    1
                };

                if selected_count == 1 {
                    items.push(MenuItem {
                        label: "Diff".to_string(),
                        action: MenuAction::Execute(LocalAction::Diff),
                        checked: None,
                    });
                    items.push(MenuItem {
                        label: "SSH".to_string(),
                        action: MenuAction::Execute(LocalAction::Ssh),
                        checked: None,
                    });
                }

                items.push(MenuItem {
                    label: "Collect Garbage".to_string(),
                    action: MenuAction::Navigate(MenuId::GarbageCollectInterval),
                    checked: None,
                });

                items.push(MenuItem {
                    label: "Cancel".to_string(),
                    action: MenuAction::Close,
                    checked: None,
                });

                items
            }
            MenuId::GarbageCollectInterval => vec![
                MenuItem {
                    label: "Delete Old (nix-collect-garbage -d)".to_string(),
                    action: MenuAction::Execute(LocalAction::Gc(None)),
                    checked: None,
                },
                MenuItem {
                    label: "Older than 3 days".to_string(),
                    action: MenuAction::Execute(LocalAction::Gc(Some("3d".to_string()))),
                    checked: None,
                },
                MenuItem {
                    label: "Older than 7 days".to_string(),
                    action: MenuAction::Execute(LocalAction::Gc(Some("7d".to_string()))),
                    checked: None,
                },
                MenuItem {
                    label: "Older than 14 days".to_string(),
                    action: MenuAction::Execute(LocalAction::Gc(Some("14d".to_string()))),
                    checked: None,
                },
                MenuItem {
                    label: "Older than 30 days".to_string(),
                    action: MenuAction::Execute(LocalAction::Gc(Some("30d".to_string()))),
                    checked: None,
                },
                MenuItem {
                    label: "Back".to_string(),
                    action: MenuAction::Navigate(MenuId::NodeContext),
                    checked: None,
                },
            ],
        }
    }
}

// Tree Builder Logic
pub fn build_tree(
    nodes: &[NodeName],
    configs: &HashMap<NodeName, NodeConfig>,
    hierarchy_keys: &[String],
) -> Vec<TreeItem> {
    if hierarchy_keys.is_empty() {
        // Flat list sorted by name
        let mut sorted_nodes = nodes.to_vec();
        sorted_nodes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        return sorted_nodes
            .into_iter()
            .map(|n| TreeItem::Node { name: n })
            .collect();
    }

    let key = &hierarchy_keys[0];
    let remaining_keys = &hierarchy_keys[1..];

    // Group nodes by current key tag value
    let mut groups: BTreeMap<String, Vec<NodeName>> = BTreeMap::new();
    let mut other: Vec<NodeName> = Vec::new();

    for node in nodes {
        if let Some(config) = configs.get(node) {
            let tags = config.tags();

            // Find tag starting with "key:"
            let prefix = format!("{}:", key);
            let mut found = false;
            for tag in tags {
                if tag.starts_with(&prefix) {
                    let value = tag[prefix.len()..].to_string();
                    groups.entry(value).or_default().push(node.clone());
                    found = true;
                    break; // Only take first matching tag
                }
            }
            if !found {
                other.push(node.clone());
            }
        } else {
            // Config not loaded yet -> treat as uncategorized/other
            other.push(node.clone());
        }
    }

    let mut items = Vec::new();

    // Process groups
    for (name, group_nodes) in groups {
        let children = build_tree(&group_nodes, configs, remaining_keys);
        items.push(TreeItem::Group {
            name,
            children,
            collapsed: false,
        });
    }

    // Process ungrouped nodes
    if !other.is_empty() {
        if remaining_keys.is_empty() {
            let mut sorted = other;
            sorted.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            for node in sorted {
                items.push(TreeItem::Node { name: node });
            }
        } else {
            let mut children = build_tree(&other, configs, remaining_keys);
            items.append(&mut children);
        }
    }

    items
}
