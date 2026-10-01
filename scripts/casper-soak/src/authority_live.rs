use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use casper_soak::{
    array, artifact, encoded, exclusive, file_hash, hash, manifest, number, object, parse, record,
    regular, relative, text, MAX_BYTES,
};
use eyre::{ensure, Result};
use serde_json::{json, Value};

#[path = "authority_observer.rs"]
mod observer;
use observer::mapping;

#[path = "authority_process.rs"]
#[allow(dead_code)]
mod process;

#[path = "authority_incarnation.rs"]
mod incarnation;

fn missing(reason: &str) -> Value { json!({"presence":"missing","value":null,"reason":reason}) }

fn observed(value: Value) -> Value { json!({"presence":"observed","value":value,"reason":null}) }

fn utc() -> Result<String> {
    let seconds: libc::time_t = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs()
        .try_into()?;
    let mut parts = std::mem::MaybeUninit::<libc::tm>::uninit();
    ensure!(
        !unsafe { libc::gmtime_r(&seconds, parts.as_mut_ptr()) }.is_null(),
        "The UTC clock is unavailable."
    );
    let parts = unsafe { parts.assume_init() };
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        parts.tm_year + 1900,
        parts.tm_mon + 1,
        parts.tm_mday,
        parts.tm_hour,
        parts.tm_min,
        parts.tm_sec
    ))
}

pub fn clock() -> Result<(String, u64)> {
    ensure!(
        cfg!(target_os = "linux"),
        "The live executor requires Linux."
    );
    let boot = regular(Path::new("/proc/sys/kernel/random/boot_id"), 128)?;
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    ensure!(
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) } == 0,
        "The monotonic clock is unavailable."
    );
    Ok((
        format!("linux-monotonic:{}", std::str::from_utf8(&boot)?.trim()),
        (now.tv_sec as u64)
            .checked_mul(1_000_000_000)
            .and_then(|n| n.checked_add(now.tv_nsec as u64))
            .ok_or_else(|| eyre::eyre!("The monotonic clock overflowed."))?,
    ))
}

fn save(root: &Path, name: &str, bytes: &[u8]) -> Result<Value> {
    exclusive(&relative(root, name)?, bytes, true)?;
    Ok(json!({"path":name,"bytes":bytes.len(),"sha256":hash(bytes),
        "producer":"authority-live","capture_state":"captured","observation_ids":[]}))
}

fn inventory(root: &Path, request: &str, receipts: &[Value]) -> Result<()> {
    let bytes = encoded(&json!({"schema_version":1,"request_sha256":request,"receipts":receipts}))?;
    let path = relative(root, "execution.json")?;
    let mut temporary = tempfile::NamedTempFile::new_in(root)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    File::open(root)?.sync_all()?;
    Ok(())
}

fn sequence(kind: &str) -> Result<Vec<&'static str>> {
    Ok(match kind {
        "replay" => vec!["load_fixture", "evaluate", "replay_fixture", "evaluate"],
        "restart" => vec![
            "load_fixture",
            "evaluate",
            "await_restart_receipt",
            "evaluate",
        ],
        "missing_dependencies" => vec!["load_fixture_with_missing_dependencies", "evaluate"],
        "duplicate_justifications" | "signature_rejection" => {
            vec!["load_justification_fixture", "evaluate"]
        }
        "threshold_boundary"
        | "strict_majority"
        | "committee_provenance"
        | "traversal_comparison" => vec!["load_fixture", "evaluate"],
        _ => eyre::bail!("The scenario kind is unsupported."),
    })
}

fn validate(envelope: &Value, output: &Path) -> Result<Vec<Value>> {
    ensure!(
        envelope["schema_version"] == 1,
        "The execution schema is unsupported."
    );
    manifest::hex(&envelope["execution_nonce"], 64)?;
    let request = &envelope["request"];
    let manifest = &envelope["manifest"];
    ensure!(
        request["evidence_kind"] == "node_observation"
            && request["phase"] == "pre_pr216_merge"
            && request["policy_variant"] == "baseline",
        "The live execution scope differs."
    );
    for field in [
        "manifest_digest",
        "run_id",
        "phase",
        "evidence_kind",
        "policy_variant",
    ] {
        ensure!(
            request[field] == manifest[field] && !request[field].is_null(),
            "The manifest identity differs: {field}."
        );
    }
    ensure!(
        Path::new(text(&envelope["output_root"])?) == output && output.is_absolute(),
        "The execution output differs."
    );
    let root = Path::new(text(&envelope["input_root"])?);
    ensure!(
        root.is_absolute() && !root.is_symlink(),
        "The input root is invalid."
    );
    for reference in object(&request["inputs"])?.values() {
        artifact(root, reference)?;
    }
    let mut members = array(&request["members"])?.clone();
    ensure!(members.len() == 2, "The paired member inventory differs.");
    members.sort_by_key(|m| m["member_id"].as_str().unwrap_or_default().to_owned());
    let mut ids = BTreeSet::new();
    let mut modes = BTreeSet::new();
    let mut operations = Vec::new();
    for member in &members {
        ensure!(
            ids.insert(text(&member["member_id"])?),
            "A member identity repeats."
        );
        modes.insert(text(&member["evaluation_mode"])?);
        for field in ["candidate_id", "node_revision", "node_binary_digest"] {
            ensure!(
                member[field] == manifest[field] && !member[field].is_null(),
                "The candidate identity differs."
            );
        }
        for role in ["dag", "electorate", "justification"] {
            ensure!(
                member[format!("{role}_digest")] == request["inputs"][role]["sha256"],
                "The member input differs."
            );
        }
        for step in sequence(text(&request["scenario_kind"])?)? {
            operations.push(json!({"index":operations.len(),"operation":step,"member":member}));
        }
    }
    ensure!(
        modes == BTreeSet::from(["bounded", "reference"]),
        "The paired evaluation modes differ."
    );
    ensure!(
        envelope["operations"] == json!(operations),
        "The generated operation inventory differs."
    );
    let faults = array(&request["fault_schedule"])?;
    for member in &members {
        incarnation::validate(member, faults)?;
    }
    ensure!(faults.len() <= 1, "Only one process fault is supported.");
    for fault in faults {
        let member = members
            .iter()
            .find(|member| member["member_id"] == fault["member_id"])
            .ok_or_else(|| eyre::eyre!("The fault member is unknown."))?;
        ensure!(
            fault["node_id"] == member["node_id"] && array(&fault["after"])?.is_empty(),
            "The fault node or ordering differs."
        );
        let action = text(&fault["action"])?;
        ensure!(
            ["pause", "restart"].contains(&action),
            "The process fault is unsupported."
        );
        ensure!(
            fault["ack_deadline"]["clock_id"] == request["observation_deadline"]["clock_id"]
                && manifest::decimal(&fault["ack_deadline"]["monotonic_ns"])?
                    <= manifest::decimal(&request["observation_deadline"]["monotonic_ns"])?,
            "The fault deadline differs."
        );
        ensure!(
            if action == "restart" {
                member["predecessor_incarnation"] == fault["incarnation"]
                    && member["incarnation"] != fault["incarnation"]
                    && fault["trigger_event"] == "await_restart_receipt"
            } else {
                member["incarnation"] == fault["incarnation"]
            },
            "The fault incarnation or trigger differs."
        );
        ensure!(
            sequence(text(&request["scenario_kind"])?)?
                .iter()
                .any(|step| fault["trigger_event"] == *step),
            "The fault trigger is unsupported."
        );
        let owner = &manifest["runtime"]["authority_live"]["members"][text(&member["member_id"])?]
            ["process_owner"];
        ensure!(
            owner.is_object() && Path::new(text(&owner["socket"])?).is_absolute(),
            "The owned process provider is required."
        );
    }
    ensure!(
        request["scenario_kind"] != "restart"
            || (faults.len() == 1 && faults[0]["action"] == "restart"),
        "The restart schedule is required."
    );
    let (clock_id, now) = clock()?;
    ensure!(
        request["observation_deadline"]["clock_id"] == clock_id
            && manifest::decimal(&request["observation_deadline"]["monotonic_ns"])? > now,
        "The observation deadline is expired or uses another clock."
    );
    ensure!(
        (1..=300_000).contains(&number(&envelope["timeout_ms"])?),
        "The execution timeout is invalid."
    );
    Ok(operations)
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

fn driver(spec: &Value, request: &Value, output: &Path, deadline: Instant) -> Result<Value> {
    if spec.is_null() {
        return Ok(json!({"status":"unknown","reason":"workload_driver_unavailable"}));
    }
    let path = Path::new(text(&spec["path"])?);
    ensure!(path.is_absolute(), "The driver path must be absolute.");
    let bytes = regular(path, 128 * MAX_BYTES)?;
    ensure!(
        hash(&bytes) == spec["sha256"] && number(&spec["bytes"])? == bytes.len() as u64,
        "The workload driver identity differs."
    );
    let args = array(&spec["arguments"])?;
    ensure!(args.len() <= 32, "The driver argument limit is exceeded.");
    let args = args.iter().map(text).collect::<Result<Vec<_>>>()?;
    ensure!(
        args.iter().all(|s| s.len() <= 4096 && !s.contains('\0')),
        "A driver argument is invalid."
    );
    fs::create_dir(output)?;
    let executable = output.join("driver");
    exclusive(&executable, &bytes, true)?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o500))?;
    let request_bytes = encoded(request)?;
    exclusive(&output.join("request.json"), &request_bytes, true)?;
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("stdout.txt"))?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("stderr.txt"))?;
    ensure!(
        Instant::now() < deadline,
        "The driver deadline expired before launch."
    );
    let mut child = Child(
        Command::new(&executable)
            .args(args)
            .env("CASPER_AUTHORITY_STEP_REQUEST", output.join("request.json"))
            .env("CASPER_AUTHORITY_STEP_OUTPUT", output)
            .current_dir(output)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .process_group(0)
            .spawn()?,
    );
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        ensure!(
            Instant::now() < deadline,
            "The workload driver deadline expired."
        );
        ensure!(
            ["stdout.txt", "stderr.txt"]
                .iter()
                .all(|p| fs::metadata(output.join(p)).is_ok_and(|m| m.len() <= MAX_BYTES)),
            "The driver output limit is exceeded."
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    drop(child);
    let launch = json!({"exit_code":status.code(),"request_sha256":hash(&request_bytes),"driver_sha256":hash(&bytes)});
    exclusive(&output.join("launch.json"), &encoded(&launch)?, true)?;
    ensure!(status.success(), "The workload driver failed.");
    ensure!(
        file_hash(&executable)? == hash(&bytes),
        "The retained workload driver changed."
    );
    for p in ["stdout.txt", "stderr.txt"] {
        regular(&output.join(p), MAX_BYTES)?;
    }
    let result = record(&output.join("result.json"))?;
    ensure!(
        result["schema_version"] == 1
            && result["request_sha256"] == hash(&request_bytes)
            && result["step"] == request["step"],
        "The driver response belongs to another step."
    );
    ensure!(
        ["applied", "not_applied", "unknown"].contains(&text(&result["status"])?),
        "The driver status is unsupported."
    );
    Ok(result)
}

fn finality(target: &Value, threshold: &Value, mode: &Value) -> Result<Value> {
    let oracle = &target["oracle"];
    if oracle["profile_threshold_compatible"] != true
        || !oracle["recomputed"].is_boolean()
        || target["persisted_finalized"]["presence"] != "observed"
    {
        return Ok(missing("finality_observation_unavailable"));
    }
    for (field, key) in [("numerator", "n"), ("denominator", "d")] {
        ensure!(
            oracle["threshold"][field] == threshold[key],
            "The observed threshold differs."
        );
    }
    let witness = &oracle["witness"]["value"];
    for (field, key) in [
        ("total_stake", "S"),
        ("agreeing_stake", "agreeing_stake"),
        ("clique_weight", "q"),
    ] {
        if let Some(actual) = witness[field].as_i64() {
            ensure!(
                actual >= 0 && actual as u64 == manifest::decimal(&threshold[key])?,
                "The captured stake differs from the fixture."
            );
        }
    }
    let fraction = |value: &Value| {
        if value["numeric"]["fraction"].is_object() {
            value["numeric"]["fraction"].clone()
        } else {
            missing("numeric_observation_unavailable")
        }
    };
    let mut decision = oracle["recomputed"].clone();
    let mut original = fraction(&target["original_ft"]);
    let mut projection = fraction(&target["display_projection"]);
    if mode == "reference" {
        let comparison = &target["reference_comparison"];
        if comparison["availability"] != "available" {
            return Ok(missing("reference_finality_unavailable"));
        }
        let value = &comparison["value"];
        ensure!(
            comparison["input_digest"] == target["evaluation_input_digest"]
                && value["algorithm"] == "immutable-subset-reference-v1"
                && value["reference"]["decision"].is_boolean()
                && value["measured_decision"] == oracle["recomputed"]
                && value["decision_matches"]
                    == (value["reference"]["decision"] == oracle["recomputed"]),
            "The reference comparison differs from the measured result."
        );
        decision = value["reference"]["decision"].clone();
        original = mapping::binary32(number(&value["reference"]["original_bits"])?.try_into()?)
            ["fraction"]
            .clone();
        projection = missing("reference_projection_unavailable");
    }
    Ok(observed(
        json!({"decision":if target["persisted_finalized"]["value"] == true {"finalized"} else {"not_finalized"},
        "hold_reason":null,"threshold_pass":decision,"original_ft":original,
        "projection":projection}),
    ))
}

fn snapshot(
    envelope: &Value,
    operation: &Value,
    mapping: &Value,
    applied: bool,
    completed: &[Value],
    sequence: usize,
) -> Result<Value> {
    let request = &envelope["request"];
    let member = &operation["member"];
    let mut result = member.clone();
    for key in [
        "manifest_digest",
        "run_id",
        "scenario_id",
        "pair_id",
        "phase",
        "evidence_kind",
        "seed",
        "segment",
        "iteration",
        "protocol_context",
        "metadata_availability",
        "threshold_inputs",
    ] {
        result[key] = request[key].clone();
    }
    let (clock_id, now) = clock()?;
    let target = array(&mapping["targets"])?.first();
    let finality = if let Some(target) = target {
        finality(
            target,
            &request["threshold_inputs"],
            &member["evaluation_mode"],
        )?
    } else {
        missing("authority_target_unavailable")
    };
    result["schema_version"] = 1.into();
    result["incarnation"] = mapping["identity"]["incarnation"].clone();
    result["record_id"] = format!("live-{sequence}").into();
    result["event_id"] = format!("live-{sequence}").into();
    result["event_kind"] = "authority_snapshot".into();
    result["producer"] = "authority-live".into();
    result["producer_sequence"] = sequence.to_string().into();
    result["time"] = json!({"clock_id":clock_id,"monotonic_ns":now.to_string(),
        "utc":utc()?});
    result["presence"] = "observed".into();
    result["reason"] = Value::Null;
    let head = &mapping["fork_choice"][format!("{}_head", text(&member["evaluation_mode"])?)];
    let head = if head.is_null() {
        missing("paired_fork_choice_unavailable")
    } else {
        head.clone()
    };
    result["payload"] = json!({"head":head,"finality":finality,
        "work":missing("exact_traversal_measurements_unavailable"),
        "evaluation_receipt":observed(json!({"status":if applied {"applied"} else {"unknown"},
            "steps":completed,"fixture_digest":request["inputs"]["fixture"]["sha256"]})),
        "node_mapping":mapping});
    Ok(result)
}

fn capture(
    binding: &Value,
    authority: &Value,
    output: &Path,
    nonce: &str,
    deadline: Instant,
    incarnation: &Value,
    owner: &Value,
) -> Result<Value> {
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .as_millis() as u64;
    ensure!(remaining >= 50, "The capture deadline expired.");
    let mut binding = binding.clone();
    binding["timeout_ms"] = number(&binding["timeout_ms"])?.min(remaining).into();
    let id = hash(nonce.as_bytes());
    binding["request_id"] = format!(
        "{}-{}-{}-{}-{}",
        &id[..8],
        &id[8..12],
        &id[12..16],
        &id[16..20],
        &id[20..32]
    )
    .into();
    let report = if owner.is_null() {
        observer::collect(&encoded(&binding)?, &encoded(authority)?, output)?
    } else {
        let (clock_id, now) = clock()?;
        let request = json!({"schema_version":1,"action":"capture","binding":binding,"authority":authority,
            "clock_id":clock_id,"deadline_monotonic_ns":now.saturating_add(deadline.saturating_duration_since(Instant::now()).as_nanos() as u64)});
        let response = process::call(
            owner,
            &request,
            deadline.saturating_duration_since(Instant::now()),
        )?;
        let root = Path::new(text(&response["capture_root"])?);
        let parent = Path::new(text(&owner["socket"])?)
            .parent()
            .ok_or_else(|| eyre::eyre!("The owner directory is absent."))?;
        ensure!(
            root.is_absolute() && root.parent() == Some(parent) && !root.is_symlink(),
            "The owner capture path differs."
        );
        let bytes = regular(&root.join("report.json"), MAX_BYTES)?;
        ensure!(
            hash(&bytes) == response["report_sha256"],
            "The owner capture report differs."
        );
        let report = parse(&bytes)?;
        ensure!(
            array(&report["artifacts"])?.len() <= 6,
            "The capture artifact inventory exceeds its bound."
        );
        fs::create_dir(output)?;
        for reference in array(&report["artifacts"])? {
            let name = text(&reference["path"])?;
            ensure!(
                [
                    "binding.json",
                    "authority.json",
                    "hello.json",
                    "request.json",
                    "response.json",
                    "mapping.json"
                ]
                .contains(&name),
                "The capture artifact name differs."
            );
            exclusive(&output.join(name), &artifact(root, reference)?, true)?;
        }
        ensure!(
            record(&output.join("binding.json"))? == binding
                && record(&output.join("authority.json"))? == *authority,
            "The owner captured another request."
        );
        exclusive(&output.join("report.json"), &bytes, true)?;
        report
    };
    ensure!(
        report["status"] == "captured" && report["mapping_status"] == "mapped",
        "The authority capture failed."
    );
    ensure!(
        Instant::now() <= deadline,
        "The authority capture exceeded the deadline."
    );
    let mapping = mapping::map(&record(&output.join("response.json"))?)?;
    ensure!(
        mapping == record(&output.join("mapping.json"))?,
        "The retained mapping differs."
    );
    ensure!(
        incarnation.is_null() || &mapping["identity"]["incarnation"] == incarnation,
        "The captured incarnation differs."
    );
    Ok(mapping)
}

pub fn run(bytes: &[u8], output: &Path) -> Result<Value> {
    let envelope = parse(bytes)?;
    let operations = validate(&envelope, output)?;
    if !output.exists() {
        fs::create_dir(output)?;
    }
    ensure!(
        !output.is_symlink() && fs::read_dir(output)?.next().is_none(),
        "The execution output must be empty."
    );
    fs::set_permissions(output, fs::Permissions::from_mode(0o700))?;
    let digest = hash(bytes);
    save(output, "live-request.json", bytes)?;
    let started = Instant::now();
    let timeout = Duration::from_millis(number(&envelope["timeout_ms"])?);
    let (_, now) = clock()?;
    let remaining =
        manifest::decimal(&envelope["request"]["observation_deadline"]["monotonic_ns"])?
            .checked_sub(now)
            .ok_or_else(|| eyre::eyre!("The observation deadline expired."))?;
    let deadline = started + timeout.min(Duration::from_nanos(remaining));
    let config = &envelope["manifest"]["runtime"]["authority_live"];
    let root = PathBuf::from(text(&envelope["input_root"])?);
    let mut receipts = Vec::new();
    let mut previous = Value::Null;
    let mut completed: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut member_complete: BTreeMap<String, bool> = BTreeMap::new();
    let mut errors = Vec::new();
    let mut captures = Vec::new();
    let mut all_applied = true;
    let mut active_bindings: BTreeMap<String, Value> = BTreeMap::new();
    let mut active_incarnations: BTreeMap<String, Value> = BTreeMap::new();
    let mut applied_faults = BTreeSet::new();
    inventory(output, &digest, &receipts)?;
    for (i, step) in operations.iter().enumerate() {
        let attempt = (|| -> Result<(Value, Vec<Value>, Value)> {
            ensure!(Instant::now() < deadline, "The execution deadline expired.");
            let member = &step["member"];
            let name = text(&member["member_id"])?;
            let spec = &config["members"][name];
            let mut binding = active_bindings
                .get(name)
                .unwrap_or(&spec["binding"])
                .clone();
            let scheduled = array(&envelope["request"]["fault_schedule"])?
                .iter()
                .find(|f| f["member_id"] == name);
            let before_incarnation = if let Some(fault) = scheduled.filter(|f| {
                f["action"] == "restart"
                    && !applied_faults.contains(f["fault_id"].as_str().unwrap_or_default())
            }) {
                fault["incarnation"].clone()
            } else {
                active_incarnations
                    .get(name)
                    .unwrap_or(&member["incarnation"])
                    .clone()
            };
            let authority = &spec["authority"];
            ensure!(
                array(&authority["targets"])?.len() == 1
                    && authority["targets"][0] == spec["target"],
                "Exactly one pinned authority target is required."
            );
            ensure!(
                authority["original"] == true
                    && authority["reference"] == true
                    && authority["strict"] == false,
                "The authority request requires both evaluators and an inclusive threshold."
            );
            let b = observer::Binding::from_value(&binding)?;
            ensure!(
                b.source_revision == text(&member["node_revision"])?
                    && b.executable_sha256 == text(&member["node_binary_digest"])?
                    && binding["configuration_sha256"] == spec["configuration_sha256"],
                "The observer candidate differs."
            );
            let step_root = output.join(format!("step-{i:02}"));
            fs::create_dir(&step_root)?;
            if !config["driver"].is_null() {
                ensure!(
                    file_hash(Path::new(text(&config["driver"]["path"])?))?
                        == config["driver"]["sha256"],
                    "The workload driver identity differs."
                );
            }
            let before = step_root.join("before");
            capture(
                &binding,
                authority,
                &before,
                &format!("{digest}:{i}:before"),
                deadline,
                &before_incarnation,
                &spec["process_owner"],
            )?;
            let before_ref = json!({"step":i,"path":format!("step-{i:02}/before/report.json"),"sha256":file_hash(&before.join("report.json"))?});
            captures.push(before_ref.clone());
            let mut process_receipt = None;
            if let Some(fault) = scheduled.filter(|f| {
                f["trigger_event"] == step["operation"]
                    && !applied_faults.contains(f["fault_id"].as_str().unwrap_or_default())
            }) {
                let fault_id = text(&fault["fault_id"])?;
                let (clock_id, now) = clock()?;
                let maximum = now.saturating_add(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_nanos() as u64,
                );
                let fault_request = json!({"schema_version":1,"execution_request_sha256":digest,"step":step,"fault":fault,
                    "clock_id":clock_id,"deadline_monotonic_ns":manifest::decimal(&fault["ack_deadline"]["monotonic_ns"])?.min(maximum),
                    "action":fault["action"],"hold_ms":spec["pause_hold_ms"].as_u64().unwrap_or(10),
                    "process":{"pid":b.node_pid,"process_start_ticks":b.process_start_ticks,"executable_sha256":b.executable_sha256}});
                save(
                    output,
                    &format!("step-{i:02}/process-request.json"),
                    &encoded(&fault_request)?,
                )?;
                let response = process::call(
                    &spec["process_owner"],
                    &fault_request,
                    deadline.saturating_duration_since(Instant::now()),
                )?;
                ensure!(
                    response["before"]["pid"] == b.node_pid
                        && response["before"]["process_start_ticks"] == b.process_start_ticks
                        && response["after"]["executable_sha256"] == b.executable_sha256,
                    "The process receipt candidate differs."
                );
                save(
                    output,
                    &format!("step-{i:02}/process-response.json"),
                    &encoded(&response)?,
                )?;
                if fault["action"] == "restart" {
                    ensure!(
                        response["prior_exit"] == true
                            && response["after"]["pid"] != response["before"]["pid"],
                        "The predecessor exit is unavailable."
                    );
                    binding["node_pid"] = response["after"]["pid"].clone();
                    binding["process_start_ticks"] =
                        response["after"]["process_start_ticks"].clone();
                    active_bindings.insert(name.to_owned(), binding.clone());
                } else {
                    ensure!(
                        response["observed_state"] == "stopped" && response["resumed"] == true,
                        "The pause and resume evidence is incomplete."
                    );
                }
                applied_faults.insert(fault_id.to_owned());
                process_receipt = Some((fault.clone(), response));
            }
            let step_request = json!({"schema_version":1,"execution_request_sha256":digest,"execution_nonce":envelope["execution_nonce"],
                "step":step,"inputs":envelope["request"]["inputs"],"input_root":root,"execution_output_root":output,
                "fault_schedule":envelope["request"]["fault_schedule"],"observer_binding":binding,"before_capture":before_ref,
                "deadline_monotonic_ns":envelope["request"]["observation_deadline"]["monotonic_ns"]});
            let applied = driver(
                &config["driver"],
                &step_request,
                &step_root.join("application"),
                deadline,
            )?;
            let is_applied = applied["status"] == "applied";
            let mut exports = json!({});
            if is_applied {
                ensure!(
                    object(&applied["observed_inputs"])?.len() == 3,
                    "The exported input inventory differs."
                );
                for role in ["dag", "electorate", "justification"] {
                    let data = artifact(
                        &step_root.join("application"),
                        &applied["observed_inputs"][role],
                    )?;
                    ensure!(
                        hash(&data) == envelope["request"]["inputs"][role]["sha256"],
                        "The applied node inputs differ."
                    );
                    exports[role] = save(output, &format!("step-{i:02}/{role}.json"), &data)?;
                }
                manifest::hex(&applied["snapshot_digest"], 64)?;
            }
            let restarted = process_receipt
                .as_ref()
                .is_some_and(|(fault, _)| fault["action"] == "restart");
            let after_incarnation = if restarted && incarnation::deferred(member) {
                Value::Null
            } else if restarted {
                member["incarnation"].clone()
            } else {
                before_incarnation
            };
            let mut capture_attempt = 0usize;
            let (mapping, capture_path) = loop {
                let label = if capture_attempt == 0 {
                    "capture".to_owned()
                } else {
                    format!("capture-{capture_attempt:03}")
                };
                let path = step_root.join(&label);
                match capture(
                    &binding,
                    authority,
                    &path,
                    &format!("{digest}:{i}:after:{capture_attempt}"),
                    deadline,
                    &after_incarnation,
                    &spec["process_owner"],
                ) {
                    Ok(mapping) => break (mapping, path),
                    Err(error)
                        if !restarted
                            || capture_attempt >= 100
                            || Instant::now() + Duration::from_millis(50) >= deadline =>
                    {
                        return Err(error)
                    }
                    Err(_) => {
                        capture_attempt += 1;
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            };
            if restarted {
                ensure!(
                    mapping["identity"]["incarnation"] != member["predecessor_incarnation"],
                    "The replacement retained the predecessor incarnation."
                );
                active_incarnations
                    .insert(name.to_owned(), mapping["identity"]["incarnation"].clone());
            }
            captures.push(json!({"step":i,"path":capture_path.strip_prefix(output)?.join("report.json"),"sha256":file_hash(&capture_path.join("report.json"))?}));
            if is_applied {
                ensure!(
                    mapping["snapshot_digest"] == applied["snapshot_digest"],
                    "The captured snapshot differs from the applied input export."
                );
            }
            for reference in object(&envelope["request"]["inputs"])?.values() {
                artifact(&root, reference)?;
            }
            let all_applied = member_complete.entry(name.to_owned()).or_insert(true);
            *all_applied &= is_applied;
            let history = completed.entry(name.to_owned()).or_default();
            history.push(step["operation"].clone());
            let mut observations = Vec::new();
            if let Some((fault, response)) = process_receipt {
                let mut value = snapshot(&envelope, step, &mapping, false, history, i * 2)?;
                value["record_id"] = format!("live-fault-{i}").into();
                value["event_kind"] = "fault_ack".into();
                value["time"]["monotonic_ns"] =
                    number(&response["monotonic_ns"])?.to_string().into();
                let mut payload = response;
                for field in ["fault_id", "action", "incarnation", "trigger_event"] {
                    payload[field] = fault[field].clone();
                }
                if fault["action"] == "restart" {
                    let (_, ready_at) = clock()?;
                    ensure!(
                        ready_at <= manifest::decimal(&fault["ack_deadline"]["monotonic_ns"])?,
                        "The restart readiness receipt is late."
                    );
                    value["time"]["monotonic_ns"] = ready_at.to_string().into();
                    payload["ready"] = true.into();
                    payload["new_incarnation"] = mapping["identity"]["incarnation"].clone();
                }
                value["payload"] = payload;
                if incarnation::deferred(member) {
                    incarnation::enroll(
                        member,
                        array(&envelope["request"]["fault_schedule"])?,
                        &value,
                    )?;
                }
                let mut reference = save(
                    output,
                    &format!("step-{i:02}/fault.json"),
                    &encoded(&value)?,
                )?;
                reference["observation_ids"] = json!([value["record_id"]]);
                observations.push(reference);
            }
            if step["operation"] == "evaluate"
                && &mapping["identity"]["incarnation"]
                    == active_incarnations
                        .get(name)
                        .unwrap_or(&member["incarnation"])
            {
                let value = snapshot(&envelope, step, &mapping, *all_applied, history, i * 2 + 1)?;
                let mut reference = save(
                    output,
                    &format!("step-{i:02}/observation.json"),
                    &encoded(&value)?,
                )?;
                reference["observation_ids"] = json!([value["record_id"]]);
                observations.push(reference);
            }
            Ok((applied["status"].clone(), observations, exports))
        })();
        let (status, observations, exports) = match attempt {
            Ok(value) => value,
            Err(error) => {
                errors.push(json!({"step":i,"error":error.to_string()}));
                (json!("unknown"), Vec::new(), json!({}))
            }
        };
        all_applied &= status == "applied";
        if Instant::now() > deadline {
            if errors.is_empty() {
                errors.push(json!({"step":i,"error":"The execution deadline expired."}));
            }
            break;
        }
        let receipt = json!({"schema_version":1,"request_sha256":digest,"previous_receipt_sha256":previous,
            "step":step,"elapsed_ns":started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            "status":status,"observed_inputs":exports,"observations":observations});
        let reference = save(
            output,
            &format!("receipts/{i:02}.json"),
            &encoded(&receipt)?,
        )?;
        previous = reference["sha256"].clone();
        receipts.push(reference);
        inventory(output, &digest, &receipts)?;
        if !errors.is_empty() {
            break;
        }
    }
    let report = json!({"schema_version":1,"status":if errors.is_empty() && all_applied {"captured"} else {"incomplete"},
        "scope":"live-executor-qualification","request_sha256":digest,"receipt_count":receipts.len(),
        "captures":captures,"errors":errors,"qualification":"pending","profile_verdict":"blocked","soak_verdict":"non_passing",
        "node_launch_count":null,"blocked_reasons":["live_adapter_unqualified","paired_fork_choice_qualification_pending","exact_traversal_measurements_unavailable"],
        "source_digests":{
            "scripts/casper-soak/src/authority_live.rs":hash(include_bytes!("authority_live.rs")),
            "scripts/casper-soak/src/authority_process.rs":hash(include_bytes!("authority_process.rs")),
            "scripts/casper-soak/src/authority_incarnation.rs":hash(include_bytes!("authority_incarnation.rs")),
            "scripts/casper-soak/src/bin/casper-authority-live.rs":hash(include_bytes!("bin/casper-authority-live.rs")),
            "scripts/casper-soak/src/authority_observer.rs":hash(include_bytes!("authority_observer.rs")),
            "scripts/casper-soak/src/authority_mapping.rs":hash(include_bytes!("authority_mapping.rs"))}});
    save(output, "live-report.json", &encoded(&report)?)?;
    Ok(report)
}
