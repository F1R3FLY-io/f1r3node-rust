#[path = "../authority_process.rs"]
#[allow(dead_code)]
mod process;
#[path = "../authority_observer.rs"]
mod observer;

use std::path::PathBuf;
use std::time::Duration;

use casper_soak::{encoded, record};
use clap::Parser;
use eyre::Result;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    owner: Option<PathBuf>,
    #[arg(long)]
    request: Option<PathBuf>,
    #[arg(long, default_value_t = 300_000)]
    lifetime_ms: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(request) = args.request {
        let owner = args
            .owner
            .ok_or_else(|| eyre::eyre!("The process owner record is required."))?;
        eyre::ensure!(
            args.config.is_none() && args.output.is_none(),
            "Client and server options cannot be combined."
        );
        let response = process::call(
            &record(&owner)?,
            &record(&request)?,
            Duration::from_secs(30),
        )?;
        println!("{response}");
        return Ok(());
    }
    eyre::ensure!(args.owner.is_none(), "The process request is required.");
    let config = args
        .config
        .ok_or_else(|| eyre::eyre!("The process configuration is required."))?;
    let output = args
        .output
        .ok_or_else(|| eyre::eyre!("The process output is required."))?;
    process::serve(
        &record(&config)?,
        &output,
        Duration::from_millis(args.lifetime_ms),
        |request, output| {
            observer::collect(
                &encoded(&request["binding"])?,
                &encoded(&request["authority"])?,
                output,
            )
        },
    )
}
