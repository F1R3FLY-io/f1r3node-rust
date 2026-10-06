use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use casper_soak::{array, artifact, file_hash, hash, manifest, object, parse, text};
use eyre::{ensure, Result};
use serde_json::{json, Value};

pub const PROFILE: &str = "recovery";
const CONTEXT: &[&str] = &[
    "manifest_digest",
    "run_id",
    "phase",
    "evidence_kind",
    "candidate_id",
    "node_revision",
    "node_binary_digest",
    "scenario_id",
    "pair_id",
    "member_id",
    "segment",
    "iteration",
    "seed",
    "policy_variant",
];
const FIELDS: &[&str] = &[
    "custodian",
    "reason_inputs",
    "joined_reason",
    "causal_references",
    "tombstone",
    "lease",
    "terminal_outcome",
    "retry_authorized",
    "body_available",
    "objective_height",
    "lifespan",
    "retry_count",
];
const METRICS: &[&str] = &[
    "block_count",
    "cadence_ns",
    "latency_ns",
    "rss_bytes",
    "cpu_ns",
];

pub fn identity() -> Value {
    let mut sources = serde_json::Map::new();
    for (path, bytes) in [
        (
            "scripts/casper-soak/src/profiles/recovery.rs",
            include_bytes!("recovery.rs").as_slice(),
        ),
        (
            "scripts/casper-soak/src/bin/casper-recovery.rs",
            include_bytes!("../bin/casper-recovery.rs").as_slice(),
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
    ] {
        sources.insert(path.into(), hash(bytes).into());
    }
    json!({"profile_id":PROFILE,"version":env!("CARGO_PKG_VERSION"),"profile_digest":sources["scripts/casper-soak/src/profiles/recovery.rs"],"source_digests":sources})
}
fn id(v: &Value) -> Result<&str> {
    let s = text(v)?;
    ensure!(
        !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control),
        "An identifier is invalid."
    );
    Ok(s)
}
fn same(a: &Value, b: &Value, keys: &[&str]) -> bool {
    keys.iter()
        .all(|key| !a[*key].is_null() && a[*key] == b[*key])
}
fn measurement(v: &Value) -> Result<Option<&Value>> {
    match text(&v["presence"])? {
        "observed" => {
            ensure!(
                !v["value"].is_null() && v["reason"].is_null(),
                "An observed measurement is invalid."
            );
            Ok(Some(&v["value"]))
        }
        "missing" | "error" => {
            ensure!(v["value"].is_null(), "An unknown value must be null.");
            id(&v["reason"])?;
            Ok(None)
        }
        _ => eyre::bail!("The measurement presence is unsupported."),
    }
}
fn strings(v: &Value) -> Result<BTreeSet<String>> {
    ensure!(array(v)?.len() <= 64, "A set exceeds its bound.");
    array(v)?.iter().map(|s| Ok(id(s)?.to_owned())).collect()
}
fn field(key: &str, v: &Value) -> Result<Value> {
    match key {
        "reason_inputs" | "causal_references" => Ok(json!(strings(v)?)),
        "objective_height" | "lifespan" | "retry_count" => {
            manifest::decimal(v)?;
            Ok(v.clone())
        }
        "tombstone" | "retry_authorized" | "body_available" => {
            ensure!(v.is_boolean(), "A state flag must be Boolean.");
            Ok(v.clone())
        }
        "terminal_outcome" => {
            ensure!(
                ["retried", "expired", "pending", "rejected"].contains(&text(v)?),
                "The terminal outcome is unsupported."
            );
            Ok(v.clone())
        }
        "lease" => {
            id(&v["holder"])?;
            manifest::decimal(&v["expires_height"])?;
            ensure!(v["expired"].is_boolean(), "The lease flag must be Boolean.");
            Ok(v.clone())
        }
        _ => {
            id(v)?;
            Ok(v.clone())
        }
    }
}
fn occurrences(v: &Value) -> Result<BTreeMap<String, Value>> {
    ensure!(
        array(v)?.len() <= 64,
        "The occurrence inventory exceeds its bound."
    );
    let mut result = BTreeMap::new();
    for item in array(v)? {
        for key in ["occurrence_id", "deploy_signature", "sender"] {
            id(&item[key])?;
        }
        manifest::hex(&item["carrier_block"], 64)?;
        ensure!(
            result
                .insert(text(&item["occurrence_id"])?.into(), item.clone())
                .is_none(),
            "An occurrence identity is duplicated."
        );
    }
    Ok(result)
}
fn request(r: &Value) -> Result<()> {
    ensure!(
        r["schema_version"] == 1 && r["profile_id"] == PROFILE,
        "The request schema differs."
    );
    for key in CONTEXT.iter().copied().chain([
        "node_id",
        "incarnation",
        "occurrence_schema",
        "recovery_lane",
        "coverage_rule",
        "leadership",
        "clock_policy",
    ]) {
        id(&r[key])?;
    }
    for key in ["manifest_digest", "frontier_digest"] {
        manifest::hex(&r[key], 64)?;
    }
    for key in [
        "seed",
        "segment",
        "iteration",
        "objective_height",
        "lifespan",
    ] {
        manifest::decimal(&r[key])?;
    }
    ensure!(
        [
            "stale_recovery",
            "convergence",
            "frontier_follow",
            "pending_deploy",
            "readiness",
            "backstop"
        ]
        .contains(&text(&r["recovery_lane"])?),
        "The recovery lane is unsupported."
    );
    for key in ["require_telemetry", "require_duration"] {
        ensure!(r[key].is_boolean(), "A requirement must be Boolean.");
    }
    id(&r["observation_deadline"]["clock_id"])?;
    let deadline = manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?;
    occurrences(&r["occurrences"])?;
    let mut ids = BTreeSet::new();
    ensure!(
        array(&r["fault_schedule"])?.len() <= 8,
        "The fault schedule exceeds its bound."
    );
    for fault in array(&r["fault_schedule"])? {
        for key in ["fault_id", "node_id", "incarnation", "trigger_id"] {
            id(&fault[key])?;
        }
        ensure!(
            ["pause", "delay_delivery"].contains(&text(&fault["action"])?),
            "The fault action is unsupported."
        );
        if fault["action"] == "delay_delivery" {
            id(&fault["message_id"])?;
        }
        ensure!(
            fault["deadline"]["clock_id"] == r["observation_deadline"]["clock_id"]
                && manifest::decimal(&fault["deadline"]["monotonic_ns"])? <= deadline,
            "The fault deadline differs."
        );
        for prior in strings(&fault["after"])? {
            ensure!(
                ids.contains(&prior),
                "Fault ordering must reference an earlier request."
            );
        }
        ensure!(
            ids.insert(id(&fault["fault_id"])?.to_owned()),
            "A fault identity is duplicated."
        );
    }
    Ok(())
}
fn blocked(r: &Value) -> Vec<String> {
    let mut reasons = Vec::new();
    if r["evidence_kind"] != "synthetic_fixture" {
        reasons.push("live_adapter_unqualified".into());
    }
    if r["phase"] != "pre_pr216_merge" {
        reasons.push("post_merge_adapter_unqualified".into());
    }
    let leadership = match r["recovery_lane"].as_str() {
        Some("stale_recovery") => "all_eligible",
        Some("convergence") => "leader_only",
        _ => "baseline",
    };
    if r["policy_variant"] != "baseline"
        || r["coverage_rule"] != "one_parent_b1"
        || r["clock_policy"] != "baseline"
        || r["leadership"] != leadership
    {
        reasons.push("experimental_policy_unapproved".into());
    }
    let mut caps = BTreeSet::from([
        "recovery-lanes",
        "exact-occurrences",
        "custody-observations",
        "objective-height",
    ]);
    if r["require_telemetry"] == true {
        caps.insert("recovery-telemetry");
    }
    if let Some(faults) = r["fault_schedule"].as_array() {
        for f in faults {
            caps.insert(if f["action"] == "pause" {
                "paused-state"
            } else {
                "delivery-receipts"
            });
        }
    }
    for cap in caps {
        if r["capabilities"][cap]["status"] != "qualified" {
            reasons.push(format!("missing_capability:{cap}"));
        }
    }
    reasons
}
pub fn generate(r: &Value) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    Ok(
        json!({"profile_id":PROFILE,"scenario_verdict":if reasons.is_empty(){"ready"}else{"blocked"},"blocked_reasons":reasons,"node_launch_count":0,"policy_activation":false,"schedule":if reasons.is_empty(){json!(["load_fixture","apply_fault_schedule","observe_recovery"])}else{json!([])},"requested_context":r,"workloads":if reasons.is_empty(){json!([{"action":"load_fixture","fixture":r["inputs"]["fixture"],"seed":r["seed"],"recovery_lane":r["recovery_lane"],"policy_variant":r["policy_variant"]},{"action":"observe_recovery","occurrence_schema":r["occurrence_schema"],"deadline":r["observation_deadline"]}])}else{json!([])},"fault_requests":if reasons.is_empty(){r["fault_schedule"].clone()}else{json!([])}}),
    )
}
pub fn prepare(m: &Value, r: &Value, root: &Path) -> Result<Value> {
    manifest::validate(m)?;
    request(r)?;
    ensure!(
        same(r, m, &[
            "manifest_digest",
            "run_id",
            "phase",
            "candidate_id",
            "node_revision",
            "node_binary_digest",
            "seed",
            "evidence_kind",
            "policy_variant",
            "profile_id"
        ]),
        "The manifest and request differ."
    );
    ensure!(
        m["occurrence_schema"] == r["occurrence_schema"],
        "The occurrence identity definition differs."
    );
    ensure!(
        array(&m["required_scenarios"])?.contains(&r["scenario_id"]),
        "The scenario is not declared."
    );
    let own = identity();
    ensure!(
        m["profile_digest"] == own["profile_digest"]
            && m["profile_binary_digest"] == file_hash(&std::env::current_exe()?)?,
        "The profile source or executable differs."
    );
    for (path, digest) in object(&own["source_digests"])? {
        ensure!(
            m["source_digests"][path] == *digest,
            "A compiled source pin differs."
        );
    }
    let mut p = r.clone();
    p["capabilities"] = m["capabilities"].clone();
    for (name, cap) in object(&m["capabilities"])? {
        ensure!(
            ["qualified", "unknown", "unsupported"].contains(&text(&cap["status"])?),
            "The capability status is unsupported."
        );
        if cap["status"] == "qualified" {
            ensure!(
                cap["qualification"]["capture_state"] == "captured",
                "The qualification is absent."
            );
            let proof = parse(&artifact(root, &cap["qualification"])?)?;
            if name == "exact-occurrences" {
                ensure!(
                    proof["occurrence_schema"] == r["occurrence_schema"],
                    "The qualified occurrence schema differs."
                );
            }
            ensure!(
                proof["capability"] == *name
                    && proof["status"] == "qualified"
                    && same(cap, m, &["provider", "node_revision"])
                    && same(&proof, m, &[
                        "provider",
                        "node_revision",
                        "node_binary_digest",
                        "external_harness_revision",
                        "evidence_kind",
                        "profile_digest",
                        "profile_binary_digest"
                    ]),
                "The qualification differs."
            );
        }
    }
    for key in ["configuration", "fixture", "expectation"] {
        let reference = &r["inputs"][key];
        ensure!(
            reference["capture_state"] == "captured"
                && reference["sha256"] == m[format!("{key}_digest")],
            "A pinned input differs."
        );
        p[key] = parse(&artifact(root, reference)?)?;
    }
    ensure!(
        same(&p["configuration"], r, &[
            "policy_variant",
            "coverage_rule",
            "leadership",
            "clock_policy"
        ]),
        "The policy configuration differs."
    );
    ensure!(
        same(&p["fixture"], r, &[
            "recovery_lane",
            "occurrence_schema",
            "frontier_digest",
            "objective_height",
            "lifespan",
            "occurrences",
            "fault_schedule"
        ]),
        "The fixture context differs."
    );
    let frontier = &r["inputs"]["frontier"];
    ensure!(
        frontier["capture_state"] == "captured" && frontier["sha256"] == r["frontier_digest"],
        "The frontier artifact differs."
    );
    p["frontier"] = parse(&artifact(root, frontier)?)?;
    object(&p["frontier"])?;
    for key in ["one_parent", "collective"] {
        ensure!(
            p["expectation"]["frontier_coverage"][key].is_boolean(),
            "A coverage expectation must be Boolean."
        );
    }
    let expected = occurrences(&p["expectation"]["occurrences"])?;
    let inputs = occurrences(&r["occurrences"])?;
    ensure!(
        expected.len() == inputs.len(),
        "The expectation inventory differs."
    );
    for (key, value) in &inputs {
        let e = expected
            .get(key)
            .ok_or_else(|| eyre::eyre!("An expected occurrence is absent."))?;
        ensure!(
            same(value, e, &[
                "occurrence_id",
                "deploy_signature",
                "carrier_block",
                "sender"
            ]),
            "An expected source identity differs."
        );
        for field_name in FIELDS {
            field(field_name, &e[*field_name])?;
        }
    }
    for key in METRICS {
        manifest::decimal(&p["expectation"]["telemetry"][*key])?;
    }
    Ok(p)
}
pub fn collect(m: &Value, a: &Value) -> Result<Value> {
    let r = &a["request"];
    request(r)?;
    let root = Path::new(text(&a["root"])?);
    let mut observations = Vec::new();
    let mut rejected = Vec::new();
    let mut duplicates = Vec::new();
    let mut ids = BTreeSet::new();
    let mut events = BTreeMap::new();
    let mut sequences = BTreeMap::new();
    ensure!(
        array(&a["records"])?.len() <= 64,
        "The transport inventory exceeds its bound."
    );
    for reference in array(&a["records"])? {
        let attempt = (|| -> Result<()> {
            let mut v = parse(&artifact(root, reference)?)?;
            ensure!(v["schema_version"] == 1, "The observation schema differs.");
            let kind = text(&v["event_kind"])?;
            ensure!(
                ["recovery_snapshot", "fault_ack"].contains(&kind),
                "The observation kind is unsupported."
            );
            for key in [
                "record_id",
                "event_id",
                "producer",
                "node_id",
                "incarnation",
            ] {
                id(&v[key])?;
            }
            ensure!(
                ids.insert(text(&v["record_id"])?.to_owned()),
                "A transport identity is duplicated."
            );
            ensure!(
                reference["producer"] == v["producer"]
                    && array(&reference["observation_ids"])?.contains(&v["record_id"]),
                "The transport association differs."
            );
            let sequence = manifest::decimal(&v["producer_sequence"])?;
            let time = manifest::decimal(&v["time"]["monotonic_ns"])?;
            id(&v["time"]["utc"])?;
            let producer = json!([
                v["member_id"],
                v["node_id"],
                v["incarnation"],
                v["producer"]
            ])
            .to_string();
            let context: Vec<_> = CONTEXT.iter().map(|key| v[*key].clone()).collect();
            let event = json!([context, producer, v["event_id"]]).to_string();
            let mut canonical = v.clone();
            canonical
                .as_object_mut()
                .ok_or_else(|| eyre::eyre!("The observation is not an object."))?
                .remove("record_id");
            if let Some(old) = events.get(&event) {
                ensure!(old == &canonical, "An event has conflicting copies.");
                duplicates.push(reference.clone());
                return Ok(());
            }
            events.insert(event, canonical);
            let subject = if kind == "fault_ack" {
                array(&r["fault_schedule"])?
                    .iter()
                    .find(|f| f["fault_id"] == v["payload"]["fault_id"])
                    .unwrap_or(&Value::Null)
            } else {
                r
            };
            if !same(&v, r, CONTEXT)
                || !same(&v, m, &["run_id", "phase", "evidence_kind"])
                || !same(&v, subject, &["node_id", "incarnation"])
                || v["time"]["clock_id"] != r["observation_deadline"]["clock_id"]
                || time > manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?
                || (kind == "recovery_snapshot"
                    && !same(&v["payload"], r, &[
                        "recovery_lane",
                        "occurrence_schema",
                        "frontier_digest",
                        "coverage_rule",
                    ]))
            {
                rejected.push(json!({"source":reference,"fatal":false,"reason":"The observation correlation or deadline differs."}));
                return Ok(());
            }
            match text(&v["presence"])? {
                "observed" => ensure!(
                    !v["payload"].is_null() && v["reason"].is_null(),
                    "The observed payload is invalid."
                ),
                "missing" | "error" => {
                    ensure!(v["payload"].is_null(), "An unknown payload must be null.");
                    id(&v["reason"])?;
                }
                _ => eyre::bail!("The presence is unsupported."),
            }
            ensure!(
                sequences.get(&producer).is_none_or(|old| sequence > *old),
                "The producer sequence is not increasing."
            );
            sequences.insert(producer, sequence);
            v["raw_artifact"] = reference.clone();
            observations.push(v);
            Ok(())
        })();
        if let Err(error) = attempt {
            rejected.push(json!({"source":reference,"reason":error.to_string(),"fatal":true}));
        }
    }
    Ok(
        json!({"observations":observations,"rejected_observations":rejected,"duplicate_sources":duplicates}),
    )
}
pub fn classify(r: &Value, c: &Value, acks: &[Value]) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    let mut out = json!({"schema_version":1,"profile_id":PROFILE,"run_id":r["run_id"],"scenario_id":r["scenario_id"],"manifest_digest":r["manifest_digest"],"evidence_kind":r["evidence_kind"],"node_launch_count":0,"policy_activation":false,"harness_verification":"pending","soak_verdict":"non_passing","scenario_verdict":"blocked","blocked_reasons":reasons,"product_failures":[],"missing_observations":[],"rejected_observations":c["rejected_observations"],"fault_receipts":acks,"raw_references":[],"measurements":{},"coverage":{"acknowledged_faults":0,"required_faults":r["fault_schedule"].as_array().map(Vec::len)}});
    if !reasons.is_empty() {
        return Ok(out);
    }
    let mut failures = Vec::new();
    let mut missing = Vec::new();
    let mut rejected = array(&c["rejected_observations"])?.clone();
    let mut raw = Vec::new();
    let mut acknowledged: BTreeMap<String, &Value> = BTreeMap::new();
    for f in array(&r["fault_schedule"])? {
        let values: Vec<_> = acks
            .iter()
            .filter(|a| a["payload"]["fault_id"] == f["fault_id"])
            .collect();
        if values.len() != 1 {
            missing.push(format!("fault:{}", f["fault_id"]));
            continue;
        }
        let a = values[0];
        let p = &a["payload"];
        let attempt = (|| -> Result<bool> {
            ensure!(
                same(p, f, &["fault_id", "action", "trigger_id"]),
                "The fault receipt differs."
            );
            ensure!(
                ["applied", "not_applied", "unknown"].contains(&text(&p["status"])?),
                "The receipt status is unsupported."
            );
            ensure!(
                p["observed"].is_boolean() && p["trigger_observed"].is_boolean(),
                "Receipt flags must be Boolean."
            );
            if a["time"]["clock_id"] != f["deadline"]["clock_id"]
                || manifest::decimal(&a["time"]["monotonic_ns"])?
                    > manifest::decimal(&f["deadline"]["monotonic_ns"])?
            {
                return Ok(false);
            }
            for prior in strings(&f["after"])? {
                let Some(old) = acknowledged.get(&prior) else {
                    return Ok(false);
                };
                if old["producer"] != a["producer"]
                    || old["time"]["clock_id"] != a["time"]["clock_id"]
                {
                    return Ok(false);
                }
                ensure!(
                    manifest::decimal(&old["producer_sequence"])?
                        < manifest::decimal(&a["producer_sequence"])?
                        && manifest::decimal(&old["time"]["monotonic_ns"])?
                            <= manifest::decimal(&a["time"]["monotonic_ns"])?,
                    "The observed delivery order differs."
                );
            }
            let state = if f["action"] == "pause" {
                p["observed_state"] == "paused" || p["observed_state"] == "stopped"
            } else {
                p["observed_state"] == "delivered" && p["message_id"] == f["message_id"]
            };
            Ok(p["status"] == "applied"
                && p["observed"] == true
                && p["trigger_observed"] == true
                && state)
        })();
        match attempt {
            Ok(true) => {
                acknowledged.insert(text(&f["fault_id"])?.into(), a);
                raw.push(a["raw_artifact"].clone());
            }
            Ok(false) => missing.push(format!("fault:{}", f["fault_id"])),
            Err(error) => rejected
                .push(json!({"source":a["raw_artifact"],"fatal":true,"reason":error.to_string()})),
        }
    }
    out["coverage"]["acknowledged_faults"] = json!(acknowledged.len());
    let snapshots: Vec<_> = array(&c["observations"])?
        .iter()
        .filter(|v| v["event_kind"] == "recovery_snapshot")
        .collect();
    if snapshots.len() != 1 {
        missing.push("recovery_snapshot".into());
    } else {
        let v = snapshots[0];
        let p = &v["payload"];
        let attempt = (|| -> Result<()> {
            if v["presence"] != "observed" {
                missing.push("recovery_snapshot".into());
                return Ok(());
            }
            raw.push(v["raw_artifact"].clone());
            for ack in acknowledged.values() {
                if manifest::decimal(&v["time"]["monotonic_ns"])?
                    < manifest::decimal(&ack["time"]["monotonic_ns"])?
                {
                    missing.push("snapshot_before_fault_completion".into());
                    return Ok(());
                }
            }
            ensure!(
                p["inventory_complete"].is_boolean(),
                "The inventory flag must be Boolean."
            );
            if p["inventory_complete"] != true {
                missing.push("occurrence_inventory".into());
            }
            if p["evaluation_receipt"]["status"] != "applied"
                || p["evaluation_receipt"]["fixture_digest"] != r["inputs"]["fixture"]["sha256"]
                || p["evaluation_receipt"]["steps"] != json!(["load_fixture", "observe_recovery"])
            {
                missing.push("evaluation_receipt".into());
            }
            for key in METRICS {
                match measurement(&p["telemetry"][*key])? {
                    Some(value) => {
                        manifest::decimal(value)?;
                        out["measurements"][*key] = p["telemetry"][*key].clone();
                        if *value != r["expectation"]["telemetry"][*key] {
                            failures.push(json!({"kind":"telemetry_mismatch","field":key,"source":v["raw_artifact"]}));
                        }
                    }
                    None => {
                        out["measurements"][*key] = p["telemetry"][*key].clone();
                        if r["require_telemetry"] == true {
                            missing.push((*key).into());
                        }
                    }
                }
            }
            let duration = if let Some(times) = measurement(&p["recovery_time"])? {
                id(&times["start"]["clock_id"])?;
                id(&times["end"]["clock_id"])?;
                let start = manifest::decimal(&times["start"]["monotonic_ns"])?;
                let end = manifest::decimal(&times["end"]["monotonic_ns"])?;
                if times["start"]["clock_id"] != times["end"]["clock_id"] {
                    json!({"presence":"missing","value":null,"reason":"clock_domains_differ"})
                } else {
                    ensure!(end >= start, "The duration is negative.");
                    json!({"presence":"observed","value":(end-start).to_string(),"reason":null})
                }
            } else {
                p["recovery_time"].clone()
            };
            if duration["presence"] != "observed" && r["require_duration"] == true {
                missing.push("recovery_duration_ns".into());
            }
            out["measurements"]["recovery_duration_ns"] = duration;
            match measurement(&p["frontier_coverage"])? {
                Some(value) => {
                    for key in ["one_parent", "collective"] {
                        ensure!(
                            value[key].is_boolean(),
                            "A coverage measurement must be Boolean."
                        );
                    }
                    if *value != r["expectation"]["frontier_coverage"] {
                        failures.push(
                            json!({"kind":"frontier_coverage_mismatch","source":v["raw_artifact"]}),
                        );
                    }
                }
                None => missing.push("frontier_coverage".into()),
            }
            out["measurements"]["frontier_coverage"] = p["frontier_coverage"].clone();
            let expected = occurrences(&r["expectation"]["occurrences"])?;
            let Some(samples) = measurement(&p["occurrences"])? else {
                missing.push("occurrences".into());
                return Ok(());
            };
            ensure!(
                array(samples)?.len() <= 64,
                "The sample inventory exceeds its bound."
            );
            let mut seen = BTreeSet::new();
            let mut sample_ids = BTreeSet::new();
            let mut custodians: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            let mut retries = BTreeSet::new();
            let mut expiries = BTreeSet::new();
            let mut terminals_known = true;
            let mut custody_known = true;
            for sample in array(samples)? {
                ensure!(
                    sample_ids.insert(id(&sample["sample_id"])?.to_owned()),
                    "A sample identity is duplicated."
                );
                let key = id(&sample["occurrence_id"])?;
                let Some(e) = expected.get(key) else {
                    eyre::bail!("An occurrence is undeclared.");
                };
                ensure!(
                    same(sample, e, &[
                        "occurrence_id",
                        "deploy_signature",
                        "carrier_block",
                        "sender"
                    ]),
                    "The source occurrence identity differs."
                );
                seen.insert(key.to_owned());
                for name in FIELDS {
                    if let Some(value) = measurement(&sample[*name])? {
                        let actual = field(name, value)?;
                        if actual != field(name, &e[*name])? {
                            failures.push(json!({"kind":"occurrence_state_mismatch","occurrence_id":key,"field":name,"expected":e[*name],"actual":value,"source":v["raw_artifact"]}));
                        }
                        if *name == "custodian" {
                            custodians
                                .entry(key.into())
                                .or_default()
                                .insert(text(value)?.into());
                        }
                        if *name == "terminal_outcome" {
                            if value == "retried" {
                                retries.insert(key.to_owned());
                            }
                            if value == "expired" {
                                expiries.insert(key.to_owned());
                            }
                        }
                    } else {
                        missing.push(format!("{key}:{name}"));
                        if *name == "terminal_outcome" {
                            terminals_known = false;
                        }
                        if *name == "custodian" {
                            custody_known = false;
                        }
                    }
                }
            }
            for key in expected.keys() {
                if !seen.contains(key) {
                    missing.push(format!("occurrence:{key}"));
                }
            }
            terminals_known &= p["inventory_complete"] == true;
            custody_known &= p["inventory_complete"] == true;
            out["measurements"]["counts"] = json!({"presence":"observed","reason":null,"value":{"occurrences":seen.len(),"observations":array(samples)?.len(),"duplicate_occurrence_observations":array(samples)?.len()-seen.len(),"custody_disagreements":if custody_known && seen.len()==expected.len(){json!(custodians.values().filter(|s|s.len()>1).count())}else{Value::Null},"retry_completed":if terminals_known && seen.len()==expected.len(){json!(retries.len())}else{Value::Null},"expired":if terminals_known && seen.len()==expected.len(){json!(expiries.len())}else{Value::Null},"unknown_reasons":{"custody_disagreements":if custody_known && seen.len()==expected.len(){Value::Null}else{json!("custody_observations_missing")},"terminal_counts":if terminals_known && seen.len()==expected.len(){Value::Null}else{json!("terminal_observations_missing")}}}});
            out["measurements"]["snapshot"] = p.clone();
            Ok(())
        })();
        if let Err(error) = attempt {
            rejected
                .push(json!({"source":v["raw_artifact"],"fatal":true,"reason":error.to_string()}));
        }
    }
    out["scenario_verdict"] = if rejected.iter().any(|v| v["fatal"] != false) {
        "invalid_input"
    } else if !failures.is_empty() {
        "product_failure"
    } else if !missing.is_empty() {
        "incomplete"
    } else {
        "passed"
    }
    .into();
    out["product_failures"] = json!(failures);
    out["missing_observations"] = json!(missing);
    out["rejected_observations"] = json!(rejected);
    out["raw_references"] = json!(raw);
    Ok(out)
}
