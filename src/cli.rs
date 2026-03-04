//! Global CLI Setup.

pub mod args;
use args::*;

use std::io;

use clap::{CommandFactory, Parser};
use clap_complete::Shell;
use tracing_subscriber::EnvFilter;

use crate::{
    command,
    error::{NaviError, NaviResult},
    nix::{hive::EvaluationMethod, Hive, HivePath},
};

async fn get_hive(opts: &Opts) -> NaviResult<Hive> {
    let path = match &opts.config {
        Some(path) => HivePath::from_string(path).await?,
        None => {
            // traverse upwards until we find hive.nix
            let mut cur = std::env::current_dir()?;
            let mut file_path = None;

            loop {
                let flake = cur.join("flake.nix");
                if flake.is_file() {
                    file_path = Some(flake);
                    break;
                }

                let legacy = cur.join("hive.nix");
                if legacy.is_file() {
                    file_path = Some(legacy);
                    break;
                }

                match cur.parent() {
                    Some(parent) => {
                        cur = parent.to_owned();
                    }
                    None => {
                        break;
                    }
                }
            }

            if file_path.is_none() {
                tracing::error!(
                    "Could not find `hive.nix` or `flake.nix` in {:?} or any parent directory",
                    std::env::current_dir()?
                );
            }

            HivePath::from_path(file_path.unwrap()).await?
        }
    };

    match &path {
        HivePath::Legacy(p) => {
            tracing::info!("Using configuration: {}", p.to_string_lossy());
        }
        HivePath::Flake(flake) => {
            tracing::info!("Using flake: {}", flake.uri());
        }
    }

    let mut hive = Hive::new(path).await?;

    if opts.show_trace {
        hive.set_show_trace(true);
    }

    if opts.impure {
        hive.set_impure(true);
    }

    if opts.deprecated_experimental_flake_eval_flag {
        tracing::error!(
            "--experimental-flake-eval is now the default and this flag no longer has an effect"
        );
        return Err(NaviError::Unsupported);
    }

    if opts.legacy_flake_eval {
        tracing::warn!("Using legacy flake eval (deprecated)");
        tracing::warn!(
            r#"Consider upgrading to the new evaluator by adding Navi as an input and expose the `naviHive` output:
  outputs = {{ self, navi, ... }}: {{
    naviHive = navi.lib.makeHive self.outputs.navi;
    navi = ...;
    }};
"#
        );
        hive.set_evaluation_method(EvaluationMethod::NixInstantiate);
    }

    for chunks in opts.nix_option.chunks_exact(2) {
        let [name, value] = chunks else {
            unreachable!()
        };
        hive.add_nix_option(name.clone(), value.clone());
    }

    Ok(hive)
}

pub async fn run() {
    let opts = Opts::parse();

    set_color_pref(&opts.color);

    if !matches!(opts.command, Command::Tui(_)) {
        init_logging();
    }

    if let Command::GenCompletions { shell } = opts.command {
        print_completions(shell, &mut Opts::command());
        return;
    }

    let hive = if let Command::Tui(tui_ctx) = &opts.command {
        if tui_ctx.demo {
            use crate::troubleshooter::run_wrapped as r;
            // Hack to satisfy the type system: we don't need a Hive for demo,
            // but run_wrapped expects a future.
            r(crate::command::tui::demo::run_demo(), None).await;
            return;
        }
        get_hive(&opts).await.expect("Failed to get flake or hive")
    } else {
        get_hive(&opts).await.expect("Failed to get flake or hive")
    };

    let hive_path = Some(hive.path().clone());

    use crate::troubleshooter::run_wrapped as r;

    match opts.command {
        Command::Daemon(cmd) => match cmd {
            DaemonCommand::Start => {
                let server = crate::daemon::server::DaemonServer::new();
                if let Err(e) = server.run().await {
                    eprintln!("Daemon error: {}", e);
                }
            }
            DaemonCommand::Status => {
                use crate::daemon::client::DaemonClient;

                match DaemonClient::connect(false).await {
                    Ok(client) => match client.get_status().await {
                        Ok(snapshot) => {
                            println!("Daemon Status: Running");
                            println!("Active Tasks: {}", snapshot.active_tasks.len());
                            for (uuid, desc) in snapshot.active_tasks {
                                println!("  - {} [{}]", desc, uuid);
                            }
                            println!("Nodes: {}", snapshot.node_states.len());
                        }
                        Err(e) => {
                            eprintln!("Failed to get status: {}", e);
                        }
                    },
                    Err(_) => {
                        println!("Daemon Status: Stopped (Socket not found or refused)");
                    }
                }
            }
        },
        Command::Apply(args) => r(command::apply::run(hive, args), hive_path).await,
        #[cfg(target_os = "linux")]
        Command::ApplyLocal(args) => r(command::apply_local::run(hive, args), hive_path).await,
        Command::Eval(args) => r(command::eval::run(hive, args), hive_path).await,
        Command::Exec(args) => r(command::exec::run(hive, args), hive_path).await,
        Command::List(args) => r(command::list::run(hive, args), hive_path).await,
        Command::NixInfo => r(command::nix_info::run(), hive_path).await,
        Command::Ssh(args) => r(command::ssh::run(hive, args), hive_path).await,
        Command::Serial(args) => r(command::serial::run(hive, args), hive_path).await,
        Command::DiskUnlock(args) => r(command::disk_unlock::run(hive, args), hive_path).await,
        Command::Tui(args) => r(command::tui::run(hive, args), hive_path).await,
        Command::Provision(args) => r(command::provision::run(hive, args), hive_path).await,
        Command::Install(args) => r(command::install::run(hive, args), hive_path).await,
        Command::Repl => r(command::repl::run(hive), hive_path).await,
        #[cfg(debug_assertions)]
        Command::TestProgress => r(command::test_progress::run(), hive_path).await,
        Command::Build { deploy } => {
            let args = command::apply::Opts {
                deploy,
                goal: crate::nix::Goal::Build,
            };
            r(command::apply::run(hive, args), hive_path).await
        }
        Command::UploadKeys { deploy } => {
            let args = command::apply::Opts {
                deploy,
                goal: crate::nix::Goal::UploadKeys,
            };
            r(command::apply::run(hive, args), hive_path).await
        }
        Command::GenCompletions { .. } => unreachable!(),
    }
}

fn print_completions(shell: Shell, cmd: &mut clap::Command) {
    let bin_name = cmd
        .get_bin_name()
        .expect("Must have a bin_name")
        .to_string();

    clap_complete::generate(shell, cmd, bin_name, &mut std::io::stdout());
}

fn set_color_pref(when: &ColorWhen) {
    if when != &ColorWhen::Auto {
        clicolors_control::set_colors_enabled(when == &ColorWhen::Always);
    }
}

fn init_logging() {
    let colors_enabled = clicolors_control::colors_enabled();
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .with_writer(io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .without_time()
        .with_ansi(colors_enabled)
        .init();
}
