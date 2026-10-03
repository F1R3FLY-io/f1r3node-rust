use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use casper_soak::{array, artifact, file_hash, hash, manifest, object, parse, text};
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

pub const PROFILE: &str = "carrier-index";
pub const CAPABILITIES: &[&str] = &[
    "carrier-path-selection",
    "carrier-path-receipts",
    "carrier-work-counters",
    "carrier-availability-controls",
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
    "segment",
    "iteration",
    "seed",
    "policy_variant",
];
const MATCHED: &[&str] = &[
    "dag_digest",
    "deploy_signature",
    "scan_window",
    "availability_digest",
    "watermark",
    "retention_boundary",
    "identity_domain",
    "carrier_case",
];
const MEMBER: &[&str] = &[
    "member_id",
    "node_id",
    "incarnation",
    "previous_incarnation",
];
const COUNTERS: &[&str] = &["probe_count", "ancestor_body_read_count"];

pub fn identity() -> Value {
    let sources = [
        (
            "scripts/casper-soak/src/profiles/carrier_index.rs",
            include_bytes!("carrier_index.rs").as_slice(),
        ),
        (
            "scripts/casper-soak/src/bin/casper-carrier-index.rs",
            include_bytes!("../bin/casper-carrier-index.rs").as_slice(),
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
    json!({"profile_id":PROFILE,"profile_digest":digests["scripts/casper-soak/src/profiles/carrier_index.rs"],"source_digests":digests,"version":env!("CARGO_PKG_VERSION")})
}
fn id(v: &Value) -> Result<&str> {
    let s = text(v)?;
    ensure!(
        !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control),
        "An identity is invalid."
    );
    Ok(s)
}
fn same(a: &Value, b: &Value, fields: &[&str]) -> bool {
    fields
        .iter()
        .all(|key| a.get(*key).is_some() && a[*key] == b[*key])
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
fn observed_field<'a>(name: &str, v: &'a Value) -> Result<Option<&'a Value>> {
    let value = measurement(v)?;
    if let Some(value) = value {
        match name {
            "engaged_path" => ensure!(
                ["index", "reference"].contains(&text(value)?),
                "The engaged path is unsupported."
            ),
            "probe_count" | "ancestor_body_read_count" => {
                manifest::decimal(value)?;
            }
            "result" => {
                result_key(value)?;
            }
            "fallback_reason" => {
                id(value)?;
            }
            _ => return Err(eyre!("The measurement field is unsupported.")),
        }
    }
    Ok(value)
}
fn unknown(reason: &str) -> Value { json!({"presence":"missing","value":null,"reason":reason}) }
fn member<'a>(r: &'a Value, name: &Value) -> Result<&'a Value> {
    array(&r["members"])?
        .iter()
        .find(|m| m["member_id"] == *name)
        .ok_or_else(|| eyre!("The member is not declared."))
}
fn result_key(v: &Value) -> Result<Value> {
    ensure!(
        ["repeat", "fresh", "unavailable"].contains(&text(&v["verdict"])?),
        "The carrier verdict is unsupported."
    );
    let carriers = array(&v["carriers"])?;
    ensure!(
        carriers.len() <= 64,
        "The carrier inventory exceeds its bound."
    );
    let mut rows = BTreeMap::new();
    for row in carriers {
        manifest::hex(&row["block_hash"], 64)?;
        ensure!(
            ["valid", "invalid", "approved"].contains(&text(&row["status"])?),
            "The carrier status is unsupported."
        );
        ensure!(
            rows.insert(text(&row["block_hash"])?, text(&row["status"])?)
                .is_none(),
            "A carrier block is duplicated."
        );
    }
    Ok(json!({"verdict":v["verdict"],"carriers":rows}))
}
fn request(r: &Value) -> Result<()> {
    ensure!(
        r["schema_version"] == 1 && r["profile_id"] == PROFILE,
        "The carrier request schema differs."
    );
    for field in CONTEXT {
        id(&r[*field])?;
    }
    for field in [
        "manifest_digest",
        "node_binary_digest",
        "dag_digest",
        "availability_digest",
    ] {
        manifest::hex(&r[field], 64)?;
    }
    manifest::hex(&r["node_revision"], 40)?;
    for field in [
        "segment",
        "iteration",
        "seed",
        "watermark",
        "retention_boundary",
    ] {
        manifest::decimal(&r[field])?;
    }
    let low = manifest::decimal(&r["scan_window"]["lower"])?;
    let high = manifest::decimal(&r["scan_window"]["upper"])?;
    ensure!(low <= high, "The scan window is reversed.");
    let sig = id(&r["deploy_signature"])?;
    ensure!(sig.len() % 2 == 0, "The signature encoding is invalid.");
    manifest::hex(&r["deploy_signature"], sig.len())?;
    ensure!(
        ["raw_user_deploy_signature", "fip_typed_envelope"].contains(&text(&r["identity_domain"])?),
        "The identity domain is unsupported."
    );
    ensure!(
        [
            "valid",
            "invalid",
            "approved",
            "fork",
            "missing_history",
            "read_failure",
            "restart",
            "watermark_boundary",
            "retention_boundary",
            "identity_domain"
        ]
        .contains(&text(&r["carrier_case"])?),
        "The carrier case is unsupported."
    );
    id(&r["observation_deadline"]["clock_id"])?;
    manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?;
    let members = array(&r["members"])?;
    ensure!(
        members.len() == 2,
        "Exactly two paired members are required."
    );
    let mut names = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for m in members {
        for field in ["member_id", "node_id", "incarnation"] {
            id(&m[field])?;
        }
        ensure!(
            m.get("previous_incarnation").is_some(),
            "The predecessor field is absent."
        );
        if !m["previous_incarnation"].is_null() {
            id(&m["previous_incarnation"])?;
            ensure!(
                m["previous_incarnation"] != m["incarnation"],
                "Restart must change incarnation."
            );
        }
        ensure!(
            names.insert(text(&m["member_id"])?),
            "A member identity is duplicated."
        );
        let path = text(&m["requested_path"])?;
        ensure!(
            ["index", "reference"].contains(&path) && paths.insert(path),
            "The traversal paths must differ."
        );
        ensure!(
            same(m, r, MATCHED)
                && same(m, r, &[
                    "candidate_id",
                    "node_revision",
                    "node_binary_digest"
                ]),
            "Paired carrier inputs differ."
        );
    }
    let faults = array(&r["fault_schedule"])?;
    ensure!(faults.len() <= 8, "The fault schedule exceeds its bound.");
    let mut seen = BTreeSet::new();
    for f in faults {
        let m = member(r, &f["member_id"])?;
        id(&f["fault_id"])?;
        id(&f["trigger_event"])?;
        ensure!(
            f["node_id"] == m["node_id"] && f["incarnation"] == m["incarnation"],
            "The fault target differs."
        );
        ensure!(
            [
                "read_failure",
                "availability",
                "watermark",
                "prune",
                "restart"
            ]
            .contains(&text(&f["action"])?),
            "The fault action is unsupported."
        );
        if f["action"] == "restart" {
            ensure!(
                !m["previous_incarnation"].is_null()
                    && f["previous_incarnation"] == m["previous_incarnation"],
                "The restart predecessor differs."
            );
        }
        let mut dependencies = BTreeSet::new();
        for dependency in array(&f["depends_on"])? {
            let d = id(dependency)?;
            ensure!(
                seen.contains(d) && dependencies.insert(d),
                "A fault dependency is invalid."
            );
        }
        ensure!(
            seen.insert(text(&f["fault_id"])?),
            "A fault identity is duplicated."
        );
    }
    for m in members {
        for action in ["restart", "read_failure"] {
            if r["carrier_case"] == action
                || (action == "restart" && !m["previous_incarnation"].is_null())
            {
                ensure!(
                    faults
                        .iter()
                        .any(|f| f["member_id"] == m["member_id"] && f["action"] == action),
                    "The scenario requires a fault for each member."
                );
            }
        }
    }
    Ok(())
}
fn blocked(r: &Value) -> Vec<String> {
    let mut reasons = Vec::new();
    for (condition, reason) in [
        (
            r["evidence_kind"] != "synthetic_fixture",
            "live_adapter_unqualified",
        ),
        (
            r["phase"] != "pre_pr216_merge",
            "post_merge_adapter_unqualified",
        ),
        (
            r["policy_variant"] != "baseline",
            "policy_activation_unapproved",
        ),
        (
            r["identity_domain"] != "raw_user_deploy_signature",
            "typed_identity_interface_unqualified",
        ),
    ] {
        if condition {
            reasons.push(reason.to_owned());
        }
    }
    for cap in CAPABILITIES.iter().copied().chain(
        (!r["fault_schedule"].as_array().is_none_or(Vec::is_empty))
            .then_some("carrier-fault-receipts"),
    ) {
        if r["capabilities"][cap]["status"] != "qualified" {
            reasons.push(format!("missing_capability:{cap}"));
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
            "profile_id",
            "identity_domain"
        ]),
        "The request and manifest differ."
    );
    ensure!(
        array(&m["required_scenarios"])?.contains(&r["scenario_id"]),
        "The scenario is not declared."
    );
    ensure!(
        r["observation_deadline"]["clock_id"] == m["deadline"]["clock_id"]
            && manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?
                <= manifest::decimal(&m["deadline"]["monotonic_ns"])?,
        "The request deadline exceeds its manifest."
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
                        "profile_binary_digest",
                        "identity_domain"
                    ]),
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
        same(&p["configuration"], r, MATCHED) && same(&p["configuration"], r, &["policy_variant"]),
        "The pinned configuration differs."
    );
    ensure!(
        same(&p["fixture"], r, &["members", "fault_schedule"]),
        "The pinned fixture differs."
    );
    let outcomes = object(&p["expectation"]["members"])?;
    ensure!(
        outcomes.len() == 2,
        "The expected member inventory differs."
    );
    let mut expected_key = None;
    for m in array(&r["members"])? {
        let expected = outcomes
            .get(text(&m["member_id"])?)
            .ok_or_else(|| eyre!("An expected member is absent."))?;
        let key = result_key(&expected["result"])?;
        ensure!(
            expected_key.as_ref().is_none_or(|old| old == &key),
            "Paired result expectations differ."
        );
        expected_key = Some(key);
        id(&expected["fallback_reason"])?;
    }
    Ok(p)
}
pub fn generate(r: &Value) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    let ready = reasons.is_empty();
    let mut workloads = Vec::new();
    if ready {
        for m in array(&r["members"])? {
            workloads.push(json!({"operation":"load_carrier_fixture","member":m,"inputs":r["inputs"],"seed":r["seed"]}));
            workloads.push(json!({"operation":"select_traversal","member_id":m["member_id"],"requested_path":m["requested_path"],"deadline":r["observation_deadline"]}));
        }
    }
    Ok(
        json!({"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"scenario_verdict":if ready {"ready"} else {"blocked"},"blocked_reasons":reasons,"node_launch_count":0,"workloads":workloads,"fault_requests":if ready {r["fault_schedule"].clone()} else {json!([])}}),
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
            for field in CONTEXT.iter().copied().chain([
                "member_id",
                "node_id",
                "incarnation",
                "record_id",
                "event_id",
                "producer",
                "event_kind",
            ]) {
                id(&v[field])?;
            }
            ensure!(
                records.insert(id(&v["record_id"])?.to_owned()),
                "A transport identity is duplicated."
            );
            ensure!(
                reference["capture_state"] == "captured"
                    && reference["producer"] == v["producer"]
                    && reference["observation_ids"] == json!([v["record_id"]]),
                "The transport association differs."
            );
            let sequence = manifest::decimal(&v["producer_sequence"])?;
            let time = manifest::decimal(&v["time"]["monotonic_ns"])?;
            id(&v["time"]["clock_id"])?;
            id(&v["time"]["utc"])?;
            ensure!(
                ["carrier_snapshot", "fault_ack"].contains(&text(&v["event_kind"])?),
                "The observation kind is unsupported."
            );
            let context: Vec<_> = CONTEXT
                .iter()
                .chain(MEMBER.iter().filter(|key| **key != "previous_incarnation"))
                .map(|key| v[*key].clone())
                .collect();
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
            let paired = member(r, &v["member_id"]);
            if !same(&v, r, CONTEXT)
                || !same(&v, m, &[
                    "manifest_digest",
                    "run_id",
                    "phase",
                    "evidence_kind",
                ])
                || !paired.as_ref().is_ok_and(|member| same(&v, member, MEMBER))
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
    let mut out = json!({"schema_version":1,"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"run_id":r["run_id"],"scenario_id":r["scenario_id"],"pair_id":r["pair_id"],"evidence_kind":r["evidence_kind"],"node_launch_count":0,"harness_verification":"pending","soak_verdict":"non_passing","scenario_verdict":"blocked","blocked_reasons":reasons,"product_failures":[],"missing_observations":[],"rejected_observations":c["rejected_observations"],"measurements":{},"coverage":{"required_members":2,"observed_members":0,"required_faults":r["fault_schedule"].as_array().map(Vec::len),"acknowledged_faults":0},"fault_receipts":[],"raw_references":[]});
    if !reasons.is_empty() {
        return Ok(out);
    }
    let mut failures = Vec::new();
    let mut missing = Vec::new();
    let mut rejected = array(&c["rejected_observations"])?.clone();
    let mut receipts: BTreeMap<String, Value> = BTreeMap::new();
    let observations = array(&c["observations"])?;
    for f in array(&r["fault_schedule"])? {
        let values: Vec<_> = acknowledgments
            .iter()
            .filter(|v| {
                v["payload"]["fault_id"] == f["fault_id"] && v["member_id"] == f["member_id"]
            })
            .collect();
        let valid = if values.len() == 1 {
            let v = values[0];
            let p = &v["payload"];
            let state = if f["action"] == "restart" {
                p["previous_incarnation"] == f["previous_incarnation"]
                    && p["previous_exited"] == true
                    && p["ready"] == true
            } else {
                p["observed_state"] == format!("{}_applied", text(&f["action"])?)
            };
            let ordered = array(&f["depends_on"])?.iter().all(|d| {
                receipts
                    .get(d.as_str().unwrap_or(""))
                    .is_some_and(|before| {
                        before["producer"] == v["producer"]
                            && before["time"]["clock_id"] == v["time"]["clock_id"]
                            && manifest::decimal(&before["producer_sequence"]).ok()
                                < manifest::decimal(&v["producer_sequence"]).ok()
                            && manifest::decimal(&before["time"]["monotonic_ns"]).ok()
                                <= manifest::decimal(&v["time"]["monotonic_ns"]).ok()
                    })
            });
            observations.contains(v)
                && v["event_kind"] == "fault_ack"
                && v["presence"] == "observed"
                && same(p, f, &[
                    "fault_id",
                    "member_id",
                    "node_id",
                    "incarnation",
                    "action",
                    "trigger_event",
                    "depends_on",
                ])
                && p["status"] == "applied"
                && state
                && ordered
        } else {
            false
        };
        if valid {
            receipts.insert(id(&f["fault_id"])?.to_owned(), values[0].clone());
        } else {
            missing.push(format!("fault:{}", id(&f["fault_id"])?));
        }
    }
    for v in acknowledgments {
        if v["presence"] == "observed"
            && !array(&r["fault_schedule"])?.iter().any(|f| {
                f["fault_id"] == v["payload"]["fault_id"] && f["member_id"] == v["member_id"]
            })
        {
            rejected.push(json!({"source":v["raw_artifact"],"fatal":true,"reason":"An acknowledgment has no declared fault."}));
        }
    }
    let mut compared = BTreeMap::new();
    let mut raw = Vec::new();
    let mut covered = 0;
    for m in array(&r["members"])? {
        let name = text(&m["member_id"])?;
        let mut measured = json!({"engaged_path":unknown("snapshot_unavailable"),"result":unknown("snapshot_unavailable"),"fallback_reason":unknown("snapshot_unavailable"),"probe_count":unknown("path_unobserved"),"ancestor_body_read_count":unknown("path_unobserved")});
        let snapshots: Vec<_> = observations
            .iter()
            .filter(|v| v["event_kind"] == "carrier_snapshot" && v["member_id"] == m["member_id"])
            .collect();
        if snapshots.len() != 1 {
            missing.push(format!("{name}:snapshot_inventory"));
        }
        for v in &snapshots {
            let attempt = (|| -> Result<()> {
                raw.push(v["raw_artifact"].clone());
                if v["presence"] != "observed" {
                    missing.push(format!("{name}:snapshot"));
                    return Ok(());
                }
                let p = &v["payload"];
                ensure!(
                    same(p, m, MATCHED)
                        && p["fixture_digest"] == r["inputs"]["fixture"]["sha256"]
                        && p["requested_path"] == m["requested_path"],
                    "The observed traversal inputs differ."
                );
                let engaged = match observed_field("engaged_path", &p["engaged_path"]) {
                    Ok(path) => {
                        measured["engaged_path"] = p["engaged_path"].clone();
                        path == Some(&m["requested_path"])
                    }
                    Err(error) => {
                        rejected.push(json!({"source":v["raw_artifact"],"fatal":true,"reason":error.to_string()}));
                        false
                    }
                };
                if !engaged {
                    missing.push(format!("{name}:path_engagement"));
                }
                for f in array(&r["fault_schedule"])?
                    .iter()
                    .filter(|f| f["member_id"] == m["member_id"])
                {
                    if receipts.get(text(&f["fault_id"])?).is_some_and(|receipt| {
                        manifest::decimal(&receipt["time"]["monotonic_ns"]).ok()
                            > manifest::decimal(&v["time"]["monotonic_ns"]).ok()
                    }) {
                        missing.push(format!("{name}:snapshot_before_fault"));
                    }
                }
                let mut complete = engaged;
                let mut comparison = None;
                for field in COUNTERS
                    .iter()
                    .copied()
                    .chain(["result", "fallback_reason"])
                {
                    let attempt = (|| -> Result<()> {
                        if let Some(value) = observed_field(field, &p[field])? {
                            if !COUNTERS.contains(&field) {
                                let expected = &r["expectation"]["members"][name][field];
                                let equal = if field == "result" {
                                    let key = result_key(value)?;
                                    comparison = Some(key.clone());
                                    key == result_key(expected)?
                                } else {
                                    value == expected
                                };
                                if !equal {
                                    failures.push(json!({"kind":format!("{field}_mismatch"),"member_id":name,"observed":value,"expected":expected,"source":v["raw_artifact"]}));
                                }
                            }
                        } else {
                            missing.push(format!("{name}:{field}"));
                            complete = false;
                        }
                        if !COUNTERS.contains(&field) || engaged {
                            measured[field] = p[field].clone();
                        }
                        Ok(())
                    })();
                    if let Err(error) = attempt {
                        rejected.push(json!({"source":v["raw_artifact"],"fatal":true,"reason":error.to_string()}));
                        complete = false;
                    }
                }
                if complete && snapshots.len() == 1 {
                    covered += 1;
                    if let Some(key) = comparison {
                        compared.insert(name.to_owned(), key);
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
        out["measurements"][name] = measured;
    }
    if compared.len() == 2 && compared.values().next() != compared.values().nth(1) {
        failures.push(json!({"kind":"differential_result_mismatch","members":compared}));
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
    out["coverage"]["observed_members"] = covered.into();
    out["coverage"]["acknowledged_faults"] = receipts.len().into();
    out["fault_receipts"] = json!(receipts.values().collect::<Vec<_>>());
    out["raw_references"] = json!(raw);
    Ok(out)
}
