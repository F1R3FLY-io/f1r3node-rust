use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use casper_soak::{encoded, hash, record};
use serde_json::{json, Value};

fn binary() -> &'static str { env!("CARGO_BIN_EXE_casper-recovery") }
fn observed(value: Value) -> Value { json!({"presence":"observed","value":value,"reason":null}) }
fn missing() -> Value { json!({"presence":"missing","value":null,"reason":"unavailable"}) }
fn reference(root: &Path, path: &str, value: &Value, ids: Vec<String>) -> Value {
    let bytes = encoded(value).unwrap();
    fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    fs::write(root.join(path), &bytes).unwrap();
    json!({"path":path,"bytes":bytes.len(),"sha256":hash(&bytes),"producer":"fixture","capture_state":"captured","observation_ids":ids})
}
struct Fixture {
    root: PathBuf,
    _temporary: Option<tempfile::TempDir>,
    manifest: Value,
    request: Value,
    observations: Vec<Value>,
    invocation: usize,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let (root, temporary) = if let Ok(root) = std::env::var("SOAK_RECOVERY_EVIDENCE") {
            let path = PathBuf::from(root).join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::create_dir(&path).unwrap();
            (path, None)
        } else {
            let dir = tempfile::tempdir().unwrap();
            (dir.path().to_path_buf(), Some(dir))
        };
        let output = Command::new(binary()).arg("identity").output().unwrap();
        assert!(output.status.success());
        let identity: Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut manifest = json!({"schema_version":1,"run_id":name,"phase":"pre_pr216_merge","candidate_id":"fixture-node","node_revision":"2".repeat(40),"node_binary_digest":"1".repeat(64),"image_digest":null,"image_digest_reason":"subprocess fixture","harness_revision":"3".repeat(40),"external_harness_revision":"4".repeat(40),"source_digests":identity["source_digests"],"profile_id":"recovery","occurrence_schema":"synthetic-source-occurrence-v1","profile_digest":identity["profile_digest"],"profile_binary_digest":identity["executable_sha256"],"seed":"7","provider":"subprocess","policy_variant":"baseline","evidence_kind":"synthetic_fixture","capabilities":{},"tool_versions":{"profile":identity["version"]},"bounds":{"scenarios":2,"observations":3},"assumptions":["controlled transcript, not a node occurrence schema"],"resource_limits":{"artifact_bytes":1048576},"required_scenarios":["scenario-1"],"deadline":{"clock_id":"observer-clock","monotonic_ns":"100"},"merge_gate":null});
        for name in [
            "recovery-lanes",
            "exact-occurrences",
            "custody-observations",
            "objective-height",
            "paused-state",
            "delivery-receipts",
            "recovery-telemetry",
        ] {
            let proof = json!({"capability":name,"status":"qualified","provider":manifest["provider"],"node_revision":manifest["node_revision"],"node_binary_digest":manifest["node_binary_digest"],"external_harness_revision":manifest["external_harness_revision"],"profile_digest":manifest["profile_digest"],"profile_binary_digest":manifest["profile_binary_digest"],"evidence_kind":"synthetic_fixture","occurrence_schema":"synthetic-source-occurrence-v1"});
            manifest["capabilities"][name] = json!({"status":"qualified","provider":manifest["provider"],"node_revision":manifest["node_revision"],"qualification":reference(&root,&format!("qualification/{name}.json"),&proof,vec![])});
        }
        let mut request = json!({"schema_version":1,"profile_id":"recovery","scenario_id":"scenario-1","pair_id":"pair-1","member_id":"member-1","node_id":"node-1","incarnation":"inc-1","segment":"1","iteration":"1","recovery_lane":"stale_recovery","occurrence_schema":"synthetic-source-occurrence-v1","coverage_rule":"one_parent_b1","leadership":"all_eligible","clock_policy":"baseline","frontier_digest":"a".repeat(64),"objective_height":"1000","lifespan":"100","require_telemetry":true,"require_duration":true,"observation_deadline":{"clock_id":"observer-clock","monotonic_ns":"100"},"occurrences":[{"occurrence_id":"A","deploy_signature":"d".repeat(64),"carrier_block":"a".repeat(64),"sender":"validator-1","origin_height":"950"},{"occurrence_id":"B","deploy_signature":"d".repeat(64),"carrier_block":"b".repeat(64),"sender":"validator-2","origin_height":"800"}],"fault_schedule":[{"fault_id":"pause-1","node_id":"node-1","incarnation":"inc-1","action":"pause","trigger_id":"trigger-1","after":[],"deadline":{"clock_id":"observer-clock","monotonic_ns":"80"}},{"fault_id":"delivery-1","node_id":"node-1","incarnation":"inc-1","action":"delay_delivery","message_id":"message-1","trigger_id":"trigger-2","after":["pause-1"],"deadline":{"clock_id":"observer-clock","monotonic_ns":"80"}}],"inputs":{}});
        for key in [
            "run_id",
            "phase",
            "evidence_kind",
            "candidate_id",
            "node_revision",
            "node_binary_digest",
            "seed",
            "policy_variant",
        ] {
            request[key] = manifest[key].clone();
        }
        let mut f = Self {
            root,
            _temporary: temporary,
            manifest,
            request,
            observations: vec![],
            invocation: 0,
        };
        f.configure();
        f
    }
    fn configure(&mut self) {
        let split = self.request["fixture_frontier_kind"] == "split";
        let frontier = json!({"kind":if split{"split"}else{"single"},"tips":["tip-A","tip-B"],"selected_parents":if split{json!([{"id":"parent-A","covers":["tip-A"]},{"id":"parent-B","covers":["tip-B"]}])}else{json!([{"id":"parent-C","covers":["tip-A","tip-B"]}])}});
        self.request["inputs"]["frontier"] =
            reference(&self.root, "frontier.json", &frontier, vec![]);
        self.request["frontier_digest"] = self.request["inputs"]["frontier"]["sha256"].clone();
        let coverage = json!({"one_parent":!split,"collective":true});
        let mut fixture = json!({});
        for key in [
            "recovery_lane",
            "occurrence_schema",
            "frontier_digest",
            "objective_height",
            "lifespan",
            "occurrences",
            "fault_schedule",
        ] {
            fixture[key] = self.request[key].clone();
        }
        let mut config = json!({});
        for key in [
            "policy_variant",
            "coverage_rule",
            "leadership",
            "clock_policy",
        ] {
            config[key] = self.request[key].clone();
        }
        let expected: Vec<_> = self.request["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| {
                let mut e = input.clone();
                let retried = e["occurrence_id"] == "A";
                for (key, value) in [
                    ("custodian", input["sender"].clone()),
                    ("reason_inputs", json!(["collateral", "duplicate"])),
                    ("joined_reason", json!("duplicate")),
                    (
                        "causal_references",
                        json!([format!(
                            "source-{}",
                            input["occurrence_id"].as_str().unwrap()
                        )]),
                    ),
                    ("tombstone", json!(true)),
                    (
                        "lease",
                        json!({"holder":input["sender"],"expires_height":"990","expired":true}),
                    ),
                    (
                        "terminal_outcome",
                        json!(if retried { "retried" } else { "expired" }),
                    ),
                    ("retry_authorized", json!(retried)),
                    ("body_available", json!(true)),
                    ("objective_height", json!("1000")),
                    ("lifespan", json!("100")),
                    ("retry_count", json!(if retried { "1" } else { "0" })),
                ] {
                    e[key] = value;
                }
                e
            })
            .collect();
        let telemetry = json!({"block_count":"1","cadence_ns":"100","latency_ns":"40","rss_bytes":"1024","cpu_ns":"5"});
        for (key, value) in [
            ("fixture", fixture),
            ("configuration", config),
            (
                "expectation",
                json!({"occurrences":expected,"telemetry":telemetry,"frontier_coverage":coverage}),
            ),
        ] {
            let a = reference(&self.root, &format!("{key}.json"), &value, vec![]);
            self.manifest[format!("{key}_digest")] = a["sha256"].clone();
            self.request["inputs"][key] = a;
        }
        let mut observations = Vec::new();
        for (i, f) in self.request["fault_schedule"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let mut v = self.envelope(i, "fault_ack");
            v["node_id"] = f["node_id"].clone();
            v["incarnation"] = f["incarnation"].clone();
            let mut p = f.clone();
            p["status"] = "applied".into();
            p["observed"] = true.into();
            p["trigger_observed"] = true.into();
            p["observed_state"] = if f["action"] == "pause" {
                "paused"
            } else {
                "delivered"
            }
            .into();
            v["payload"] = p;
            v["time"]["monotonic_ns"] = ((i + 1) * 10).to_string().into();
            observations.push(v);
        }
        let mut snapshot = self.envelope(observations.len(), "recovery_snapshot");
        snapshot["time"]["monotonic_ns"] = "90".into();
        let mut samples = Vec::new();
        for e in &expected {
            let mut sample =
                json!({"sample_id":format!("sample-{}",e["occurrence_id"].as_str().unwrap())});
            for key in [
                "occurrence_id",
                "deploy_signature",
                "carrier_block",
                "sender",
            ] {
                sample[key] = e[key].clone();
            }
            for key in [
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
            ] {
                sample[key] = observed(e[key].clone());
            }
            samples.push(sample.clone());
            if e["occurrence_id"] == "A" {
                sample["sample_id"] = "sample-A-copy".into();
                samples.push(sample);
            }
        }
        let mut measurements = json!({});
        for (key, value) in telemetry.as_object().unwrap() {
            measurements[key] = observed(value.clone());
        }
        snapshot["payload"] = json!({"recovery_lane":self.request["recovery_lane"],"occurrence_schema":self.request["occurrence_schema"],"frontier_digest":self.request["frontier_digest"],"coverage_rule":self.request["coverage_rule"],"inventory_complete":true,"frontier_coverage":observed(coverage),"evaluation_receipt":{"status":"applied","fixture_digest":self.request["inputs"]["fixture"]["sha256"],"steps":["load_fixture","observe_recovery"]},"occurrences":observed(json!(samples)),"telemetry":measurements,"recovery_time":observed(json!({"start":{"clock_id":"observer-clock","monotonic_ns":"10"},"end":{"clock_id":"observer-clock","monotonic_ns":"50"}}))});
        observations.push(snapshot);
        self.observations = observations;
        self.seal();
    }
    fn envelope(&self, i: usize, kind: &str) -> Value {
        let mut v = json!({"schema_version":1,"record_id":format!("record-{i}"),"event_id":format!("event-{i}"),"event_kind":kind,"producer":"fixture","producer_sequence":(i+1).to_string(),"time":{"clock_id":"observer-clock","monotonic_ns":"90","utc":"2026-01-01T00:00:00Z"},"presence":"observed","reason":null});
        for key in [
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
        ] {
            v[key] = self.request[key].clone();
        }
        v
    }
    fn seal(&mut self) {
        let digest = hash(&encoded(&self.manifest).unwrap());
        self.request["manifest_digest"] = digest.clone().into();
        for v in &mut self.observations {
            v["manifest_digest"] = digest.clone().into();
        }
    }
    fn payload(&mut self) -> &mut Value { &mut self.observations.last_mut().unwrap()["payload"] }
    fn invoke(&mut self, code: i32, verdict: &str) -> Value {
        self.invoke_edited(code, verdict, |_| {})
    }
    fn invoke_edited(&mut self, code: i32, verdict: &str, edit: impl FnOnce(&Path)) -> Value {
        self.invocation += 1;
        let references: Vec<_> = self
            .observations
            .iter()
            .enumerate()
            .map(|(i, v)| {
                reference(&self.root, &format!("raw/{i}.json"), v, vec![v
                    ["record_id"]
                    .as_str()
                    .unwrap()
                    .into()])
            })
            .collect();
        for (path, value) in [
            ("manifest.json", self.manifest.clone()),
            ("request.json", self.request.clone()),
            ("observations.json", json!({"records":references})),
        ] {
            fs::write(self.root.join(path), encoded(&value).unwrap()).unwrap();
        }
        edit(&self.root);
        let out = format!("output-{}", self.invocation);
        let args = [
            "run",
            "--manifest",
            "manifest.json",
            "--request",
            "request.json",
            "--artifacts",
            ".",
            "--output",
            &out,
        ];
        let output = Command::new(binary())
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap();
        fs::write(self.root.join(format!("invocation-{}.json",self.invocation)),encoded(&json!({"command":"casper-recovery","arguments":args,"expected_exit":code,"actual_exit":output.status.code(),"expected_verdict":verdict,"stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr)})).unwrap()).unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["scenario_verdict"], verdict, "{value}");
        assert_eq!(value["node_launch_count"], 0);
        assert_eq!(value["soak_verdict"], "non_passing");
        if self.root.join(&out).join("report.json").exists() {
            assert_eq!(
                record(&self.root.join(out).join("report.json")).unwrap(),
                value
            );
        }
        value
    }
}
#[test]
fn required_recovery_fixtures() {
    Fixture::new("recovery_complete").invoke(0, "passed");
    let mut f = Fixture::new("recovery_capability_missing");
    f.manifest["capabilities"]["exact-occurrences"]["status"] = "unsupported".into();
    f.seal();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("recovery_generation");
    f.invoke(0, "passed");
    f.invoke(0, "passed");
    assert_eq!(
        record(&f.root.join("output-1/generation.json")).unwrap(),
        record(&f.root.join("output-2/generation.json")).unwrap()
    );
    let mut f = Fixture::new("recovery_lane_mismatch");
    f.request["recovery_lane"] = "convergence".into();
    f.request["leadership"] = "leader_only".into();
    f.configure();
    f.payload()["recovery_lane"] = "stale_recovery".into();
    f.invoke(1, "incomplete");
    let r = Fixture::new("recovery_occurrence_counts").invoke(0, "passed");
    assert_eq!(r["measurements"]["counts"]["value"]["occurrences"], 2);
    assert_eq!(
        r["measurements"]["counts"]["value"]["duplicate_occurrence_observations"],
        1
    );
    let mut f = Fixture::new("recovery_pause_unacknowledged");
    f.observations[0]["payload"]["observed"] = false.into();
    assert_eq!(
        f.invoke(1, "incomplete")["coverage"]["acknowledged_faults"],
        0
    );
}
#[test]
fn lanes_empty_work_and_policy_isolation() {
    for lane in [
        "convergence",
        "frontier_follow",
        "pending_deploy",
        "readiness",
        "backstop",
    ] {
        let mut f = Fixture::new(&format!("baseline-{lane}"));
        f.request["recovery_lane"] = lane.into();
        f.request["leadership"] = if lane == "convergence" {
            "leader_only"
        } else {
            "baseline"
        }
        .into();
        f.configure();
        f.invoke(0, "passed");
    }
    let mut f = Fixture::new("empty-work");
    f.request["occurrences"] = json!([]);
    f.configure();
    assert_eq!(
        f.invoke(0, "passed")["measurements"]["counts"]["value"]["occurrences"],
        0
    );
    for (key, value) in [
        ("leadership", "rotating"),
        ("leadership", "leader_free"),
        ("coverage_rule", "collective"),
        ("clock_policy", "progress_clock"),
        ("policy_variant", "frontier_disabled"),
    ] {
        let mut f = Fixture::new(&format!("blocked-{value}"));
        f.request[key] = value.into();
        if key == "policy_variant" {
            f.manifest[key] = value.into();
        }
        f.configure();
        f.invoke(3, "blocked");
    }
    let mut f = Fixture::new("live-blocked");
    f.request["evidence_kind"] = "node_observation".into();
    f.manifest["evidence_kind"] = "node_observation".into();
    for v in f.manifest["capabilities"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        v["status"] = "unknown".into();
    }
    f.seal();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("post-merge-blocked");
    f.request["phase"] = "post_pr216_merge".into();
    f.manifest["phase"] = "post_pr216_merge".into();
    f.manifest["merge_gate"] = json!({});
    f.seal();
    f.invoke(3, "blocked");
}
#[test]
fn occurrence_state_and_unknown_values() {
    let mut f = Fixture::new("custody-disagreement");
    f.payload()["occurrences"]["value"][1]["custodian"] = observed(json!("other-validator"));
    let r = f.invoke(1, "product_failure");
    assert_eq!(
        r["measurements"]["counts"]["value"]["custody_disagreements"],
        1
    );
    for (key, value) in [
        ("joined_reason", json!("unspecified")),
        ("tombstone", json!(false)),
        ("retry_authorized", json!(false)),
        ("objective_height", json!("999")),
        ("lifespan", json!("101")),
        ("terminal_outcome", json!("pending")),
    ] {
        let mut f = Fixture::new(&format!("wrong-{key}"));
        f.payload()["occurrences"]["value"][0][key] = observed(value);
        f.invoke(1, "product_failure");
    }
    let mut f = Fixture::new("reason-order-and-duplicates");
    f.payload()["occurrences"]["value"][0]["reason_inputs"] =
        observed(json!(["duplicate", "collateral", "duplicate"]));
    f.invoke(0, "passed");
    let mut f = Fixture::new("missing-state-retains-failure");
    f.payload()["occurrences"]["value"][0]["custodian"] = observed(json!("wrong"));
    f.payload()["occurrences"]["value"][1]["terminal_outcome"] = missing();
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("missing-occurrences-retains-telemetry-failure");
    f.payload()["occurrences"] = missing();
    f.payload()["telemetry"]["block_count"] = observed(json!("9"));
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("unknown-terminal-is-not-zero");
    for sample in f.payload()["occurrences"]["value"].as_array_mut().unwrap() {
        sample["terminal_outcome"] = missing();
    }
    let r = f.invoke(1, "incomplete");
    assert!(
        r["measurements"]["counts"]["value"]["retry_completed"].is_null(),
        "{r}"
    );
}
#[test]
fn receipts_identity_and_duration() {
    for key in [
        "candidate_id",
        "node_id",
        "incarnation",
        "policy_variant",
        "segment",
    ] {
        let mut f = Fixture::new(&format!("foreign-{key}"));
        f.observations[2][key] = "foreign".into();
        f.invoke(1, "incomplete");
    }
    let mut f = Fixture::new("delivery-without-observation");
    f.observations[1]["payload"]["observed"] = false.into();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("delivery-order");
    f.observations[1]["time"]["monotonic_ns"] = "5".into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("receipt-boolean");
    f.observations[0]["payload"]["observed"] = 1.into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("missing-applied-receipt");
    f.payload()["evaluation_receipt"] = Value::Null;
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("cross-clock-duration");
    f.payload()["recovery_time"]["value"]["end"]["clock_id"] = "other-clock".into();
    let r = f.invoke(1, "incomplete");
    assert!(r["measurements"]["recovery_duration_ns"]["value"].is_null());
    let mut f = Fixture::new("snapshot-before-faults");
    f.observations[2]["time"]["monotonic_ns"] = "5".into();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("missing-telemetry");
    f.payload()["telemetry"]["rss_bytes"] = missing();
    f.invoke(1, "incomplete");
}
#[test]
fn qualification_schema_is_pinned() {
    let mut f = Fixture::new("wrong-schema-qualification");
    let path = "qualification/exact-occurrences.json";
    let mut proof = record(&f.root.join(path)).unwrap();
    proof["occurrence_schema"] = "different-identity-definition".into();
    f.manifest["capabilities"]["exact-occurrences"]["qualification"] =
        reference(&f.root, path, &proof, vec![]);
    f.seal();
    f.invoke(2, "invalid_input");
}
#[test]
fn source_pins_and_duplicate_transports() {
    let mut f = Fixture::new("duplicate-transport");
    let mut copy = f.observations[2].clone();
    copy["record_id"] = "copy".into();
    f.observations.push(copy);
    let r = f.invoke(0, "passed");
    assert_eq!(r["measurements"]["counts"]["value"]["observations"], 3);
    f.observations[3]["payload"]["frontier_digest"] = "b".repeat(64).into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("conflicting-event");
    let mut copy = f.observations[2].clone();
    copy["record_id"] = "copy".into();
    copy["payload"]["inventory_complete"] = false.into();
    f.observations.push(copy);
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("corrupt-artifact");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::write(root.join("raw/2.json"), b"{}").unwrap();
    });
    let mut f = Fixture::new("wrong-qualification");
    let path = "qualification/exact-occurrences.json";
    let mut proof = record(&f.root.join(path)).unwrap();
    proof["node_binary_digest"] = "f".repeat(64).into();
    f.manifest["capabilities"]["exact-occurrences"]["qualification"] =
        reference(&f.root, path, &proof, vec![]);
    f.seal();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("missing-inventory");
    f.payload()["inventory_complete"] = false.into();
    f.invoke(1, "incomplete");
}

#[test]
fn frontier_and_lease_evidence() {
    let mut f = Fixture::new("split-frontier");
    f.request["fixture_frontier_kind"] = "split".into();
    f.configure();
    f.invoke(0, "passed");
    f.payload()["frontier_coverage"]["value"]["one_parent"] = true.into();
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("missing-frontier-coverage");
    f.payload()["frontier_coverage"] = missing();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("frontier-artifact-corruption");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::write(root.join("frontier.json"), b"{}").unwrap();
    });
    let mut f = Fixture::new("lease-cannot-override-refusal");
    let mut e = record(&f.root.join("expectation.json")).unwrap();
    e["occurrences"][0]["body_available"] = false.into();
    e["occurrences"][0]["retry_authorized"] = false.into();
    e["occurrences"][0]["terminal_outcome"] = "pending".into();
    let a = reference(&f.root, "expectation.json", &e, vec![]);
    f.manifest["expectation_digest"] = a["sha256"].clone();
    f.request["inputs"]["expectation"] = a;
    f.seal();
    for sample in f.payload()["occurrences"]["value"].as_array_mut().unwrap() {
        if sample["occurrence_id"] == "A" {
            sample["body_available"] = observed(json!(false));
        }
    }
    f.invoke(1, "product_failure");
}
