#[path = "../authority_live.rs"]
mod live;

use std::path::PathBuf;

use casper_soak::{regular, MAX_BYTES};
use clap::Parser;
use eyre::Result;
use serde_json::json;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    request: Option<PathBuf>,
    #[arg(long)]
    output: Option<PathBuf>,
}

fn run(args: Args) -> Result<i32> {
    let request = args
        .request
        .or_else(|| std::env::var_os("CASPER_AUTHORITY_EXECUTION_REQUEST").map(Into::into))
        .ok_or_else(|| eyre::eyre!("The execution request is required."))?;
    let output = args
        .output
        .or_else(|| std::env::var_os("CASPER_AUTHORITY_EXECUTION_OUTPUT").map(Into::into))
        .ok_or_else(|| eyre::eyre!("The execution output is required."))?;
    let report = live::run(&regular(&request, MAX_BYTES)?, &output)?;
    println!("{report}");
    Ok(if report["status"] == "captured" { 0 } else { 1 })
}

fn main() {
    match run(Args::parse()) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            println!(
                "{}",
                json!({"status":"rejected","error":error.to_string(),
                "qualification":"pending","soak_verdict":"non_passing"})
            );
            std::process::exit(2);
        }
    }
}
