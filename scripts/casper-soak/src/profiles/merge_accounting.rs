use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use casper_soak::{array, artifact, file_hash, hash, manifest, object, parse, text};
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

pub const PROFILE: &str = "merge-accounting";
pub const CAPABILITIES: &[&str] = &[
    "execution-identities",
    "admission-outcomes",
    "settlement-observations",
    "causal-relationships",
];
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
    "node_id",
    "incarnation",
    "segment",
    "iteration",
    "seed",
    "policy_variant",
];
const COMPATIBILITY: &[&str] = &[
    "execution_schema",
    "protocol_epoch",
    "accounting_mode",
    "record_version",
    "token_domain",
];
const IDENTITY: &[&str] = &[
    "admission_id",
    "source_block_hash",
    "execution_position",
    "deploy_signature",
    "context_digest",
];
const FIELDS: &[&str] = &[
    "admission_result",
    "body_result",
    "effect_digest",
    "settlement",
];

pub fn identity() -> Value {
    let sources = [
        (
            "scripts/casper-soak/src/profiles/merge_accounting.rs",
            include_bytes!("merge_accounting.rs").as_slice(),
        ),
        (
            "scripts/casper-soak/src/bin/casper-merge-accounting.rs",
            include_bytes!("../bin/casper-merge-accounting.rs").as_slice(),
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
    json!({"profile_id":PROFILE,"profile_digest":digests["scripts/casper-soak/src/profiles/merge_accounting.rs"],"source_digests":digests,"version":env!("CARGO_PKG_VERSION")})
}
fn id(v: &Value) -> Result<&str> {
    let s = text(v)?;
    ensure!(
        !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control),
        "An identity is invalid."
    );
    Ok(s)
}
fn same(a: &Value, b: &Value, fields: &[&str]) -> bool {
    fields.iter().all(|key| a[*key] == b[*key])
}
fn measurement(v: &Value) -> Result<Option<&Value>> {
    match text(&v["presence"])? {
        "observed" => {
            ensure!(
                !v["value"].is_null() && v["reason"].is_null(),
                "The observed measurement is invalid."
            );
            Ok(Some(&v["value"]))
        }
        "missing" | "error" => {
            ensure!(v["value"].is_null(), "An unknown measurement must be null.");
            id(&v["reason"])?;
            Ok(None)
        }
        _ => Err(eyre!("The measurement presence is unsupported.")),
    }
}
fn key(v: &Value) -> Result<String> {
    id(&v["admission_id"])?;
    id(&v["deploy_signature"])?;
    manifest::hex(&v["source_block_hash"], 64)?;
    manifest::hex(&v["context_digest"], 64)?;
    if v["execution_position"].is_null() {
        Ok(json!(["admission", v["admission_id"]]).to_string())
    } else {
        manifest::decimal(&v["execution_position"])?;
        Ok(json!([v["source_block_hash"], v["execution_position"]]).to_string())
    }
}
fn settlement(v: &Value, token: &Value) -> Result<()> {
    ensure!(
        v["applied"].is_boolean() && &v["token_domain"] == token,
        "The settlement flag or token domain differs."
    );
    for field in ["prepayment", "charge", "refund", "pooled"] {
        manifest::decimal(&v[field])?;
    }
    Ok(())
}
fn aggregate(v: &Value, token: &Value) -> Result<()> {
    ensure!(
        &v["token_domain"] == token,
        "The aggregate token domain differs."
    );
    for field in [
        "balance_before",
        "balance_after",
        "charge",
        "refund",
        "pool_applications",
    ] {
        manifest::decimal(&v[field])?;
    }
    ensure!(
        ["checked", "overflow_rejected"].contains(&text(&v["arithmetic_outcome"])?),
        "The arithmetic outcome is unsupported."
    );
    Ok(())
}
fn field(name: &str, v: &Value, token: &Value) -> Result<()> {
    match name {
        "admission_result" => ensure!(
            ["admitted", "rejected"].contains(&text(v)?),
            "The admission outcome is unsupported."
        ),
        "body_result" => ensure!(
            ["success", "failure", "not_executed"].contains(&text(v)?),
            "The body outcome is unsupported."
        ),
        "effect_digest" => {
            if v != "none" {
                manifest::hex(v, 64)?;
            }
        }
        "settlement" => settlement(v, token)?,
        _ => return Err(eyre!("The measurement field is unsupported.")),
    }
    Ok(())
}
fn descriptors(v: &Value) -> Result<BTreeMap<String, Value>> {
    let rows = array(v)?;
    ensure!(rows.len() <= 64, "The execution fixture exceeds its bound.");
    let mut result = BTreeMap::new();
    let mut admissions = BTreeSet::new();
    for row in rows {
        let k = key(row)?;
        ensure!(
            admissions.insert(id(&row["admission_id"])?.to_owned())
                && result.insert(k, row.clone()).is_none(),
            "A fixture identity is duplicated."
        );
    }
    Ok(result)
}
fn edges(v: &Value, rows: &BTreeMap<String, Value>) -> Result<BTreeSet<String>> {
    let items = array(v)?;
    ensure!(items.len() <= 64, "The causal edge bound is exceeded.");
    let executions: BTreeSet<_> = rows
        .values()
        .filter(|v| !v["execution_position"].is_null())
        .map(|v| json!([v["source_block_hash"], v["execution_position"]]).to_string())
        .collect();
    let mut result = BTreeSet::new();
    for edge in items {
        manifest::hex(&edge["effect_digest"], 64)?;
        for endpoint in ["producer", "consumer"] {
            ensure!(
                executions.contains(&edge[endpoint].to_string()),
                "A causal endpoint is not a declared execution."
            );
        }
        ensure!(
            edge["producer"] != edge["consumer"] && result.insert(edge.to_string()),
            "A causal edge is duplicated or self-referential."
        );
    }
    Ok(result)
}
fn rejection_set(v: &Value, rows: &BTreeMap<String, Value>) -> Result<BTreeSet<String>> {
    let values = array(v)?;
    ensure!(
        values.len() <= 64,
        "The rejection inventory exceeds its bound."
    );
    let mut result = BTreeSet::new();
    for value in values {
        let k = value.to_string();
        ensure!(
            rows.get(&k)
                .is_some_and(|r| !r["execution_position"].is_null())
                && result.insert(k),
            "A rejected execution is unknown or duplicated."
        );
    }
    Ok(result)
}
fn request(r: &Value) -> Result<()> {
    ensure!(
        r["schema_version"] == 1 && r["profile_id"] == PROFILE,
        "The accounting request schema differs."
    );
    for key in CONTEXT.iter().chain(COMPATIBILITY) {
        id(&r[*key])?;
    }
    for key in ["manifest_digest", "node_binary_digest"] {
        manifest::hex(&r[key], 64)?;
    }
    manifest::hex(&r["node_revision"], 40)?;
    for key in ["segment", "iteration", "seed", "protocol_epoch"] {
        manifest::decimal(&r[key])?;
    }
    ensure!(
        ["legacy_max_union", "conditional_additive"].contains(&text(&r["accounting_mode"])?),
        "The accounting mode is unsupported."
    );
    ensure!(
        ["legacy", "exact"].contains(&text(&r["record_version"])?),
        "The record version is unsupported."
    );
    ensure!(
        (r["accounting_mode"] == "legacy_max_union" && r["policy_variant"] == "baseline")
            || (r["accounting_mode"] == "conditional_additive"
                && r["policy_variant"] == "conditional-additive"),
        "The accounting policy differs."
    );
    id(&r["observation_deadline"]["clock_id"])?;
    manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?;
    let rows = descriptors(&r["execution_fixture"])?;
    edges(&r["causal_edges"], &rows)?;
    let faults = array(&r["fault_schedule"])?;
    ensure!(faults.len() <= 8, "The fault schedule exceeds its bound.");
    let mut seen = BTreeSet::new();
    for fault in faults {
        let fault_id = id(&fault["fault_id"])?;
        ensure!(
            ["pause", "delay_delivery"].contains(&text(&fault["action"])?),
            "The fault action is unsupported."
        );
        let mut dependencies = BTreeSet::new();
        for dependency in array(&fault["depends_on"])? {
            let d = id(dependency)?;
            ensure!(
                seen.contains(d) && dependencies.insert(d),
                "A fault dependency is invalid."
            );
        }
        ensure!(seen.insert(fault_id), "A fault identity is duplicated.");
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
    if r["accounting_mode"] != "legacy_max_union"
        || r["record_version"] != "legacy"
        || r["policy_variant"] != "baseline"
    {
        reasons.push("additive_activation_unapproved".into());
    }
    for capability in CAPABILITIES.iter().copied().chain(
        (r["fault_schedule"]
            .as_array()
            .is_some_and(|v| !v.is_empty()))
        .then_some("accounting-fault-receipts"),
    ) {
        if r["capabilities"][capability]["status"] != "qualified" {
            reasons.push(format!("missing_capability:{capability}"));
        }
    }
    reasons
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
        "The request and manifest differ."
    );
    ensure!(
        same(r, m, COMPATIBILITY),
        "The manifest compatibility differs."
    );
    ensure!(
        array(&m["required_scenarios"])?.contains(&r["scenario_id"]),
        "The scenario is not declared."
    );
    let own = identity();
    ensure!(
        own["profile_digest"] == m["profile_digest"]
            && m["profile_binary_digest"] == file_hash(&std::env::current_exe()?)?,
        "The profile source or executable differs."
    );
    for (path, digest) in object(&own["source_digests"])? {
        ensure!(
            m["source_digests"][path] == *digest,
            "A compiled source differs."
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
            let proof = parse(&artifact(root, &cap["qualification"])?)?;
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
                    ])
                    && same(&proof, r, COMPATIBILITY),
                "The capability qualification differs."
            );
        }
    }
    ensure!(
        object(&r["inputs"])?.len() == 3,
        "The pinned input inventory differs."
    );
    for name in ["configuration", "fixture", "expectation"] {
        let reference = &r["inputs"][name];
        ensure!(
            reference["sha256"] == m[format!("{name}_digest")],
            "A pinned input differs."
        );
        p[name] = parse(&artifact(root, reference)?)?;
    }
    ensure!(
        same(&p["configuration"], r, COMPATIBILITY)
            && p["configuration"]["policy_variant"] == r["policy_variant"],
        "The pinned configuration differs."
    );
    ensure!(
        same(&p["fixture"], r, &[
            "execution_fixture",
            "causal_edges",
            "fault_schedule"
        ]),
        "The pinned fixture differs."
    );
    let rows = descriptors(&r["execution_fixture"])?;
    let expected = descriptors(&p["expectation"]["outcomes"])?;
    ensure!(
        rows.keys().eq(expected.keys()),
        "The expected execution inventory differs."
    );
    for (k, row) in &rows {
        let e = &expected[k];
        ensure!(
            same(row, e, IDENTITY),
            "An expected execution identity differs."
        );
        for name in FIELDS {
            field(name, &e[*name], &r["token_domain"])?;
        }
        ensure!(
            (e["admission_result"] == "rejected") == e["execution_position"].is_null(),
            "The expected admission identity is inconsistent."
        );
        if e["admission_result"] == "rejected" {
            ensure!(
                e["body_result"] == "not_executed"
                    && e["effect_digest"] == "none"
                    && e["settlement"]["applied"] == false,
                "An expected admission rejection has execution effects."
            );
        } else {
            ensure!(
                e["body_result"] != "not_executed",
                "An admitted execution needs a body outcome."
            );
        }
    }
    aggregate(&p["expectation"]["aggregate"], &r["token_domain"])?;
    rejection_set(&p["expectation"]["rejected_executions"], &rows)?;
    Ok(p)
}
pub fn generate(r: &Value) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    let ready = reasons.is_empty();
    Ok(
        json!({"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"scenario_verdict":if ready {"ready"} else {"blocked"},"blocked_reasons":reasons,"node_launch_count":0,"schedule":if ready {json!(["load_execution_fixture","apply_fault_schedule","capture_accounting"])} else {json!([])},"workloads":if ready {json!([{"operation":"load_execution_fixture","inputs":r["inputs"],"seed":r["seed"],"execution_fixture":r["execution_fixture"],"causal_edges":r["causal_edges"]},{"operation":"capture_accounting","deadline":r["observation_deadline"]}])} else {json!([])},"fault_requests":if ready {r["fault_schedule"].clone()} else {json!([])}}),
    )
}
pub fn collect(m: &Value, a: &Value) -> Result<Value> {
    let r = &a["request"];
    request(r)?;
    let root = Path::new(text(&a["root"])?);
    let references = array(&a["records"])?;
    ensure!(
        references.len() <= 64,
        "The transport inventory exceeds its bound."
    );
    let mut observations = Vec::new();
    let mut rejected = Vec::new();
    let mut duplicates = Vec::new();
    let mut records = BTreeSet::new();
    let mut events = BTreeMap::new();
    let mut sequences = BTreeMap::new();
    for reference in references {
        let attempt = (|| -> Result<()> {
            let mut v = parse(&artifact(root, reference)?)?;
            ensure!(v["schema_version"] == 1, "The observation schema differs.");
            for name in
                CONTEXT
                    .iter()
                    .copied()
                    .chain(["record_id", "event_id", "producer", "event_kind"])
            {
                id(&v[name])?;
            }
            ensure!(
                records.insert(id(&v["record_id"])?.to_owned()),
                "A transport identity is duplicated."
            );
            ensure!(
                reference["producer"] == v["producer"]
                    && array(&reference["observation_ids"])?.contains(&v["record_id"]),
                "The transport association differs."
            );
            let sequence = manifest::decimal(&v["producer_sequence"])?;
            let time = manifest::decimal(&v["time"]["monotonic_ns"])?;
            id(&v["time"]["clock_id"])?;
            id(&v["time"]["utc"])?;
            let kind = text(&v["event_kind"])?;
            ensure!(
                ["accounting_snapshot", "fault_ack"].contains(&kind),
                "The observation kind is unsupported."
            );
            let context: Vec<_> = CONTEXT.iter().map(|key| v[*key].clone()).collect();
            let producer = json!([context, v["producer"]]).to_string();
            let event = json!([producer, v["event_id"]]).to_string();
            let mut canonical = v.clone();
            canonical
                .as_object_mut()
                .ok_or_else(|| eyre!("The observation is not an object."))?
                .remove("record_id");
            if let Some(old) = events.get(&event) {
                ensure!(old == &canonical, "An event has conflicting copies.");
                duplicates.push(reference.clone());
                return Ok(());
            }
            events.insert(event, canonical);
            if !same(&v, r, CONTEXT)
                || !same(&v, m, &[
                    "manifest_digest",
                    "run_id",
                    "phase",
                    "evidence_kind",
                ])
                || v["time"]["clock_id"] != r["observation_deadline"]["clock_id"]
                || time > manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?
            {
                rejected.push(json!({"source":reference,"fatal":false,"reason":"The observation correlation or deadline differs."}));
                return Ok(());
            }
            measurement(
                &json!({"presence":v["presence"],"value":v["payload"],"reason":v["reason"]}),
            )?;
            ensure!(
                sequences.get(&producer).is_none_or(|s| *s < sequence),
                "The producer sequence is not increasing."
            );
            sequences.insert(producer, sequence);
            v["raw_artifact"] = reference.clone();
            observations.push(v);
            Ok(())
        })();
        if let Err(error) = attempt {
            rejected.push(json!({"source":reference,"fatal":true,"reason":error.to_string()}));
        }
    }
    Ok(
        json!({"observations":observations,"rejected_observations":rejected,"duplicate_sources":duplicates}),
    )
}
pub fn classify(r: &Value, c: &Value, acknowledgments: &[Value]) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    let mut out = json!({"schema_version":1,"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"run_id":r["run_id"],"scenario_id":r["scenario_id"],"evidence_kind":r["evidence_kind"],"node_launch_count":0,"harness_verification":"pending","soak_verdict":"non_passing","scenario_verdict":"blocked","blocked_reasons":reasons,"product_failures":[],"missing_observations":[],"rejected_observations":c["rejected_observations"],"measurements":{"counts":{"presence":"missing","value":null,"reason":"snapshot_unavailable"}},"coverage":{"acknowledged_faults":0,"required_faults":r["fault_schedule"].as_array().map(Vec::len)},"raw_references":[]});
    if !reasons.is_empty() {
        return Ok(out);
    }
    let mut failures = Vec::new();
    let mut missing = Vec::new();
    let mut rejected = array(&c["rejected_observations"])?.clone();
    let mut raw = Vec::new();
    let mut receipts: BTreeMap<String, Value> = BTreeMap::new();
    for fault in array(&r["fault_schedule"])? {
        let values: Vec<_> = acknowledgments
            .iter()
            .filter(|v| v["payload"]["fault_id"] == fault["fault_id"])
            .collect();
        if values.len() != 1 {
            missing.push(format!("fault:{}", id(&fault["fault_id"])?));
            continue;
        }
        let v = values[0];
        let p = &v["payload"];
        let valid = v["presence"] == "observed"
            && same(p, fault, &["fault_id", "action", "depends_on"])
            && p["status"] == "applied"
            && p["observed"] == true;
        if !p["observed"].is_boolean() {
            rejected.push(json!({"source":v["raw_artifact"],"fatal":true,"reason":"The applied flag must be Boolean."}));
        }
        let ordered = array(&fault["depends_on"])?.iter().all(|dependency| {
            receipts
                .get(dependency.as_str().unwrap_or(""))
                .is_some_and(|before| {
                    before["producer"] == v["producer"]
                        && before["time"]["clock_id"] == v["time"]["clock_id"]
                        && manifest::decimal(&before["producer_sequence"]).ok()
                            < manifest::decimal(&v["producer_sequence"]).ok()
                        && manifest::decimal(&before["time"]["monotonic_ns"]).ok()
                            <= manifest::decimal(&v["time"]["monotonic_ns"]).ok()
                })
        });
        if valid && ordered {
            receipts.insert(id(&fault["fault_id"])?.to_owned(), v.clone());
            raw.push(v["raw_artifact"].clone());
        } else {
            missing.push(format!("fault:{}", id(&fault["fault_id"])?));
        }
    }
    out["coverage"]["acknowledged_faults"] = receipts.len().into();
    let snapshots: Vec<_> = array(&c["observations"])?
        .iter()
        .filter(|v| v["event_kind"] == "accounting_snapshot")
        .collect();
    if snapshots.len() != 1 {
        missing.push("accounting_snapshot".to_owned());
    } else {
        let v = snapshots[0];
        let attempt = (|| -> Result<()> {
            if v["presence"] != "observed" {
                missing.push("accounting_snapshot".to_owned());
                return Ok(());
            }
            let p = &v["payload"];
            ensure!(
                same(p, r, COMPATIBILITY)
                    && p["fixture_digest"] == r["inputs"]["fixture"]["sha256"],
                "The snapshot compatibility or fixture differs."
            );
            if p["applied_steps"] != json!(["load_execution_fixture", "capture_accounting"]) {
                missing.push("applied_steps".to_owned());
            }
            for receipt in receipts.values() {
                if receipt["time"]["clock_id"] != v["time"]["clock_id"]
                    || manifest::decimal(&receipt["time"]["monotonic_ns"])?
                        > manifest::decimal(&v["time"]["monotonic_ns"])?
                {
                    missing.push("snapshot_before_fault".to_owned());
                }
            }
            let rows = descriptors(&r["execution_fixture"])?;
            let expected = descriptors(&r["expectation"]["outcomes"])?;
            raw.push(v["raw_artifact"].clone());
            for name in ["aggregate", "causal_edges", "rejected_executions"] {
                if let Some(value) = measurement(&p[name])? {
                    let equal = match name {
                        "aggregate" => {
                            aggregate(value, &r["token_domain"])?;
                            value == &r["expectation"][name]
                        }
                        "causal_edges" => edges(value, &rows)? == edges(&r[name], &rows)?,
                        _ => {
                            rejection_set(value, &rows)?
                                == rejection_set(&r["expectation"][name], &rows)?
                        }
                    };
                    if !equal {
                        failures.push(
                            json!({"kind":format!("{name}_mismatch"),"source":v["raw_artifact"]}),
                        );
                    }
                    out["measurements"][name] = p[name].clone();
                } else {
                    missing.push(name.to_owned());
                    out["measurements"][name] = p[name].clone();
                }
            }
            let Some(entries) = measurement(&p["entries"])? else {
                missing.push("execution_entries".to_owned());
                return Ok(());
            };
            let entries = array(entries)?;
            ensure!(
                entries.len() <= 64,
                "The execution observation bound is exceeded."
            );
            let mut seen = BTreeMap::new();
            let mut duplicates = 0usize;
            let mut rejected_duplicates = 0usize;
            for row in entries {
                let attempt = (|| -> Result<()> {
                    let k = key(row)?;
                    if let Some(old) = seen.get(&k) {
                        ensure!(old == row, "An execution has conflicting observations.");
                        if row["execution_position"].is_null() {
                            rejected_duplicates += 1;
                        } else {
                            duplicates += 1;
                        }
                        return Ok(());
                    }
                    seen.insert(k.clone(), row.clone());
                    let e = expected
                        .get(&k)
                        .ok_or_else(|| eyre!("An observed identity is outside the fixture."))?;
                    ensure!(
                        same(row, e, IDENTITY),
                        "The execution signature or context differs."
                    );
                    for name in FIELDS {
                        if let Some(value) = measurement(&row[*name])? {
                            field(name, value, &r["token_domain"])?;
                            if value != &e[*name] {
                                failures.push(json!({"kind":format!("{name}_mismatch"),"execution":k,"source":v["raw_artifact"]}));
                            }
                        } else {
                            missing.push(format!("{k}:{name}"));
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = attempt {
                    rejected.push(
                        json!({"source":v["raw_artifact"],"fatal":true,"reason":error.to_string()}),
                    );
                }
            }
            let absent: Vec<_> = rows.keys().filter(|k| !seen.contains_key(*k)).collect();
            if absent.is_empty() && seen.len() == rows.len() {
                out["measurements"]["counts"] = json!({"presence":"observed","value":{"executions":rows.values().filter(|v| !v["execution_position"].is_null()).count(),"admission_rejections":rows.values().filter(|v| v["execution_position"].is_null()).count(),"duplicate_execution_observations":duplicates,"duplicate_admission_observations":rejected_duplicates},"reason":null});
            } else {
                missing.push("execution_inventory".to_owned());
            }
            out["measurements"]["entries"] = p["entries"].clone();
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
