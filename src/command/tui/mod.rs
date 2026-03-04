pub mod core;
pub mod demo;
pub mod events;
pub mod execution;
pub mod input;
pub mod loading;
pub mod logging;
pub mod model;
pub mod view;

use crate::error::NaviResult;
use crate::nix::Hive;
use clap::Args;

#[derive(Debug, Args)]
pub struct Opts {
    /// Limit the maximum number of hosts to be deployed in parallel
    #[arg(short, long, default_value_t = 10)]
    parallel: usize,

    /// Disable daemon and force local execution
    #[arg(long, default_value_t = false)]
    no_daemon: bool,

    /// Run in demo mode with fake data
    #[arg(long, default_value_t = false, hide = true)]
    pub demo: bool,
}

pub async fn run(hive: Hive, opts: Opts) -> NaviResult<()> {
    // TODO: The previous implementation in run.rs only checked env var.  We
    // should propagate this.
    if opts.no_daemon {
        std::env::set_var("NAVI_NO_DAEMON", "1");
    }

    core::run::run(hive, opts).await
}
