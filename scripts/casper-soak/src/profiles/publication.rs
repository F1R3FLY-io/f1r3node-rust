use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use casper_soak::{array, artifact, file_hash, hash, manifest, number, object, parse, text};
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

pub const PROFILE: &str = "publication";
pub const CAPABILITIES: &[&str] = &[
    "publication-cut-point",
    "process-exit",
    "linked-restart",
    "atomic-publication-snapshot",
    "durable-work-inventory",
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
    "segment",
    "iteration",
    "seed",
];

pub fn identity() -> Value {
    let sources = [
        (
            "scripts/casper-soak/src/profiles/publication.rs",
            include_bytes!("publication.rs").as_slice(),
        ),
        (
            "scripts/casper-soak/src/bin/casper-publication.rs",
            include_bytes!("../bin/casper-publication.rs").as_slice(),
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
    json!({"profile_id":PROFILE,"profile_digest":digests["scripts/casper-soak/src/profiles/publication.rs"],"source_digests":digests,"version":env!("CARGO_PKG_VERSION")})
}
fn id(v: &Value) -> Result<&str> {
    let s = text(v)?;
    ensure!(
        !s.trim().is_empty() && s.len() <= 256,
        "An identity is empty or too long."
    );
    Ok(s)
}
fn same(a: &Value, b: &Value, fields: &[&str]) -> bool {
    fields.iter().all(|f| a.get(*f).is_some() && a[*f] == b[*f])
}
fn measurement(v: &Value) -> Result<Option<&Value>> {
    match text(&v["presence"])? {
        "observed" => {
            ensure!(
                !v["value"].is_null() && v["reason"].is_null(),
                "An observed measurement is malformed."
            );
            Ok(Some(&v["value"]))
        }
        "missing" | "error" => {
            ensure!(v["value"].is_null(), "An unknown measurement must be null.");
            id(&v["reason"])?;
            Ok(None)
        }
        _ => Err(eyre!("The presence state is unsupported.")),
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct PublicationTuple {
    publication_id: String,
    generation: String,
    block_hash: String,
    state_root: String,
    effect_digest: String,
}
fn tuple(v: &Value) -> Result<PublicationTuple> {
    ensure!(object(v)?.len() == 5, "The tuple field inventory differs.");
    let result = PublicationTuple {
        publication_id: text(&v["publication_id"])?.to_owned(),
        generation: text(&v["generation"])?.to_owned(),
        block_hash: text(&v["block_hash"])?.to_owned(),
        state_root: text(&v["state_root"])?.to_owned(),
        effect_digest: text(&v["effect_digest"])?.to_owned(),
    };
    id(&v["publication_id"])?;
    manifest::decimal(&v["generation"])?;
    for key in ["block_hash", "state_root", "effect_digest"] {
        manifest::hex(&v[key], 64)?;
    }
    Ok(result)
}
fn work(v: &Value) -> Result<BTreeMap<String, String>> {
    let values = array(v)?;
    ensure!(values.len() <= 64, "The occurrence bound is exceeded.");
    let mut result = BTreeMap::new();
    for item in values {
        let name = id(&item["occurrence_id"])?;
        manifest::hex(&item["deploy_signature"], 64)?;
        ensure!(
            result
                .insert(name.to_owned(), text(&item["deploy_signature"])?.to_owned())
                .is_none(),
            "An occurrence identity is duplicated."
        );
    }
    Ok(result)
}
fn terminals(v: &Value, publication: &str, generation: u64) -> Result<BTreeMap<String, Value>> {
    ensure!(
        array(v)?.len() <= 64,
        "The terminal-verdict bound is exceeded."
    );
    let mut result = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for item in array(v)? {
        let occurrence = id(&item["occurrence_id"])?;
        id(&item["verdict_id"])?;
        manifest::hex(&item["deploy_signature"], 64)?;
        ensure!(
            item["durable"].is_boolean(),
            "The durable flag must be Boolean."
        );
        ensure!(
            ["executed", "rejected", "expired"].contains(&text(&item["verdict"])?),
            "The verdict is not terminal."
        );
        ensure!(
            item["publication_id"] == publication
                && manifest::decimal(&item["generation"])? <= generation,
            "The verdict publication context differs."
        );
        ensure!(
            ids.insert(text(&item["verdict_id"])?),
            "A verdict identity is duplicated."
        );
        ensure!(
            result.insert(occurrence.to_owned(), item.clone()).is_none(),
            "An occurrence has conflicting verdicts."
        );
    }
    Ok(result)
}
fn request(r: &Value) -> Result<()> {
    ensure!(
        number(&r["schema_version"])? == 1 && r["profile_id"] == PROFILE,
        "The publication schema is unsupported."
    );
    for field in CONTEXT {
        id(&r[*field])?;
    }
    for key in ["manifest_digest", "node_binary_digest"] {
        manifest::hex(&r[key], 64)?;
    }
    manifest::hex(&r["node_revision"], 40)?;
    for key in [
        "segment",
        "iteration",
        "seed",
        "generation",
        "minimum_generation",
    ] {
        manifest::decimal(&r[key])?;
    }
    for key in [
        "publication_id",
        "predecessor_incarnation",
        "incarnation",
        "fault_id",
        "policy_variant",
    ] {
        id(&r[key])?;
    }
    ensure!(
        r["incarnation"] != r["predecessor_incarnation"],
        "A restart must create a new incarnation."
    );
    ensure!(
        ["before_publication", "after_publication"].contains(&text(&r["cut_point"])?),
        "The cut point is unsupported."
    );
    ensure!(
        ["single-flight", "parallel"].contains(&text(&r["mode"])?),
        "The publication mode is unsupported."
    );
    ensure!(
        (r["mode"] == "single-flight" && r["policy_variant"] == "baseline")
            || (r["mode"] == "parallel" && r["policy_variant"] == "publication-parallel"),
        "The publication policy and mode differ."
    );
    id(&r["observation_deadline"]["clock_id"])?;
    manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?;
    let expected = tuple(&r["expected_tuple"])?;
    ensure!(
        expected.publication_id == text(&r["publication_id"])?
            && expected.generation == text(&r["generation"])?
            && manifest::decimal(&r["minimum_generation"])? <= manifest::decimal(&r["generation"])?,
        "The expected tuple context differs."
    );
    work(&r["unresolved_occurrences"])?;
    Ok(())
}
fn blocked(r: &Value) -> Vec<String> {
    let mut reasons = Vec::new();
    if r["evidence_kind"] != "synthetic_fixture" {
        reasons.push("live_adapters_unqualified".into());
    }
    if r["phase"] != "pre_pr216_merge" {
        reasons.push("post_merge_adapter_unqualified".into());
    }
    if r["mode"] != "single-flight" {
        reasons.push("parallel_policy_not_approved".into());
    }
    for cap in CAPABILITIES {
        if r["capabilities"][*cap]["status"] != "qualified" {
            reasons.push((*cap).into());
        }
    }
    reasons
}
pub fn generate(r: &Value) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    Ok(
        json!({"profile_id":PROFILE,"scenario_verdict":if reasons.is_empty(){"ready"}else{"blocked"},"blocked_reasons":reasons,"node_launch_count":0,"manifest_digest":r["manifest_digest"],"schedule":if reasons.is_empty(){json!(["prepare_publication_fixture","capture_before","crash_then_restart","capture_recovered"])}else{json!([])},"workloads":if reasons.is_empty(){json!([{"operation":"prepare_publication_fixture","inputs":r["inputs"],"seed":r["seed"],"mode":r["mode"]},{"operation":"capture_before","incarnation":r["predecessor_incarnation"]},{"operation":"capture_recovered","incarnation":r["incarnation"]}])}else{json!([])},"fault_requests":if reasons.is_empty(){json!([{"action":"crash_then_restart","fault_id":r["fault_id"],"cut_point":r["cut_point"],"publication_id":r["publication_id"],"generation":r["generation"],"node_id":r["node_id"],"predecessor_incarnation":r["predecessor_incarnation"],"incarnation":r["incarnation"],"deadline":r["observation_deadline"]}])}else{json!([])}}),
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
        "The request and manifest identities differ."
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
            "A compiled source pin differs."
        );
    }
    let mut prepared = r.clone();
    prepared["capabilities"] = m["capabilities"].clone();
    for (name, cap) in object(&m["capabilities"])? {
        ensure!(
            ["qualified", "unknown", "unsupported"].contains(&text(&cap["status"])?),
            "The capability status is unsupported."
        );
        if cap["status"] == "qualified" {
            ensure!(
                cap["qualification"]["capture_state"] == "captured",
                "The qualification is not captured."
            );
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
                    ]),
                "The capability qualification differs."
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
        prepared[key] = parse(&artifact(root, reference)?)?;
    }
    ensure!(
        same(&prepared["configuration"], r, &["mode", "policy_variant"]),
        "The configuration differs."
    );
    ensure!(
        same(&prepared["fixture"], r, &[
            "publication_id",
            "generation",
            "minimum_generation",
            "cut_point",
            "expected_tuple",
            "unresolved_occurrences"
        ]),
        "The fixture differs."
    );
    let before = tuple(&prepared["expectation"]["before_tuple"])?;
    ensure!(
        before.publication_id == text(&r["publication_id"])?
            && manifest::decimal(&json!(before.generation))? < manifest::decimal(&r["generation"])?,
        "The pre-crash tuple context differs."
    );
    let expected = tuple(&r["expected_tuple"])?;
    let mut allowed = vec![expected];
    if r["cut_point"] == "before_publication" {
        allowed.push(before);
    }
    let supplied = array(&prepared["expectation"]["permitted_tuples"])?;
    ensure!(
        supplied.len() == allowed.len(),
        "The permitted tuple inventory differs."
    );
    let supplied = supplied.iter().map(tuple).collect::<Result<Vec<_>>>()?;
    ensure!(
        allowed.iter().all(|t| supplied.contains(t)),
        "A permitted tuple differs."
    );
    Ok(prepared)
}

pub fn collect(m: &Value, artifacts: &Value) -> Result<Value> {
    let r = &artifacts["request"];
    request(r)?;
    let root = Path::new(text(&artifacts["root"])?);
    let references = array(&artifacts["records"])?;
    ensure!(references.len() <= 64, "The transport bound is exceeded.");
    let mut observations = Vec::new();
    let mut rejected = Vec::new();
    let mut duplicates = Vec::new();
    let mut records = BTreeSet::new();
    let mut events = BTreeMap::new();
    let mut sequences = BTreeMap::new();
    for reference in references {
        let attempt = (|| -> Result<Option<Value>> {
            ensure!(
                reference["capture_state"] == "captured",
                "The observation is not captured."
            );
            let mut v = parse(&artifact(root, reference)?)?;
            ensure!(
                number(&v["schema_version"])? == 1,
                "The observation schema is unsupported."
            );
            for key in CONTEXT.iter().copied().chain([
                "record_id",
                "event_id",
                "producer",
                "incarnation",
                "event_kind",
            ]) {
                id(&v[key])?;
            }
            ensure!(
                records.insert(text(&v["record_id"])?.to_owned()),
                "The transport identity is duplicated."
            );
            ensure!(
                reference["producer"] == v["producer"]
                    && array(&reference["observation_ids"])?.contains(&v["record_id"]),
                "The artifact producer or record identity differs."
            );
            let sequence = manifest::decimal(&v["producer_sequence"])?;
            let time = manifest::decimal(&v["time"]["monotonic_ns"])?;
            id(&v["time"]["utc"])?;
            id(&v["time"]["clock_id"])?;
            let producer =
                serde_json::to_string(&json!([v["node_id"], v["incarnation"], v["producer"]]))?;
            let context: Vec<_> = CONTEXT.iter().map(|key| v[*key].clone()).collect();
            let event = serde_json::to_string(&json!([context, producer, v["event_id"]]))?;
            let mut comparable = v.clone();
            comparable
                .as_object_mut()
                .ok_or_else(|| eyre!("The observation is not an object."))?
                .remove("record_id");
            if let Some(old) = events.get(&event) {
                ensure!(*old == comparable, "Copies of an event disagree.");
                duplicates.push(reference.clone());
                return Ok(None);
            }
            events.insert(event, comparable);
            let kind = text(&v["event_kind"])?;
            ensure!(
                ["before_snapshot", "recovered_snapshot", "fault_ack"].contains(&kind),
                "The observation kind is unsupported."
            );
            let incarnation = if kind == "recovered_snapshot" {
                &r["incarnation"]
            } else {
                &r["predecessor_incarnation"]
            };
            let linked = (kind != "recovered_snapshot"
                || v["predecessor_incarnation"] == r["predecessor_incarnation"])
                && (kind != "fault_ack"
                    || v["presence"] != "observed"
                    || same(&v["payload"], r, &[
                        "fault_id",
                        "cut_point",
                        "publication_id",
                        "generation",
                        "predecessor_incarnation",
                        "incarnation",
                    ]));
            if !linked
                || !same(&v, r, CONTEXT)
                || !same(&v, m, &["run_id", "phase", "evidence_kind"])
                || v["incarnation"] != *incarnation
                || v["time"]["clock_id"] != r["observation_deadline"]["clock_id"]
                || time > manifest::decimal(&r["observation_deadline"]["monotonic_ns"])?
            {
                rejected.push(json!({"source":reference,"reason":"correlation_or_deadline_mismatch","fatal":false}));
                return Ok(None);
            }
            ensure!(
                sequences.get(&producer).is_none_or(|s| *s < sequence),
                "The producer sequence did not increase."
            );
            sequences.insert(producer, sequence);
            measurement(
                &json!({"presence":v["presence"],"value":v["payload"],"reason":v["reason"]}),
            )?;
            v["raw_artifact"] = reference.clone();
            Ok(Some(v))
        })();
        match attempt {
            Ok(Some(v)) => observations.push(v),
            Ok(None) => {}
            Err(error) => {
                rejected.push(json!({"source":reference,"reason":error.to_string(),"fatal":true}))
            }
        }
    }
    Ok(
        json!({"observations":observations,"rejected_observations":rejected,"duplicate_sources":duplicates}),
    )
}
fn receipt(r: &Value, v: &Value) -> Result<bool> {
    if v["presence"] != "observed" {
        return Ok(false);
    }
    let p = &v["payload"];
    ensure!(
        same(p, r, &[
            "fault_id",
            "cut_point",
            "publication_id",
            "generation",
            "predecessor_incarnation",
            "incarnation"
        ]),
        "The crash receipt context differs."
    );
    ensure!(
        p["action"] == "crash_then_restart",
        "The receipt action differs."
    );
    ensure!(
        ["applied", "not_applied", "unknown"].contains(&text(&p["status"])?),
        "The receipt status is unsupported."
    );
    let mut previous_sequence = None;
    let mut previous_time = None;
    let mut ids = BTreeSet::new();
    let mut applied = true;
    for stage in ["boundary", "exit", "restart"] {
        let event = &p[stage];
        ensure!(
            event["observed"].is_boolean(),
            "Receipt observation flags must be Boolean."
        );
        id(&event["event_id"])?;
        ensure!(
            event["producer"] == v["producer"],
            "The receipt producer differs."
        );
        ensure!(
            ids.insert(text(&event["event_id"])?),
            "A receipt event identity is duplicated."
        );
        let sequence = manifest::decimal(&event["producer_sequence"])?;
        let time = manifest::decimal(&event["monotonic_ns"])?;
        ensure!(
            event["clock_id"] == r["observation_deadline"]["clock_id"]
                && time <= manifest::decimal(&v["time"]["monotonic_ns"])?
                && previous_sequence.is_none_or(|old| old < sequence)
                && previous_time.is_none_or(|old| old <= time),
            "Receipt ordering or clock identity differs."
        );
        previous_sequence = Some(sequence);
        previous_time = Some(time);
        applied &= event["observed"] == true;
    }
    ensure!(
        p["exit"]["exited"].is_boolean() && p["restart"]["ready"].is_boolean(),
        "Exit and readiness must be Boolean."
    );
    Ok(p["status"] == "applied"
        && applied
        && p["exit"]["exited"] == true
        && p["restart"]["ready"] == true)
}
pub fn classify(r: &Value, collection: &Value, acknowledgments: &[Value]) -> Result<Value> {
    request(r)?;
    let reasons = blocked(r);
    let mut result = json!({"schema_version":1,"profile_id":PROFILE,"manifest_digest":r["manifest_digest"],"run_id":r["run_id"],"scenario_id":r["scenario_id"],"evidence_kind":r["evidence_kind"],"node_launch_count":0,"harness_verification":"pending","soak_verdict":"non_passing","scenario_verdict":"blocked","blocked_reasons":reasons,"product_failures":[],"missing_observations":[],"rejected_observations":collection["rejected_observations"],"fault_receipts":acknowledgments,"measurements":[],"raw_references":[],"coverage":{"required":["before_snapshot","cut_point_exit_restart","recovered_snapshot"],"observed":[],"acknowledged_faults":0}});
    if !reasons.is_empty() {
        return Ok(result);
    }
    let mut failures = Vec::new();
    let mut missing = Vec::new();
    let mut rejected = array(&collection["rejected_observations"])?.clone();
    let mut observed = Vec::new();
    let mut measurements = Vec::new();
    let mut raw = Vec::new();
    let mut snapshots = BTreeMap::new();
    if acknowledgments.len() == 1 {
        match receipt(r, &acknowledgments[0]) {
            Ok(true) => {
                observed.push("cut_point_exit_restart");
                result["coverage"]["acknowledged_faults"] = 1.into();
                raw.push(acknowledgments[0]["raw_artifact"].clone());
            }
            Ok(false) => missing.push("cut_point_exit_restart".to_owned()),
            Err(error) => {
                rejected.push(json!({"source":acknowledgments[0]["raw_artifact"],"reason":error.to_string(),"fatal":true}));
                missing.push("cut_point_exit_restart".to_owned());
            }
        }
    } else {
        missing.push("cut_point_exit_restart".to_owned());
    }
    for kind in ["before_snapshot", "recovered_snapshot"] {
        let values: Vec<_> = array(&collection["observations"])?
            .iter()
            .filter(|v| v["event_kind"] == kind)
            .collect();
        if values.len() != 1 {
            missing.push(kind.to_owned());
            continue;
        }
        let v = values[0];
        let attempt = (|| -> Result<()> {
            if v["presence"] != "observed" {
                missing.push(kind.to_owned());
                return Ok(());
            }
            if acknowledgments.len() == 1 && receipt(r, &acknowledgments[0]).unwrap_or(false) {
                let time = manifest::decimal(&v["time"]["monotonic_ns"])?;
                let receipt = &acknowledgments[0]["payload"];
                if (kind == "before_snapshot"
                    && time > manifest::decimal(&receipt["boundary"]["monotonic_ns"])?)
                    || (kind == "recovered_snapshot"
                        && time < manifest::decimal(&receipt["restart"]["monotonic_ns"])?)
                {
                    missing.push(format!("{kind}:capture_order"));
                    return Ok(());
                }
            }
            let p = &v["payload"];
            ensure!(p["atomic"].is_boolean(), "The atomic flag must be Boolean.");
            if p["atomic"] != true {
                missing.push(format!("{kind}:atomic_snapshot"));
                return Ok(());
            }
            ensure!(
                p["fixture_digest"] == r["inputs"]["fixture"]["sha256"],
                "The observed fixture differs."
            );
            let Some(value) = measurement(&p["tuple"])? else {
                missing.push(format!("{kind}:tuple"));
                return Ok(());
            };
            let actual = tuple(value)?;
            let generation = manifest::decimal(&value["generation"])?;
            if kind == "before_snapshot" {
                if actual != tuple(&r["expectation"]["before_tuple"])? {
                    failures.push(json!({"kind":"pre_tuple_mismatch","source":v["raw_artifact"]}));
                }
            } else {
                if !array(&r["expectation"]["permitted_tuples"])?
                    .iter()
                    .map(tuple)
                    .collect::<Result<Vec<_>>>()?
                    .contains(&actual)
                {
                    failures.push(
                        json!({"kind":"torn_or_unpermitted_tuple","source":v["raw_artifact"]}),
                    );
                }
                if generation < manifest::decimal(&r["minimum_generation"])? {
                    failures.push(json!({"kind":"stale_generation","source":v["raw_artifact"]}));
                }
            }
            let retained = measurement(&p["retained_work"])?;
            let verdicts = measurement(&p["terminal_verdicts"])?;
            if let Some(items) = retained {
                work(items)?;
            } else {
                missing.push(format!("{kind}:retained_work"));
            }
            if let Some(items) = verdicts {
                terminals(items, text(&r["publication_id"])?, generation)?;
            } else {
                missing.push(format!("{kind}:terminal_verdicts"));
            }
            snapshots.insert(kind, p.clone());
            measurements.push(json!({"kind":kind,"snapshot":p}));
            raw.push(v["raw_artifact"].clone());
            if retained.is_some() && verdicts.is_some() {
                observed.push(kind);
            }
            Ok(())
        })();
        if let Err(error) = attempt {
            rejected
                .push(json!({"source":v["raw_artifact"],"reason":error.to_string(),"fatal":true}));
        }
    }
    if let (Some(before), Some(after)) = (
        snapshots.get("before_snapshot"),
        snapshots.get("recovered_snapshot"),
    ) {
        if let Some(at) = measurement(&after["terminal_verdicts"])? {
            let terminal = terminals(
                at,
                text(&r["publication_id"])?,
                manifest::decimal(&after["tuple"]["value"]["generation"])?,
            )?;
            if let (Some(bw), Some(aw)) = (
                measurement(&before["retained_work"])?,
                measurement(&after["retained_work"])?,
            ) {
                let prior = work(bw)?;
                let retained = work(aw)?;
                let requested = work(&r["unresolved_occurrences"])?;
                for (occurrence, signature) in requested {
                    if prior.get(&occurrence) != Some(&signature) {
                        missing.push(format!("initial_occurrence:{occurrence}"));
                        continue;
                    }
                    let settled = terminal.get(&occurrence).is_some_and(|v| {
                        v["durable"] == true && v["deploy_signature"] == signature
                    });
                    if retained.get(&occurrence) != Some(&signature) && !settled {
                        failures.push(
                            json!({"kind":"lost_unresolved_work","occurrence_id":occurrence}),
                        );
                    }
                }
            }
            if let Some(bt) = measurement(&before["terminal_verdicts"])? {
                let prior_terminal = terminals(
                    bt,
                    text(&r["publication_id"])?,
                    manifest::decimal(&before["tuple"]["value"]["generation"])?,
                )?;
                for (occurrence, verdict) in prior_terminal {
                    if verdict["durable"] == true && terminal.get(&occurrence) != Some(&verdict) {
                        failures.push(json!({"kind":"durable_verdict_lost_or_changed","occurrence_id":occurrence}));
                    }
                }
            }
        }
    }
    let fatal = rejected.iter().any(|v| v["fatal"] != false);
    result["scenario_verdict"] = if fatal {
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
    result["rejected_observations"] = json!(rejected);
    result["measurements"] = json!(measurements);
    result["raw_references"] = json!(raw);
    result["coverage"]["observed"] = json!(observed);
    Ok(result)
}
