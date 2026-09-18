use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde_json::json;

use crate::*;

#[derive(Debug)]
pub struct Blocked;
impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("The profile or required qualification is unavailable.")
    }
}
impl std::error::Error for Blocked {}
fn env(name: &str) -> Result<String> { Ok(std::env::var(name)?) }
fn yes(v: &Value) -> bool { v.as_bool() == Some(true) }
fn identity(values: &[&Value]) -> Result<String> { Ok(serde_json::to_string(values)?) }
pub struct Context {
    pub manifest: Value,
    pub digest: String,
    pub request: Value,
    pub inputs: PathBuf,
    pub approval: Value,
}
pub fn admit(root: &Path, iteration: u64) -> Result<Context> {
    let bytes = regular(Path::new(&env("SOAK_MANIFEST_PATH")?), MAX_BYTES)?;
    let m = parse(&bytes)?;
    manifest::validate(&m)?;
    let digest = hash(&bytes);
    let inputs = PathBuf::from(env("SOAK_INPUT_DIR")?);
    let approved = regular(Path::new(&env("SOAK_APPROVAL_PATH")?), MAX_BYTES)?;
    ensure!(
        hash(&approved) == env("SOAK_APPROVAL_SHA256")?,
        "The approval digest differs."
    );
    let approval = parse(&approved)?;
    ensure!(
        approval["manifest_digest"] == digest && approval["evidence_kind"] == m["evidence_kind"],
        "The approval identity differs."
    );
    if !yes(&approval["approved"]) {
        return Err(Blocked.into());
    }
    if m["profile_id"] != "harness-lifecycle" {
        return Err(Blocked.into());
    }
    ensure!(
        m["evidence_kind"] == "synthetic_fixture",
        "The fixture cannot produce node observations."
    );
    for name in CORE {
        ensure!(
            m["source_digests"].get(*name).is_some(),
            "A required source pin is missing."
        );
    }
    for (name, expected) in object(&m["source_digests"])? {
        ensure!(
            file_hash(&relative(root, name)?)? == text(expected)?,
            "The source bytes differ."
        );
    }
    for (name, bytes) in compiled_sources() {
        ensure!(
            m["source_digests"][name] == hash(bytes),
            "The executable was compiled from different source bytes."
        );
    }
    let rt = &m["runtime"];
    ensure!(
        text(&rt["harness_digest"])? == file_hash(&std::env::current_exe()?)?,
        "The harness executable differs."
    );
    let iterations = number(&rt["iterations"])?;
    let segment_limit = number(&rt["iterations_per_segment"])?;
    ensure!(
        (1..=100000).contains(&iterations) && (1..=iterations).contains(&segment_limit),
        "The iteration bounds are invalid."
    );
    ensure!(
        !array(&rt["required_capabilities"])?.is_empty(),
        "The capability inventory is empty."
    );
    for capability in array(&rt["required_capabilities"])? {
        let cap = &m["capabilities"][text(capability)?];
        if cap["status"] != "qualified" {
            return Err(Blocked.into());
        }
        ensure!(
            cap["provider"] == m["provider"] && cap["revision"] == m["node_revision"],
            "The capability identity differs."
        );
        let reference = &cap["qualification"];
        let qualification = parse(&artifact(&inputs, reference)?)?;
        ensure!(
            array(&approval["qualification_digests"])?.contains(&reference["sha256"]),
            "The qualification has no approval."
        );
        ensure!(
            qualification
                == json!({"capability":capability,"provider":m["provider"],"node_revision":m["node_revision"],"status":"qualified","evidence_kind":m["evidence_kind"]}),
            "The qualification is inconsistent."
        );
    }
    let assets = &rt["assets"];
    for reference in object(assets)?.values() {
        let path = relative(&inputs, text(&reference["path"])?)?;
        ensure!(
            file_hash(&path)? == text(&reference["sha256"])?
                && fs::metadata(path)?.len() == number(&reference["bytes"])?,
            "An input artifact differs."
        );
    }
    for (role, field) in [
        ("configuration", "configuration_digest"),
        ("fixture", "fixture_digest"),
        ("expectation", "expectation_digest"),
        ("node_binary", "node_binary_digest"),
    ] {
        ensure!(assets[role]["sha256"] == m[field], "An input pin differs.");
    }
    ensure!(
        m["profile_digest"] == file_hash(&root.join("scripts/casper-soak/src/runtime.rs"))?,
        "The profile digest differs."
    );
    ensure!(
        assets["executor"]["sha256"] == approval["executor_digest"],
        "The executor is not approved."
    );
    if m["provider"] == "docker" {
        ensure!(
            assets["image_manifest"]["sha256"] == m["image_digest"],
            "The image manifest differs."
        );
    }
    let config = parse(&artifact(&inputs, &assets["configuration"])?)?;
    let limits = &m["resource_limits"];
    ensure!(
        number(&limits["children"])? == 1,
        "The driver requires one workload child."
    );
    for key in [
        "timeout_seconds",
        "rss_ceiling_mb",
        "host_free_floor_mb",
        "disk_free_floor_mb",
        "artifact_bytes",
    ] {
        ensure!(
            number(&limits[key])? <= i64::MAX as u64,
            "A resource limit exceeds the driver range."
        );
    }
    ensure!(
        number(&limits["timeout_seconds"])? > 0 && number(&limits["artifact_bytes"])? > 0,
        "A resource bound is empty."
    );
    for (variable, key) in [
        ("SOAK_DURATION_SECONDS", "duration_seconds"),
        ("SOAK_RSS_CEILING_MB", "rss_ceiling_mb"),
        ("SOAK_HOST_FREE_FLOOR_MB", "host_free_floor_mb"),
        ("SOAK_DISK_FREE_FLOOR_MB", "disk_free_floor_mb"),
    ] {
        ensure!(
            number(&config[key])?.to_string() == env(variable)?,
            "The effective configuration differs."
        );
        if limits.get(key).is_some() {
            ensure!(
                config[key] == limits[key],
                "The resource configuration differs."
            );
        }
    }
    ensure!(
        config["policy_variant"] == m["policy_variant"],
        "The policy configuration differs."
    );
    ensure!(
        std::env::var("SOAK_RUN_BENCHMARKS").unwrap_or_else(|_| "false".into()) == "false",
        "Unpinned benchmarks are forbidden."
    );
    ensure!(
        m["deadline"]["clock_id"] == "unix-seconds"
            && manifest::decimal(&m["deadline"]["epoch_seconds"])? <= i64::MAX as u64,
        "The deadline is invalid."
    );
    ensure!(
        m["tool_versions"]["casper_soak"] == env!("CARGO_PKG_VERSION"),
        "The harness version differs."
    );
    if m["phase"] == "post_pr216_merge" {
        let gate = &m["merge_gate"];
        let proof = parse(&artifact(&inputs, &gate["ancestry"])?)?;
        ensure!(
            array(&approval["qualification_digests"])?.contains(&gate["ancestry"]["sha256"]),
            "The merge evidence has no approval."
        );
        ensure!(
            proof["pr"] == 216 && yes(&proof["merged"]) && yes(&proof["accepted_handoff"]),
            "The actual merge and accepted handoff are required."
        );
        ensure!(
            proof["merge_revision"] == gate["merge_revision"]
                && proof["selected_dev_revision"] == m["node_revision"],
            "The merge identity differs."
        );
        let status = Command::new("timeout")
            .args([
                "10",
                "git",
                "-C",
                &env("SOAK_NODE_REPO_DIR")?,
                "merge-base",
                "--is-ancestor",
                text(&proof["merge_revision"])?,
                text(&proof["selected_dev_revision"])?,
            ])
            .status()?;
        ensure!(
            status.success(),
            "The merge is not in selected dev ancestry."
        );
    }
    let envelope = parse(&artifact(&inputs, &assets["request"])?)?;
    let requests = envelope["scenarios"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![envelope.clone()]);
    ensure!(
        !requests.is_empty()
            && requests.len() as u64 <= number(&m["bounds"]["scenarios"])?
            && iterations >= requests.len() as u64,
        "The scenario bounds are invalid."
    );
    let names: BTreeSet<_> = requests
        .iter()
        .map(|v| text(&v["scenario_id"]))
        .collect::<Result<_>>()?;
    let required: BTreeSet<_> = array(&m["required_scenarios"])?
        .iter()
        .map(text)
        .collect::<Result<_>>()?;
    ensure!(
        names.len() == requests.len() && names == required,
        "The scenario inventory differs."
    );
    let mut request = requests[(iteration.saturating_sub(1) as usize) % requests.len()].clone();
    ensure!(
        request["seed"] == m["seed"] && !array(&request["required_observations"])?.is_empty(),
        "The request identity differs."
    );
    request["fixture"] = parse(&artifact(&inputs, &assets["fixture"])?)?;
    request["expectation"] = parse(&artifact(&inputs, &assets["expectation"])?)?;
    request["policy_variant"] = m["policy_variant"].clone();
    Ok(Context {
        manifest: m,
        digest,
        request,
        inputs,
        approval,
    })
}
pub fn compiled_sources() -> Vec<(&'static str, &'static [u8])> {
    vec![
        (
            "scripts/casper-soak/Cargo.toml",
            include_bytes!("../Cargo.toml"),
        ),
        ("scripts/casper-soak/src/lib.rs", include_bytes!("lib.rs")),
        ("scripts/casper-soak/src/main.rs", include_bytes!("main.rs")),
        (
            "scripts/casper-soak/src/manifest.rs",
            include_bytes!("manifest.rs"),
        ),
        (
            "scripts/casper-soak/src/models.rs",
            include_bytes!("models.rs"),
        ),
        (
            "scripts/casper-soak/src/runtime.rs",
            include_bytes!("runtime.rs"),
        ),
    ]
}
pub fn history(output: &Path) -> Result<Vec<Value>> {
    let root = relative(output, "casper-history")?;
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut paths: Vec<_> = fs::read_dir(root)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();
    let mut entries = Vec::new();
    let mut previous = Value::Null;
    for path in paths {
        let raw = regular(&path, MAX_BYTES)?;
        let entry = parse(&raw)?;
        let iteration = entries.len() + 1;
        ensure!(
            entry["iteration"] == iteration
                && entry["previous_digest"] == previous
                && path.file_name().and_then(|n| n.to_str())
                    == Some(&format!("{iteration:08}.json")),
            "The history chain differs."
        );
        let prefix = format!("casper-capture/{iteration:08}");
        let refs = array(&entry["artifact_inventory"])?;
        let mut names = BTreeSet::new();
        for reference in refs {
            let name = text(&reference["path"])?;
            ensure!(
                name.starts_with(&(prefix.clone() + "/")) && names.insert(name.to_string()),
                "The artifact inventory differs."
            );
            artifact(output, reference)?;
        }
        if yes(&entry["capture_complete"]) {
            let actual: BTreeSet<_> = walk(&relative(output, &prefix)?)?
                .iter()
                .map(|p| {
                    p.strip_prefix(output)
                        .map(|p| p.to_string_lossy().into_owned())
                })
                .collect::<std::result::Result<_, _>>()?;
            ensure!(
                !actual.is_empty() && actual == names,
                "The capture inventory is incomplete."
            );
        }
        previous = hash(&raw).into();
        entries.push(entry);
    }
    Ok(entries)
}
pub fn check_history(output: &Path, iterations: u64, failures: u64) -> Result<()> {
    let entries = history(output)?;
    ensure!(
        entries.len() as u64 == iterations && entries.iter().all(|e| yes(&e["capture_complete"])),
        "The checkpoint or capture is incomplete."
    );
    ensure!(
        failures
            >= entries
                .iter()
                .filter(|e| e["scenario_verdict"] != "passed")
                .count() as u64,
        "The checkpoint erased failure history."
    );
    let count = fs::read_dir(output)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("iteration-"))
        .count();
    ensure!(
        count as u64 == iterations,
        "An uncommitted iteration cannot be reused."
    );
    Ok(())
}
fn generate(request: &Value) -> Value {
    json!({"workloads":[{"scenario_id":request["scenario_id"],"action":"fixture-observation","seed":request["seed"]}],"faults":request["fault_schedule"]})
}
fn request_context(c: &Context, segment: u64, iteration: u64) -> Value {
    let mut request = c.request.clone();
    request["manifest_digest"] = c.digest.clone().into();
    request["run_id"] = c.manifest["run_id"].clone();
    request["segment"] = segment.into();
    request["iteration"] = iteration.into();
    request
}
pub fn run(
    root: &Path,
    output: &Path,
    directory: &Path,
    segment: u64,
    iteration: u64,
) -> Result<i32> {
    let c = admit(root, iteration)?;
    ensure!(
        iteration == history(output)?.len() as u64 + 1,
        "The iteration would overwrite history."
    );
    let request = request_context(&c, segment, iteration);
    exclusive(
        &directory.join("request.json"),
        &encoded(&json!({"generated":generate(&request),"request":request}))?,
        false,
    )?;
    exclusive(
        &directory.join("launch.json"),
        &encoded(
            &json!({"manifest_digest":c.digest,"iteration":iteration,"executor_digest":c.approval["executor_digest"],"launch_count":1}),
        )?,
        false,
    )?;
    let executor = relative(
        &c.inputs,
        text(&c.manifest["runtime"]["assets"]["executor"]["path"])?,
    )?;
    let mut command = Command::new("timeout");
    command.args([
        "--signal=TERM",
        "--kill-after=5",
        &number(&c.manifest["resource_limits"]["timeout_seconds"])?.to_string(),
    ]);
    match text(&c.manifest["runtime"]["executor_kind"])? {
        "bash" => {
            command.arg("bash");
        }
        "native" => {}
        _ => return Err(eyre!("The executor kind is unsupported.")),
    }
    Ok(command
        .arg(executor)
        .arg(directory.join("request.json"))
        .arg(directory)
        .status()?
        .code()
        .unwrap_or(128))
}
#[derive(Default)]
struct Observations {
    accepted: Vec<Value>,
    rejected: Vec<Value>,
    receipts: Vec<Value>,
    missing: Vec<Value>,
}
fn correlate(
    c: &Context,
    records: &[Value],
    segment: u64,
    iteration: u64,
    directory: &Path,
) -> Result<Observations> {
    let mut out = Observations::default();
    let mut ids = BTreeSet::new();
    let mut events = BTreeMap::new();
    let mut sequences = BTreeMap::new();
    let required = array(&c.request["required_observations"])?;
    for r in records {
        let checked = (|| -> Result<bool> {
            ensure!(
                number(&r["schema_version"])? == 1,
                "The observation schema differs."
            );
            for key in [
                "record_id",
                "producer",
                "event_id",
                "event_kind",
                "node",
                "incarnation",
                "member_id",
            ] {
                ensure!(
                    !text(&r[key])?.is_empty(),
                    "The observation identity is incomplete."
                );
            }
            ensure!(
                ids.insert(text(&r["record_id"])?.to_string()),
                "A transport record is duplicated."
            );
            ensure!(
                r["manifest_digest"] == c.digest
                    && r["run_id"] == c.manifest["run_id"]
                    && r["scenario_id"] == c.request["scenario_id"]
                    && r["pair_id"] == c.request["pair_id"]
                    && number(&r["segment"])? == segment
                    && number(&r["iteration"])? == iteration,
                "The observation identity differs."
            );
            ensure!(
                required
                    .iter()
                    .any(|item| ["node", "incarnation", "member_id"]
                        .iter()
                        .all(|key| r[*key] == item[*key])),
                "The observation context differs."
            );
            let sequence = manifest::decimal(&r["producer_sequence"])?;
            let producer = identity(&[&r["producer"], &r["node"], &r["incarnation"]])?;
            ensure!(
                sequences.get(&producer).is_none_or(|old| sequence > *old),
                "The producer sequence is not increasing."
            );
            sequences.insert(producer, sequence);
            let clock = &r["observation_time"];
            ensure!(
                !text(&clock["clock_id"])?.is_empty() && !text(&clock["utc"])?.is_empty(),
                "The observation clock is missing."
            );
            manifest::decimal(&clock["monotonic_ns"])?;
            ensure!(
                ["observed", "missing", "error"].contains(&text(&r["presence"])?),
                "The presence state is invalid."
            );
            if r["presence"] != "observed" {
                ensure!(
                    r["payload"].is_null() && !text(&r["reason"])?.is_empty(),
                    "Missing data must remain null."
                );
            }
            let reference = &r["raw_reference"];
            artifact(directory, reference)?;
            ensure!(
                reference["capture_state"] == "complete"
                    && reference["producer"] == r["producer"]
                    && array(&reference["observation_ids"])?.contains(&r["record_id"]),
                "The raw reference is not correlated."
            );
            let event = identity(&[
                &r["manifest_digest"],
                &r["run_id"],
                &r["scenario_id"],
                &r["pair_id"],
                &r["member_id"],
                &r["node"],
                &r["incarnation"],
                &r["segment"],
                &r["iteration"],
                &r["event_id"],
            ])?;
            let payload = identity(&[
                &r["event_kind"],
                &r["payload"],
                &r["presence"],
                &r["reason"],
            ])?;
            if let Some(old) = events.get(&event) {
                ensure!(*old == payload, "Copies of an event conflict.");
                return Ok(false);
            }
            events.insert(event, payload);
            if r["event_kind"] == "fault_acknowledgment" {
                let receipt = &r["payload"];
                let fault = array(&c.request["fault_schedule"])?
                    .iter()
                    .find(|item| item["fault_id"] == receipt["fault_id"])
                    .ok_or_else(|| eyre!("The fault was not requested."))?;
                ensure!(
                    receipt["status"] == "applied"
                        && receipt["action"] == fault["action"]
                        && r["node"] == fault["node"]
                        && r["incarnation"] == fault["incarnation"]
                        && receipt["observed_state"] == fault["required_state"],
                    "The fault receipt differs."
                );
                if receipt["action"] == "restart" {
                    ensure!(
                        yes(&receipt["prior_exit"])
                            && yes(&receipt["ready"])
                            && receipt["predecessor"] != r["incarnation"]
                            && receipt["predecessor"] == fault["predecessor"],
                        "The restart lacks a verified transition."
                    );
                }
            }
            Ok(true)
        })();
        match checked {
            Ok(true) if r["event_kind"] == "fault_acknowledgment" => out.receipts.push(r.clone()),
            Ok(true) => out.accepted.push(r.clone()),
            Ok(false) => {}
            Err(_) => out
                .rejected
                .push(json!({"record_id":r["record_id"],"reason":"invalid_record"})),
        }
    }
    for item in required {
        if !out.accepted.iter().any(|r| {
            r["presence"] == "observed"
                && item
                    .as_object()
                    .is_some_and(|fields| fields.iter().all(|(k, v)| r[k] == *v))
        }) {
            out.missing.push(item.clone());
        }
    }
    Ok(out)
}
fn assess(
    c: &Context,
    directory: &Path,
    segment: u64,
    iteration: u64,
    status: i32,
    termination: &str,
) -> Result<Value> {
    let transport = record(&directory.join("transport.json"));
    let (records, transport_error) = match transport
        .and_then(|v| Ok(array(&v["observations"])?.clone()))
    {
        Ok(records) if records.len() as u64 <= number(&c.manifest["bounds"]["observations"])? => {
            (records, false)
        }
        _ => (Vec::new(), true),
    };
    let out = correlate(c, &records, segment, iteration, directory)?;
    let failures: Vec<_> = out.accepted.iter().filter(|r| r["presence"] == "observed" && r["payload"] != c.request["expected"]).map(|r| json!({"event_id":r["event_id"],"expected":c.request["expected"],"observed":r["payload"]})).collect();
    let missing_faults: Vec<_> = array(&c.request["fault_schedule"])?
        .iter()
        .filter(|fault| {
            !out.receipts
                .iter()
                .any(|r| r["payload"]["fault_id"] == fault["fault_id"])
        })
        .map(|f| f["fault_id"].clone())
        .collect();
    let verdict = if !failures.is_empty() {
        "product_failure"
    } else if transport_error
        || !out.missing.is_empty()
        || !out.rejected.is_empty()
        || !missing_faults.is_empty()
        || status != 0
        || termination != "completed"
    {
        "incomplete"
    } else {
        "passed"
    };
    Ok(
        json!({"scenario_verdict":verdict,"product_failures":failures,"observations":out.accepted,"rejected_observations":out.rejected,"fault_receipts":out.receipts,"missing_observations":out.missing,"missing_faults":missing_faults,"transport_error":transport_error}),
    )
}
pub fn finish(
    root: &Path,
    output: &Path,
    directory: &Path,
    segment: u64,
    iteration: u64,
    status: i32,
    termination: &str,
) -> Result<i32> {
    let c = admit(root, iteration)?;
    ensure!(
        iteration == history(output)?.len() as u64 + 1,
        "The iteration would overwrite history."
    );
    let previous = if iteration == 1 {
        Value::Null
    } else {
        file_hash(&output.join(format!("casper-history/{:08}.json", iteration - 1)))?.into()
    };
    let mut entry = assess(&c, directory, segment, iteration, status, termination)?;
    let mut inventory = Vec::new();
    let capture = (|| -> Result<()> {
        let mut total = 0u64;
        let mut captured = BTreeMap::new();
        for path in walk(directory)? {
            let data = regular(&path, MAX_BYTES)?;
            total += data.len() as u64;
            ensure!(
                total <= number(&c.manifest["resource_limits"]["artifact_bytes"])?,
                "The capture byte budget was exceeded."
            );
            let local = path.strip_prefix(directory)?.to_string_lossy().into_owned();
            let name = format!("casper-capture/{iteration:08}/{local}");
            exclusive(&relative(output, &name)?, &data, true)?;
            let reference = json!({"path":name,"bytes":data.len(),"sha256":hash(&data)});
            artifact(output, &reference)?;
            inventory.push(reference.clone());
            captured.insert(local, reference);
        }
        for r in array(&entry["observations"])?
            .iter()
            .chain(array(&entry["fault_receipts"])?)
        {
            let reference = &r["raw_reference"];
            let kept = captured
                .get(text(&reference["path"])?)
                .ok_or_else(|| eyre!("The raw capture is missing."))?;
            ensure!(
                kept["sha256"] == reference["sha256"] && kept["bytes"] == reference["bytes"],
                "The raw artifact changed before capture."
            );
        }
        Ok(())
    })();
    let complete = capture.is_ok()
        && entry["transport_error"] == false
        && array(&entry["rejected_observations"])?.is_empty();
    if !complete && entry["scenario_verdict"] == "passed" {
        entry["scenario_verdict"] = "incomplete".into();
    }
    let header = json!({"schema_version":1,"manifest_digest":c.digest,"scenario_id":c.request["scenario_id"],"segment":segment,"iteration":iteration,"previous_digest":previous,"termination":termination,"exit_code":status,"capture_complete":complete,"capture_error":capture.is_err(),"artifact_inventory":inventory});
    object(&header)?.iter().for_each(|(k, v)| {
        entry[k] = v.clone();
    });
    exclusive(
        &output.join(format!("casper-history/{iteration:08}.json")),
        &encoded(&entry)?,
        true,
    )?;
    Ok(if entry["scenario_verdict"] == "passed" {
        0
    } else {
        1
    })
}
pub fn verification(root: &Path, c: &Context) -> Result<(&'static str, Option<String>)> {
    let Some(reference) = c.manifest["runtime"]["assets"].get("verification") else {
        return Ok(("pending", None));
    };
    let report = parse(&artifact(&c.inputs, reference)?)?;
    ensure!(
        array(&c.approval["qualification_digests"])?.contains(&reference["sha256"]),
        "The verification receipt has no approval."
    );
    ensure!(
        report["phase"] == c.manifest["phase"]
            && report["claim_id"] == "CLAIM-CASPER-SOAK-001"
            && report["source_digests"] == c.manifest["source_digests"]
            && report["construction"] == "not-applicable"
            && report["status"] == "passed",
        "The verification scope differs."
    );
    ensure!(
        object(&report["bindings"])?.len() == 10
            && (1..=10).all(|n| yes(&report["bindings"][format!("H{n:02}")])),
        "A required binding is missing."
    );
    let origin = text(&report["origin"])?.to_string();
    if origin == "fixture-substitute" {
        ensure!(
            c.manifest["profile_id"] == "harness-lifecycle"
                && c.manifest["evidence_kind"] == "synthetic_fixture",
            "A fixture receipt cannot qualify node observations."
        );
    } else {
        ensure!(
            origin == "executed-verifier",
            "The verifier origin is unsupported."
        );
        let model_report = parse(&artifact(&c.inputs, &report["model_report"])?)?;
        let plan = record(&root.join(models::PLAN))?;
        let controls = models::controls(&plan)?;
        ensure!(
            model_report["runner_sha256"]
                == file_hash(&root.join("scripts/casper-soak/src/models.rs"))?
                && model_report["plan_sha256"] == file_hash(&root.join(models::PLAN))?
                && model_report["status"] == "passed",
            "The verifier identity differs."
        );
        let results = array(&model_report["results"])?;
        ensure!(
            results.len() == controls.len(),
            "The model search is incomplete."
        );
        for (control, result) in controls.iter().zip(results) {
            let config = text(&control["configuration"])?;
            ensure!(
                result["configuration"] == config
                    && result["exit"] == control["expected_exit"]
                    && result["model_sha256"] == file_hash(&root.join(text(&plan["model"])?))?
                    && result["configuration_sha256"]
                        == file_hash(&root.join("formal/tlaplus/casper_soak").join(config))?,
                "The model control differs."
            );
            let log = artifact(&c.inputs, &report["model_logs"][config])?;
            ensure!(
                result["log_sha256"] == hash(&log)
                    && models::classify(
                        number(&result["exit"])? as i32,
                        &String::from_utf8(log)?,
                        control["property"].as_str()
                    ),
                "The control verdict is not exact."
            );
        }
    }
    Ok(("passed", Some(origin)))
}
pub fn publish(root: &Path, output: &Path, termination: &str) -> Result<Value> {
    let c = admit(root, 1)?;
    ensure!(
        record(&output.join(".casper-manifest.json"))? == c.manifest,
        "The publication manifest differs."
    );
    let entries = history(output)?;
    for entry in &entries {
        ensure!(
            entry["manifest_digest"] == c.digest,
            "The history belongs to another manifest."
        );
        if !yes(&entry["capture_complete"]) {
            ensure!(
                entry["scenario_verdict"] != "passed",
                "An incomplete capture claims success."
            );
            continue;
        }
        let iteration = number(&entry["iteration"])?;
        let current = admit(root, iteration)?;
        let directory = output.join(format!("casper-capture/{iteration:08}"));
        let request = request_context(&current, number(&entry["segment"])?, iteration);
        ensure!(
            record(&directory.join("request.json"))?
                == json!({"generated":generate(&request),"request":request}),
            "The retained generation differs."
        );
        let replay = assess(
            &current,
            &directory,
            number(&entry["segment"])?,
            iteration,
            number(&entry["exit_code"])? as i32,
            text(&entry["termination"])?,
        )?;
        for key in [
            "scenario_verdict",
            "product_failures",
            "observations",
            "fault_receipts",
        ] {
            ensure!(
                entry[key] == replay[key],
                "A cached verdict differs from raw evidence."
            );
        }
    }
    let (verified, origin) = verification(root, &c)?;
    let mut missing = Vec::new();
    for scenario in array(&c.manifest["required_scenarios"])? {
        if !entries
            .iter()
            .any(|e| e["scenario_id"] == *scenario && e["scenario_verdict"] == "passed")
        {
            missing.push(scenario.clone());
        }
    }
    let complete = entries.len() as u64 == number(&c.manifest["runtime"]["iterations"])?
        && missing.is_empty()
        && entries
            .iter()
            .all(|e| e["scenario_verdict"] == "passed" && yes(&e["capture_complete"]));
    let failures: Vec<_> = entries
        .iter()
        .flat_map(|e| {
            e["product_failures"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned()
        })
        .collect();
    let inventory: Vec<_> = entries
        .iter()
        .flat_map(|e| {
            e["artifact_inventory"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned()
        })
        .collect();
    Ok(
        json!({"schema_version":1,"manifest_digest":c.digest,"evidence_kind":c.manifest["evidence_kind"],"harness_verification":verified,"verification_origin":origin,"soak_verdict":if complete && verified == "passed" && termination == "completed" {"passed"} else {"non_passing"},"termination":termination,"requested_cases":c.manifest["runtime"]["iterations"],"completed_cases":entries.len(),"scenario_results":entries,"product_failures":failures,"missing_scenarios":missing,"coverage_complete":complete,"artifact_inventory":inventory}),
    )
}
