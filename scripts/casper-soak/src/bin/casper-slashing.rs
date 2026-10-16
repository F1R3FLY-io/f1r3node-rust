#[path = "../profiles/slashing.rs"]
mod profile;

use std::fs;
use std::path::{Path, PathBuf};

use casper_soak::{
    array, encoded, exclusive, file_hash, hash, models, parse, record, regular, relative, text,
    MAX_BYTES,
};
use clap::{Parser, Subcommand};
use eyre::{ensure, Result};
use serde_json::{json, Value};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    Identity,
    Run {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        artifacts: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Models {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        jar: PathBuf,
        #[arg(long, default_value = "java")]
        java: String,
    },
}
const MODEL_DIR: &str = "formal/tlaplus/casper_soak/profiles/slashing";
fn run(m_path: &Path, r_path: &Path, inputs: &Path, output: &Path) -> Result<Value> {
    ensure!(
        !output.exists() && !output.is_symlink(),
        "The output must be new."
    );
    let m_bytes = regular(m_path, MAX_BYTES)?;
    let r_bytes = regular(r_path, MAX_BYTES)?;
    let mut manifest = parse(&m_bytes)?;
    let request = parse(&r_bytes)?;
    let digest = hash(&m_bytes);
    ensure!(
        request["manifest_digest"] == digest,
        "The exact manifest bytes differ."
    );
    manifest["manifest_digest"] = digest.into();
    let prepared = profile::prepare(&manifest, &request, inputs)?;
    let generation = profile::generate(&prepared)?;
    let mut sources = Vec::new();
    let collection = if generation["scenario_verdict"] == "blocked" {
        json!({"observations":[],"rejected_observations":[],"duplicate_sources":[]})
    } else {
        let inventory_bytes = regular(&relative(inputs, "observations.json")?, MAX_BYTES)?;
        let inventory = parse(&inventory_bytes)?;
        let mut collected = profile::collect(
            &manifest,
            &json!({"request":prepared,"root":inputs,"records":inventory["records"]}),
        )?;
        fs::create_dir_all(output.parent().unwrap_or(Path::new(".")))?;
        fs::create_dir(output)?;
        exclusive(&output.join("observations.json"), &inventory_bytes, true)?;
        for (index, reference) in array(&inventory["records"])?.iter().enumerate() {
            let attempt = (|| -> Result<()> {
                let source = relative(inputs, text(&reference["path"])?)?;
                let bytes = regular(&source, MAX_BYTES)?;
                let retained = format!("raw/{index:03}.json");
                exclusive(&output.join(&retained), &bytes, true)?;
                sources.push(json!({"source":reference,"retained":retained,"sha256":hash(&bytes)}));
                ensure!(
                    hash(&bytes) == reference["sha256"],
                    "A source changed during capture."
                );
                Ok(())
            })();
            if let Err(error) = attempt {
                collected["rejected_observations"]
                    .as_array_mut()
                    .ok_or_else(|| eyre::eyre!("The rejection inventory is invalid."))?
                    .push(json!({"source":reference,"reason":error.to_string()}));
            }
        }
        collected
    };
    let acknowledgments: Vec<_> = array(&collection["observations"])?
        .iter()
        .filter(|v| v["event_kind"] == "step_ack")
        .cloned()
        .collect();
    let mut result = profile::classify(&prepared, &collection, &acknowledgments)?;
    result["manifest_sha256"] = hash(&m_bytes).into();
    result["request_sha256"] = hash(&r_bytes).into();
    result["profile_identity"] = profile::identity();
    result["profile_binary_sha256"] = file_hash(&std::env::current_exe()?)?.into();
    result["retained_sources"] = json!(sources);
    exclusive(&output.join("manifest.json"), &m_bytes, true)?;
    exclusive(&output.join("request.json"), &r_bytes, true)?;
    let mut input_sources = Vec::new();
    for (index, (_, capability)) in casper_soak::object(&manifest["capabilities"])?
        .iter()
        .enumerate()
    {
        if capability["status"] == "qualified" {
            let bytes = casper_soak::artifact(inputs, &capability["qualification"])?;
            let retained = format!("qualification/{index}.json");
            exclusive(&output.join(&retained), &bytes, true)?;
            input_sources.push(json!({"source":capability["qualification"],"retained":retained,"sha256":hash(&bytes)}));
        }
    }
    for (name, reference) in casper_soak::object(&prepared["inputs"])? {
        ensure!(
            name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "The input role is invalid."
        );
        let retained = format!("inputs/{name}.json");
        let bytes = casper_soak::artifact(inputs, reference)?;
        exclusive(&output.join(&retained), &bytes, true)?;
        input_sources.push(json!({"source":reference,"retained":retained,"sha256":hash(&bytes)}));
    }
    result["retained_inputs"] = json!(input_sources);
    exclusive(
        &output.join("generation.json"),
        &encoded(&generation)?,
        true,
    )?;
    exclusive(
        &output.join("collection.json"),
        &encoded(&collection)?,
        true,
    )?;
    exclusive(&output.join("report.json"), &encoded(&result)?, true)?;
    Ok(result)
}
fn model_controls(root: &Path, output: &Path, java: &str, jar: &Path) -> Result<i32> {
    let plan_path = root.join(MODEL_DIR).join("verification-plan.jsonc");
    let plan_bytes = regular(&plan_path, MAX_BYTES)?;
    let plan = parse(&plan_bytes)?;
    let staging = tempfile::Builder::new()
        .prefix("casper-slashing-")
        .tempdir()?;
    let staged_plan = staging.path().join(models::PLAN);
    let parent = staged_plan
        .parent()
        .ok_or_else(|| eyre::eyre!("The plan parent is absent."))?;
    fs::create_dir_all(parent)?;
    let model = text(&plan["model"])?;
    ensure!(
        model == format!("{MODEL_DIR}/Slashing.tla"),
        "The profile model path differs."
    );
    let mut staged = plan.clone();
    staged["model"] = "formal/tlaplus/casper_soak/Slashing.tla".into();
    let staged_bytes = encoded(&staged)?;
    fs::write(&staged_plan, &staged_bytes)?;
    for control in models::controls(&plan)? {
        let name = text(&control["configuration"])?;
        fs::write(
            parent.join(name),
            regular(&relative(&root.join(MODEL_DIR), name)?, MAX_BYTES)?,
        )?;
    }
    let target = relative(staging.path(), text(&staged["model"])?)?;
    fs::create_dir_all(
        target
            .parent()
            .ok_or_else(|| eyre::eyre!("The model parent is absent."))?,
    )?;
    fs::write(&target, regular(&relative(root, model)?, MAX_BYTES)?)?;
    let source = "scripts/casper-soak/src/models.rs";
    let copied = staging.path().join(source);
    fs::create_dir_all(
        copied
            .parent()
            .ok_or_else(|| eyre::eyre!("The runner parent is absent."))?,
    )?;
    let runner = regular(&root.join(source), MAX_BYTES)?;
    ensure!(
        profile::identity()["source_digests"][source] == hash(&runner),
        "The compiled verifier source differs."
    );
    fs::write(copied, &runner)?;
    let status = models::run(staging.path(), output, java, jar, 120)?;
    ensure!(
        file_hash(&plan_path)? == hash(&plan_bytes),
        "The original plan changed."
    );
    ensure!(
        file_hash(&root.join(source))? == hash(&runner),
        "The original verifier source changed."
    );
    let report = record(&output.join("report.json"))?;
    for control in array(&report["results"])? {
        if control["model_sha256"].is_string() {
            ensure!(
                control["model_sha256"] == file_hash(&root.join(model))?,
                "The original model changed."
            );
            ensure!(
                control["configuration_sha256"]
                    == file_hash(&root.join(MODEL_DIR).join(text(&control["configuration"])?))?,
                "An original configuration changed."
            );
        }
    }
    exclusive(&output.join("source-plan.jsonc"), &plan_bytes, true)?;
    exclusive(&output.join("staged-plan.jsonc"), &staged_bytes, true)?;
    exclusive(
        &output.join("profile-model.json"),
        &encoded(
            &json!({"profile_id":profile::PROFILE,"claim":"CLAIM-CASPER-SOAK-006","plan":format!("{MODEL_DIR}/verification-plan.jsonc"),"plan_sha256":hash(&plan_bytes),"staged_plan_sha256":hash(&staged_bytes),"source_model":model,"staged_model":staged["model"],"layout":"Only the plan model path changes for the existing runner layout. Configuration and model bytes remain exact.","driver_binding":"pending","construction":"not-applicable","node_execution":false,"exit":status}),
        )?,
        true,
    )?;
    Ok(status)
}
fn main() {
    let attempt = match Args::parse().action {
        Action::Identity => {
            let mut value = profile::identity();
            match std::env::current_exe()
                .map_err(eyre::Report::from)
                .and_then(|p| file_hash(&p))
            {
                Ok(digest) => {
                    value["executable_sha256"] = digest.into();
                    println!("{value}");
                    Ok(0)
                }
                Err(error) => Err(error),
            }
        }
        Action::Run {
            manifest,
            request,
            artifacts,
            output,
        } => run(&manifest, &request, &artifacts, &output).map(|v| {
            println!("{v}");
            match v["scenario_verdict"].as_str() {
                Some("passed") => 0,
                Some("blocked") => 3,
                Some("invalid_input") => 2,
                _ => 1,
            }
        }),
        Action::Models {
            root,
            output,
            jar,
            java,
        } => model_controls(&root, &output, &java, &jar),
    };
    match attempt {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            println!(
                "{}",
                json!({"profile_id":profile::PROFILE,"scenario_verdict":"invalid_input","soak_verdict":"non_passing","node_launch_count":0,"error":error.to_string()})
            );
            std::process::exit(2);
        }
    }
}
