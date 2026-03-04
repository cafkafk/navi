use std::env;

use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use const_format::{concatcp, formatcp};

use crate::{
    command::{self, apply::DeployOpts},
    nix::{hive::EvaluationMethod, HivePath},
};

/// Base URL of the manual, without the trailing slash.
const MANUAL_URL_BASE: &str = "https://navi.cli.rs";

/// URL to the manual.
///
/// We maintain CLI and Nix API stability for each minor version.
/// This ensures that the user always sees accurate documentations, and we can
/// easily perform updates to the manual after a release.
const MANUAL_URL: &str = concatcp!(
    MANUAL_URL_BASE,
    "/",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);

/// The note shown when the user is using a pre-release version.
///
/// API stability cannot be guaranteed for pre-release versions.
/// Links to the version currently in development automatically
/// leads the user to the unstable manual.
const MANUAL_DISCREPANCY_NOTE: &str = "\nNote: You are using a pre-release version of Navi, so the supported options may be different from what's in the manual.";

static LONG_ABOUT: &str = formatcp!(
    r#"NixOS deployment tool

Navi helps you deploy to multiple hosts running NixOS.
For more details, read the manual at <{}>.

{}"#,
    MANUAL_URL,
    if !env!("CARGO_PKG_VERSION_PRE").is_empty() {
        MANUAL_DISCREPANCY_NOTE
    } else {
        ""
    }
);

static CONFIG_HELP: &str = formatcp!(
    r#"If this argument is not specified, Navi will search upwards from the current working directory for a file named "flake.nix" or "hive.nix". This behavior is disabled if --config/-f is given explicitly.

For a sample configuration, check the manual at <{}>.
"#,
    MANUAL_URL
);

/// Display order in `--help` for arguments that should be shown first.
///
/// Currently reserved for -f/--config.
const HELP_ORDER_FIRST: usize = 100;

/// Display order in `--help` for arguments that are not very important.
const HELP_ORDER_LOW: usize = 2000;

/// When to display color.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorWhen {
    /// Detect automatically.
    #[default]
    Auto,

    /// Always display colors.
    Always,

    /// Never display colors.
    Never,
}

impl std::fmt::Display for ColorWhen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        })
    }
}

/// NixOS deployment tool
#[derive(Parser, Debug)]
#[command(
    name = "Navi",
    bin_name = "navi",
    author = "Christina Sørensen <ces@fem.gg>",
    version = env!("CARGO_PKG_VERSION"),
    long_about = LONG_ABOUT,
    max_term_width = 100,
)]
pub struct Opts {
    /// Path to a Hive expression, a flake.nix, or a Nix Flake URI
    #[arg(
        short = 'f',
        long,
        value_name = "CONFIG",
        long_help = CONFIG_HELP,
        display_order = HELP_ORDER_FIRST,
        global = true,
    )]
    pub config: Option<String>,

    /// Show debug information for Nix commands
    ///
    /// Passes --show-trace to Nix commands
    #[arg(long, global = true)]
    pub show_trace: bool,

    /// Allow impure expressions
    ///
    /// Passes --impure to Nix commands
    #[arg(long, global = true)]
    pub impure: bool,

    /// Passes an arbitrary option to Nix commands
    ///
    /// This only works when building locally.
    #[arg(
        long,
        global = true,
        num_args = 2,
        value_names = ["NAME", "VALUE"],
    )]
    pub nix_option: Vec<String>,

    /// Use legacy flake evaluation (deprecated)
    ///
    /// If enabled, flakes will be evaluated using `builtins.getFlake` with the `nix-instantiate` CLI.
    #[arg(long, default_value_t, global = true, hide = true)]
    pub legacy_flake_eval: bool,

    /// This flag no longer has an effect
    ///
    /// Previously, it enabled direct flake evaluation which is now the default.
    #[arg(
        long = "experimental-flake-eval",
        default_value_t,
        global = true,
        hide = true
    )]
    pub deprecated_experimental_flake_eval_flag: bool,

    /// When to colorize the output
    ///
    /// By default, Navi enables colorized output when the terminal supports it.
    ///
    /// It's also possible to specify the preference using environment variables. See
    /// <https://bixense.com/clicolors>.
    #[arg(
        long,
        value_name = "WHEN",
        default_value_t,
        global = true,
        display_order = HELP_ORDER_LOW,
    )]
    pub color: ColorWhen,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Apply(command::apply::Opts),

    #[cfg(target_os = "linux")]
    ApplyLocal(command::apply_local::Opts),

    /// Build configurations but not push to remote hosts
    ///
    /// This subcommand behaves as if you invoked `apply` with the `build` goal.
    Build {
        #[command(flatten)]
        deploy: DeployOpts,
    },

    Eval(command::eval::Opts),

    /// Upload keys to remote hosts
    ///
    /// This subcommand behaves as if you invoked `apply` with the pseudo `keys` goal.
    UploadKeys {
        #[command(flatten)]
        deploy: DeployOpts,
    },

    Exec(command::exec::Opts),

    /// List available hosts and their configuration
    List(command::list::Opts),

    /// Start an interactive REPL with the complete configuration
    ///
    /// In the REPL, you can inspect the configuration interactively with tab
    /// completion. The node configurations are accessible under the `nodes`
    /// attribute set.
    Repl,

    /// Show information about the current Nix installation
    NixInfo,

    /// Start an interactive TUI dashboard
    Tui(command::tui::Opts),

    /// Connect to node serial console
    Serial(command::serial::Opts),

    /// SSH into a host
    Ssh(command::ssh::Opts),


    /// Unlock a disk on a remote host
    DiskUnlock(command::disk_unlock::Opts),

    /// Provision infrastructure for nodes
    Provision(command::provision::Opts),

    /// Install NixOS on target nodes
    Install(command::install::Opts),

    /// Run progress spinner tests
    #[cfg(debug_assertions)]
    #[command(hide = true)]
    TestProgress,

    /// Start the Navi Daemon
    #[command(subcommand)]
    Daemon(DaemonCommand),

    /// Generate shell auto-completion files (Internal)
    #[command(hide = true)]
    GenCompletions {
        shell: Shell,
    },
}

#[derive(Subcommand, Debug)]
pub enum DaemonCommand {
    /// Start the daemon server
    Start,
    /// Get daemon status
    Status,
}
