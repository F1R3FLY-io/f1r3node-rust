use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use casper_soak::models::classify;
use casper_soak::{encoded, exclusive, file_hash, record, regular, MAX_BYTES};
use eyre::{ensure, Result};
use serde_json::{json, Value};

pub const DIRECTORY: &str = "formal/tlaplus/casper_soak/campaign";
pub const JAR_SHA256: &str = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88";
pub const CONTROLS: [(&str, Option<&str>, Option<&str>); 5] = [
    ("MC_CampaignControl.cfg", None, None),
    (
        "MC_CampaignControl_approval_unsafe.cfg",
        Some("AuthorizedLaunch"),
        Some("BreakApproval"),
    ),
    (
        "MC_CampaignControl_reservation_unsafe.cfg",
        Some("OneSubmission"),
        Some("BreakReservation"),
    ),
    (
        "MC_CampaignControl_schedule_unsafe.cfg",
        Some("ScheduledLaunch"),
        Some("BreakSchedule"),
    ),
    (
        "MC_CampaignControl_termination_unsafe.cfg",
        Some("ConfirmedCleanup"),
        Some("BreakTermination"),
    ),
];

pub fn registration() -> Value {
    json!({"schema_version":1,"scope":"bounded-campaign-control-refutation",
        "model":"CampaignControl.tla","tlc_sha256":JAR_SHA256,
        "properties":["TypeOK","OneSubmission","AuthorizedLaunch","ScheduledLaunch","ConfirmedCleanup","PreservedFailure"],
        "controls":CONTROLS.iter().map(|(name, property, knob)| json!({
            "configuration":name,"property":property,"knob":knob,
            "expected_exit":if property.is_some(){12}else{0}})).collect::<Vec<_>>()})
}

pub fn configuration(knob: Option<&str>) -> String {
    let mut body = "SPECIFICATION Spec\nCONSTANTS\n".to_string();
    for name in [
        "BreakReservation",
        "BreakApproval",
        "BreakSchedule",
        "BreakTermination",
    ] {
        body.push_str(&format!(
            "  {name} = {}\n",
            if knob == Some(name) { "TRUE" } else { "FALSE" }
        ));
    }
    body.push_str("INVARIANTS TypeOK OneSubmission AuthorizedLaunch ScheduledLaunch ConfirmedCleanup PreservedFailure\nCHECK_DEADLOCK FALSE\n");
    body
}

pub fn validate(root: &Path) -> Result<Value> {
    let directory = root.join(DIRECTORY);
    ensure!(
        record(&directory.join("verification-plan.jsonc"))? == registration(),
        "The campaign model registration differs."
    );
    let mut sources = json!({});
    for path in ["CampaignControl.tla", "verification-plan.jsonc"]
        .into_iter()
        .chain(CONTROLS.iter().map(|(name, _, _)| *name))
    {
        sources[format!("{DIRECTORY}/{path}")] = json!(file_hash(&directory.join(path))?);
    }
    for (name, _, knob) in CONTROLS {
        ensure!(
            regular(&directory.join(name), MAX_BYTES)? == configuration(knob).as_bytes(),
            "A campaign model configuration differs from its registration."
        );
    }
    Ok(sources)
}

pub fn run(
    root: &Path,
    output: &Path,
    java: &str,
    jar: &Path,
    timeout: &str,
    cap: u64,
) -> Result<i32> {
    ensure!(
        (1..=300).contains(&cap),
        "The model timeout must be between 1 and 300 seconds."
    );
    fs::create_dir(output)?;
    let mut report = json!({"schema_version":1,"scope":"bounded-campaign-control-refutation",
        "status":"failed","claim_discharge":"pending","binding":"pending","soak":"pending",
        "node_launches":0,"cloud_launches":0,"seed":1,"workers":1,"heap_limit_mb":512,
        "timeout_seconds":cap,"kill_grace_seconds":5,"results":[]});
    let attempt = (|| -> Result<()> {
        ensure!(
            file_hash(jar)? == JAR_SHA256,
            "The TLC JAR differs from the pinned verifier."
        );
        report["tlc_sha256"] = json!(JAR_SHA256);
        let sources = validate(root)?;
        report["source_digests"] = sources.clone();
        report["runner_sha256"] = json!(file_hash(
            &root.join("scripts/casper-soak/src/campaign_control/models.rs")
        )?);
        report["binary_sha256"] = json!(file_hash(&std::env::current_exe()?)?);
        for (name, property, _) in CONTROLS {
            let log_name = format!("{name}.log");
            let log_path = output.join(&log_name);
            let started = Instant::now();
            let mut result = json!({"configuration":name,"expected_property":property,
                "expected_exit":if property.is_some(){12}else{0},"exit":null,"outcome":"tool_error","log":log_name});
            let model_attempt = (|| -> Result<()> {
                let metadata = tempfile::tempdir()?;
                let log = File::create(&log_path)?;
                let status = Command::new(timeout)
                    .args([
                        "--signal=TERM",
                        "--kill-after=5",
                        &cap.to_string(),
                        java,
                        "-Xmx512m",
                        "-XX:+UseParallelGC",
                        "-cp",
                    ])
                    .arg(jar)
                    .args(["tlc2.TLC", "-workers", "1", "-seed", "1", "-metadir"])
                    .arg(metadata.path())
                    .arg("-config")
                    .arg(root.join(DIRECTORY).join(name))
                    .arg(root.join(DIRECTORY).join("CampaignControl.tla"))
                    .stdout(Stdio::from(log.try_clone()?))
                    .stderr(Stdio::from(log))
                    .status()?;
                let code = status.code().unwrap_or(128);
                result["exit"] = json!(code);
                let log = String::from_utf8(regular(&log_path, MAX_BYTES)?)?;
                result["outcome"] = json!(if code == 124 {
                    "timeout"
                } else if classify(code, &log, property) {
                    "passed"
                } else {
                    "unexpected_result"
                });
                Ok(())
            })();
            if model_attempt.is_err() {
                result["outcome"] = json!("tool_error");
            }
            if !log_path.exists() {
                fs::write(&log_path, "The model verifier could not start.\n")?;
            }
            result["log_sha256"] = json!(file_hash(&log_path)?);
            result["elapsed_seconds"] = json!(started.elapsed().as_secs_f64());
            report["results"].as_array_mut().unwrap().push(result);
        }
        ensure!(
            validate(root)? == sources && file_hash(jar)? == JAR_SHA256,
            "The model inputs changed during verification."
        );
        ensure!(
            report["results"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["outcome"] == "passed"),
            "A campaign model control failed."
        );
        report["status"] = json!("passed");
        Ok(())
    })();
    if let Err(error) = &attempt {
        report["error"] = json!(error.to_string());
    }
    exclusive(&output.join("report.json"), &encoded(&report)?, false)?;
    Ok(if attempt.is_ok() { 0 } else { 1 })
}
