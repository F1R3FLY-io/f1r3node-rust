use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use casper_soak::{array, artifact, file_hash, hash, manifest, object, parse, text};
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

pub const PROFILE: &str = "slashing";
pub const CAPABILITIES: &[&str] = &[
    "slashing-delivery-receipts",
    "slashing-prestate",
    "slashing-epoch-authorization",
    "slashing-recovery",
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
    "evidence_schema",
    "protocol_version",
    "authorization_rule",
    "economic_neglect_slashing",
];
const IDENTITY: &[&str] = &[
    "evidence_id",
    "evidence_digest",
    "invalid_block_hash",
    "offender",
    "evidence_epoch",
    "stake",
    "validator",
    "sequence",
    "evidence_status",
    "origin",
];
const FIELDS: &[&str] = &[
    "authorization",
    "recovery_outcome",
    "offender_deduplicated",
    "invalid_hash_seed",
];
const FAMILIES: &[&str] = &[
    "delivery_permutation",
    "merge_lost_slash",
    "same_key_rebond",
    "stale_epoch",
    "missing_evidence",
    "forged_deploy",
    "duplicate_evidence",
    "restart_during_delivery",
];

pub fn identity() -> Value {
    let sources = [
        (
            "scripts/casper-soak/src/profiles/slashing.rs",
            include_bytes!("slashing.rs").as_slice(),
        ),
        (
            "scripts/casper-soak/src/bin/casper-slashing.rs",
            include_bytes!("../bin/casper-slashing.rs").as_slice(),
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
    json!({"profile_id":PROFILE,"profile_digest":digests["scripts/casper-soak/src/profiles/slashing.rs"],"source_digests":digests,"version":env!("CARGO_PKG_VERSION")})
}
fn id(v: &Value) -> Result<&str> {
    let s = text(v)?;
    ensure!(
        !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control),
        "An identity is invalid."
    );
    Ok(s)
}
fn same(a: &Value, b: &Value, fields: &[&str]) -> bool { fields.iter().all(|k| a[*k] == b[*k]) }
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
fn field(name: &str, v: &Value) -> Result<()> {
    match name {
        "authorization" | "offender_deduplicated" => ensure!(
            v.is_boolean(),
            "The authorization or deduplication flag must be Boolean."
        ),
        "recovery_outcome" => ensure!(
            ["not_required", "reissued", "rejected", "pending"].contains(&text(v)?),
            "The recovery outcome is unsupported."
        ),
        "invalid_hash_seed" => {
            manifest::hex(v, 64)?;
        }
        _ => return Err(eyre!("The measurement field is unsupported.")),
    }
    Ok(())
}
fn descriptors(v: &Value) -> Result<BTreeMap<String, Value>> {
    let rows = array(v)?;
    ensure!(
        !rows.is_empty() && rows.len() <= 16,
        "The evidence fixture bound is invalid."
    );
    let mut result = BTreeMap::new();
    for row in rows {
        let key = id(&row["evidence_id"])?;
        for name in ["offender", "validator"] {
            id(&row[name])?;
        }
        for name in ["evidence_digest", "invalid_block_hash"] {
            manifest::hex(&row[name], 64)?;
        }
        for name in ["evidence_epoch", "stake", "sequence"] {
            manifest::decimal(&row[name])?;
        }
        ensure!(
            ["current", "missing", "forged", "stale"].contains(&text(&row["evidence_status"])?),
            "The evidence status is unsupported."
        );
        ensure!(
            ["invalid_latest_message", "merge_rejected_slash"].contains(&text(&row["origin"])?),
            "The evidence origin is unsupported."
        );
        ensure!(
            result.insert(key.to_owned(), row.clone()).is_none(),
            "An evidence identity is duplicated."
        );
    }
    Ok(result)
}
fn request(r: &Value) -> Result<()> {
    ensure!(
        r["schema_version"] == 1 && r["profile_id"] == PROFILE,
        "The slashing request schema differs."
    );
    for name in CONTEXT {
        id(&r[*name])?;
    }
    for name in ["evidence_schema", "protocol_version", "authorization_rule"] {
        id(&r[name])?;
    }
    ensure!(
        r["economic_neglect_slashing"].is_boolean(),
        "The neglect flag must be Boolean."
    );
    for name in [
        "manifest_digest",
        "node_binary_digest",
        "parent_prestate_digest",
    ] {
        manifest::hex(&r[name], 64)?;
    }
    manifest::hex(&r["node_revision"], 40)?;
    for name in [
        "segment",
        "iteration",
        "seed",
        "epoch",
        "rebond_epoch",
        "protocol_version",
    ] {
        manifest::decimal(&r[name])?;
    }
    ensure!(
        manifest::decimal(&r["rebond_epoch"])? <= manifest::decimal(&r["epoch"])?,
        "The rebond epoch is later than the observed epoch."
    );
    ensure!(
        FAMILIES.contains(&text(&r["scenario_family"])?),
        "The scenario family is unsupported."
    );
    id(&r["observation_deadline"]["clock_id"])?;
    manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?;
    let rows = descriptors(&r["evidence_fixture"])?;
    for row in rows.values() {
        manifest::hex(&row["invalid_hash_seed"], 64)?;
    }
    let steps = array(&r["schedule"])?;
    ensure!(
        !steps.is_empty() && steps.len() <= 24,
        "The schedule bound is invalid."
    );
    let mut seen = BTreeSet::new();
    let mut delivered = BTreeSet::new();
    let mut incarnation = r["incarnation"].clone();
    let mut incarnations = BTreeSet::from([id(&incarnation)?.to_owned()]);
    for step in steps {
        let step_id = id(&step["step_id"])?;
        let mut dependencies = BTreeSet::new();
        for dependency in array(&step["depends_on"])? {
            let d = id(dependency)?;
            ensure!(
                seen.contains(d) && dependencies.insert(d),
                "A schedule dependency is invalid."
            );
        }
        match text(&step["action"])? {
            "deliver_evidence" => {
                let evidence = id(&step["evidence_id"])?;
                ensure!(
                    rows.contains_key(evidence) && step["incarnation"] == incarnation,
                    "The delivery target differs."
                );
                delivered.insert(evidence.to_owned());
            }
            "restart" => {
                let next = id(&step["to_incarnation"])?;
                ensure!(
                    step["from_incarnation"] == incarnation && incarnations.insert(next.to_owned()),
                    "The restart identity differs."
                );
                incarnation = step["to_incarnation"].clone();
            }
            "rebond" => {
                id(&step["offender"])?;
                let before = manifest::decimal(&step["from_epoch"])?;
                let after = manifest::decimal(&step["to_epoch"])?;
                ensure!(
                    before < after
                        && step["to_epoch"] == r["rebond_epoch"]
                        && rows.values().any(|v| v["offender"] == step["offender"]),
                    "The rebond identity differs."
                );
            }
            "pause" | "delay_delivery" => {
                ensure!(
                    step["incarnation"] == incarnation,
                    "The fault target differs."
                );
            }
            _ => return Err(eyre!("The schedule action is unsupported.")),
        }
        ensure!(seen.insert(step_id), "A schedule identity is duplicated.");
    }
    ensure!(
        delivered.iter().eq(rows.keys()),
        "The scheduled evidence inventory differs."
    );
    let covered = match text(&r["scenario_family"])? {
        "merge_lost_slash" => rows.values().any(|v| v["origin"] == "merge_rejected_slash"),
        "same_key_rebond" => steps.iter().any(|s| s["action"] == "rebond"),
        "restart_during_delivery" => steps.iter().enumerate().any(|(i, s)| {
            s["action"] == "restart"
                && steps[..i].iter().any(|s| s["action"] == "deliver_evidence")
                && steps[i + 1..]
                    .iter()
                    .any(|s| s["action"] == "deliver_evidence")
        }),
        "stale_epoch" => rows
            .values()
            .any(|v| v["evidence_status"] == "stale" && v["evidence_epoch"] != r["epoch"]),
        "missing_evidence" => rows.values().any(|v| v["evidence_status"] == "missing"),
        "forged_deploy" => rows.values().any(|v| v["evidence_status"] == "forged"),
        "duplicate_evidence" => {
            steps
                .iter()
                .filter(|s| s["action"] == "deliver_evidence")
                .count()
                > delivered.len()
        }
        _ => delivered.len() >= 2,
    };
    ensure!(covered, "The fixture does not cover its scenario family.");
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
    if r["policy_variant"] != "baseline"
        || r["authorization_rule"] != "dev_rejected_slash_loop"
        || r["economic_neglect_slashing"] != false
        || r["protocol_version"] != "6"
        || r["evidence_schema"] != "synthetic-dev-unary-v1"
    {
        reasons.push("protocol_activation_unapproved".into());
    }
    for capability in
        CAPABILITIES
            .iter()
            .copied()
            .chain(array(&r["schedule"]).into_iter().flatten().filter_map(|s| {
                match s["action"].as_str() {
                    Some("restart") => Some("slashing-linked-restart"),
                    Some("rebond") => Some("slashing-rebond-receipts"),
                    _ => None,
                }
            }))
    {
        if r["capabilities"][capability]["status"] != "qualified" {
            reasons.push(format!("missing_capability:{capability}"));
        }
    }
    reasons.sort();
    reasons.dedup();
    reasons
}
pub fn prepare(m: &Value, r: &Value, root: &Path) -> Result<Value> {
    manifest::validate(m)?;
    request(r)?;
    ensure!(
        r["observation_deadline"] == m["deadline"],
        "The observation deadline differs from the manifest."
    );
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
        ]) && same(r, m, COMPATIBILITY),
        "The request and manifest differ."
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
            "evidence_fixture",
            "schedule",
            "parent_prestate_digest",
            "epoch",
            "rebond_epoch",
            "scenario_family"
        ]),
        "The pinned fixture differs."
    );
    let rows = descriptors(&r["evidence_fixture"])?;
    let expected = descriptors(&p["expectation"]["outcomes"])?;
    ensure!(
        rows.keys().eq(expected.keys()),
        "The expected evidence inventory differs."
    );
    for (key, row) in &rows {
        let e = &expected[key];
        ensure!(
            same(row, e, IDENTITY) && row["invalid_hash_seed"] == e["invalid_hash_seed"],
            "An expected evidence identity differs."
        );
        for name in FIELDS {
            field(name, &e[*name])?;
        }
        if e["authorization"] == true {
            ensure!(
                manifest::decimal(&row["stake"])? > 0
                    && row["evidence_epoch"] == r["epoch"]
                    && manifest::decimal(&row["evidence_epoch"])?
                        >= manifest::decimal(&r["rebond_epoch"])?
                    && row["evidence_status"] == "current",
                "The expected authorization violates fixture prerequisites."
            );
        }
    }
    Ok(p)
}
pub fn generate(r: &Value) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    let ready = reasons.is_empty();
    Ok(
        json!({"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"scenario_verdict":if ready {"ready"} else {"blocked"},"blocked_reasons":reasons,"node_launch_count":0,"schedule":if ready {r["schedule"].clone()} else {json!([])},"workloads":if ready {json!([{"operation":"load_evidence_fixture","inputs":r["inputs"],"seed":r["seed"],"evidence_fixture":r["evidence_fixture"]},{"operation":"capture_authorization","deadline":r["observation_deadline"]}])} else {json!([])}}),
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
            manifest::decimal(&v["epoch"])?;
            manifest::decimal(&v["rebond_epoch"])?;
            manifest::hex(&v["parent_prestate_digest"], 64)?;
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
            ensure!(
                ["slashing_snapshot", "step_ack"].contains(&text(&v["event_kind"])?),
                "The observation kind is unsupported."
            );
            let context: Vec<_> = CONTEXT.iter().map(|k| v[*k].clone()).collect();
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
                || !same(&v, r, &["epoch", "rebond_epoch", "parent_prestate_digest"])
                || !same(&v, m, &[
                    "manifest_digest",
                    "run_id",
                    "phase",
                    "evidence_kind",
                ])
                || v["time"]["clock_id"] != r["observation_deadline"]["clock_id"]
                || time > manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?
            {
                rejected.push(json!({"source":reference,"fatal":false,"reason":"The observation epoch, pre-state, correlation, or deadline differs."}));
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
fn before(a: &Value, b: &Value) -> Result<bool> {
    Ok(a["producer"] == b["producer"]
        && a["time"]["clock_id"] == b["time"]["clock_id"]
        && manifest::decimal(&a["producer_sequence"])?
            < manifest::decimal(&b["producer_sequence"])?
        && manifest::decimal(&a["time"]["monotonic_ns"])?
            <= manifest::decimal(&b["time"]["monotonic_ns"])?)
}
pub fn classify(r: &Value, c: &Value, acknowledgments: &[Value]) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    let mut out = json!({"schema_version":1,"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"run_id":r["run_id"],"scenario_id":r["scenario_id"],"evidence_kind":r["evidence_kind"],"node_launch_count":0,"harness_verification":"pending","soak_verdict":"non_passing","scenario_verdict":"blocked","blocked_reasons":reasons,"product_failures":[],"missing_observations":[],"rejected_observations":c["rejected_observations"],"measurements":{"counts":{"presence":"missing","value":null,"reason":"snapshot_unavailable"},"delivery_order":{"presence":"missing","value":null,"reason":"receipts_unavailable"}},"coverage":{"acknowledged_steps":0,"required_steps":r["schedule"].as_array().map(Vec::len)},"raw_references":[]});
    if !reasons.is_empty() {
        return Ok(out);
    }
    let observations = array(&c["observations"])?;
    ensure!(
        acknowledgments
            .iter()
            .all(|v| v["event_kind"] == "step_ack" && observations.contains(v)),
        "An acknowledgment is outside the collection."
    );
    let mut failures = Vec::new();
    let mut missing = Vec::new();
    let mut rejected = array(&c["rejected_observations"])?.clone();
    let mut raw = Vec::new();
    let rows = descriptors(&r["evidence_fixture"])?;
    let steps = array(&r["schedule"])?;
    let mut receipts: BTreeMap<String, Value> = BTreeMap::new();
    let mut deliveries = Vec::new();
    for step in steps {
        let values: Vec<_> = acknowledgments
            .iter()
            .filter(|v| v["payload"]["step_id"] == step["step_id"])
            .collect();
        if values.len() != 1 {
            missing.push(format!("step:{}", id(&step["step_id"])?));
            continue;
        }
        let v = values[0];
        let p = &v["payload"];
        raw.push(v["raw_artifact"].clone());
        let attempt = (|| -> Result<bool> {
            if v["presence"] != "observed" {
                return Ok(false);
            }
            ensure!(
                p["observed"].is_boolean(),
                "The applied flag must be Boolean."
            );
            if p["status"] != "applied"
                || p["observed"] != true
                || !object(step)?.iter().all(|(k, value)| &p[k] == value)
            {
                return Ok(false);
            }
            match text(&step["action"])? {
                "deliver_evidence" => {
                    let e = &rows[id(&step["evidence_id"])?];
                    if p["evidence_digest"] != e["evidence_digest"] {
                        return Ok(false);
                    }
                    deliveries.push(v.clone());
                }
                "restart" => {
                    ensure!(
                        p["exit_observed"].is_boolean() && p["restart_ready"].is_boolean(),
                        "The restart flags must be Boolean."
                    );
                    ensure!(
                        p["exit_code"]
                            .as_i64()
                            .is_some_and(|n| i32::try_from(n).is_ok()),
                        "The exit code is invalid."
                    );
                    if p["exit_observed"] != true || p["restart_ready"] != true {
                        return Ok(false);
                    }
                }
                "rebond" => {
                    ensure!(
                        p["rebond_observed"].is_boolean(),
                        "The rebond flag must be Boolean."
                    );
                    if p["rebond_observed"] != true {
                        return Ok(false);
                    }
                }
                _ => (),
            }
            for dependency in array(&step["depends_on"])? {
                let Some(prior) = receipts.get(id(dependency)?) else {
                    return Ok(false);
                };
                if !before(prior, v)? {
                    return Ok(false);
                }
            }
            Ok(true)
        })();
        match attempt {
            Ok(true) => {
                receipts.insert(id(&step["step_id"])?.to_owned(), v.clone());
            }
            Ok(false) => missing.push(format!("step:{}", id(&step["step_id"])?)),
            Err(error) => {
                rejected.push(
                    json!({"source":v["raw_artifact"],"fatal":true,"reason":error.to_string()}),
                );
                missing.push(format!("step:{}", id(&step["step_id"])?));
            }
        }
    }
    for v in acknowledgments {
        if !steps
            .iter()
            .any(|s| s["step_id"] == v["payload"]["step_id"])
        {
            rejected.push(json!({"source":v["raw_artifact"],"fatal":true,"reason":"An acknowledgment has an unknown step."}));
        }
    }
    out["coverage"]["acknowledged_steps"] = receipts.len().into();
    let requested: Vec<_> = steps
        .iter()
        .filter(|s| s["action"] == "deliver_evidence")
        .map(|s| s["step_id"].clone())
        .collect();
    out["requested_delivery_order"] = json!(requested);
    deliveries.sort_by_key(|v| manifest::decimal(&v["producer_sequence"]).unwrap_or(0));
    let comparable = deliveries
        .windows(2)
        .all(|w| before(&w[0], &w[1]).unwrap_or(false));
    out["delivery_receipts"] = json!(deliveries);
    if deliveries.len() == requested.len() && comparable {
        let actual: Vec<_> = deliveries
            .iter()
            .map(|v| v["payload"]["step_id"].clone())
            .collect();
        out["measurements"]["delivery_order"] =
            json!({"presence":"observed","value":actual,"reason":null});
        if actual != requested {
            missing.push("requested_delivery_order".to_owned());
        }
    } else {
        missing.push("delivery_order".to_owned());
    }
    let snapshots: Vec<_> = observations
        .iter()
        .filter(|v| v["event_kind"] == "slashing_snapshot")
        .collect();
    if snapshots.len() != 1 {
        missing.push("slashing_snapshot".to_owned());
    }
    for v in snapshots {
        raw.push(v["raw_artifact"].clone());
        let attempt = (|| -> Result<()> {
            if v["presence"] != "observed" {
                missing.push("slashing_snapshot".to_owned());
                return Ok(());
            }
            let p = &v["payload"];
            ensure!(
                same(p, r, COMPATIBILITY)
                    && p["fixture_digest"] == r["inputs"]["fixture"]["sha256"],
                "The snapshot compatibility or fixture differs."
            );
            if p["applied_steps"] != json!(["load_evidence_fixture", "capture_authorization"]) {
                missing.push("applied_steps".to_owned());
            }
            let target = steps
                .iter()
                .rev()
                .find(|s| s["action"] == "restart")
                .map_or(&r["incarnation"], |s| &s["to_incarnation"]);
            ensure!(
                &p["target_incarnation"] == target,
                "The snapshot target incarnation differs."
            );
            for receipt in receipts.values() {
                if !before(receipt, v)? {
                    missing.push("snapshot_before_step".to_owned());
                }
            }
            let Some(entries) = measurement(&p["entries"])? else {
                missing.push("evidence_entries".to_owned());
                return Ok(());
            };
            let entries = array(entries)?;
            ensure!(
                entries.len() <= 32,
                "The evidence observation bound is exceeded."
            );
            let expected = descriptors(&r["expectation"]["outcomes"])?;
            let mut seen = BTreeMap::new();
            let mut counts_valid = true;
            let mut duplicates = 0usize;
            for row in entries {
                let attempt = (|| -> Result<()> {
                    let k = id(&row["evidence_id"])?;
                    if let Some(old) = seen.get(k) {
                        ensure!(
                            old == row,
                            "An evidence identity has conflicting observations."
                        );
                        duplicates += 1;
                        return Ok(());
                    }
                    seen.insert(k.to_owned(), row.clone());
                    let e = expected.get(k).ok_or_else(|| {
                        eyre!("An observed evidence identity is outside the fixture.")
                    })?;
                    ensure!(
                        same(row, e, IDENTITY),
                        "The observed evidence identity differs."
                    );
                    for name in FIELDS {
                        let attempt = (|| -> Result<()> {
                            if let Some(value) = measurement(&row[*name])? {
                                field(name, value)?;
                                if value != &e[*name] {
                                    failures.push(json!({"kind":format!("{name}_mismatch"),"evidence_id":k,"expected":e[*name],"observed":value,"source":v["raw_artifact"]}));
                                }
                            } else {
                                missing.push(format!("{k}:{name}"));
                            }
                            Ok(())
                        })();
                        if let Err(error) = attempt {
                            rejected.push(json!({"source":v["raw_artifact"],"fatal":true,"reason":error.to_string()}));
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = attempt {
                    counts_valid = false;
                    rejected.push(
                        json!({"source":v["raw_artifact"],"fatal":true,"reason":error.to_string()}),
                    );
                }
            }
            if counts_valid
                && seen.keys().eq(rows.keys())
                && observations
                    .iter()
                    .filter(|v| v["event_kind"] == "slashing_snapshot")
                    .count()
                    == 1
            {
                let offenders: BTreeSet<_> =
                    seen.values().map(|v| v["offender"].to_string()).collect();
                out["measurements"]["counts"] = json!({"presence":"observed","value":{"evidence":seen.len(),"offenders":offenders.len(),"duplicate_observations":duplicates},"reason":null});
            } else {
                missing.push("evidence_inventory".to_owned());
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
