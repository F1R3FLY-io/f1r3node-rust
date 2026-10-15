#[path = "../campaign_control/models.rs"]
mod models;

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    jar: PathBuf,
    #[arg(long, default_value = "java")]
    java: String,
    #[arg(long, default_value = "timeout")]
    timeout: String,
    #[arg(long, default_value_t = 120)]
    cap: u64,
}

fn main() {
    let args = Args::parse();
    let result = models::run(
        &args.root,
        &args.output,
        &args.java,
        &args.jar,
        &args.timeout,
        args.cap,
    );
    std::process::exit(match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Campaign verification rejected: {error}");
            2
        }
    });
}
