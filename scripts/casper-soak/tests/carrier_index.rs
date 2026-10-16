use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use casper_soak::{encoded, hash, record};
use serde_json::{json, Value};

fn binary() -> &'static str { env!("CARGO_BIN_EXE_casper-carrier-index") }
fn observed(value: Value) -> Value { json!({"presence":"observed","value":value,"reason":null}) }
fn missing() -> Value { json!({"presence":"missing","value":null,"reason":"unavailable"}) }
fn reference(root: &Path, path: &str, value: &Value, ids: Vec<Value>) -> Value {
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
    expectation: Value,
    observations: Vec<Value>,
    invocation: usize,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let (root, temporary) = if let Ok(base) = std::env::var("SOAK_CARRIER_EVIDENCE") {
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
        let manifest = json!({"schema_version":1,"run_id":name,"phase":"pre_pr216_merge","candidate_id":"fixture-node","node_revision":"2".repeat(40),"node_binary_digest":"1".repeat(64),"image_digest":null,"image_digest_reason":"subprocess fixture","harness_revision":"3".repeat(40),"external_harness_revision":"4".repeat(40),"source_digests":own["source_digests"],"profile_id":"carrier-index","profile_digest":own["profile_digest"],"profile_binary_digest":own["executable_sha256"],"seed":"7","provider":"subprocess","policy_variant":"baseline","evidence_kind":"synthetic_fixture","identity_domain":"raw_user_deploy_signature","capabilities":{},"tool_versions":{"profile":own["version"]},"bounds":{"scenarios":2,"observations":3},"assumptions":["controlled transcript"],"resource_limits":{"artifact_bytes":1048576},"required_scenarios":["scenario-1"],"deadline":{"clock_id":"observer-clock","monotonic_ns":"100"},"merge_gate":null});
        let request = json!({"schema_version":1,"profile_id":"carrier-index","scenario_id":"scenario-1","pair_id":"pair-1","segment":"1","iteration":"1","dag_digest":"d".repeat(64),"deploy_signature":"ab".repeat(64),"scan_window":{"lower":"10","upper":"30"},"availability_digest":"a".repeat(64),"watermark":"30","retention_boundary":"10","carrier_case":"valid","members":[],"fault_schedule":[],"observation_deadline":{"clock_id":"observer-clock","monotonic_ns":"100"},"inputs":{}});
        let result =
            json!({"verdict":"repeat","carriers":[{"block_hash":"b".repeat(64),"status":"valid"}]});
        let expectation = json!({"members":{"index":{"result":result,"fallback_reason":"none"},"reference":{"result":result,"fallback_reason":"none"}}});
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
            "identity_domain",
        ] {
            self.request[key] = self.manifest[key].clone();
        }
        let mut members = Vec::new();
        for path in ["index", "reference"] {
            let mut m = json!({"member_id":path,"requested_path":path,"node_id":format!("node-{path}"),"incarnation":"current","previous_incarnation":if self.request["carrier_case"]=="restart" {json!("previous")} else {Value::Null}});
            for key in [
                "candidate_id",
                "node_revision",
                "node_binary_digest",
                "dag_digest",
                "deploy_signature",
                "scan_window",
                "availability_digest",
                "watermark",
                "retention_boundary",
                "identity_domain",
                "carrier_case",
            ] {
                m[key] = self.request[key].clone();
            }
            members.push(m);
        }
        self.request["members"] = json!(members);
        self.manifest["capabilities"] = json!({});
        for name in [
            "carrier-path-selection",
            "carrier-path-receipts",
            "carrier-work-counters",
            "carrier-availability-controls",
            "carrier-fault-receipts",
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
                "identity_domain",
            ] {
                proof[key] = self.manifest[key].clone();
            }
            self.manifest["capabilities"][name] = json!({"status":"qualified","provider":self.manifest["provider"],"node_revision":self.manifest["node_revision"],"qualification":reference(&self.root,&format!("qualification/{name}.json"),&proof,vec![])});
        }
        self.pin();
    }
    fn pin(&mut self) {
        let mut configuration = json!({});
        for key in [
            "dag_digest",
            "deploy_signature",
            "scan_window",
            "availability_digest",
            "watermark",
            "retention_boundary",
            "identity_domain",
            "carrier_case",
            "policy_variant",
        ] {
            configuration[key] = self.request[key].clone();
        }
        let fixture = json!({"members":self.request["members"],"fault_schedule":self.request["fault_schedule"]});
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
        for m in self.request["members"].as_array().unwrap() {
            let name = m["member_id"].as_str().unwrap();
            let mut payloads = Vec::new();
            for fault in self.request["fault_schedule"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["member_id"] == m["member_id"])
            {
                let mut p = fault.clone();
                p["status"] = "applied".into();
                p["observed_state"] = format!("{}_applied", p["action"].as_str().unwrap()).into();
                p["previous_exited"] = true.into();
                p["ready"] = true.into();
                payloads.push(("fault_ack", p));
            }
            let mut p = m.clone();
            p["fixture_digest"] = self.request["inputs"]["fixture"]["sha256"].clone();
            p["engaged_path"] = observed(m["requested_path"].clone());
            p["result"] = observed(self.expectation["members"][name]["result"].clone());
            p["fallback_reason"] =
                observed(self.expectation["members"][name]["fallback_reason"].clone());
            p["probe_count"] = observed(json!("0"));
            p["ancestor_body_read_count"] =
                observed(json!(if name == "index" { "0" } else { "3" }));
            payloads.push(("carrier_snapshot", p));
            for (i, (kind, payload)) in payloads.into_iter().enumerate() {
                let mut v = self.request.clone();
                for key in [
                    "member_id",
                    "node_id",
                    "incarnation",
                    "previous_incarnation",
                ] {
                    v[key] = m[key].clone();
                }
                v["record_id"] = format!("record-{name}-{i}").into();
                v["event_id"] = format!("event-{name}-{i}").into();
                v["producer"] = "fixture".into();
                v["producer_sequence"] = (i + 1).to_string().into();
                v["event_kind"] = kind.into();
                v["time"] = json!({"clock_id":"observer-clock","monotonic_ns":if kind=="carrier_snapshot" {"90".to_owned()} else {((i+1)*10).to_string()},"utc":"2026-09-19T00:00:00Z"});
                v["presence"] = "observed".into();
                v["reason"] = Value::Null;
                v["payload"] = payload;
                self.observations.push(v);
            }
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
    fn payload(&mut self, name: &str) -> &mut Value {
        &mut self
            .observations
            .iter_mut()
            .find(|v| v["event_kind"] == "carrier_snapshot" && v["member_id"] == name)
            .unwrap()["payload"]
    }
    fn fault(&mut self, action: &str) {
        self.request["carrier_case"] = match action {
            "restart" => "restart",
            "watermark" => "watermark_boundary",
            "prune" => "retention_boundary",
            "availability" => "missing_history",
            _ => "read_failure",
        }
        .into();
        self.configure();
        self.request["fault_schedule"] = json!(self.request["members"].as_array().unwrap().iter().map(|m| json!({"fault_id":format!("fault-{}",m["member_id"].as_str().unwrap()),"member_id":m["member_id"],"node_id":m["node_id"],"incarnation":m["incarnation"],"previous_incarnation":m["previous_incarnation"],"action":action,"trigger_event":"fixture-boundary","depends_on":[]})).collect::<Vec<_>>());
        self.pin();
    }
    fn invoke(&mut self, code: i32, verdict: &str) -> Value {
        self.invoke_edited(code, verdict, |_| {})
    }
    fn invoke_edited(&mut self, code: i32, verdict: &str, edit: impl FnOnce(&Path)) -> Value {
        self.invoke_into(code, verdict, None, edit)
    }
    fn invoke_into(
        &mut self,
        code: i32,
        verdict: &str,
        output: Option<&str>,
        edit: impl FnOnce(&Path),
    ) -> Value {
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
        let input = self
            .root
            .join(format!("attempt-{}/inputs", self.invocation));
        for path in casper_soak::walk(&self.root).unwrap() {
            let relative = path.strip_prefix(&self.root).unwrap();
            let first = relative
                .components()
                .next()
                .unwrap()
                .as_os_str()
                .to_string_lossy();
            if first.starts_with("attempt-")
                || first.starts_with("output-")
                || first.starts_with("invocation-")
            {
                continue;
            }
            let target = input.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(path, target).unwrap();
        }
        let out = output
            .map(str::to_owned)
            .unwrap_or_else(|| format!("output-{}", self.invocation));
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
        fs::write(self.root.join(format!("invocation-{}.json",self.invocation)),encoded(&json!({"command":"casper-carrier-index","arguments":args,"retained_inputs":input.strip_prefix(&self.root).unwrap(),"expected_exit":code,"actual_exit":result.status.code(),"expected_verdict":verdict,"stdout":String::from_utf8_lossy(&result.stdout),"stderr":String::from_utf8_lossy(&result.stderr)})).unwrap()).unwrap();
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
        if value["profile_identity"].is_object() {
            assert_eq!(
                record(&self.root.join(out).join("report.json")).unwrap(),
                value
            );
        }
        value
    }
}

#[test]
fn carrier_index_complete() {
    let result = Fixture::new("carrier_index_complete").invoke(0, "passed");
    assert_eq!(
        result["measurements"]["index"]["ancestor_body_read_count"],
        observed(json!("0"))
    );
    assert_eq!(result["coverage"]["observed_members"], 2);
}
#[test]
fn carrier_index_capability_missing() {
    for status in ["unknown", "unsupported"] {
        let mut f = Fixture::new(&format!("carrier_index_capability_missing_{status}"));
        f.manifest["capabilities"]["carrier-path-receipts"]["status"] = status.into();
        f.seal();
        f.invoke_edited(3, "blocked", |root| {
            fs::remove_file(root.join("observations.json")).unwrap();
        });
        assert_eq!(
            record(&f.root.join("output-1/generation.json")).unwrap()["workloads"],
            json!([])
        );
    }
}
#[test]
fn carrier_index_generation() {
    let mut f = Fixture::new("carrier_index_generation");
    f.invoke(0, "passed");
    f.invoke(0, "passed");
    assert_eq!(
        fs::read(f.root.join("output-1/generation.json")).unwrap(),
        fs::read(f.root.join("output-2/generation.json")).unwrap()
    );
    let generation = record(&f.root.join("output-1/generation.json")).unwrap();
    assert_eq!(generation["workloads"][1]["requested_path"], "index");
    assert_eq!(generation["workloads"][3]["requested_path"], "reference");
}
#[test]
fn carrier_path_unobserved() {
    let mut f = Fixture::new("carrier_path_unobserved");
    f.payload("index")["engaged_path"] = missing();
    let result = f.invoke(1, "incomplete");
    assert_eq!(
        result["measurements"]["index"]["probe_count"]["value"],
        Value::Null
    );
    assert_eq!(
        result["measurements"]["index"]["ancestor_body_read_count"]["reason"],
        "path_unobserved"
    );
}
#[test]
fn carrier_window_mismatch() {
    let mut f = Fixture::new("carrier_window_mismatch");
    f.request["members"][1]["scan_window"]["lower"] = "11".into();
    f.pin();
    f.invoke(2, "invalid_input");
}
#[test]
fn carrier_counter_missing() {
    for field in ["probe_count", "ancestor_body_read_count"] {
        let mut f = Fixture::new(&format!("carrier_counter_missing_{field}"));
        f.payload("index")[field] = missing();
        let r = f.invoke(1, "incomplete");
        assert_eq!(r["measurements"]["index"][field], missing());
    }
}
#[test]
fn carrier_cases_preserve_block_identities() {
    for case in [
        "valid",
        "invalid",
        "approved",
        "fork",
        "missing_history",
        "watermark_boundary",
        "retention_boundary",
    ] {
        let mut f = Fixture::new(&format!("carrier_case_{case}"));
        f.request["carrier_case"] = case.into();
        for name in ["index", "reference"] {
            let result = &mut f.expectation["members"][name]["result"];
            if ["invalid", "approved"].contains(&case) {
                result["carriers"][0]["status"] = case.into();
            }
            if case == "fork" {
                result["carriers"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"block_hash":"c".repeat(64),"status":"valid"}));
            }
            if case == "missing_history" {
                result["verdict"] = "unavailable".into();
            }
        }
        f.configure();
        f.invoke(0, "passed");
    }
}
#[test]
fn mismatched_pair_context_is_invalid() {
    for field in [
        "candidate_id",
        "node_revision",
        "node_binary_digest",
        "dag_digest",
        "deploy_signature",
        "availability_digest",
        "watermark",
        "retention_boundary",
        "identity_domain",
    ] {
        let mut f = Fixture::new(&format!("carrier_pair_{field}"));
        f.request["members"][1][field] = "different".into();
        f.pin();
        f.invoke(2, "invalid_input");
    }
}
#[test]
fn live_post_merge_and_typed_identity_stay_blocked() {
    for mode in ["live", "post_merge", "typed", "policy"] {
        let mut f = Fixture::new(&format!("carrier_blocked_{mode}"));
        match mode {
            "live" => f.manifest["evidence_kind"] = "node_observation".into(),
            "post_merge" => {
                f.manifest["phase"] = "post_pr216_merge".into();
                f.manifest["merge_gate"] = json!({});
            }
            "typed" => f.manifest["identity_domain"] = "fip_typed_envelope".into(),
            _ => f.manifest["policy_variant"] = "experiment".into(),
        }
        f.configure();
        f.invoke(3, "blocked");
    }
}
#[test]
fn fallback_does_not_establish_index_engagement() {
    let mut f = Fixture::new("carrier_fallback");
    f.expectation["members"]["index"]["fallback_reason"] = "watermark_unavailable".into();
    f.pin();
    f.payload("index")["engaged_path"] = observed(json!("reference"));
    let r = f.invoke(1, "incomplete");
    assert_eq!(
        r["measurements"]["index"]["ancestor_body_read_count"]["value"],
        Value::Null
    );
    assert_eq!(
        r["measurements"]["index"]["fallback_reason"]["value"],
        "watermark_unavailable"
    );
}
#[test]
fn result_mismatch_survives_missing_later_observations() {
    let mut f = Fixture::new("carrier_failure_then_missing");
    f.payload("index")["result"]["value"]["verdict"] = "fresh".into();
    f.observations.retain(|v| v["member_id"] == "index");
    let r = f.invoke(1, "product_failure");
    assert!(!r["product_failures"].as_array().unwrap().is_empty());
    assert!(!r["missing_observations"].as_array().unwrap().is_empty());
}
#[test]
fn result_sets_are_order_independent_but_reject_duplicates() {
    let mut f = Fixture::new("carrier_set_order");
    for name in ["index", "reference"] {
        f.expectation["members"][name]["result"]["carriers"]
            .as_array_mut()
            .unwrap()
            .push(json!({"block_hash":"c".repeat(64),"status":"approved"}));
    }
    f.pin();
    f.payload("index")["result"]["value"]["carriers"]
        .as_array_mut()
        .unwrap()
        .reverse();
    f.invoke(0, "passed");
    let item = f.payload("index")["result"]["value"]["carriers"][0].clone();
    f.payload("index")["result"]["value"]["carriers"]
        .as_array_mut()
        .unwrap()
        .push(item);
    f.invoke(2, "invalid_input");
}
#[test]
fn event_copies_are_deduplicated_and_conflicts_remain_fatal() {
    let mut f = Fixture::new("carrier_duplicate");
    let mut copy = f.observations[0].clone();
    copy["record_id"] = "copy".into();
    f.observations.push(copy);
    f.invoke(0, "passed");
    assert_eq!(
        record(&f.root.join("output-1/collection.json")).unwrap()["duplicate_sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut f = Fixture::new("carrier_conflicting_late_copy");
    let mut copy = f.observations[0].clone();
    copy["record_id"] = "late-copy".into();
    copy["time"]["monotonic_ns"] = "101".into();
    copy["payload"]["result"]["value"]["verdict"] = "fresh".into();
    f.observations.push(copy);
    f.invoke(2, "invalid_input");
}
#[test]
fn malformed_measurements_cannot_pass() {
    for value in [
        json!(0),
        json!("-1"),
        json!("01"),
        json!("18446744073709551616"),
        json!(null),
    ] {
        let label = hash(&serde_json::to_vec(&value).unwrap());
        let mut f = Fixture::new(&format!("carrier_bad_counter_{label}"));
        f.payload("index")["probe_count"] = observed(value);
        f.invoke(2, "invalid_input");
    }
    let mut f = Fixture::new("carrier_missing_with_zero");
    f.payload("index")["probe_count"] = json!({"presence":"missing","value":"0","reason":"absent"});
    f.invoke(2, "invalid_input");
}
#[test]
fn fault_receipts_and_restart_links_are_required() {
    for action in [
        "read_failure",
        "availability",
        "watermark",
        "prune",
        "restart",
    ] {
        let mut f = Fixture::new(&format!("carrier_fault_{action}"));
        f.fault(action);
        let r = f.invoke(0, "passed");
        assert_eq!(r["coverage"]["acknowledged_faults"], 2);
    }
    for defect in [
        "missing",
        "not_applied",
        "trigger",
        "readiness",
        "predecessor",
        "late",
    ] {
        let mut f = Fixture::new(&format!("carrier_restart_{defect}"));
        f.fault("restart");
        let ack = f
            .observations
            .iter_mut()
            .find(|v| v["event_kind"] == "fault_ack")
            .unwrap();
        match defect {
            "missing" => {
                ack["presence"] = "missing".into();
                ack["payload"] = Value::Null;
                ack["reason"] = "absent".into();
            }
            "not_applied" => ack["payload"]["status"] = "not_applied".into(),
            "trigger" => ack["payload"]["trigger_event"] = "other".into(),
            "readiness" => ack["payload"]["ready"] = false.into(),
            "predecessor" => ack["payload"]["previous_incarnation"] = "other".into(),
            _ => ack["time"]["monotonic_ns"] = "95".into(),
        }
        f.invoke(1, "incomplete");
    }
}
#[test]
fn artifact_identity_and_manifest_pins_are_checked() {
    for defect in ["raw", "fixture", "qualification", "binary", "source"] {
        let mut f = Fixture::new(&format!("carrier_pin_{defect}"));
        if defect == "binary" {
            f.manifest["profile_binary_digest"] = "0".repeat(64).into();
            f.seal();
        }
        if defect == "source" {
            f.manifest["source_digests"]["scripts/casper-soak/src/profiles/carrier_index.rs"] =
                "0".repeat(64).into();
            f.seal();
        }
        f.invoke_edited(2, "invalid_input", |root| {
            let file = match defect {
                "raw" => Some("raw/0.json"),
                "fixture" => Some("fixture.json"),
                "qualification" => Some("qualification/carrier-path-selection.json"),
                _ => None,
            };
            if let Some(file) = file {
                fs::write(root.join(file), b"{}\n").unwrap();
            }
        });
    }
}
#[test]
fn no_observations_and_foreign_correlation_cannot_pass() {
    let mut f = Fixture::new("carrier_no_observations");
    f.observations.clear();
    f.invoke(1, "incomplete");
    for field in ["run_id", "incarnation", "member_id", "pair_id"] {
        let mut f = Fixture::new(&format!("carrier_foreign_{field}"));
        f.observations[0][field] = "foreign".into();
        f.invoke(1, "incomplete");
    }
    let mut f = Fixture::new("carrier_late_snapshot");
    f.observations[0]["time"]["monotonic_ns"] = "101".into();
    f.invoke(1, "incomplete");
}
#[test]
fn retained_reports_are_immutable() {
    let mut f = Fixture::new("carrier_immutable");
    f.invoke(0, "passed");
    let before = fs::read(f.root.join("output-1/report.json")).unwrap();
    f.invoke_into(2, "invalid_input", Some("output-1"), |_| {});
    assert_eq!(
        record(&f.root.join("invocation-1.json")).unwrap()["actual_exit"],
        0
    );
    assert_eq!(
        record(&f.root.join("invocation-2.json")).unwrap()["actual_exit"],
        2
    );
    assert_eq!(
        before,
        fs::read(f.root.join("output-1/report.json")).unwrap()
    );
}

#[test]
fn independent_failures_survive_malformed_measurements() {
    for field in [
        "engaged_path",
        "probe_count",
        "ancestor_body_read_count",
        "fallback_reason",
    ] {
        let mut f = Fixture::new(&format!("carrier_failure_and_malformed_{field}"));
        f.payload("index")["result"]["value"]["verdict"] = "fresh".into();
        f.payload("index")[field] = observed(json!(false));
        let r = f.invoke(2, "invalid_input");
        assert!(r["product_failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "result_mismatch"));
    }
}

#[test]
fn previous_incarnation_cannot_hide_a_conflicting_event_copy() {
    let mut f = Fixture::new("carrier_conflicting_predecessor_copy");
    f.fault("restart");
    let mut copy = f.observations[1].clone();
    copy["record_id"] = "copy".into();
    copy["previous_incarnation"] = "wrong-predecessor".into();
    copy["payload"]["result"]["value"]["verdict"] = "fresh".into();
    f.observations.push(copy);
    f.invoke(2, "invalid_input");
}

#[test]
fn fault_cases_require_a_schedule_for_each_member() {
    for case in ["restart", "read_failure"] {
        for absent in ["both", "reference"] {
            let mut f = Fixture::new(&format!("carrier_required_fault_{case}_{absent}"));
            f.fault(case);
            f.request["fault_schedule"]
                .as_array_mut()
                .unwrap()
                .retain(|v| absent != "both" && v["member_id"] != absent);
            f.pin();
            f.invoke(2, "invalid_input");
        }
    }
}

#[test]
fn work_comparisons_require_complete_counters() {
    let mut f = Fixture::new("carrier_comparison_counter_missing");
    f.payload("index")["probe_count"] = missing();
    f.payload("index")["result"]["value"]["verdict"] = "fresh".into();
    let r = f.invoke(1, "product_failure");
    assert!(r["product_failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["kind"] == "result_mismatch"));
    assert!(!r["product_failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["kind"] == "differential_result_mismatch"));
}

#[test]
fn boundary_values_are_preserved_without_node_inference() {
    for field in ["watermark", "retention_boundary"] {
        for value in [
            "0",
            "9",
            "10",
            "11",
            "29",
            "30",
            "31",
            "18446744073709551615",
        ] {
            let mut f = Fixture::new(&format!("carrier_boundary_{field}_{value}"));
            f.request[field] = value.into();
            f.configure();
            f.invoke(0, "passed");
            let generated = record(&f.root.join("output-1/generation.json")).unwrap();
            assert_eq!(generated["workloads"][0]["member"][field], value);
            assert_eq!(generated["workloads"][2]["member"][field], value);
        }
    }
}
