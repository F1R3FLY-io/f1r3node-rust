#[path = "../authority_observer.rs"]
mod observer;

use std::path::PathBuf;

use casper_soak::{regular, MAX_BYTES};
use clap::Parser;
use eyre::Result;
use serde_json::json;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    binding: PathBuf,
    #[arg(long)]
    authority: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

fn run(args: Args) -> Result<i32> {
    let binding = regular(&args.binding, MAX_BYTES)?;
    let authority = regular(&args.authority, MAX_BYTES)?;
    let report = observer::collect(&binding, &authority, &args.output)?;
    println!("{report}");
    Ok(if report["status"] == "captured" { 0 } else { 2 })
}

fn main() {
    match run(Args::parse()) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            println!(
                "{}",
                json!({"status":"rejected","error":error.to_string(),"qualification":"pending","soak_verdict":"non_passing","node_launch_count":0})
            );
            std::process::exit(2);
        }
    }
}
