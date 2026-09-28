use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use casper_soak::{
    array, artifact, encoded, exclusive, file_hash, hash, number, object, parse, record, regular,
    relative, text, MAX_BYTES,
};
use eyre::{ensure, Result};
use serde_json::{json, Value};

fn retain(root: &Path, reference: &Value, source: &Path) -> Result<()> {
    let bytes = artifact(source, reference)?;
    let target = relative(root, text(&reference["path"])?)?;
    if target.exists() {
        ensure!(
            regular(&target, MAX_BYTES)? == bytes,
            "Retained input paths collide."
        );
    } else {
        exclusive(&target, &bytes, true)?;
    }
    Ok(())
}

pub fn stage(manifest: &Value, request: &Value, inputs: &Path, output: &Path) -> Result<()> {
    fs::DirBuilder::new().mode(0o700).create(output)?;
    for reference in object(&request["inputs"])?.values() {
        retain(output, reference, inputs)?;
    }
    for capability in object(&manifest["capabilities"])?.values() {
        if capability["status"] == "qualified" {
            retain(output, &capability["qualification"], inputs)?;
        }
    }
    Ok(())
}

fn operations(generation: &Value) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for workload in array(&generation["workloads"])? {
        for operation in array(&workload["steps"])? {
            result.push(
                json!({"index":result.len(),"operation":operation,"member":workload["member"]}),
            );
        }
    }
    ensure!(
        !result.is_empty() && result.len() <= 16,
        "The execution step inventory is invalid."
    );
    Ok(result)
}

fn bind_receipts(root: &Path, envelope: &Value, request_digest: &str) -> Result<Value> {
    if !root.join("execution.json").exists() && !root.join("execution.json").is_symlink() {
        exclusive(
            &root.join("observations.json"),
            &encoded(&json!({"records":[]}))?,
            true,
        )?;
        return Ok(
            json!({"status":"incomplete","reason":"execution_inventory_missing",
            "receipt_count":0,"last_receipt_sha256":null,"sources":[]}),
        );
    }
    let execution = record(&root.join("execution.json"))?;
    ensure!(
        execution["schema_version"] == 1 && execution["request_sha256"] == request_digest,
        "The execution report belongs to another request."
    );
    let receipts = array(&execution["receipts"])?;
    let expected = array(&envelope["operations"])?;
    ensure!(
        receipts.len() <= expected.len(),
        "The execution receipt inventory differs."
    );
    let mut previous = Value::Null;
    let mut last_time = 0;
    let mut last_evaluations = BTreeMap::new();
    let mut faults = BTreeMap::new();
    let mut sources = Vec::new();
    let mut record_ids = BTreeSet::new();
    let mut complete = receipts.len() == expected.len();
    for (reference, operation) in receipts.iter().zip(expected) {
        let bytes = artifact(root, reference)?;
        let receipt = parse(&bytes)?;
        ensure!(
            receipt["schema_version"] == 1
                && receipt["request_sha256"] == request_digest
                && receipt["previous_receipt_sha256"] == previous
                && receipt["step"] == *operation,
            "The execution receipt chain or step differs."
        );
        let elapsed = number(&receipt["elapsed_ns"])?;
        ensure!(
            elapsed >= last_time && elapsed <= number(&envelope["timeout_ms"])? * 1_000_000,
            "The execution receipt time is invalid."
        );
        last_time = elapsed;
        let status = text(&receipt["status"])?;
        ensure!(
            ["applied", "not_applied", "unknown"].contains(&status),
            "The step status is unsupported."
        );
        complete &= status == "applied";
        let input_records = object(&receipt["observed_inputs"])?;
        ensure!(
            input_records.len() == 3 || (status != "applied" && input_records.is_empty()),
            "The observed input inventory differs."
        );
        for role in ["dag", "electorate", "justification"]
            .into_iter()
            .filter(|_| !input_records.is_empty())
        {
            let observed = &receipt["observed_inputs"][role];
            let data = artifact(root, observed)?;
            ensure!(
                hash(&data) == envelope["request"]["inputs"][role]["sha256"],
                "The executed input bytes differ from the pinned fixture."
            );
            sources.push(observed.clone());
        }
        let observations = array(&receipt["observations"])?;
        let mut evaluation = None;
        for observation_ref in observations {
            let observation = parse(&artifact(root, observation_ref)?)?;
            for field in [
                "manifest_digest",
                "run_id",
                "scenario_id",
                "pair_id",
                "evidence_kind",
                "phase",
                "seed",
                "segment",
                "iteration",
            ] {
                ensure!(
                    observation[field] == envelope["request"][field],
                    "An execution observation identity differs: {field}."
                );
            }
            for field in ["member_id", "node_id", "incarnation", "evaluation_mode"] {
                ensure!(
                    observation[field] == operation["member"][field],
                    "An execution member differs: {field}."
                );
            }
            let id = text(&observation["record_id"])?;
            ensure!(
                !id.is_empty() && record_ids.insert(id.to_owned()),
                "An execution observation identity repeats."
            );
            ensure!(
                observation_ref["capture_state"] == "captured"
                    && observation_ref["producer"] == observation["producer"]
                    && array(&observation_ref["observation_ids"])?
                        .contains(&observation["record_id"]),
                "The execution observation reference differs."
            );
            match text(&observation["event_kind"])? {
                "authority_snapshot" => {
                    ensure!(
                        operation["operation"] == "evaluate" && evaluation.is_none(),
                        "An authority observation has no unique evaluation step."
                    );
                    evaluation = Some(observation_ref.clone());
                }
                "fault_ack" => {
                    let id = text(&observation["payload"]["fault_id"])?;
                    ensure!(
                        array(&envelope["generation"]["fault_requests"])?
                            .iter()
                            .any(|f| f["fault_id"] == id
                                && f["member_id"] == operation["member"]["member_id"]
                                && (f["action"] == "pause"
                                    || operation["operation"] == "await_restart_receipt")),
                        "The fault acknowledgment is not scheduled for this member."
                    );
                    ensure!(
                        faults
                            .insert(id.to_owned(), observation_ref.clone())
                            .is_none(),
                        "The fault acknowledgment repeats."
                    );
                }
                _ => eyre::bail!("The execution observation kind is unsupported."),
            }
            sources.push(observation_ref.clone());
        }
        if operation["operation"] == "evaluate" {
            let member = text(&operation["member"]["member_id"])?;
            last_evaluations.insert(member.to_owned(), evaluation);
        }
        sources.push(reference.clone());
        previous = hash(&bytes).into();
    }
    let selected: Vec<_> = last_evaluations
        .into_values()
        .flatten()
        .chain(faults.into_values())
        .collect();
    let mut ordered = selected
        .into_iter()
        .map(|reference| -> Result<(u64, Value)> {
            let observation = parse(&artifact(root, &reference)?)?;
            Ok((
                casper_soak::manifest::decimal(&observation["producer_sequence"])?,
                reference,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    ordered.sort_by_key(|(sequence, _)| *sequence);
    let selected: Vec<_> = ordered
        .into_iter()
        .map(|(_, reference)| reference)
        .collect();
    exclusive(
        &root.join("observations.json"),
        &encoded(&json!({"records":selected}))?,
        true,
    )?;
    Ok(
        json!({"status":if complete {"bound"} else {"incomplete"},"receipt_count":receipts.len(),
        "last_receipt_sha256":previous,"sources":sources}),
    )
}

pub fn classifier_inputs(
    manifest: &Value,
    request: &Value,
    execution: &Path,
    output: &Path,
    binding: &Value,
) -> Result<()> {
    stage(manifest, request, &execution.join("inputs"), output)?;
    for source in array(&binding["sources"])? {
        retain(output, source, &execution.join("results"))?;
    }
    let inventory = regular(&execution.join("results/observations.json"), MAX_BYTES)?;
    exclusive(&output.join("observations.json"), &inventory, true)?;
    Ok(())
}

struct ChildGroup(std::process::Child);
impl Drop for ChildGroup {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

pub fn launch(
    manifest: &Value,
    request: &Value,
    generation: &Value,
    inputs: &Path,
    output: &Path,
) -> Result<Value> {
    ensure!(
        generation["scenario_verdict"] == "ready",
        "The profile has not admitted this scenario."
    );
    let specification = &manifest["runtime"]["authority_executor"];
    let timeout_ms = number(&specification["timeout_ms"])?;
    ensure!(
        (1..=300_000).contains(&timeout_ms),
        "The executor timeout is invalid."
    );
    let args = array(&specification["arguments"])?;
    ensure!(args.len() <= 32, "The executor argument bound is exceeded.");
    let args: Vec<_> = args.iter().map(text).collect::<Result<_>>()?;
    ensure!(
        args.iter().all(|s| s.len() <= 4096 && !s.contains('\0')),
        "An executor argument is invalid."
    );
    let reference = &specification["artifact"];
    let source = relative(inputs, text(&reference["path"])?)?;
    let bytes = regular(&source, 128 * MAX_BYTES)?;
    ensure!(
        hash(&bytes) == reference["sha256"] && bytes.len() as u64 == number(&reference["bytes"])?,
        "The pinned executor bytes differ."
    );
    fs::DirBuilder::new().mode(0o700).create(output)?;
    let output = fs::canonicalize(output)?;
    let sealed = output.join("inputs");
    stage(manifest, request, inputs, &sealed)?;
    for asset in array(&specification["assets"])? {
        retain(&sealed, asset, inputs)?;
    }
    let executable = output.join("executor");
    exclusive(&executable, &bytes, true)?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o500))?;
    let mut nonce = [0; 32];
    File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    let envelope = json!({"schema_version":1,"execution_nonce":hash(&nonce),"manifest":manifest,
        "request":request,"generation":generation,"operations":operations(generation)?,
        "timeout_ms":timeout_ms,"input_root":sealed,"output_root":output.join("results")});
    let encoded_request = encoded(&envelope)?;
    exclusive(&output.join("request.json"), &encoded_request, true)?;
    let results = output.join("results");
    fs::DirBuilder::new().mode(0o700).create(&results)?;
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("stdout.txt"))?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("stderr.txt"))?;
    let started = Instant::now();
    let mut child = ChildGroup(
        Command::new(&executable)
            .args(args)
            .env(
                "CASPER_AUTHORITY_EXECUTION_REQUEST",
                output.join("request.json"),
            )
            .env("CASPER_AUTHORITY_EXECUTION_OUTPUT", &results)
            .current_dir(&sealed)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .process_group(0)
            .spawn()?,
    );
    let mut failure = None;
    let exit = loop {
        if let Some(status) = child.0.try_wait()? {
            break status.code();
        }
        if started.elapsed() >= Duration::from_millis(timeout_ms) {
            failure = Some("executor_deadline");
            break None;
        }
        if ["stdout.txt", "stderr.txt"]
            .iter()
            .any(|name| fs::metadata(output.join(name)).is_ok_and(|m| m.len() > MAX_BYTES))
        {
            failure = Some("executor_output_limit");
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    drop(child);
    if ["stdout.txt", "stderr.txt"]
        .iter()
        .any(|name| fs::metadata(output.join(name)).is_ok_and(|m| m.len() > MAX_BYTES))
    {
        failure = Some("executor_output_limit");
    }
    if failure.is_none() && exit != Some(0) {
        failure = Some("executor_exit");
    }
    let attempt = (|| -> Result<Value> {
        ensure!(
            file_hash(&executable)? == reference["sha256"],
            "The retained executor changed."
        );
        for input in object(&request["inputs"])?.values() {
            artifact(&sealed, input)?;
        }
        for asset in array(&specification["assets"])? {
            artifact(&sealed, asset)?;
        }
        bind_receipts(&results, &envelope, &hash(&encoded_request))
    })();
    let binding = match attempt {
        Ok(value) => value,
        Err(error) => json!({"status":"invalid_input","error":error.to_string()}),
    };
    let report = json!({"schema_version":1,"executor_sha256":reference["sha256"],
        "request_sha256":hash(&encoded_request),"exit_code":exit,"failure":failure,
        "binding":binding,"executor_launch_count":1,"qualification":"pending","soak_verdict":"non_passing"});
    exclusive(&output.join("report.json"), &encoded(&report)?, true)?;
    Ok(report)
}
