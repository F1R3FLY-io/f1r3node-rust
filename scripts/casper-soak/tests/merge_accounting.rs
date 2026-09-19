use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use casper_soak::{encoded, hash, record};
use serde_json::{json, Value};
fn binary() -> &'static str { env!("CARGO_BIN_EXE_casper-merge-accounting") }
fn observed(v: Value) -> Value { json!({"presence":"observed","value":v,"reason":null}) }
fn missing() -> Value { json!({"presence":"missing","value":null,"reason":"unavailable"}) }
fn reference(root: &Path, path: &str, v: &Value, ids: Vec<Value>) -> Value {
    let bytes = encoded(v).unwrap();
    fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    fs::write(root.join(path), &bytes).unwrap();
    json!({"path":path,"bytes":bytes.len(),"sha256":hash(&bytes),"producer":"fixture","capture_state":"captured","observation_ids":ids})
}
fn descriptor(name: &str, block: &str, position: Value) -> Value {
    json!({"admission_id":name,"source_block_hash":block.repeat(64),"execution_position":position,"deploy_signature":"same-signature","context_digest":"c".repeat(64)})
}
fn expected(d: &Value) -> Value {
    let mut v = d.clone();
    let rejected = d["execution_position"].is_null();
    v["admission_result"] = if rejected { "rejected" } else { "admitted" }.into();
    v["body_result"] = if rejected { "not_executed" } else { "success" }.into();
    v["effect_digest"] = if rejected {
        "none".to_owned()
    } else {
        "e".repeat(64)
    }
    .into();
    v["settlement"] = json!({"applied":!rejected,"token_domain":"asset-1","prepayment":"100","charge":"30","refund":"70","pooled":"0"});
    v
}
struct Fixture {
    root: PathBuf,
    _temporary: Option<tempfile::TempDir>,
    manifest: Value,
    request: Value,
    expectation: Value,
    observations: Vec<Value>,
    invocation: usize,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let (root, temporary) = if let Ok(base) = std::env::var("SOAK_ACCOUNTING_EVIDENCE") {
            let root = PathBuf::from(base).join(name);
            fs::create_dir_all(root.parent().unwrap()).unwrap();
            fs::create_dir(&root).unwrap();
            (root, None)
        } else {
            let t = tempfile::tempdir().unwrap();
            (t.path().to_owned(), Some(t))
        };
        let output = Command::new(binary()).arg("identity").output().unwrap();
        assert!(output.status.success());
        let own: Value = serde_json::from_slice(&output.stdout).unwrap();
        let manifest = json!({"schema_version":1,"run_id":name,"phase":"pre_pr216_merge","candidate_id":"fixture-node","node_revision":"2".repeat(40),"node_binary_digest":"1".repeat(64),"image_digest":null,"image_digest_reason":"subprocess fixture","harness_revision":"3".repeat(40),"external_harness_revision":"4".repeat(40),"source_digests":own["source_digests"],"profile_id":"merge-accounting","profile_digest":own["profile_digest"],"profile_binary_digest":own["executable_sha256"],"seed":"7","provider":"subprocess","policy_variant":"baseline","evidence_kind":"synthetic_fixture","capabilities":{},"tool_versions":{"profile":own["version"]},"bounds":{"scenarios":2,"observations":3},"assumptions":["controlled transcript"],"resource_limits":{"artifact_bytes":1048576},"required_scenarios":["scenario-1"],"deadline":{"clock_id":"observer-clock","monotonic_ns":"100"},"merge_gate":null});
        let request = json!({"schema_version":1,"profile_id":"merge-accounting","scenario_id":"scenario-1","pair_id":"pair-1","member_id":"member-1","node_id":"node-1","incarnation":"incarnation-1","segment":"1","iteration":"1","execution_schema":"synthetic-execution-position-v1","protocol_epoch":"6","record_version":"legacy","accounting_mode":"legacy_max_union","token_domain":"asset-1","execution_fixture":[descriptor("a","a",json!("0")),descriptor("b","b",json!("0"))],"causal_edges":[{"producer":["a".repeat(64),"0"],"consumer":["b".repeat(64),"0"],"effect_digest":"e".repeat(64)}],"fault_schedule":[{"fault_id":"pause-1","action":"pause","depends_on":[]},{"fault_id":"delivery-1","action":"delay_delivery","depends_on":["pause-1"]}],"observation_deadline":{"clock_id":"observer-clock","monotonic_ns":"100"},"inputs":{}});
        let expectation = json!({"outcomes":[],"aggregate":{"token_domain":"asset-1","balance_before":"1000","balance_after":"940","charge":"60","refund":"140","pool_applications":"1","arithmetic_outcome":"checked"},"rejected_executions":[]});
        let mut f = Self {
            root,
            _temporary: temporary,
            manifest,
            request,
            expectation,
            observations: vec![],
            invocation: 0,
        };
        f.configure();
        f
    }
    fn configure(&mut self) {
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
            self.request[key] = self.manifest[key].clone();
        }
        for key in [
            "execution_schema",
            "protocol_epoch",
            "accounting_mode",
            "record_version",
            "token_domain",
        ] {
            self.manifest[key] = self.request[key].clone();
        }
        self.manifest["capabilities"] = json!({});
        for name in [
            "execution-identities",
            "admission-outcomes",
            "settlement-observations",
            "causal-relationships",
            "accounting-fault-receipts",
        ] {
            let mut proof = json!({"capability":name,"status":"qualified"});
            for key in [
                "provider",
                "node_revision",
                "node_binary_digest",
                "external_harness_revision",
                "profile_digest",
                "profile_binary_digest",
                "evidence_kind",
                "execution_schema",
                "protocol_epoch",
                "accounting_mode",
                "record_version",
                "token_domain",
            ] {
                proof[key] = self.manifest[key].clone();
            }
            self.manifest["capabilities"][name] = json!({"status":"qualified","provider":self.manifest["provider"],"node_revision":self.manifest["node_revision"],"qualification":reference(&self.root,&format!("qualification/{name}.json"),&proof,vec![])});
        }
        self.expectation["outcomes"] = json!(self.request["execution_fixture"]
            .as_array()
            .unwrap()
            .iter()
            .map(expected)
            .collect::<Vec<_>>());
        self.pin();
    }
    fn pin(&mut self) {
        let mut configuration = json!({});
        for key in [
            "execution_schema",
            "protocol_epoch",
            "accounting_mode",
            "record_version",
            "token_domain",
            "policy_variant",
        ] {
            configuration[key] = self.request[key].clone();
        }
        let fixture = json!({"execution_fixture":self.request["execution_fixture"],"causal_edges":self.request["causal_edges"],"fault_schedule":self.request["fault_schedule"]});
        for (name, value) in [
            ("configuration", configuration),
            ("fixture", fixture),
            ("expectation", self.expectation.clone()),
        ] {
            let rf = reference(&self.root, &format!("{name}.json"), &value, vec![]);
            self.manifest[format!("{name}_digest")] = rf["sha256"].clone();
            self.request["inputs"][name] = rf;
        }
        self.observations.clear();
        let mut payloads: Vec<_> = self.request["fault_schedule"]
            .as_array()
            .unwrap()
            .iter()
            .map(|fault| {
                let mut p = fault.clone();
                p["status"] = "applied".into();
                p["observed"] = true.into();
                ("fault_ack", p)
            })
            .collect();
        let mut entries: Vec<_> = self.expectation["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|expected| {
                let mut row = expected.clone();
                for key in [
                    "admission_result",
                    "body_result",
                    "effect_digest",
                    "settlement",
                ] {
                    row[key] = observed(expected[key].clone());
                }
                row
            })
            .collect();
        if !entries.is_empty() {
            entries.push(entries[0].clone());
        }
        let mut p = json!({"fixture_digest":self.request["inputs"]["fixture"]["sha256"],"applied_steps":["load_execution_fixture","capture_accounting"],"entries":observed(json!(entries)),"aggregate":observed(self.expectation["aggregate"].clone()),"causal_edges":observed(self.request["causal_edges"].clone()),"rejected_executions":observed(self.expectation["rejected_executions"].clone())});
        for key in [
            "execution_schema",
            "protocol_epoch",
            "accounting_mode",
            "record_version",
            "token_domain",
        ] {
            p[key] = self.request[key].clone();
        }
        payloads.push(("accounting_snapshot", p));
        for (i, (kind, payload)) in payloads.into_iter().enumerate() {
            let mut v = self.request.clone();
            v["record_id"] = format!("record-{i}").into();
            v["event_id"] = format!("event-{i}").into();
            v["producer"] = "fixture".into();
            v["producer_sequence"] = (i + 1).to_string().into();
            v["event_kind"] = kind.into();
            v["time"] = json!({"clock_id":"observer-clock","monotonic_ns":if kind=="accounting_snapshot" {"90".to_owned()} else {((i+1)*10).to_string()},"utc":"2026-01-01T00:00:00Z"});
            v["presence"] = "observed".into();
            v["reason"] = Value::Null;
            v["payload"] = payload;
            self.observations.push(v);
        }
        self.seal();
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
        let refs: Vec<_> = self
            .observations
            .iter()
            .enumerate()
            .map(|(i, v)| {
                reference(&self.root, &format!("raw/{i}.json"), v, vec![v
                    ["record_id"]
                    .clone()])
            })
            .collect();
        for (name, v) in [
            ("manifest.json", self.manifest.clone()),
            ("request.json", self.request.clone()),
            ("observations.json", json!({"records":refs})),
        ] {
            fs::write(self.root.join(name), encoded(&v).unwrap()).unwrap();
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
        let result = Command::new(binary())
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap();
        fs::write(self.root.join(format!("invocation-{}.json",self.invocation)),encoded(&json!({"command":"casper-merge-accounting","arguments":args,"expected_exit":code,"actual_exit":result.status.code(),"expected_verdict":verdict,"stdout":String::from_utf8_lossy(&result.stdout),"stderr":String::from_utf8_lossy(&result.stderr)})).unwrap()).unwrap();
        assert_eq!(
            result.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
        let value: Value = serde_json::from_slice(&result.stdout).unwrap();
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
fn required_accounting_fixtures() {
    Fixture::new("accounting_complete").invoke(0, "passed");
    let mut f = Fixture::new("accounting_capability_missing");
    f.manifest["capabilities"]["settlement-observations"]["status"] = "unknown".into();
    f.seal();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("accounting_generation");
    f.invoke(0, "passed");
    f.invoke(0, "passed");
    assert_eq!(
        fs::read(f.root.join("output-1/generation.json")).unwrap(),
        fs::read(f.root.join("output-2/generation.json")).unwrap()
    );
    let v = Fixture::new("accounting_multiplicity").invoke(0, "passed");
    assert_eq!(v["measurements"]["counts"]["value"]["executions"], 2);
    assert_eq!(
        v["measurements"]["counts"]["value"]["duplicate_execution_observations"],
        1
    );
    let mut f = Fixture::new("accounting_settlement_missing");
    f.payload()["entries"]["value"][1]["settlement"] = missing();
    let v = f.invoke(1, "incomplete");
    assert!(v["measurements"]["entries"]["value"][1]["settlement"]["value"].is_null());
    let mut f = Fixture::new("accounting_epoch_mismatch");
    f.payload()["protocol_epoch"] = "7".into();
    f.invoke(2, "invalid_input");
}
#[test]
fn execution_identity_and_admission() {
    for field in ["deploy_signature", "context_digest"] {
        let mut f = Fixture::new(&format!("conflicting-execution-{field}"));
        f.payload()["entries"]["value"][2][field] = "f".repeat(64).into();
        f.invoke(2, "invalid_input");
    }
    let mut f = Fixture::new("distinct-position");
    f.request["execution_fixture"][1]["source_block_hash"] = "a".repeat(64).into();
    f.request["execution_fixture"][1]["execution_position"] = "1".into();
    f.request["causal_edges"][0]["consumer"] = json!(["a".repeat(64), "1"]);
    f.configure();
    let v = f.invoke(0, "passed");
    assert_eq!(v["measurements"]["counts"]["value"]["executions"], 2);
    let mut f = Fixture::new("admission-rejection");
    f.request["execution_fixture"]
        .as_array_mut()
        .unwrap()
        .push(descriptor("rejected", "d", Value::Null));
    f.configure();
    let v = f.invoke(0, "passed");
    assert_eq!(v["measurements"]["counts"]["value"]["executions"], 2);
    assert_eq!(
        v["measurements"]["counts"]["value"]["admission_rejections"],
        1
    );
    f.payload()["entries"]["value"][2]["execution_position"] = "0".into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("failed-body-settlement");
    f.expectation["outcomes"][1]["body_result"] = "failure".into();
    f.pin();
    f.invoke(0, "passed");
    f.payload()["entries"]["value"][1]["settlement"] = missing();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("empty-execution-fixture");
    f.request["execution_fixture"] = json!([]);
    f.request["causal_edges"] = json!([]);
    f.configure();
    let v = f.invoke(0, "passed");
    assert_eq!(v["measurements"]["counts"]["value"]["executions"], 0);
}
#[test]
fn missing_measurements_and_known_failures() {
    for field in [
        "admission_result",
        "body_result",
        "effect_digest",
        "settlement",
    ] {
        let mut f = Fixture::new(&format!("missing-{field}"));
        f.payload()["entries"]["value"][1][field] = missing();
        f.invoke(1, "incomplete");
    }
    for field in [
        "entries",
        "aggregate",
        "causal_edges",
        "rejected_executions",
    ] {
        let mut f = Fixture::new(&format!("missing-payload-{field}"));
        f.payload()[field] = missing();
        f.invoke(1, "incomplete");
    }
    let mut f = Fixture::new("loss-is-not-zero");
    f.payload()["entries"]["value"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    let v = f.invoke(1, "incomplete");
    assert!(v["measurements"]["counts"]["value"].is_null());
    let mut f = Fixture::new("unknown-inventory-keeps-failure");
    f.payload()["aggregate"]["value"]["charge"] = "61".into();
    f.payload()["entries"] = missing();
    let v = f.invoke(1, "product_failure");
    assert!(!v["product_failures"].as_array().unwrap().is_empty());
    let mut f = Fixture::new("missing-settlement-keeps-effect-failure");
    f.payload()["entries"]["value"][1]["effect_digest"] = observed(json!("f".repeat(64)));
    f.payload()["entries"]["value"][1]["settlement"] = missing();
    f.invoke(1, "product_failure");
}
#[test]
fn accounting_expectations_and_causal_evidence() {
    for field in ["prepayment", "charge", "refund", "pooled", "applied"] {
        let mut f = Fixture::new(&format!("settlement-mismatch-{field}"));
        f.payload()["entries"]["value"][1]["settlement"]["value"][field] = if field == "applied" {
            json!(false)
        } else {
            json!("999")
        };
        f.invoke(1, "product_failure");
    }
    for field in [
        "balance_before",
        "balance_after",
        "charge",
        "refund",
        "pool_applications",
    ] {
        let mut f = Fixture::new(&format!("aggregate-mismatch-{field}"));
        f.payload()["aggregate"]["value"][field] = "2".into();
        f.invoke(1, "product_failure");
    }
    let mut f = Fixture::new("causal-edge-loss");
    f.payload()["causal_edges"] = observed(json!([]));
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("pinned-rejection-closure");
    f.expectation["rejected_executions"] = json!([["a".repeat(64), "0"], ["b".repeat(64), "0"]]);
    f.pin();
    f.invoke(0, "passed");
    f.payload()["rejected_executions"]["value"] = json!([["a".repeat(64), "0"]]);
    f.invoke(1, "product_failure");
    let mut f = Fixture::new("overflow-outcome");
    f.expectation["aggregate"]["arithmetic_outcome"] = "overflow_rejected".into();
    f.expectation["aggregate"]["balance_before"] = u64::MAX.to_string().into();
    f.pin();
    f.invoke(0, "passed");
    f.payload()["aggregate"]["value"]["arithmetic_outcome"] = "checked".into();
    f.invoke(1, "product_failure");
    for value in [
        json!(true),
        json!("01"),
        json!("-1"),
        json!("18446744073709551616"),
        json!(1.25),
    ] {
        let mut f = Fixture::new(&format!(
            "invalid-amount-{}",
            hash(&encoded(&value).unwrap())
        ));
        f.payload()["aggregate"]["value"]["charge"] = value;
        f.invoke(2, "invalid_input");
    }
}
#[test]
fn receipt_coverage_and_transport_conflicts() {
    let mut f = Fixture::new("unacknowledged-pause");
    f.observations.remove(0);
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("snapshot-before-fault");
    f.observations[2]["time"]["monotonic_ns"] = "5".into();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("missing-applied-steps");
    f.payload()["applied_steps"] = json!([]);
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("boolean-receipt");
    f.observations[0]["payload"]["observed"] = "true".into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("duplicate-transport");
    let mut copy = f.observations[2].clone();
    copy["record_id"] = "copy".into();
    f.observations.push(copy);
    f.invoke(0, "passed");
    for first in [false, true] {
        let mut f = Fixture::new(&format!("late-conflicting-copy-{first}"));
        let mut copy = f.observations[2].clone();
        copy["record_id"] = "copy".into();
        copy["time"]["monotonic_ns"] = "999".into();
        f.observations.insert(if first { 2 } else { 3 }, copy);
        f.invoke(2, "invalid_input");
    }
    let mut f = Fixture::new("foreign-snapshot");
    f.observations[2]["node_id"] = "foreign".into();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("dependency-producer-mismatch");
    f.observations[1]["producer"] = "foreign".into();
    f.invoke(2, "invalid_input");
}
#[test]
fn qualification_policy_and_raw_input_refusals() {
    for field in [
        "protocol_epoch",
        "accounting_mode",
        "record_version",
        "execution_schema",
        "token_domain",
    ] {
        let mut f = Fixture::new(&format!("compatibility-{field}"));
        f.payload()[field] = "different".into();
        f.invoke(2, "invalid_input");
    }
    for field in [
        "execution_schema",
        "node_binary_digest",
        "profile_binary_digest",
    ] {
        let mut f = Fixture::new(&format!("qualification-{field}"));
        let path = "qualification/execution-identities.json";
        let mut proof = record(&f.root.join(path)).unwrap();
        proof[field] = "f".repeat(64).into();
        f.manifest["capabilities"]["execution-identities"]["qualification"] =
            reference(&f.root, path, &proof, vec![]);
        f.seal();
        f.invoke(2, "invalid_input");
    }
    let mut f = Fixture::new("conditional-additive-blocked");
    f.request["accounting_mode"] = "conditional_additive".into();
    f.request["record_version"] = "exact".into();
    f.manifest["policy_variant"] = "conditional-additive".into();
    f.configure();
    f.invoke(3, "blocked");
    assert_eq!(
        record(&f.root.join("output-1/generation.json")).unwrap()["workloads"],
        json!([])
    );
    let mut f = Fixture::new("live-blocked");
    f.manifest["evidence_kind"] = "node_observation".into();
    f.configure();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("post-merge-blocked");
    f.manifest["phase"] = "post_pr216_merge".into();
    f.manifest["merge_gate"] = json!({});
    f.configure();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("corrupt-raw");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::write(root.join("raw/2.json"), b"{}").unwrap()
    });
    let mut f = Fixture::new("symlink-raw");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::remove_file(root.join("raw/2.json")).unwrap();
        std::os::unix::fs::symlink("../request.json", root.join("raw/2.json")).unwrap();
    });
    let mut f = Fixture::new("duplicate-json-key");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::write(
            root.join("request.json"),
            b"{\"schema_version\":1,\"schema_version\":1}",
        )
        .unwrap()
    });
    let mut f = Fixture::new("request-fixture-differs");
    f.request["execution_fixture"][0]["context_digest"] = "d".repeat(64).into();
    f.invoke(2, "invalid_input");
}
