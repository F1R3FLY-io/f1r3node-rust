use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use casper_soak::{encoded, hash, record};
use serde_json::{json, Value};

fn binary() -> &'static str { env!("CARGO_BIN_EXE_casper-authority-finality") }
fn observed(value: Value) -> Value { json!({"presence":"observed","value":value,"reason":null}) }
fn missing() -> Value { json!({"presence":"missing","value":null,"reason":"not available"}) }
fn reference(root: &Path, name: &str, value: &Value, ids: Vec<String>) -> Value {
    let bytes = encoded(value).unwrap();
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, &bytes).unwrap();
    json!({"path":name,"bytes":bytes.len(),"sha256":hash(&bytes),"producer":"fixture-producer","capture_state":"captured","observation_ids":ids})
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
        let (root, temporary) = if let Ok(base) = std::env::var("SOAK_AUTHORITY_EVIDENCE") {
            let root = PathBuf::from(base).join(name);
            fs::create_dir_all(root.parent().unwrap()).unwrap();
            fs::create_dir(&root).unwrap();
            (root, None)
        } else {
            let temporary = tempfile::tempdir().unwrap();
            (temporary.path().to_path_buf(), Some(temporary))
        };
        let output = Command::new(binary()).arg("identity").output().unwrap();
        assert!(output.status.success());
        let identity: Value = serde_json::from_slice(&output.stdout).unwrap();
        let digest = "1".repeat(64);
        let revision = "2".repeat(40);
        let mut manifest = json!({
            "schema_version":1,"run_id":name,"phase":"pre_pr216_merge","candidate_id":"fixture-node",
            "node_revision":revision,"node_binary_digest":digest,"image_digest":null,"image_digest_reason":"subprocess fixture",
            "harness_revision":"3".repeat(40),"external_harness_revision":"4".repeat(40),"source_digests":identity["source_digests"],
            "configuration_digest":"5".repeat(64),"profile_id":"authority-finality","profile_digest":identity["profile_digest"],
            "profile_binary_digest":identity["executable_sha256"],"fixture_digest":digest,"expectation_digest":digest,
            "seed":"7","provider":"subprocess","policy_variant":"baseline","evidence_kind":"synthetic_fixture",
            "capabilities":{},"tool_versions":{"profile":identity["version"]},"bounds":{"scenarios":2,"observations":3},
            "assumptions":["controlled transcript"],"resource_limits":{"artifact_bytes":1048576},
            "required_scenarios":["scenario-1"],"deadline":{"clock_id":"fixture-clock","epoch_seconds":"9999999999"},"merge_gate":null
        });
        for capability in [
            "same-dag-evaluation",
            "electorate-context",
            "finality-decision",
            "original-ft-projection",
            "traversal-counters",
            "fault-receipts",
            "replay-control",
            "justification-injection",
            "dependency-control",
        ] {
            let proof = json!({"capability":capability,"provider":"subprocess","node_revision":revision,"profile_digest":identity["profile_digest"],"evidence_kind":"synthetic_fixture","status":"qualified","profile_binary_digest":manifest["profile_binary_digest"],"external_harness_revision":manifest["external_harness_revision"]});
            let proof_ref = reference(
                &root,
                &format!("qualification/{capability}.json"),
                &proof,
                vec![],
            );
            manifest["capabilities"][capability] = json!({"status":"qualified","provider":"subprocess","node_revision":revision,"qualification":proof_ref});
        }
        let mut inputs = json!({});
        for name in ["dag", "electorate", "justification"] {
            inputs[name] = reference(
                &root,
                &format!("{name}.json"),
                &json!({"fixture":name,"entries":["entry-1","entry-1"],"provenance":"controlled upstream fixture"}),
                vec![],
            );
        }
        let members: Vec<_> = ["reference", "bounded"].iter().map(|mode| json!({"member_id":mode,"evaluation_mode":mode,"candidate_id":"fixture-node","node_revision":revision,"node_binary_digest":digest,"node_id":format!("node-{mode}"),"incarnation":"incarnation-1","dag_digest":inputs["dag"]["sha256"],"electorate_digest":inputs["electorate"]["sha256"],"justification_digest":inputs["justification"]["sha256"]})).collect();
        let request = json!({"schema_version":1,"profile_id":"authority-finality","manifest_digest":digest,"run_id":manifest["run_id"],"scenario_id":"scenario-1","pair_id":"pair-1","phase":"pre_pr216_merge","evidence_kind":"synthetic_fixture","seed":"7","segment":"1","iteration":"1","policy_variant":"baseline","scenario_kind":"threshold_boundary","members":members,"inputs":inputs,"threshold_inputs":{"q":"8","S":"12","agreeing_stake":"12","n":"1","d":"3"},"metadata_availability":"complete","protocol_context":{"protocol":"7","accounting_authority":"8"},"require_work":true,"fault_schedule":[],"observation_deadline":{"clock_id":"fixture-clock","monotonic_ns":"100"}});
        let mut fixture = Self {
            root,
            _temporary: temporary,
            manifest,
            request,
            observations: Vec::new(),
            invocation: 0,
        };
        fixture.configure("threshold_boundary", "8", "12", "1", "finalized", true);
        fixture
    }
    fn configure(
        &mut self,
        kind: &str,
        q: &str,
        agreeing: &str,
        n: &str,
        decision: &str,
        component: bool,
    ) {
        self.request["scenario_kind"] = kind.into();
        self.request["metadata_availability"] = if decision == "hold" {
            "missing_dependencies"
        } else {
            "complete"
        }
        .into();
        let configuration = reference(
            &self.root,
            "configuration.json",
            &json!({"protocol_context":self.request["protocol_context"],"policy_variant":self.request["policy_variant"]}),
            vec![],
        );
        self.manifest["configuration_digest"] = configuration["sha256"].clone();
        self.request["inputs"]["configuration"] = configuration;
        self.request["threshold_inputs"]["q"] = q.into();
        self.request["threshold_inputs"]["agreeing_stake"] = agreeing.into();
        self.request["threshold_inputs"]["n"] = n.into();
        let fixture = json!({"scenario_kind":kind,"threshold_inputs":self.request["threshold_inputs"],"metadata_availability":self.request["metadata_availability"],"protocol_context":self.request["protocol_context"]});
        let finality = json!({"decision":decision,"hold_reason":if decision=="hold"{json!("missing dependencies")}else{Value::Null},"threshold_pass":component,"original_ft":if decision=="hold"{missing()}else{observed(json!({"numerator":"1","denominator":"3"}))},"projection":if decision=="hold"{missing()}else{observed(json!({"numerator":"1","denominator":"3"}))}});
        let expectation = json!({"head_hash":"a".repeat(64),"finality":finality});
        for (name, value) in [("fixture", fixture), ("expectation", expectation.clone())] {
            let r = reference(&self.root, &format!("{name}.json"), &value, vec![]);
            self.manifest[format!("{name}_digest")] = r["sha256"].clone();
            self.request["inputs"][name] = r;
        }
        let steps = match kind {
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
        };
        self.observations = self.request["members"].as_array().unwrap().iter().map(|member| {
            let mut v = member.clone();
            for key in ["run_id", "scenario_id", "pair_id", "seed", "segment", "iteration", "protocol_context", "metadata_availability", "threshold_inputs", "phase", "evidence_kind", "manifest_digest"] { v[key] = self.request[key].clone(); }
            v["schema_version"] = 1.into();
            v["record_id"] = format!("record-{}", member["member_id"].as_str().unwrap()).into();
            v["event_id"] = format!("event-{}", member["member_id"].as_str().unwrap()).into();
            v["event_kind"] = "authority_snapshot".into();
            v["producer"] = "fixture-producer".into();
            v["producer_sequence"] = "1".into();
            v["time"] = json!({"clock_id":"fixture-clock","monotonic_ns":"10","utc":"2026-01-01T00:00:00Z"});
            v["presence"] = "observed".into();
            v["reason"] = Value::Null;
            v["payload"] = json!({"evaluation_receipt":observed(json!({"status":"applied","steps":steps,"fixture_digest":self.request["inputs"]["fixture"]["sha256"]})),"head":observed(expectation["head_hash"].clone()),"finality":observed(finality.clone()),"work":observed(json!({"visited_vertices":"0","traversed_edges":"0"}))});
            v
        }).collect();
        self.seal();
    }
    fn seal(&mut self) {
        let digest = hash(&encoded(&self.manifest).unwrap());
        self.request["manifest_digest"] = digest.clone().into();
        for observation in &mut self.observations {
            observation["manifest_digest"] = digest.clone().into();
        }
    }
    fn invoke(&mut self, expected_code: i32, expected_verdict: &str) -> Value {
        self.invoke_edited(expected_code, expected_verdict, |_| {})
    }
    fn invoke_edited(
        &mut self,
        expected_code: i32,
        expected_verdict: &str,
        edit: impl FnOnce(&Path),
    ) -> Value {
        self.invocation += 1;
        let refs: Vec<_> = self
            .observations
            .iter()
            .enumerate()
            .map(|(i, v)| {
                reference(&self.root, &format!("observations/{i}.json"), v, vec![v
                    ["record_id"]
                    .as_str()
                    .unwrap()
                    .to_owned()])
            })
            .collect();
        fs::write(
            self.root.join("observations.json"),
            encoded(&json!({"records":refs})).unwrap(),
        )
        .unwrap();
        fs::write(
            self.root.join("manifest.json"),
            encoded(&self.manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            self.root.join("request.json"),
            encoded(&self.request).unwrap(),
        )
        .unwrap();
        edit(&self.root);
        let output_name = format!("output-{}", self.invocation);
        let arguments = [
            "run",
            "--manifest",
            "manifest.json",
            "--request",
            "request.json",
            "--artifacts",
            ".",
            "--output",
            &output_name,
        ];
        let output = Command::new(binary())
            .args(arguments)
            .current_dir(&self.root)
            .output()
            .unwrap();
        fs::write(self.root.join(format!("invocation-{}.json", self.invocation)), encoded(&json!({"command":["casper-authority-finality",arguments],"expected_exit":expected_code,"actual_exit":output.status.code(),"expected_verdict":expected_verdict,"stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr)})).unwrap()).unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected_code),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["scenario_verdict"], expected_verdict);
        assert_eq!(result["node_launch_count"], 0);
        assert_eq!(result["soak_verdict"], "non_passing");
        if self.root.join(&output_name).join("report.json").exists() {
            assert_eq!(
                record(&self.root.join(output_name).join("report.json")).unwrap(),
                result
            );
        }
        result
    }
}

#[test]
fn required_authority_fixture_contracts() {
    let mut complete = Fixture::new("authority_finality_complete");
    complete.invoke(0, "passed");
    let mut blocked = Fixture::new("authority_finality_capability_missing");
    blocked.request["capabilities"] = blocked.manifest["capabilities"].clone();
    blocked.manifest["capabilities"]["same-dag-evaluation"]["status"] = "unsupported".into();
    blocked.seal();
    blocked.invoke(3, "blocked");
    let mut generation = Fixture::new("authority_finality_generation");
    generation.invoke(0, "passed");
    generation.invoke(0, "passed");
    let a = record(&generation.root.join("output-1/generation.json")).unwrap();
    let b = record(&generation.root.join("output-2/generation.json")).unwrap();
    assert_eq!(a, b);
    assert_ne!(
        a["workloads"][0]["member"]["evaluation_mode"],
        a["workloads"][1]["member"]["evaluation_mode"]
    );
    assert_eq!(a["workloads"][0]["inputs"], a["workloads"][1]["inputs"]);
    let mut pair = Fixture::new("authority_pair_mismatch");
    pair.request["members"][1]["dag_digest"] = "b".repeat(64).into();
    pair.invoke(2, "invalid_input");
    let mut absent = Fixture::new("authority_finality_missing");
    absent.observations[1]["payload"]["finality"] = missing();
    absent.invoke(1, "incomplete");
    let mut mismatch = Fixture::new("authority_head_mismatch");
    mismatch.observations[1]["payload"]["head"]["value"] = "b".repeat(64).into();
    let result = mismatch.invoke(1, "product_failure");
    assert!(result["product_failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "head_mismatch" && f["sources"].as_array().unwrap().len() == 2));
}

#[test]
fn threshold_boundaries_and_explicit_holds() {
    let mut rational = Fixture::new("equivalent-fractions");
    rational.observations[0]["payload"]["finality"]["value"]["original_ft"]["value"] =
        json!({"numerator":"2","denominator":"6"});
    rational.invoke(0, "passed");
    for (name, q, agree, n, decision, component) in [
        ("below", "7", "12", "1", "not_finalized", false),
        ("equal", "8", "12", "1", "finalized", true),
        ("above", "9", "12", "1", "finalized", true),
        ("strict-majority", "6", "6", "0", "not_finalized", false),
        ("explicit-hold", "8", "12", "1", "hold", true),
    ] {
        let mut f = Fixture::new(name);
        f.configure("threshold_boundary", q, agree, n, decision, component);
        assert_eq!(
            f.invoke(0, "passed")["threshold_component_expected"],
            component
        );
        if decision == "hold" {
            f.observations[0]["payload"]["finality"] = missing();
            f.invoke(1, "incomplete");
        }
    }
    for (index, value) in [
        json!(true),
        json!("01"),
        json!("-1"),
        json!("18446744073709551616"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut f = Fixture::new(&format!("invalid-number-{index}"));
        f.request["threshold_inputs"]["q"] = value;
        f.invoke(2, "invalid_input");
    }
}

#[test]
fn correlated_evidence_rejects_relabeling_and_conflicting_copies() {
    let mut copies = Fixture::new("duplicate-copy");
    let mut duplicate = copies.observations[0].clone();
    duplicate["record_id"] = "copy-1".into();
    copies.observations.push(duplicate);
    let r = copies.invoke(0, "passed");
    assert_eq!(r["measurements"].as_array().unwrap().len(), 2);
    assert_eq!(
        record(&copies.root.join("output-1/collection.json")).unwrap()["duplicate_sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    copies.observations[2]["payload"]["head"]["value"] = "c".repeat(64).into();
    assert!(!copies.invoke(2, "invalid_input")["rejected_observations"]
        .as_array()
        .unwrap()
        .is_empty());
    for field in [
        "scenario_id",
        "pair_id",
        "candidate_id",
        "member_id",
        "node_id",
        "incarnation",
        "dag_digest",
        "electorate_digest",
        "justification_digest",
        "seed",
        "manifest_digest",
        "evaluation_mode",
        "event_kind",
        "segment",
        "iteration",
        "protocol_context",
        "threshold_inputs",
    ] {
        let mut f = Fixture::new(&format!("wrong-{field}"));
        f.observations[0][field] = "wrong".into();
        f.invoke(2, "invalid_input");
    }
}

#[test]
fn unknown_measurements_never_become_zero_or_erase_failures() {
    let mut f = Fixture::new("failure-with-missing-work");
    f.observations[0]["payload"]["head"]["value"] = "c".repeat(64).into();
    f.observations[1]["payload"]["work"] = missing();
    let r = f.invoke(1, "product_failure");
    assert!(!r["missing_observations"].as_array().unwrap().is_empty());
    let mut f = Fixture::new("missing-work");
    f.observations[1]["payload"]["work"] = missing();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("boolean-work");
    f.observations[1]["payload"]["work"]["value"]["visited_vertices"] = true.into();
    f.invoke(2, "invalid_input");
    let mut ft = Fixture::new("missing-original-ft");
    ft.observations[0]["payload"]["finality"]["value"]["original_ft"] = missing();
    let r = ft.invoke(1, "incomplete");
    assert!(r["product_failures"].as_array().unwrap().is_empty());
    let mut f = Fixture::new("empty-observations");
    f.observations.clear();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("late-observation");
    f.observations[1]["time"]["monotonic_ns"] = "101".into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("missing-with-value");
    f.observations[0]["payload"]["work"]["presence"] = "missing".into();
    f.invoke(2, "invalid_input");
}

#[test]
fn scenario_generation_preserves_context_and_requires_restart_receipts() {
    for kind in [
        "committee_provenance",
        "duplicate_justifications",
        "signature_rejection",
        "replay",
        "missing_dependencies",
        "traversal_comparison",
    ] {
        let mut f = Fixture::new(kind);
        let decision = match kind {
            "signature_rejection" => "rejected",
            "missing_dependencies" => "hold",
            _ => "finalized",
        };
        f.configure(kind, "8", "12", "1", decision, true);
        f.invoke(0, "passed");
        if kind == "replay" {
            f.observations[0]["payload"]["evaluation_receipt"] = missing();
            f.invoke(1, "incomplete");
        }
        let generation = record(&f.root.join("output-1/generation.json")).unwrap();
        assert_eq!(generation["workloads"][0]["scenario_kind"], kind);
        assert_eq!(
            generation["workloads"][0]["inputs"]["justification"],
            f.request["inputs"]["justification"]
        );
    }
    let mut f = Fixture::new("restart");
    f.request["members"][0]["predecessor_incarnation"] = "incarnation-1".into();
    f.request["members"][0]["incarnation"] = "incarnation-2".into();
    f.request["fault_schedule"] = json!([{"fault_id":"restart-1","member_id":"reference","node_id":"node-reference","incarnation":"incarnation-1","action":"restart","trigger_event":"fixture-ready","after":[],"ack_deadline":{"clock_id":"fixture-clock","monotonic_ns":"90"}}]);
    f.configure("restart", "8", "12", "1", "finalized", true);
    f.invoke(1, "incomplete");
    let mut acknowledgment = f.observations[0].clone();
    acknowledgment["record_id"] = "restart-ack".into();
    acknowledgment["event_id"] = "restart-event".into();
    acknowledgment["event_kind"] = "fault_ack".into();
    acknowledgment["producer_sequence"] = "2".into();
    acknowledgment["payload"] = json!({"fault_id":"restart-1","action":"restart","incarnation":"incarnation-1","trigger_event":"fixture-ready","status":"applied","prior_exit":true,"ready":true,"new_incarnation":"incarnation-2"});
    f.observations.push(acknowledgment);
    f.invoke(0, "passed");
    f.observations[2]["payload"]["ready"] = false.into();
    f.invoke(1, "incomplete");
    f.observations[2]["payload"]["ready"] = true.into();
    f.observations[2]["candidate_id"] = "wrong-candidate".into();
    f.invoke(2, "invalid_input");
}

#[test]
fn unqualified_live_and_post_merge_requests_are_blocked() {
    let mut live = Fixture::new("live-unqualified");
    live.manifest["evidence_kind"] = "node_observation".into();
    live.request["evidence_kind"] = "node_observation".into();
    for cap in live.manifest["capabilities"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        cap["status"] = "unknown".into();
    }
    live.seal();
    live.invoke(3, "blocked");
    let mut post = Fixture::new("post-merge-unqualified");
    post.manifest["phase"] = "post_pr216_merge".into();
    post.manifest["merge_gate"] = json!({});
    post.request["phase"] = "post_pr216_merge".into();
    post.seal();
    post.invoke(3, "blocked");
    let mut bad = Fixture::new("wrong-profile-binary");
    bad.manifest["profile_binary_digest"] = "f".repeat(64).into();
    bad.seal();
    bad.invoke(2, "invalid_input");
}

#[test]
fn corrupted_artifacts_and_fixture_expectations_cannot_pass() {
    let mut f = Fixture::new("corrupt-source-digest");
    let result = f.invoke_edited(2, "invalid_input", |root| {
        let mut inventory = record(&root.join("observations.json")).unwrap();
        inventory["records"][0]["sha256"] = "0".repeat(64).into();
        fs::write(root.join("observations.json"), encoded(&inventory).unwrap()).unwrap();
    });
    assert_eq!(result["retained_sources"].as_array().unwrap().len(), 2);
    let mut f = Fixture::new("malformed-raw-json");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::write(
            root.join("observations/0.json"),
            b"{\"duplicate\":1,\"duplicate\":2}",
        )
        .unwrap();
    });
    let mut f = Fixture::new("symlinked-observation");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::remove_file(root.join("observations/0.json")).unwrap();
        std::os::unix::fs::symlink("../dag.json", root.join("observations/0.json")).unwrap();
    });
    let mut f = Fixture::new("escape-observation");
    f.invoke_edited(2, "invalid_input", |root| {
        let mut inventory = record(&root.join("observations.json")).unwrap();
        inventory["records"][0]["path"] = "../outside.json".into();
        fs::write(root.join("observations.json"), encoded(&inventory).unwrap()).unwrap();
    });
    let mut f = Fixture::new("forged-inline-expectation");
    f.request["expected"] = json!({"head_hash":"b".repeat(64)});
    f.observations[0]["payload"]["head"]["value"] = "b".repeat(64).into();
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("invalid-finalization-expectation");
    f.configure("threshold_boundary", "7", "12", "1", "finalized", false);
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("stale-qualification");
    f.manifest["external_harness_revision"] = "9".repeat(40).into();
    f.seal();
    f.invoke(2, "invalid_input");
}
