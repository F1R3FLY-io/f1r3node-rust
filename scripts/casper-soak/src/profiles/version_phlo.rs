use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use casper_soak::{array, artifact, file_hash, hash, manifest, object, parse, text};
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

pub const PROFILE: &str = "version-phlo";
pub const CAPABILITIES: &[&str] = &[
    "signed-envelope-capture",
    "version-rejection",
    "minimum-price-configuration",
    "phlo-settlement-observations",
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
const CONFIGURATION: &[&str] = &[
    "casper_protocol_version",
    "accounting_authority_version",
    "proposed_protocol_version",
    "shard_minimum_price",
    "phlo_case",
    "policy_variant",
    "funding_policy",
];
const ENVELOPE_FIELDS: &[&str] = &[
    "phloLimit",
    "phloPrice",
    "signed_envelope_digest",
    "deploy_signature",
];
const ADMISSION_FIELDS: &[&str] = &[
    "casper_protocol_version",
    "accounting_authority_version",
    "proposed_protocol_version",
    "shard_minimum_price",
    "acceptance",
];
const SETTLEMENT_FIELDS: &[&str] = &[
    "execution_outcome",
    "prepayment",
    "charge",
    "refund",
    "exhausted",
];

pub fn identity() -> Value {
    let sources = [
        (
            "scripts/casper-soak/src/profiles/version_phlo.rs",
            include_bytes!("version_phlo.rs").as_slice(),
        ),
        (
            "scripts/casper-soak/src/bin/casper-version-phlo.rs",
            include_bytes!("../bin/casper-version-phlo.rs").as_slice(),
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
    json!({"profile_id":PROFILE,"profile_digest":digests["scripts/casper-soak/src/profiles/version_phlo.rs"],"source_digests":digests,"version":env!("CARGO_PKG_VERSION")})
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
fn phlo(v: &Value) -> Result<u64> {
    let n = manifest::decimal(v)?;
    ensure!(
        n <= i64::MAX as u64,
        "The Phlo value exceeds the signed field bound."
    );
    Ok(n)
}
fn signature(v: &Value) -> Result<()> {
    let s = id(v)?;
    ensure!(s.len() % 2 == 0, "The signature encoding is invalid.");
    manifest::hex(v, s.len())
}
fn value_type(name: &str, value: &Value) -> Result<()> {
    match name {
        "phloLimit" | "phloPrice" | "shard_minimum_price" => {
            phlo(value)?;
        }
        "casper_protocol_version"
        | "accounting_authority_version"
        | "proposed_protocol_version"
        | "prepayment"
        | "charge"
        | "refund" => {
            manifest::decimal(value)?;
        }
        "signed_envelope_digest" => manifest::hex(value, 64)?,
        "deploy_signature" => signature(value)?,
        "acceptance" => ensure!(
            ["accepted", "rejected"].contains(&text(value)?),
            "The acceptance value is unsupported."
        ),
        "execution_outcome" => ensure!(
            ["completed", "exhausted", "not_executed"].contains(&text(value)?),
            "The execution outcome is unsupported."
        ),
        "exhausted" => ensure!(value.is_boolean(), "The exhaustion value must be Boolean."),
        _ => return Err(eyre!("The measurement field is unsupported.")),
    }
    Ok(())
}
fn measurement<'a>(name: &str, v: &'a Value) -> Result<Option<&'a Value>> {
    match text(&v["presence"])? {
        "observed" => {
            ensure!(
                !v["value"].is_null() && v["reason"].is_null(),
                "The observed measurement is invalid."
            );
            value_type(name, &v["value"])?;
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
fn unknown() -> Value {
    json!({"presence":"missing","value":null,"reason":"observation_unavailable"})
}
fn request(r: &Value) -> Result<()> {
    ensure!(
        r["schema_version"] == 1 && r["profile_id"] == PROFILE,
        "The Phlo request schema differs."
    );
    for field in CONTEXT {
        id(&r[*field])?;
    }
    for field in [
        "manifest_digest",
        "node_binary_digest",
        "signed_envelope_digest",
    ] {
        manifest::hex(&r[field], 64)?;
    }
    manifest::hex(&r["node_revision"], 40)?;
    signature(&r["deploy_signature"])?;
    for field in ["segment", "iteration", "seed", "proposed_protocol_version"] {
        manifest::decimal(&r[field])?;
    }
    ensure!(
        r["casper_protocol_version"] == "7" && r["accounting_authority_version"] == "8",
        "The protocol and accounting authority labels differ from D-01."
    );
    phlo(&r["phloLimit"])?;
    let price = phlo(&r["phloPrice"])?;
    let minimum = phlo(&r["shard_minimum_price"])?;
    id(&r["funding_policy"])?;
    id(&r["observation_deadline"]["clock_id"])?;
    manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?;
    let case = text(&r["phlo_case"])?;
    ensure!(
        [
            "complete",
            "version_rejected",
            "minimum_equal",
            "minimum_below",
            "minimum_above",
            "exhausted",
            "signed_field_mutation"
        ]
        .contains(&case),
        "The Phlo case is unsupported."
    );
    ensure!(
        (case == "version_rejected")
            == (r["proposed_protocol_version"] != r["casper_protocol_version"]),
        "The proposed version does not match the scenario."
    );
    match case {
        "minimum_equal" => ensure!(
            price == minimum,
            "The minimum-price equality fixture differs."
        ),
        "minimum_below" => ensure!(
            price.checked_add(1) == Some(minimum),
            "The below-minimum fixture is not adjacent."
        ),
        "minimum_above" => ensure!(
            minimum.checked_add(1) == Some(price),
            "The above-minimum fixture is not adjacent."
        ),
        _ => ensure!(
            price >= minimum,
            "An unrelated scenario is below the price minimum."
        ),
    }
    let faults = array(&r["fault_schedule"])?;
    ensure!(
        faults.len() == usize::from(case == "signed_field_mutation"),
        "The mutation schedule does not match the scenario."
    );
    for f in faults {
        id(&f["fault_id"])?;
        let field = text(&f["field"])?;
        ensure!(
            f["action"] == "mutate_signed_field" && ["phloLimit", "phloPrice"].contains(&field),
            "The fault action is unsupported."
        );
        ensure!(
            same(f, r, &["member_id", "node_id", "incarnation"]),
            "The fault target differs."
        );
        ensure!(
            f["trigger_event"] == "after_signing"
                && f["original_value"] == r[field]
                && f["value"] != r[field],
            "The signed-field mutation differs."
        );
        ensure!(
            array(&f["depends_on"])?.is_empty(),
            "The single mutation cannot have dependencies."
        );
        phlo(&f["value"])?;
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
            r["funding_policy"] != "single_deployer",
            "funding_mapping_unqualified",
        ),
    ] {
        if condition {
            reasons.push(reason.to_owned());
        }
    }
    for cap in CAPABILITIES.iter().copied().chain(
        (r["phlo_case"] == "signed_field_mutation").then_some("signed-field-mutation-receipts"),
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
            "casper_protocol_version",
            "accounting_authority_version",
            "funding_policy"
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
                        "casper_protocol_version",
                        "accounting_authority_version",
                        "funding_policy"
                    ]),
                "The capability qualification differs."
            );
        }
    }
    ensure!(
        object(&r["inputs"])?.len() == 4,
        "The pinned input inventory differs."
    );
    for name in ["configuration", "fixture", "expectation", "signed_envelope"] {
        let reference = &r["inputs"][name];
        ensure!(
            reference["sha256"] == m[format!("{name}_digest")],
            "A pinned input differs."
        );
        p[name] = parse(&artifact(root, reference)?)?;
    }
    ensure!(
        same(&p["configuration"], r, CONFIGURATION),
        "The pinned configuration differs."
    );
    ensure!(
        same(&p["fixture"], r, &[
            "member_id",
            "node_id",
            "incarnation",
            "deploy_signature",
            "signed_envelope_digest",
            "phloLimit",
            "phloPrice",
            "fault_schedule"
        ]),
        "The pinned fixture differs."
    );
    ensure!(
        r["signed_envelope_digest"] == r["inputs"]["signed_envelope"]["sha256"],
        "The signed-envelope commitment differs."
    );
    ensure!(
        p["signed_envelope"]["encoding"] == "controlled-json-v1"
            && same(&p["signed_envelope"], r, &[
                "phloLimit",
                "phloPrice",
                "deploy_signature"
            ]),
        "The retained synthetic envelope differs."
    );
    manifest::hex(&p["signed_envelope"]["payload_digest"], 64)?;
    for field in ["acceptance"].iter().chain(SETTLEMENT_FIELDS.iter()) {
        value_type(field, &p["expectation"][*field])?;
    }
    let refused = ["version_rejected", "minimum_below", "signed_field_mutation"]
        .contains(&text(&r["phlo_case"])?);
    ensure!(
        p["expectation"]["acceptance"] == if refused { "rejected" } else { "accepted" },
        "The expected acceptance conflicts with the scenario."
    );
    let outcome = if refused {
        "not_executed"
    } else if r["phlo_case"] == "exhausted" {
        "exhausted"
    } else {
        "completed"
    };
    ensure!(
        p["expectation"]["execution_outcome"] == outcome
            && p["expectation"]["exhausted"] == (outcome == "exhausted"),
        "The expected execution conflicts with the scenario."
    );
    Ok(p)
}
pub fn generate(r: &Value) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    let mut workloads = Vec::new();
    if reasons.is_empty() {
        let mut submission = json!({"phloLimit":r["phloLimit"],"phloPrice":r["phloPrice"]});
        for fault in array(&r["fault_schedule"])? {
            submission[text(&fault["field"])?] = fault["value"].clone();
        }
        workloads.push(json!({"operation":"capture_signed_envelope","artifact":r["inputs"]["signed_envelope"],"signed_envelope_digest":r["signed_envelope_digest"],"phloLimit":r["phloLimit"],"phloPrice":r["phloPrice"],"trigger_event":"after_signing"}));
        workloads.push(json!({"operation":"submit_deploy","deploy_signature":r["deploy_signature"],"signed_envelope_digest":r["signed_envelope_digest"],"signed_fields":{"phloLimit":r["phloLimit"],"phloPrice":r["phloPrice"]},"submitted_fields":submission,"casper_protocol_version":r["casper_protocol_version"],"accounting_authority_version":r["accounting_authority_version"],"proposed_protocol_version":r["proposed_protocol_version"],"shard_minimum_price":r["shard_minimum_price"],"seed":r["seed"],"fault_dependencies":r["fault_schedule"],"deadline":r["observation_deadline"]}));
    }
    Ok(
        json!({"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"scenario_verdict":if reasons.is_empty() {"ready"} else {"blocked"},"blocked_reasons":reasons,"node_launch_count":0,"workloads":workloads,"fault_requests":if reasons.is_empty() {r["fault_schedule"].clone()} else {json!([])}}),
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
    let mut sequences: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for reference in references {
        let attempt = (|| -> Result<()> {
            let mut v = parse(&artifact(root, reference)?)?;
            ensure!(v["schema_version"] == 1, "The observation schema differs.");
            for field in
                CONTEXT
                    .iter()
                    .copied()
                    .chain(["record_id", "event_id", "producer", "event_kind"])
            {
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
                ["envelope", "admission", "settlement", "fault_ack"]
                    .contains(&text(&v["event_kind"])?),
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
            match text(&v["presence"])? {
                "observed" => {
                    object(&v["payload"])?;
                    ensure!(
                        v["reason"].is_null(),
                        "An observed payload cannot have an unknown reason."
                    );
                }
                "missing" | "error" => {
                    ensure!(v["payload"].is_null(), "An unknown payload must be null.");
                    id(&v["reason"])?;
                }
                _ => return Err(eyre!("The observation presence is unsupported.")),
            }
            ensure!(
                sequences
                    .get(&producer)
                    .is_none_or(|(s, t)| *s < sequence && *t <= time),
                "The producer sequence or clock regressed."
            );
            sequences.insert(producer, (sequence, time));
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
fn ordered(before: &Value, after: &Value) -> bool {
    before["producer"] == after["producer"]
        && before["time"]["clock_id"] == after["time"]["clock_id"]
        && manifest::decimal(&before["producer_sequence"])
            .ok()
            .zip(manifest::decimal(&after["producer_sequence"]).ok())
            .is_some_and(|(a, b)| a < b)
        && manifest::decimal(&before["time"]["monotonic_ns"])
            .ok()
            .zip(manifest::decimal(&after["time"]["monotonic_ns"]).ok())
            .is_some_and(|(a, b)| a <= b)
}
pub fn classify(r: &Value, c: &Value, acknowledgments: &[Value]) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    let mut out = json!({"schema_version":1,"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"run_id":r["run_id"],"scenario_id":r["scenario_id"],"pair_id":r["pair_id"],"evidence_kind":r["evidence_kind"],"node_launch_count":0,"harness_verification":"pending","soak_verdict":"non_passing","scenario_verdict":"blocked","blocked_reasons":reasons,"product_failures":[],"missing_observations":[],"rejected_observations":c["rejected_observations"],"measurements":{},"coverage":{"required_stages":3,"observed_stages":0,"required_faults":r["fault_schedule"].as_array().map(Vec::len),"acknowledged_faults":0},"fault_receipts":[],"raw_references":[]});
    if !reasons.is_empty() {
        return Ok(out);
    }
    let observations = array(&c["observations"])?;
    let mut failures = Vec::new();
    let mut missing = Vec::new();
    let mut rejected = array(&c["rejected_observations"])?.clone();
    let mut raw = Vec::new();
    let mut covered = 0;
    let mut stages: BTreeMap<&str, &Value> = BTreeMap::new();
    for (kind, fields) in [
        ("envelope", ENVELOPE_FIELDS),
        ("admission", ADMISSION_FIELDS),
        ("settlement", SETTLEMENT_FIELDS),
    ] {
        let values: Vec<_> = observations
            .iter()
            .filter(|v| v["event_kind"] == kind)
            .collect();
        let mut measured = json!({});
        for field in fields {
            measured[*field] = unknown();
        }
        if values.len() != 1 {
            missing.push(format!("{kind}:inventory"));
        }
        for v in &values {
            raw.push(v["raw_artifact"].clone());
            if v["presence"] != "observed" {
                missing.push(kind.to_owned());
                continue;
            }
            let p = &v["payload"];
            if p["fixture_digest"] != r["inputs"]["fixture"]["sha256"]
                || p["deploy_signature"] != r["deploy_signature"]
            {
                rejected.push(json!({"source":v["raw_artifact"],"fatal":true,"reason":"The observation fixture or deploy identity differs."}));
                continue;
            }
            let mut complete = true;
            for field in fields {
                let expected = if [
                    "acceptance",
                    "execution_outcome",
                    "prepayment",
                    "charge",
                    "refund",
                    "exhausted",
                ]
                .contains(field)
                {
                    &r["expectation"][*field]
                } else {
                    &r[*field]
                };
                let actual = if *field == "deploy_signature" {
                    json!({"presence":"observed","value":p[*field],"reason":null})
                } else if p.get(*field).is_none() {
                    unknown()
                } else {
                    p[*field].clone()
                };
                match measurement(field, &actual) {
                    Ok(value) => {
                        if let Some(value) = value {
                            if value != expected {
                                failures.push(json!({"kind":format!("{field}_mismatch"),"stage":kind,"observed":value,"expected":expected,"source":v["raw_artifact"]}));
                            }
                        } else {
                            missing.push(format!("{kind}:{field}"));
                            complete = false;
                        }
                        measured[*field] = actual;
                    }
                    Err(error) => {
                        rejected.push(json!({"source":v["raw_artifact"],"fatal":true,"field":field,"reason":error.to_string()}));
                        complete = false;
                    }
                }
            }
            if complete && values.len() == 1 {
                covered += 1;
                stages.insert(kind, v);
            }
        }
        out["measurements"][kind] = measured;
    }
    for (before, after) in [("envelope", "admission"), ("admission", "settlement")] {
        if let (Some(a), Some(b)) = (stages.get(before), stages.get(after)) {
            if !ordered(a, b) {
                missing.push(format!("{before}:{after}:order"));
            }
        }
    }
    let mut receipts = Vec::new();
    for f in array(&r["fault_schedule"])? {
        let matches: Vec<_> = acknowledgments
            .iter()
            .filter(|v| v["payload"]["fault_id"] == f["fault_id"])
            .collect();
        let valid = matches.len() == 1 && {
            let v = matches[0];
            observations.contains(v)
                && v["event_kind"] == "fault_ack"
                && v["presence"] == "observed"
                && same(&v["payload"], f, &[
                    "fault_id",
                    "member_id",
                    "node_id",
                    "incarnation",
                    "action",
                    "field",
                    "original_value",
                    "value",
                    "trigger_event",
                    "depends_on",
                ])
                && v["payload"]["status"] == "applied"
                && v["payload"]["fixture_digest"] == r["inputs"]["fixture"]["sha256"]
                && v["payload"]["deploy_signature"] == r["deploy_signature"]
                && v["payload"]["signed_envelope_digest"] == r["signed_envelope_digest"]
                && stages.get("envelope").is_some_and(|e| ordered(e, v))
                && stages.get("admission").is_some_and(|a| ordered(v, a))
        };
        if valid {
            receipts.push(matches[0].clone());
        } else {
            missing.push(format!("fault:{}", id(&f["fault_id"])?));
        }
    }
    for v in acknowledgments {
        if !array(&r["fault_schedule"])?
            .iter()
            .any(|f| f["fault_id"] == v["payload"]["fault_id"])
        {
            rejected.push(json!({"source":v["raw_artifact"],"fatal":true,"reason":"An acknowledgment has no declared fault."}));
        }
        raw.push(v["raw_artifact"].clone());
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
    out["coverage"]["observed_stages"] = covered.into();
    out["coverage"]["acknowledged_faults"] = receipts.len().into();
    out["fault_receipts"] = json!(receipts);
    out["raw_references"] = json!(raw);
    Ok(out)
}
