use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use casper_soak::{encoded, hash, record};
use serde_json::{json, Value};

fn binary() -> &'static str { env!("CARGO_BIN_EXE_casper-publication") }
fn observed(value: Value) -> Value { json!({"presence":"observed","value":value,"reason":null}) }
fn missing() -> Value { json!({"presence":"missing","value":null,"reason":"unavailable"}) }
fn reference(root: &Path, path: &str, value: &Value, ids: Vec<String>) -> Value {
    let bytes = encoded(value).unwrap();
    fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    fs::write(root.join(path), &bytes).unwrap();
    json!({"path":path,"bytes":bytes.len(),"sha256":hash(&bytes),"producer":"fixture","capture_state":"captured","observation_ids":ids})
}
fn tuple(generation: &str, value: &str) -> Value {
    json!({"publication_id":"publication-1","generation":generation,"block_hash":value.repeat(64),"state_root":value.repeat(64),"effect_digest":value.repeat(64)})
}
fn terminal(occurrence: &str, generation: &str) -> Value {
    json!({"occurrence_id":occurrence,"deploy_signature":"d".repeat(64),"verdict_id":format!("terminal-{occurrence}"),"verdict":"executed","durable":true,"publication_id":"publication-1","generation":generation})
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
        let (root, temporary) = if let Ok(root) = std::env::var("SOAK_PUBLICATION_EVIDENCE") {
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
        let mut manifest = json!({"schema_version":1,"run_id":name,"phase":"pre_pr216_merge","candidate_id":"fixture-node","node_revision":"2".repeat(40),"node_binary_digest":"1".repeat(64),"image_digest":null,"image_digest_reason":"subprocess fixture","harness_revision":"3".repeat(40),"external_harness_revision":"4".repeat(40),"source_digests":identity["source_digests"],"profile_id":"publication","profile_digest":identity["profile_digest"],"profile_binary_digest":identity["executable_sha256"],"seed":"7","provider":"subprocess","policy_variant":"baseline","evidence_kind":"synthetic_fixture","capabilities":{},"tool_versions":{"profile":identity["version"]},"bounds":{"scenarios":2,"observations":3},"assumptions":["controlled transcript"],"resource_limits":{"artifact_bytes":1048576},"required_scenarios":["scenario-1"],"deadline":{"clock_id":"observer-clock","monotonic_ns":"100"},"merge_gate":null});
        for name in [
            "publication-cut-point",
            "process-exit",
            "linked-restart",
            "atomic-publication-snapshot",
            "durable-work-inventory",
        ] {
            let proof = json!({"capability":name,"status":"qualified","provider":manifest["provider"],"node_revision":manifest["node_revision"],"node_binary_digest":manifest["node_binary_digest"],"external_harness_revision":manifest["external_harness_revision"],"profile_digest":manifest["profile_digest"],"profile_binary_digest":manifest["profile_binary_digest"],"evidence_kind":"synthetic_fixture"});
            manifest["capabilities"][name] = json!({"status":"qualified","provider":manifest["provider"],"node_revision":manifest["node_revision"],"qualification":reference(&root,&format!("qualification/{name}.json"),&proof,vec![])});
        }
        let mut request = json!({"schema_version":1,"profile_id":"publication","scenario_id":"scenario-1","pair_id":"pair-1","member_id":"member-1","node_id":"node-1","segment":"1","iteration":"1","incarnation":"new","predecessor_incarnation":"old","fault_id":"fault-1","publication_id":"publication-1","generation":"2","minimum_generation":"1","cut_point":"after_publication","mode":"single-flight","expected_tuple":tuple("2","b"),"unresolved_occurrences":[{"occurrence_id":"occurrence-1","deploy_signature":"d".repeat(64)},{"occurrence_id":"occurrence-2","deploy_signature":"d".repeat(64)}],"observation_deadline":{"clock_id":"observer-clock","monotonic_ns":"100"},"inputs":{}});
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
        let mut fixture = Self {
            root,
            _temporary: temporary,
            manifest,
            request,
            observations: vec![],
            invocation: 0,
        };
        fixture.configure();
        fixture
    }
    fn configure(&mut self) {
        let mut fixture = json!({});
        for key in [
            "publication_id",
            "generation",
            "minimum_generation",
            "cut_point",
            "expected_tuple",
            "unresolved_occurrences",
        ] {
            fixture[key] = self.request[key].clone();
        }
        let mut permitted = vec![self.request["expected_tuple"].clone()];
        if self.request["cut_point"] == "before_publication" {
            permitted.push(tuple("1", "a"));
        }
        for (key, value) in [
            ("fixture", fixture),
            (
                "configuration",
                json!({"mode":self.request["mode"],"policy_variant":self.request["policy_variant"]}),
            ),
            (
                "expectation",
                json!({"before_tuple":tuple("1","a"),"permitted_tuples":permitted}),
            ),
        ] {
            let reference = reference(&self.root, &format!("{key}.json"), &value, vec![]);
            self.manifest[format!("{key}_digest")] = reference["sha256"].clone();
            self.request["inputs"][key] = reference;
        }
        let mut observations = Vec::new();
        for (index, kind) in ["before_snapshot", "fault_ack", "recovered_snapshot"]
            .iter()
            .enumerate()
        {
            let mut v = json!({"schema_version":1,"record_id":format!("record-{index}"),"event_id":format!("event-{index}"),"event_kind":kind,"producer":"fixture","producer_sequence":(index+1).to_string(),"time":{"clock_id":"observer-clock","monotonic_ns":(["5","35","40"][index]),"utc":"2026-01-01T00:00:00Z"},"presence":"observed","reason":null});
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
                "segment",
                "iteration",
                "seed",
            ] {
                v[key] = self.request[key].clone();
            }
            v["incarnation"] = if index == 2 { "new" } else { "old" }.into();
            v["predecessor_incarnation"] = "old".into();
            if index == 1 {
                let mut p = json!({"action":"crash_then_restart","status":"applied"});
                for key in [
                    "fault_id",
                    "cut_point",
                    "publication_id",
                    "generation",
                    "predecessor_incarnation",
                    "incarnation",
                ] {
                    p[key] = self.request[key].clone();
                }
                for (index, stage) in ["boundary", "exit", "restart"].iter().enumerate() {
                    p[*stage] = json!({"event_id":format!("receipt-{stage}"),"producer":"fixture","observed":true,"clock_id":"observer-clock","monotonic_ns":((index+1)*10).to_string(),"producer_sequence":(index+1).to_string()});
                }
                p["exit"]["exited"] = true.into();
                p["restart"]["ready"] = true.into();
                v["payload"] = p;
            } else {
                v["payload"] = json!({"atomic":true,"fixture_digest":self.request["inputs"]["fixture"]["sha256"],"tuple":observed(if index==0{tuple("1","a")}else{tuple("2","b")}),"retained_work":observed(self.request["unresolved_occurrences"].clone()),"terminal_verdicts":observed(json!([terminal("settled-0","1")]))});
            }
            observations.push(v);
        }
        self.observations = observations;
        self.seal();
    }
    fn seal(&mut self) {
        let digest = hash(&encoded(&self.manifest).unwrap());
        self.request["manifest_digest"] = digest.clone().into();
        for v in &mut self.observations {
            v["manifest_digest"] = digest.clone().into();
        }
    }
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
        fs::write(self.root.join(format!("invocation-{}.json",self.invocation)),encoded(&json!({"command":"casper-publication","arguments":args,"expected_exit":code,"actual_exit":output.status.code(),"expected_verdict":verdict,"stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr)})).unwrap()).unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["scenario_verdict"], verdict, "{result}");
        assert_eq!(result["node_launch_count"], 0);
        assert_eq!(result["soak_verdict"], "non_passing");
        if self.root.join(&out).join("report.json").exists() {
            assert_eq!(
                record(&self.root.join(out).join("report.json")).unwrap(),
                result
            );
        }
        result
    }
}
#[test]
fn required_publication_fixtures() {
    Fixture::new("publication_complete").invoke(0, "passed");
    let mut f = Fixture::new("publication_capability_missing");
    f.manifest["capabilities"]["publication-cut-point"]["status"] = "unsupported".into();
    f.seal();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("publication_generation");
    f.invoke(0, "passed");
    f.invoke(0, "passed");
    assert_eq!(
        record(&f.root.join("output-1/generation.json")).unwrap(),
        record(&f.root.join("output-2/generation.json")).unwrap()
    );
    let mut f = Fixture::new("publication_fault_unacknowledged");
    f.observations[1]["payload"]["boundary"]["observed"] = false.into();
    assert_eq!(
        f.invoke(1, "incomplete")["coverage"]["acknowledged_faults"],
        0
    );
    let mut f = Fixture::new("publication_restart_mismatch");
    f.observations[2]["incarnation"] = "unrelated".into();
    assert_eq!(
        f.invoke(1, "incomplete")["rejected_observations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut f = Fixture::new("publication_torn_tuple");
    f.observations[2]["payload"]["tuple"]["value"]["state_root"] = "a".repeat(64).into();
    f.invoke(1, "product_failure");
}
#[test]
fn complete_tuples_and_work_retention() {
    let mut f = Fixture::new("before-cut-old-tuple");
    f.request["cut_point"] = "before_publication".into();
    f.configure();
    f.observations[2]["payload"]["tuple"] = observed(tuple("1", "a"));
    f.invoke(0, "passed");
    let mut f = Fixture::new("lost-occurrence");
    f.observations[2]["payload"]["retained_work"]["value"]
        .as_array_mut()
        .unwrap()
        .pop();
    let r = f.invoke(1, "product_failure");
    assert!(r["product_failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["kind"] == "lost_unresolved_work"));
    f.observations[2]["payload"]["terminal_verdicts"]["value"]
        .as_array_mut()
        .unwrap()
        .push(terminal("occurrence-2", "2"));
    f.invoke(0, "passed");
    f.observations[2]["payload"]["terminal_verdicts"]["value"][1]["durable"] = false.into();
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("durable-verdict-lost");
    f.observations[2]["payload"]["terminal_verdicts"]["value"] = json!([]);
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("stale-generation");
    f.observations[2]["payload"]["tuple"]["value"]["generation"] = "0".into();
    f.observations[2]["payload"]["terminal_verdicts"]["value"] = json!([]);
    f.invoke(1, "product_failure");
}
#[test]
fn missing_evidence_and_prior_failures_remain_distinct() {
    for field in ["tuple", "retained_work", "terminal_verdicts"] {
        let mut f = Fixture::new(&format!("missing-{field}"));
        f.observations[2]["payload"][field] = missing();
        f.invoke(1, "incomplete");
    }
    let mut f = Fixture::new("failure-then-missing");
    f.observations[0]["payload"]["tuple"]["value"]["effect_digest"] = "f".repeat(64).into();
    f.observations.pop();
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("torn-with-missing-verdicts");
    f.observations[2]["payload"]["tuple"]["value"]["block_hash"] = "a".repeat(64).into();
    f.observations[2]["payload"]["terminal_verdicts"] = missing();
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("empty-transcript");
    f.observations.clear();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("lost-work-with-unknown-prior-verdicts");
    f.observations[0]["payload"]["terminal_verdicts"] = missing();
    f.observations[2]["payload"]["retained_work"]["value"] = json!([]);
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("lost-verdict-with-unknown-work");
    f.observations[2]["payload"]["retained_work"] = missing();
    f.observations[2]["payload"]["terminal_verdicts"]["value"] = json!([]);
    f.invoke(1, "product_failure");
}
#[test]
fn receipt_and_restart_identities_are_required() {
    for field in [
        "candidate_id",
        "node_id",
        "incarnation",
        "predecessor_incarnation",
        "scenario_id",
        "segment",
        "iteration",
    ] {
        let mut f = Fixture::new(&format!("foreign-{field}"));
        f.observations[2][field] = "foreign".into();
        f.invoke(1, "incomplete");
    }
    for stage in ["boundary", "exit", "restart"] {
        let mut f = Fixture::new(&format!("receipt-missing-{stage}"));
        f.observations[1]["payload"][stage]["observed"] = false.into();
        assert_eq!(
            f.invoke(1, "incomplete")["coverage"]["acknowledged_faults"],
            0
        );
    }
    let mut f = Fixture::new("receipt-wrong-successor");
    f.observations[1]["payload"]["incarnation"] = "unrelated".into();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("receipt-not-applied");
    f.observations[1]["payload"]["status"] = "not_applied".into();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("receipt-boolean");
    f.observations[1]["payload"]["exit"]["exited"] = 1.into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("receipt-order");
    f.observations[1]["payload"]["restart"]["producer_sequence"] = "1".into();
    f.invoke(2, "invalid_input");
}
#[test]
fn copies_corruption_and_policy_gates() {
    let mut f = Fixture::new("duplicate-copy");
    let mut copy = f.observations[2].clone();
    copy["record_id"] = "copy".into();
    f.observations.push(copy);
    f.invoke(0, "passed");
    f.observations[3]["payload"]["tuple"]["value"]["block_hash"] = "a".repeat(64).into();
    f.invoke(2, "invalid_input");
    for field in [
        "cut_point",
        "publication_id",
        "generation",
        "predecessor_incarnation",
        "incarnation",
        "clock_id",
        "monotonic_ns",
        "utc",
        "snapshot_predecessor",
    ] {
        for copy_first in [false, true] {
            let mut f = Fixture::new(&format!("conflicting-copy-{field}-{copy_first}"));
            let index = if field == "snapshot_predecessor" {
                2
            } else {
                1
            };
            let mut copy = f.observations[index].clone();
            copy["record_id"] = "conflicting-copy".into();
            match field {
                "clock_id" => copy["time"][field] = "other-clock".into(),
                "monotonic_ns" => copy["time"][field] = "999".into(),
                "utc" => copy["time"][field] = "2026-01-02T00:00:00Z".into(),
                "snapshot_predecessor" => copy["predecessor_incarnation"] = "unrelated".into(),
                "generation" => copy["payload"][field] = "999".into(),
                _ => copy["payload"][field] = "unrelated".into(),
            }
            f.observations
                .insert(index + usize::from(!copy_first), copy);
            let result = f.invoke(2, "invalid_input");
            assert!(result["rejected_observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| { v["fatal"] == true && v["reason"] == "Copies of an event disagree." }));
        }
    }
    for field in ["candidate_id", "scenario_id", "member_id"] {
        let mut f = Fixture::new(&format!("foreign-event-{field}"));
        let mut copy = f.observations[1].clone();
        copy["record_id"] = "foreign-copy".into();
        copy[field] = "foreign".into();
        copy["payload"]["generation"] = "999".into();
        f.observations.insert(1, copy);
        f.invoke(0, "passed");
    }
    let mut f = Fixture::new("conflict-retains-product-failure");
    f.observations[2]["payload"]["tuple"]["value"]["block_hash"] = "a".repeat(64).into();
    let mut copy = f.observations[1].clone();
    copy["record_id"] = "conflicting-copy".into();
    copy["payload"]["generation"] = "999".into();
    f.observations.push(copy);
    let result = f.invoke(2, "invalid_input");
    assert!(!result["product_failures"].as_array().unwrap().is_empty());
    let mut f = Fixture::new("corrupt-raw");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::write(root.join("raw/2.json"), b"{}").unwrap();
    });
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
    let mut f = Fixture::new("parallel-blocked");
    f.request["mode"] = "parallel".into();
    f.request["policy_variant"] = "publication-parallel".into();
    f.manifest["policy_variant"] = "publication-parallel".into();
    f.configure();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("binary-mismatch");
    f.manifest["profile_binary_digest"] = "f".repeat(64).into();
    f.seal();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("symlinked-record");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::remove_file(root.join("raw/2.json")).unwrap();
        std::os::unix::fs::symlink("../fixture.json", root.join("raw/2.json")).unwrap();
    });
    let mut f = Fixture::new("post-merge-blocked");
    f.request["phase"] = "post_pr216_merge".into();
    f.manifest["phase"] = "post_pr216_merge".into();
    f.manifest["merge_gate"] = json!({});
    f.seal();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("qualification-wrong-node-binary");
    let path = "qualification/linked-restart.json";
    let mut proof = record(&f.root.join(path)).unwrap();
    proof["node_binary_digest"] = "f".repeat(64).into();
    f.manifest["capabilities"]["linked-restart"]["qualification"] =
        reference(&f.root, path, &proof, vec![]);
    f.seal();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("recovery-too-early");
    f.observations[2]["time"]["monotonic_ns"] = "25".into();
    f.invoke(1, "incomplete");
}

#[test]
fn tuple_components_are_never_combined_across_generations() {
    for cut in ["before_publication", "after_publication"] {
        for bits in 0..8 {
            let mut f = Fixture::new(&format!("tuple-{cut}-{bits}"));
            f.request["cut_point"] = cut.into();
            f.configure();
            let mut value = tuple(if bits == 0 { "1" } else { "2" }, "a");
            for (index, key) in ["block_hash", "state_root", "effect_digest"]
                .iter()
                .enumerate()
            {
                if bits & (1 << index) != 0 {
                    value[*key] = "b".repeat(64).into();
                }
            }
            f.observations[2]["payload"]["tuple"] = observed(value);
            let passes = bits == 7 || (bits == 0 && cut == "before_publication");
            f.invoke(
                if passes { 0 } else { 1 },
                if passes { "passed" } else { "product_failure" },
            );
        }
    }
}
