mod host_control;

use std::path::PathBuf;

use casper_soak::{encoded, manifest, models, runtime};
use clap::{Parser, ValueEnum};
use eyre::{eyre, Result};
use serde_json::json;

#[derive(Clone, ValueEnum)]
enum Action {
    Bind,
    Admit,
    History,
    Run,
    Stop,
    Finish,
    Publish,
    Models,
}
#[derive(Parser)]
struct Args {
    action: Action,
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    manifest: Option<PathBuf>,
    #[arg(long)]
    directory: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    segment: u64,
    #[arg(long, default_value_t = 0)]
    iteration: u64,
    #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
    status: i32,
    #[arg(long, default_value_t = 0)]
    failures: u64,
    #[arg(long, default_value = "completed", value_parser = ["completed", "deadline", "resource_stop", "cancelled", "tool_error", "infrastructure_failure"])]
    termination: String,
    #[arg(long, default_value = "java")]
    java: String,
    #[arg(long)]
    jar: Option<PathBuf>,
    #[arg(long, default_value_t = 120)]
    timeout: u64,
}
fn execute(args: Args) -> Result<i32> {
    let root = args.root.canonicalize()?;
    let output = if args.output.is_absolute() {
        args.output
    } else {
        std::env::current_dir()?.join(args.output)
    };
    let mut result = None;
    match args.action {
        Action::Bind => println!(
            "{}",
            manifest::bind(
                &args
                    .manifest
                    .ok_or_else(|| eyre!("A manifest is required."))?,
                &output
            )?
        ),
        Action::Admit => {
            let c = runtime::admit(&root, 1)?;
            result = Some(
                json!({"iterations":c.manifest["runtime"]["iterations"],"iterations_per_segment":c.manifest["runtime"]["iterations_per_segment"],"provider":c.manifest["provider"],"deadline":c.manifest["deadline"]["epoch_seconds"]}),
            );
        }
        Action::History => runtime::check_history(&output, args.iteration, args.failures)?,
        Action::Stop => runtime::stop(&output)?,
        Action::Run => {
            return runtime::run(
                &root,
                &output,
                &args
                    .directory
                    .ok_or_else(|| eyre!("An iteration directory is required."))?,
                args.segment,
                args.iteration,
            )
        }
        Action::Finish => {
            return runtime::finish(
                &root,
                &output,
                &args
                    .directory
                    .ok_or_else(|| eyre!("An iteration directory is required."))?,
                args.segment,
                args.iteration,
                args.status,
                &args.termination,
            )
        }
        Action::Publish => result = Some(runtime::publish(&root, &output, &args.termination)?),
        Action::Models => {
            let jar = args
                .jar
                .or_else(|| std::env::var_os("TLA_TOOLS_JAR").map(PathBuf::from))
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                        .join(".tla/tla2tools.jar")
                });
            return models::run(
                &root,
                &output,
                &args.java,
                &jar.canonicalize()?,
                args.timeout,
            );
        }
    }
    if let Some(value) = result {
        print!("{}", String::from_utf8(encoded(&value)?)?);
    }
    Ok(0)
}
fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("host-control")) {
        std::process::exit(host_control::main());
    }
    let code = match execute(Args::parse()) {
        Ok(code) => code,
        Err(error) => {
            let blocked = error.downcast_ref::<runtime::Blocked>().is_some();
            eprintln!(
                "{}",
                json!({"scenario_verdict":if blocked {"blocked"} else {"invalid_input"},"soak_verdict":"non_passing","launch_count":0})
            );
            if blocked {
                3
            } else {
                2
            }
        }
    };
    std::process::exit(code);
}
