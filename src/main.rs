#![deny(unused_must_use)]

mod cli;
mod command;
mod daemon;
mod error;
mod job;
mod nix;
mod progress;
mod registrants;
mod terraform;
mod troubleshooter;
mod util;

#[tokio::main]
#[quit::main]
async fn main() {
    cli::run().await;
}
