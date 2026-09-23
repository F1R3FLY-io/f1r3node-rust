#[path = "../campaign_control/mod.rs"]
pub mod campaign_control;

use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

use campaign_control::{Config, Provider};
use casper_soak::{encoded, exclusive, file_hash, record, regular, relative, text, MAX_BYTES};
use clap::Parser;
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

#[derive(Parser)]
struct Args {
    command: String,
    #[arg(long)]
    config: PathBuf,
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long)]
    request: Option<PathBuf>,
    #[arg(long)]
    plan: Option<PathBuf>,
    #[arg(long)]
    run: Option<String>,
    #[arg(long)]
    evidence: Option<PathBuf>,
    #[arg(long)]
    slot: Option<String>,
    #[arg(long)]
    observations: Option<PathBuf>,
    #[arg(long)]
    snapshot: Option<PathBuf>,
    #[arg(long)]
    snapshot_sha256: Option<String>,
    #[arg(long)]
    worker_result: Option<PathBuf>,
    #[arg(long)]
    artifact: Option<PathBuf>,
    #[arg(long)]
    archive: Option<PathBuf>,
}

fn required<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| eyre!("A required command input is missing."))
}

fn run() -> Result<()> {
    let args = Args::parse();
    let config = Config::new(record(&args.config)?)?;
    if args.command == "config-digest" {
        println!("{}", config.digest);
        return Ok(());
    }
    let request = regular(&required(args.request)?, MAX_BYTES)?;
    let plan = record(&required(args.plan)?)?;
    let run = required(args.run)?;
    campaign_control::validate_plan(&config, &request, &plan)?;
    if args.command == "approval" {
        println!(
            "{}",
            campaign_control::approval_comment(&config, &request, &plan, &run)?
        );
        return Ok(());
    }
    ensure!(
        matches!(
            args.command.as_str(),
            "dispatch" | "host-admission" | "finish"
        ),
        "The controller command is unsupported."
    );
    let evidence = required(args.evidence)?;
    DirBuilder::new().mode(0o700).create(&evidence)?;
    let result = (|| -> Result<Value> {
        ensure!(
            std::env::var("CASPER_CAMPAIGN_CONFIG_SHA256")
                .ok()
                .as_deref()
                == Some(config.digest.as_str()),
            "The controller configuration differs from the trusted deployment pin."
        );
        if args.command != "finish" {
            config.verify_sources(&args.root)?;
            campaign_control::qualified(&args.root, &plan)?;
        }
        let mut provider = campaign_control::transport::Cli::new(config.clone(), &evidence)?;
        if args.command == "host-admission" {
            let slot_name = required(args.slot)?;
            ensure!(
                campaign_control::SLOTS.contains(&slot_name.as_str()),
                "The host slot is invalid."
            );
            let state = record(&required(args.snapshot)?)?;
            ensure!(
                casper_soak::hash(&encoded(&state)?) == required(args.snapshot_sha256)?,
                "The host snapshot differs from the controller output."
            );
            campaign_control::validate_state(&config, &state)?;
            ensure!(
                state["slots"][&slot_name]["run_id"] == run
                    && state["slots"][&slot_name]["plan"] == plan
                    && state["slots"][&slot_name]["request_sha256"] == casper_soak::hash(&request),
                "The host run differs from its reservation."
            );
            return campaign_control::host_admission(
                &config,
                &state["slots"][&slot_name],
                &record(&required(args.observations)?)?,
                provider.now(),
            );
        }
        ensure!(
            std::env::var("GITHUB_REPOSITORY").ok().as_deref()
                == Some(campaign_control::REPOSITORY)
                && std::env::var("GITHUB_EVENT_NAME").ok().as_deref() == Some("workflow_dispatch")
                && std::env::var("GITHUB_RUN_ATTEMPT").ok().as_deref() == Some("1")
                && std::env::var("GITHUB_RUN_ID").ok().as_deref() == Some(run.as_str()),
            "The execution is not the bound first-attempt workflow."
        );
        if args.command == "finish" {
            let worker = (|| -> Result<(Value, Value)> {
                let artifact = record(&required(args.artifact)?)?;
                let result =
                    campaign_control::results::archive_result(&required(args.archive)?, &artifact)?;
                if let Some(path) = args.worker_result {
                    ensure!(
                        record(&path)? == result,
                        "The extracted result differs from the authenticated archive."
                    );
                }
                Ok((result, artifact))
            })()
            .ok();
            let result = campaign_control::results::finish(
                &mut provider,
                &config,
                &request,
                &plan,
                &run,
                worker.as_ref().map(|(r, a)| (r, a)),
            )?;
            exclusive(
                &evidence.join("campaign-result.json"),
                &encoded(&result)?,
                false,
            )?;
            ensure!(
                result["result"] == "passed",
                "The campaign result is non-passing."
            );
            return Ok(result);
        }
        let candidate = &config.value["compute"]["candidates"][text(&plan["candidate_id"])?];
        let bootstrap = relative(&args.root, text(&candidate["bootstrap_path"])?)?;
        ensure!(
            file_hash(&bootstrap)? == text(&candidate["bootstrap_sha256"])?,
            "The launch bootstrap differs from its pin."
        );
        let bootstrap = String::from_utf8(regular(&bootstrap, 32768)?)?;
        ensure!(
            bootstrap.matches("__CASPER_GUARD_BASE64__").count() == 1,
            "The bootstrap does not bind the host guardian."
        );
        let guardian = regular(
            &args.root.join("scripts/casper-soak/campaign-host-guard.sh"),
            16384,
        )?;
        let bootstrap = bootstrap.replace(
            "__CASPER_GUARD_BASE64__",
            &campaign_control::operations::base64(&guardian),
        );
        campaign_control::operations::dispatch(
            &mut provider,
            &config,
            &request,
            &plan,
            &run,
            bootstrap.trim(),
        )
    })();
    let report = match &result {
        Ok(receipt) => json!({"schema_version":1,"control_result":"passed","receipt":receipt,
            "request_sha256":casper_soak::hash(&request),"config_digest":config.digest,
            "claim_discharge":"pending","campaign_result":"pending"}),
        Err(_) => json!({"schema_version":1,"control_result":"non_passing",
            "request_sha256":casper_soak::hash(&request),"config_digest":config.digest,
            "launch_outcome":"consult_authoritative_record","termination_confirmed":false,
            "claim_discharge":"pending","campaign_result":"non_passing"}),
    };
    exclusive(&evidence.join("report.json"), &encoded(&report)?, false)?;
    fs::File::open(&evidence)?.sync_all()?;
    result?;
    println!("{}", report);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Campaign control rejected: {error}");
        std::process::exit(2);
    }
}
