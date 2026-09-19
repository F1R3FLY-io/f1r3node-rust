use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use casper_soak::{array, artifact, file_hash, hash, manifest, object, parse, text};
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

pub const PROFILE: &str = "authority-finality";
pub const CAPABILITIES: &[&str] = &[
    "same-dag-evaluation",
    "electorate-context",
    "finality-decision",
    "original-ft-projection",
];
pub const KINDS: &[&str] = &[
    "threshold_boundary",
    "strict_majority",
    "committee_provenance",
    "duplicate_justifications",
    "signature_rejection",
    "replay",
    "restart",
    "missing_dependencies",
    "traversal_comparison",
];

pub fn identity() -> Value {
    let sources = [
        (
            "scripts/casper-soak/src/profiles/authority_finality.rs",
            include_bytes!("authority_finality.rs").as_slice(),
        ),
        (
            "scripts/casper-soak/src/bin/casper-authority-finality.rs",
            include_bytes!("../bin/casper-authority-finality.rs").as_slice(),
        ),
        (
            "scripts/casper-soak/src/lib.rs",
            include_bytes!("../lib.rs").as_slice(),
        ),
        (
            "scripts/casper-soak/src/manifest.rs",
            include_bytes!("../manifest.rs").as_slice(),
        ),
        (
            "scripts/casper-soak/src/models.rs",
            include_bytes!("../models.rs").as_slice(),
        ),
    ];
    let digests: BTreeMap<_, _> = sources.into_iter().map(|(p, b)| (p, hash(b))).collect();
    json!({"profile_id":PROFILE,"profile_digest":digests["scripts/casper-soak/src/profiles/authority_finality.rs"],"source_digests":digests,"version":env!("CARGO_PKG_VERSION")})
}

fn id(v: &Value) -> Result<&str> {
    let s = text(v)?;
    ensure!(
        !s.trim().is_empty() && s.len() <= 256,
        "An identity is empty or too long."
    );
    Ok(s)
}
fn equal(a: &Value, b: &Value, fields: &[&str]) -> Result<()> {
    for field in fields {
        ensure!(
            a.get(*field).is_some() && a[*field] == b[*field],
            "A paired identity differs: {field}."
        );
    }
    Ok(())
}
pub fn threshold(v: &Value) -> Result<bool> {
    let q = manifest::decimal(&v["q"])? as u128;
    let total = manifest::decimal(&v["S"])? as u128;
    let agreeing = manifest::decimal(&v["agreeing_stake"])? as u128;
    let n = manifest::decimal(&v["n"])? as u128;
    let d = manifest::decimal(&v["d"])? as u128;
    ensure!(
        total > 0 && q <= agreeing && agreeing <= total && d > 0 && n <= d,
        "Threshold inputs are outside the supported domain."
    );
    if agreeing * 2 <= total {
        return Ok(false);
    }
    let lhs = q
        .checked_mul(2)
        .and_then(|x| x.checked_mul(d))
        .ok_or_else(|| eyre!("Threshold arithmetic overflow."))?;
    let rhs = total
        .checked_mul(d + n)
        .ok_or_else(|| eyre!("Threshold arithmetic overflow."))?;
    Ok(lhs >= rhs)
}
fn measurement(v: &Value) -> Result<Option<&Value>> {
    match text(&v["presence"])? {
        "observed" => {
            ensure!(
                !v["value"].is_null() && v["reason"].is_null(),
                "An observed value is malformed."
            );
            Ok(Some(&v["value"]))
        }
        "missing" | "error" => {
            ensure!(v["value"].is_null(), "An unknown value must be null.");
            id(&v["reason"])?;
            Ok(None)
        }
        _ => Err(eyre!("The presence state is unsupported.")),
    }
}
fn fraction(v: &Value) -> Result<()> {
    if let Some(value) = measurement(v)? {
        let numerator = text(&value["numerator"])?;
        let parsed = numerator.parse::<i64>()?;
        ensure!(
            parsed.to_string() == numerator,
            "A signed numerator is not canonical."
        );
        ensure!(
            manifest::decimal(&value["denominator"])? > 0,
            "A fraction denominator is zero."
        );
    }
    Ok(())
}
fn same_fraction(a: &Value, b: &Value) -> Result<bool> {
    let left = text(&a["numerator"])?.parse::<i64>()? as i128;
    let right = text(&b["numerator"])?.parse::<i64>()? as i128;
    let left_denominator = manifest::decimal(&a["denominator"])? as i128;
    let right_denominator = manifest::decimal(&b["denominator"])? as i128;
    Ok(left * right_denominator == right * left_denominator)
}
fn finality(v: &Value) -> Result<()> {
    ensure!(
        ["finalized", "not_finalized", "hold", "rejected"].contains(&text(&v["decision"])?),
        "The finality decision is unsupported."
    );
    if v["decision"] == "hold" {
        id(&v["hold_reason"])?;
    } else {
        ensure!(
            v["hold_reason"].is_null(),
            "A non-hold decision has a hold reason."
        );
    }
    ensure!(
        v["threshold_pass"].is_boolean(),
        "The threshold observation must be Boolean."
    );
    fraction(&v["original_ft"])?;
    fraction(&v["projection"])?;
    Ok(())
}
fn request(r: &Value) -> Result<()> {
    ensure!(
        r["schema_version"] == 1 && r["profile_id"] == PROFILE,
        "The profile schema is unsupported."
    );
    for k in ["run_id", "scenario_id", "pair_id", "evidence_kind", "phase"] {
        id(&r[k])?;
    }
    manifest::hex(&r["manifest_digest"], 64)?;
    for field in ["seed", "segment", "iteration"] {
        manifest::decimal(&r[field])?;
    }
    id(&r["policy_variant"])?;
    ensure!(
        KINDS.contains(&text(&r["scenario_kind"])?),
        "The scenario kind is unsupported."
    );
    ensure!(
        r["require_work"].is_boolean(),
        "The work requirement must be Boolean."
    );
    threshold(&r["threshold_inputs"])?;
    ensure!(
        ["complete", "missing_dependencies"].contains(&text(&r["metadata_availability"])?),
        "The metadata state is unsupported."
    );
    ensure!(
        r["scenario_kind"] != "traversal_comparison" || r["require_work"] == true,
        "A traversal comparison requires counters."
    );
    ensure!(
        r["scenario_kind"] != "missing_dependencies"
            || r["metadata_availability"] == "missing_dependencies",
        "The dependency fixture must declare missing metadata."
    );
    object(&r["protocol_context"])?;
    id(&r["observation_deadline"]["clock_id"])?;
    manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?;
    let members = array(&r["members"])?;
    ensure!(members.len() == 2, "Exactly two members are required.");
    let mut names = BTreeSet::new();
    let mut modes = BTreeSet::new();
    for member in members {
        for key in [
            "member_id",
            "candidate_id",
            "node_id",
            "incarnation",
            "evaluation_mode",
        ] {
            id(&member[key])?;
        }
        ensure!(
            names.insert(text(&member["member_id"])?),
            "A member identity is duplicated."
        );
        modes.insert(text(&member["evaluation_mode"])?);
        manifest::hex(&member["node_revision"], 40)?;
        for key in [
            "node_binary_digest",
            "dag_digest",
            "electorate_digest",
            "justification_digest",
        ] {
            manifest::hex(&member[key], 64)?;
        }
        equal(member, &members[0], &[
            "candidate_id",
            "node_revision",
            "node_binary_digest",
            "dag_digest",
            "electorate_digest",
            "justification_digest",
        ])?;
    }
    ensure!(
        modes == BTreeSet::from(["bounded", "reference"]),
        "The paired modes must be bounded and reference."
    );
    let faults = array(&r["fault_schedule"])?;
    ensure!(
        faults.len() <= 1,
        "The initial profile supports one scheduled fault."
    );
    for fault in faults {
        for key in [
            "fault_id",
            "member_id",
            "node_id",
            "incarnation",
            "trigger_event",
        ] {
            id(&fault[key])?;
        }
        let member = members
            .iter()
            .find(|m| m["member_id"] == fault["member_id"])
            .ok_or_else(|| eyre!("A fault member is unknown."))?;
        ensure!(
            fault["node_id"] == member["node_id"],
            "The fault node differs."
        );
        equal(&fault["ack_deadline"], &r["observation_deadline"], &[
            "clock_id",
        ])?;
        ensure!(
            manifest::decimal(&fault["ack_deadline"]["monotonic_ns"])?
                <= manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?,
            "A fault deadline exceeds the observation deadline."
        );
        ensure!(
            array(&fault["after"])?.is_empty(),
            "Cross-node ordering needs a qualified adapter."
        );
        match text(&fault["action"])? {
            "pause" => ensure!(
                fault["incarnation"] == member["incarnation"],
                "The pause incarnation differs."
            ),
            "restart" => {
                ensure!(
                    member["predecessor_incarnation"] == fault["incarnation"]
                        && member["incarnation"] != fault["incarnation"],
                    "The restart relation differs."
                );
            }
            _ => return Err(eyre!("The fault action is unsupported.")),
        }
    }
    if r["scenario_kind"] == "restart" {
        ensure!(
            faults.len() == 1 && faults[0]["action"] == "restart",
            "A restart scenario requires a restart schedule."
        );
    }
    Ok(())
}
fn blocked(r: &Value) -> Result<Vec<String>> {
    let mut reasons = Vec::new();
    if r["evidence_kind"] != "synthetic_fixture" {
        reasons.push("live_adapter_unqualified".into());
    }
    if r["phase"] != "pre_pr216_merge" {
        reasons.push("post_merge_adapter_unqualified".into());
    }
    if r["policy_variant"] != "baseline" {
        reasons.push("policy_variant_unqualified".into());
    }
    let mut capabilities = CAPABILITIES.to_vec();
    if r["require_work"] == true {
        capabilities.push("traversal-counters");
    }
    if !array(&r["fault_schedule"])?.is_empty() {
        capabilities.push("fault-receipts");
    }
    match text(&r["scenario_kind"])? {
        "replay" => capabilities.push("replay-control"),
        "duplicate_justifications" | "signature_rejection" => {
            capabilities.push("justification-injection")
        }
        "missing_dependencies" => capabilities.push("dependency-control"),
        _ => {}
    }
    for capability in capabilities {
        if r["capabilities"][capability]["status"] != "qualified" {
            reasons.push(capability.to_owned());
        }
    }
    Ok(reasons)
}
fn steps(r: &Value) -> Result<Vec<&'static str>> {
    Ok(match text(&r["scenario_kind"])? {
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
        _ => vec!["load_fixture", "evaluate"],
    })
}
pub fn generate(r: &Value) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r)?;
    let mut members = array(&r["members"])?.clone();
    members.sort_by_key(|m| m["member_id"].as_str().unwrap_or_default().to_owned());
    let steps = steps(r)?;
    let workloads: Vec<_> = if reasons.is_empty() {
        members.iter().map(|m| json!({"operation":"evaluate_fixture","steps":steps,"member":m,"inputs":r["inputs"],"seed":r["seed"],"scenario_kind":r["scenario_kind"],"threshold_inputs":r["threshold_inputs"],"metadata_availability":r["metadata_availability"],"protocol_context":r["protocol_context"]})).collect()
    } else {
        Vec::new()
    };
    Ok(
        json!({"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"scenario_id":r["scenario_id"],"pair_id":r["pair_id"],"evidence_kind":r["evidence_kind"],"scenario_verdict":if reasons.is_empty(){"ready"}else{"blocked"},"blocked_reasons":reasons,"workloads":workloads,"fault_requests":if reasons.is_empty(){r["fault_schedule"].clone()}else{json!([])},"node_launch_count":0}),
    )
}

pub fn collect(m: &Value, artifacts: &Value) -> Result<Value> {
    let root = Path::new(text(&artifacts["root"])?);
    let refs = array(&artifacts["records"])?;
    ensure!(refs.len() <= 64, "The transport record bound is exceeded.");
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    let mut duplicates = Vec::new();
    let mut records = BTreeSet::new();
    let mut events: BTreeMap<String, Value> = BTreeMap::new();
    let mut sequences = BTreeMap::new();
    for reference in refs {
        let attempt = (|| -> Result<Option<Value>> {
            ensure!(
                reference["capture_state"] == "captured",
                "The artifact is not captured."
            );
            let bytes = artifact(root, reference)?;
            let mut v = parse(&bytes)?;
            ensure!(
                v["schema_version"] == 1,
                "The observation schema is unsupported."
            );
            ensure!(
                ["authority_snapshot", "fault_ack"].contains(&text(&v["event_kind"])?),
                "The event kind is unsupported."
            );
            equal(&v, m, &[
                "manifest_digest",
                "run_id",
                "phase",
                "evidence_kind",
            ])?;
            ensure!(
                array(&m["required_scenarios"])?.contains(&v["scenario_id"]),
                "The observation scenario is unknown."
            );
            for key in [
                "record_id",
                "scenario_id",
                "pair_id",
                "member_id",
                "node_id",
                "incarnation",
                "producer",
                "event_id",
                "event_kind",
            ] {
                id(&v[key])?;
            }
            ensure!(
                records.insert(text(&v["record_id"])?.to_owned()),
                "A transport identity is duplicated."
            );
            ensure!(
                reference["producer"] == v["producer"]
                    && array(&reference["observation_ids"])?.contains(&v["record_id"]),
                "The artifact producer or observation identity differs."
            );
            let sequence = manifest::decimal(&v["producer_sequence"])?;
            id(&v["time"]["clock_id"])?;
            id(&v["time"]["utc"])?;
            manifest::decimal(&v["time"]["monotonic_ns"])?;
            let producer = serde_json::to_string(&json!([
                v["scenario_id"],
                v["member_id"],
                v["node_id"],
                v["incarnation"],
                v["producer"]
            ]))?;
            let key = format!("{producer}:{}", text(&v["event_id"])?);
            let mut comparable = v.clone();
            comparable.as_object_mut().unwrap().remove("record_id");
            if let Some(old) = events.get(&key) {
                ensure!(*old == comparable, "Copies of an event disagree.");
                duplicates.push(reference.clone());
                return Ok(None);
            }
            ensure!(
                sequences.get(&producer).is_none_or(|old| *old < sequence),
                "The producer sequence did not increase."
            );
            sequences.insert(producer, sequence);
            events.insert(key, comparable);
            v["raw_artifact"] = reference.clone();
            Ok(Some(v))
        })();
        match attempt {
            Ok(Some(v)) => accepted.push(v),
            Ok(None) => {}
            Err(error) => rejected.push(json!({"source":reference,"reason":error.to_string()})),
        }
    }
    Ok(
        json!({"observations":accepted,"rejected_observations":rejected,"duplicate_sources":duplicates,"transport_records":refs.len()}),
    )
}

pub fn classify(r: &Value, collection: &Value, acknowledgments: &[Value]) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r)?;
    let mut result = json!({"schema_version":1,"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"run_id":r["run_id"],"scenario_id":r["scenario_id"],"pair_id":r["pair_id"],"evidence_kind":r["evidence_kind"],"scenario_verdict":"incomplete","product_failures":[],"missing_observations":[],"rejected_observations":collection["rejected_observations"],"fault_receipts":acknowledgments,"measurements":[],"raw_references":[],"coverage":{"required":[],"observed":[]},"blocked_reasons":reasons,"node_launch_count":0,"harness_verification":"pending","soak_verdict":"non_passing","threshold_component_expected":threshold(&r["threshold_inputs"])?});
    if !reasons.is_empty() {
        result["scenario_verdict"] = "blocked".into();
        return Ok(result);
    }
    let mut failures = Vec::new();
    let mut missing = Vec::new();
    let mut invalid = array(&collection["rejected_observations"])?.clone();
    let mut measures = Vec::new();
    let mut references = Vec::new();
    let mut heads = Vec::new();
    let mut required = Vec::new();
    let mut observed = Vec::new();
    let members = array(&r["members"])?;
    let observations = array(&collection["observations"])?;
    for v in observations {
        if !members
            .iter()
            .any(|member| member["member_id"] == v["member_id"])
        {
            invalid.push(json!({"source":v["raw_artifact"],"reason":"unknown_member"}));
        }
    }
    for member in members {
        let name = text(&member["member_id"])?;
        let snapshots: Vec<_> = observations
            .iter()
            .filter(|v| v["member_id"] == name && v["event_kind"] == "authority_snapshot")
            .collect();
        for kind in ["head", "finality", "evaluation_receipt"]
            .into_iter()
            .chain((r["require_work"] == true).then_some("work"))
        {
            required.push(format!("{name}:{kind}"));
        }
        if snapshots.is_empty() {
            missing.push(format!("{name}:snapshot"));
            continue;
        }
        if snapshots.len() != 1 {
            invalid.push(json!({"member_id":name,"reason":"ambiguous_snapshot"}));
            continue;
        }
        let v = snapshots[0];
        let attempt = (|| -> Result<()> {
            equal(v, r, &[
                "manifest_digest",
                "run_id",
                "scenario_id",
                "pair_id",
                "seed",
                "segment",
                "iteration",
                "protocol_context",
                "metadata_availability",
                "threshold_inputs",
                "phase",
                "evidence_kind",
            ])?;
            equal(v, member, &[
                "member_id",
                "candidate_id",
                "node_revision",
                "node_binary_digest",
                "node_id",
                "incarnation",
                "evaluation_mode",
                "dag_digest",
                "electorate_digest",
                "justification_digest",
            ])?;
            equal(&v["time"], &r["observation_deadline"], &["clock_id"])?;
            ensure!(
                manifest::decimal(&v["time"]["monotonic_ns"])?
                    <= manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?,
                "The observation is late."
            );
            if let Some(payload) = measurement(
                &json!({"presence":v["presence"],"value":v["payload"],"reason":v["reason"]}),
            )? {
                if let Some(head) = measurement(&payload["head"])? {
                    manifest::hex(head, 64)?;
                    observed.push(format!("{name}:head"));
                    heads.push(json!({"member_id":name,"head":head,"source":v["raw_artifact"]}));
                    if *head != r["expected"]["head_hash"] {
                        failures.push(json!({"kind":"unexpected_head","member_id":name,"source":v["raw_artifact"]}));
                    }
                } else {
                    missing.push(format!("{name}:head"));
                }
                if let Some(f) = measurement(&payload["finality"])? {
                    finality(f)?;
                    observed.push(format!("{name}:finality"));
                    let expected = &r["expected"]["finality"];
                    for field in ["decision", "hold_reason", "threshold_pass"] {
                        if f[field] != expected[field] {
                            failures.push(json!({"kind":"finality_mismatch","field":field,"member_id":name,"source":v["raw_artifact"]}));
                        }
                    }
                    for field in ["original_ft", "projection"] {
                        match (measurement(&f[field])?, measurement(&expected[field])?) {
                            (Some(actual), Some(expected)) if !same_fraction(actual, expected)? => failures.push(json!({"kind":"finality_mismatch","field":field,"member_id":name,"source":v["raw_artifact"]})),
                            (None, Some(_)) => missing.push(format!("{name}:{field}")),
                            (None, None) if f["decision"] != "hold" => missing.push(format!("{name}:{field}")),
                            _ => {},
                        }
                    }
                } else {
                    missing.push(format!("{name}:finality"));
                }
                if let Some(work) = measurement(&payload["work"])? {
                    for key in ["visited_vertices", "traversed_edges"] {
                        manifest::decimal(&work[key])?;
                    }
                    observed.push(format!("{name}:work"));
                } else if r["require_work"] == true {
                    missing.push(format!("{name}:work"));
                }
                if let Some(receipt) = measurement(&payload["evaluation_receipt"])? {
                    ensure!(
                        ["applied", "not_applied", "unknown"].contains(&text(&receipt["status"])?),
                        "The evaluation status is unsupported."
                    );
                    ensure!(
                        receipt["fixture_digest"] == r["inputs"]["fixture"]["sha256"],
                        "The evaluated fixture differs."
                    );
                    if receipt["status"] == "applied" && receipt["steps"] == json!(steps(r)?) {
                        observed.push(format!("{name}:evaluation_receipt"));
                    } else {
                        missing.push(format!("{name}:evaluation_receipt"));
                    }
                } else {
                    missing.push(format!("{name}:evaluation_receipt"));
                }
                measures.push(json!({"member_id":name,"values":payload}));
            } else {
                missing.push(format!("{name}:snapshot"));
            }
            references.push(v["raw_artifact"].clone());
            Ok(())
        })();
        if let Err(error) = attempt {
            invalid.push(json!({"source":v["raw_artifact"],"reason":error.to_string()}));
        }
    }
    if heads.len() == 2 && heads[0]["head"] != heads[1]["head"] {
        failures.push(
            json!({"kind":"head_mismatch","sources":[heads[0]["source"],heads[1]["source"]]}),
        );
    }
    for acknowledgment in acknowledgments {
        if !array(&r["fault_schedule"])?
            .iter()
            .any(|f| f["fault_id"] == acknowledgment["payload"]["fault_id"])
        {
            invalid.push(json!({"source":acknowledgment["raw_artifact"],"reason":"unscheduled_fault_receipt"}));
        }
    }
    for fault in array(&r["fault_schedule"])? {
        let key = format!("fault:{}", text(&fault["fault_id"])?);
        required.push(key.clone());
        let receipts: Vec<_> = acknowledgments
            .iter()
            .filter(|v| v["payload"]["fault_id"] == fault["fault_id"])
            .collect();
        if receipts.len() != 1 {
            missing.push(key);
            continue;
        }
        let v = receipts[0];
        let valid = (|| -> Result<bool> {
            equal(v, r, &[
                "manifest_digest",
                "run_id",
                "scenario_id",
                "pair_id",
                "segment",
                "iteration",
                "phase",
                "evidence_kind",
            ])?;
            equal(v, fault, &["member_id", "node_id"])?;
            equal(&v["time"], &fault["ack_deadline"], &["clock_id"])?;
            ensure!(
                manifest::decimal(&v["time"]["monotonic_ns"])?
                    <= manifest::decimal(&fault["ack_deadline"]["monotonic_ns"])?,
                "The fault receipt is late."
            );
            let p = &v["payload"];
            equal(p, fault, &[
                "fault_id",
                "action",
                "incarnation",
                "trigger_event",
            ])?;
            ensure!(
                ["applied", "not_applied", "unknown"].contains(&text(&p["status"])?),
                "The fault status is unsupported."
            );
            let member = members
                .iter()
                .find(|m| m["member_id"] == fault["member_id"])
                .unwrap();
            equal(v, member, &[
                "candidate_id",
                "node_revision",
                "node_binary_digest",
                "incarnation",
            ])?;
            let presence = measurement(
                &json!({"presence":v["presence"],"value":v["payload"],"reason":v["reason"]}),
            )?
            .is_some();
            if p["status"] == "applied" && fault["action"] == "restart" {
                ensure!(
                    p["prior_exit"].is_boolean() && p["ready"].is_boolean(),
                    "Restart receipt fields must be Boolean."
                );
            }
            Ok(presence
                && p["status"] == "applied"
                && if fault["action"] == "restart" {
                    p["prior_exit"] == true
                        && p["ready"] == true
                        && p["new_incarnation"] == member["incarnation"]
                } else {
                    p["observed_state"] == "stopped" || p["observed_state"] == "paused"
                })
        })();
        match valid {
            Ok(true) => {
                observed.push(key);
                references.push(v["raw_artifact"].clone());
            }
            Ok(false) => missing.push(key),
            Err(error) => {
                invalid.push(json!({"source":v["raw_artifact"],"reason":error.to_string()}))
            }
        }
    }
    result["scenario_verdict"] = if !invalid.is_empty() {
        "invalid_input"
    } else if !failures.is_empty() {
        "product_failure"
    } else if !missing.is_empty() {
        "incomplete"
    } else {
        "passed"
    }
    .into();
    result["product_failures"] = json!(failures);
    result["missing_observations"] = json!(missing);
    result["rejected_observations"] = json!(invalid);
    result["measurements"] = json!(measures);
    result["raw_references"] = json!(references);
    result["coverage"] = json!({"required":required,"observed":observed});
    Ok(result)
}

pub fn prepare(m: &Value, r: &Value, root: &Path) -> Result<Value> {
    manifest::validate(m)?;
    request(r)?;
    equal(r, m, &[
        "run_id",
        "phase",
        "seed",
        "evidence_kind",
        "profile_id",
        "policy_variant",
    ])?;
    ensure!(
        array(&m["required_scenarios"])?.contains(&r["scenario_id"]),
        "The scenario is not in the manifest."
    );
    let own = identity();
    ensure!(
        m["profile_digest"] == own["profile_digest"],
        "The profile source differs."
    );
    for (path, digest) in object(&own["source_digests"])? {
        ensure!(
            m["source_digests"][path] == *digest,
            "A compiled source pin differs."
        );
    }
    ensure!(
        m["profile_binary_digest"] == file_hash(&std::env::current_exe()?)?,
        "The profile executable differs."
    );
    for member in array(&r["members"])? {
        equal(member, m, &[
            "candidate_id",
            "node_revision",
            "node_binary_digest",
        ])?;
    }
    let mut result = r.clone();
    result["capabilities"] = m["capabilities"].clone();
    for (name, cap) in object(&m["capabilities"])? {
        ensure!(
            ["qualified", "unsupported", "unknown"].contains(&text(&cap["status"])?),
            "A capability status is unsupported."
        );
        if cap["status"] == "qualified" {
            ensure!(
                cap["qualification"]["capture_state"] == "captured",
                "A qualification is not captured."
            );
            let proof = parse(&artifact(root, &cap["qualification"])?)?;
            ensure!(
                proof["capability"] == *name
                    && proof["evidence_kind"] == m["evidence_kind"]
                    && proof["profile_digest"] == own["profile_digest"]
                    && proof["status"] == "qualified",
                "A qualification differs."
            );
            equal(&proof, m, &[
                "provider",
                "node_revision",
                "profile_binary_digest",
                "external_harness_revision",
            ])?;
            equal(cap, m, &["provider", "node_revision"])?;
        }
    }
    for (name, reference) in object(&r["inputs"])? {
        ensure!(
            reference["capture_state"] == "captured",
            "A fixture input is not captured."
        );
        let bytes = artifact(root, reference)?;
        parse(&bytes)?;
        let field = format!("{name}_digest");
        if ["dag", "electorate", "justification"].contains(&name.as_str()) {
            for member in array(&r["members"])? {
                ensure!(
                    member[&field] == reference["sha256"],
                    "A fixture component differs."
                );
            }
        }
    }
    ensure!(
        r["inputs"]["fixture"]["sha256"] == m["fixture_digest"]
            && r["inputs"]["expectation"]["sha256"] == m["expectation_digest"],
        "A fixture or expectation pin differs."
    );
    for name in [
        "dag",
        "electorate",
        "justification",
        "fixture",
        "expectation",
        "configuration",
    ] {
        object(&r["inputs"][name])?;
    }
    ensure!(
        r["inputs"]["configuration"]["sha256"] == m["configuration_digest"],
        "The configuration pin differs."
    );
    let configuration = parse(&artifact(root, &r["inputs"]["configuration"])?)?;
    equal(&configuration, r, &["protocol_context", "policy_variant"])?;
    let fixture = parse(&artifact(root, &r["inputs"]["fixture"])?)?;
    equal(&fixture, r, &[
        "scenario_kind",
        "threshold_inputs",
        "metadata_availability",
        "protocol_context",
    ])?;
    result["expected"] = parse(&artifact(root, &r["inputs"]["expectation"])?)?;
    manifest::hex(&result["expected"]["head_hash"], 64)?;
    finality(&result["expected"]["finality"])?;
    if r["scenario_kind"] == "signature_rejection" {
        ensure!(
            result["expected"]["finality"]["decision"] == "rejected",
            "The signature fixture must expect rejection."
        );
    }
    let component = threshold(&r["threshold_inputs"])?;
    ensure!(
        result["expected"]["finality"]["threshold_pass"] == component,
        "The threshold fixture expectation differs."
    );
    if result["expected"]["finality"]["decision"] == "finalized" {
        ensure!(
            component && r["metadata_availability"] == "complete",
            "The finalization fixture contradicts a required gate."
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_checked_arithmetic_matches_two_thousand_small_integer_cases() {
        for seed in 0u64..2000 {
            let total = seed % 32 + 1;
            let agreeing = (seed / 3) % (total + 1);
            let q = (seed / 7) % (agreeing + 1);
            let d = seed % 7 + 1;
            let n = (seed / 11) % (d + 1);
            let minimum = (total * (d + n)).div_ceil(2 * d);
            let expected = agreeing > total / 2 && q >= minimum;
            let input = json!({"q":q.to_string(),"S":total.to_string(),"agreeing_stake":agreeing.to_string(),"n":n.to_string(),"d":d.to_string()});
            assert_eq!(threshold(&input).unwrap(), expected);
        }
        let max = u64::MAX.to_string();
        assert!(
            !threshold(&json!({"q":"0","S":max,"agreeing_stake":"0","n":max,"d":max})).unwrap()
        );
        assert!(threshold(&json!({"q":max,"S":max,"agreeing_stake":max,"n":"0","d":max})).is_err());
        assert!(threshold(&json!({"q":"1","S":"1","agreeing_stake":"1","n":"0","d":"0"})).is_err());
    }
}
