use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use casper_soak::{encoded, hash, record};
use serde_json::{json, Value};

fn binary() -> &'static str { env!("CARGO_BIN_EXE_casper-version-phlo") }
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
        let (root, temporary) = if let Ok(base) = std::env::var("SOAK_VERSION_PHLO_EVIDENCE") {
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
        let manifest = json!({"schema_version":1,"run_id":name,"phase":"pre_pr216_merge","candidate_id":"fixture-node","node_revision":"2".repeat(40),"node_binary_digest":"1".repeat(64),"image_digest":null,"image_digest_reason":"subprocess fixture","harness_revision":"3".repeat(40),"external_harness_revision":"4".repeat(40),"source_digests":own["source_digests"],"profile_id":"version-phlo","profile_digest":own["profile_digest"],"profile_binary_digest":own["executable_sha256"],"seed":"7","provider":"subprocess","policy_variant":"baseline","evidence_kind":"synthetic_fixture","casper_protocol_version":"7","accounting_authority_version":"8","funding_policy":"single_deployer","capabilities":{},"tool_versions":{"profile":own["version"]},"bounds":{"scenarios":2,"observations":3},"assumptions":["controlled transcript"],"resource_limits":{"artifact_bytes":1048576},"required_scenarios":["scenario-1"],"deadline":{"clock_id":"observer-clock","monotonic_ns":"100"},"merge_gate":null});
        let request = json!({"schema_version":1,"profile_id":"version-phlo","scenario_id":"scenario-1","pair_id":"pair-1","member_id":"member-1","node_id":"node-1","incarnation":"current","segment":"1","iteration":"1","deploy_signature":"ab".repeat(64),"phloLimit":"10","phloPrice":"2","shard_minimum_price":"1","proposed_protocol_version":"7","phlo_case":"complete","fault_schedule":[],"observation_deadline":{"clock_id":"observer-clock","monotonic_ns":"100"},"inputs":{}});
        let expectation = json!({"acceptance":"accepted","execution_outcome":"completed","prepayment":"20","charge":"6","refund":"14","exhausted":false});
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
            "casper_protocol_version",
            "accounting_authority_version",
            "funding_policy",
        ] {
            self.request[key] = self.manifest[key].clone();
        }
        self.manifest["capabilities"] = json!({});
        for name in [
            "signed-envelope-capture",
            "version-rejection",
            "minimum-price-configuration",
            "phlo-settlement-observations",
            "signed-field-mutation-receipts",
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
                "casper_protocol_version",
                "accounting_authority_version",
                "funding_policy",
            ] {
                proof[key] = self.manifest[key].clone();
            }
            self.manifest["capabilities"][name] = json!({"status":"qualified","provider":self.manifest["provider"],"node_revision":self.manifest["node_revision"],"qualification":reference(&self.root,&format!("qualification/{name}.json"),&proof,vec![])});
        }
        self.pin();
    }
    fn pin(&mut self) {
        let envelope = json!({"encoding":"controlled-json-v1","deploy_signature":self.request["deploy_signature"],"phloLimit":self.request["phloLimit"],"phloPrice":self.request["phloPrice"],"payload_digest":"a".repeat(64)});
        let rf = reference(&self.root, "signed-envelope.json", &envelope, vec![]);
        self.request["signed_envelope_digest"] = rf["sha256"].clone();
        self.manifest["signed_envelope_digest"] = rf["sha256"].clone();
        self.request["inputs"]["signed_envelope"] = rf;
        let mut configuration = json!({});
        for key in [
            "casper_protocol_version",
            "accounting_authority_version",
            "proposed_protocol_version",
            "shard_minimum_price",
            "phlo_case",
            "policy_variant",
            "funding_policy",
        ] {
            configuration[key] = self.request[key].clone();
        }
        let mut fixture = json!({});
        for key in [
            "member_id",
            "node_id",
            "incarnation",
            "deploy_signature",
            "signed_envelope_digest",
            "phloLimit",
            "phloPrice",
            "fault_schedule",
        ] {
            fixture[key] = self.request[key].clone();
        }
        for (name, value) in [
            ("configuration", configuration),
            ("fixture", fixture),
            ("expectation", self.expectation.clone()),
        ] {
            let rf = reference(&self.root, &format!("{name}.json"), &value, vec![]);
            self.manifest[format!("{name}_digest")] = rf["sha256"].clone();
            self.request["inputs"][name] = rf;
        }
        let mut payloads = Vec::new();
        let mut envelope = json!({});
        for key in ["phloLimit", "phloPrice", "signed_envelope_digest"] {
            envelope[key] = observed(self.request[key].clone());
        }
        payloads.push(("envelope", envelope));
        for f in self.request["fault_schedule"].as_array().unwrap() {
            let mut p = f.clone();
            p["status"] = "applied".into();
            p["signed_envelope_digest"] = self.request["signed_envelope_digest"].clone();
            payloads.push(("fault_ack", p));
        }
        let mut admission = json!({"acceptance":observed(self.expectation["acceptance"].clone())});
        for key in [
            "casper_protocol_version",
            "accounting_authority_version",
            "proposed_protocol_version",
            "shard_minimum_price",
        ] {
            admission[key] = observed(self.request[key].clone());
        }
        payloads.push(("admission", admission));
        let mut settlement = json!({});
        for key in [
            "execution_outcome",
            "prepayment",
            "charge",
            "refund",
            "exhausted",
        ] {
            settlement[key] = observed(self.expectation[key].clone());
        }
        payloads.push(("settlement", settlement));
        self.observations.clear();
        for (i, (kind, mut payload)) in payloads.into_iter().enumerate() {
            payload["fixture_digest"] = self.request["inputs"]["fixture"]["sha256"].clone();
            payload["deploy_signature"] = self.request["deploy_signature"].clone();
            let mut v = self.request.clone();
            v["record_id"] = format!("record-{i}").into();
            v["event_id"] = format!("event-{i}").into();
            v["producer"] = "fixture".into();
            v["producer_sequence"] = (i + 1).to_string().into();
            v["event_kind"] = kind.into();
            v["time"] = json!({"clock_id":"observer-clock","monotonic_ns":((i+1)*10).to_string(),"utc":"2026-09-19T00:00:00Z"});
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
    fn payload(&mut self, kind: &str) -> &mut Value {
        &mut self
            .observations
            .iter_mut()
            .find(|v| v["event_kind"] == kind)
            .unwrap()["payload"]
    }
    fn rejected(&mut self) {
        self.expectation = json!({"acceptance":"rejected","execution_outcome":"not_executed","prepayment":"0","charge":"0","refund":"0","exhausted":false});
    }
    fn mutation(&mut self, field: &str) {
        self.request["phlo_case"] = "signed_field_mutation".into();
        self.request["fault_schedule"] = json!([{"fault_id":"mutation-1","member_id":self.request["member_id"],"node_id":self.request["node_id"],"incarnation":self.request["incarnation"],"action":"mutate_signed_field","field":field,"original_value":self.request[field],"value":"11","trigger_event":"after_signing","depends_on":[]}]);
        self.rejected();
        self.pin();
    }
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
                let mut source = reference(&self.root, &format!("raw/{i}.json"), v, vec![v
                    ["record_id"]
                    .clone()]);
                source["producer"] = v["producer"].clone();
                source
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
        fs::write(self.root.join(format!("invocation-{}.json",self.invocation)),encoded(&json!({"command":"casper-version-phlo","arguments":args,"retained_inputs":input.strip_prefix(&self.root).unwrap(),"expected_exit":code,"actual_exit":result.status.code(),"expected_verdict":verdict,"stdout":String::from_utf8_lossy(&result.stdout),"stderr":String::from_utf8_lossy(&result.stderr)})).unwrap()).unwrap();
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
fn version_phlo_complete() {
    let r = Fixture::new("version_phlo_complete").invoke(0, "passed");
    assert_eq!(r["coverage"]["observed_stages"], 3);
    assert_eq!(
        r["measurements"]["envelope"]["phloLimit"],
        observed(json!("10"))
    );
    assert_eq!(
        r["measurements"]["envelope"]["phloPrice"],
        observed(json!("2"))
    );
}
#[test]
fn version_phlo_capability_missing() {
    for cap in [
        "signed-envelope-capture",
        "version-rejection",
        "minimum-price-configuration",
        "phlo-settlement-observations",
    ] {
        for status in ["unknown", "unsupported"] {
            let mut f = Fixture::new(&format!("version_phlo_capability_missing_{cap}_{status}"));
            f.manifest["capabilities"][cap]["status"] = status.into();
            f.seal();
            f.invoke_edited(3, "blocked", |root| {
                fs::remove_file(root.join("observations.json")).unwrap();
            });
            let g = record(&f.root.join("output-1/generation.json")).unwrap();
            assert_eq!(g["workloads"], json!([]));
            assert_eq!(g["fault_requests"], json!([]));
        }
    }
}
#[test]
fn version_phlo_generation() {
    let mut f = Fixture::new("version_phlo_generation");
    f.invoke(0, "passed");
    f.invoke(0, "passed");
    assert_eq!(
        fs::read(f.root.join("output-1/generation.json")).unwrap(),
        fs::read(f.root.join("output-2/generation.json")).unwrap()
    );
    let g = record(&f.root.join("output-1/generation.json")).unwrap();
    assert_eq!(g["workloads"][1]["casper_protocol_version"], "7");
    assert_eq!(g["workloads"][1]["accounting_authority_version"], "8");
    assert_eq!(
        g["workloads"][1]["submitted_fields"],
        json!({"phloLimit":"10","phloPrice":"2"})
    );
}
#[test]
fn phlo_version_labels() {
    for (field, value) in [
        ("casper_protocol_version", "8"),
        ("accounting_authority_version", "7"),
    ] {
        let mut f = Fixture::new(&format!("phlo_version_labels_{field}"));
        f.manifest[field] = value.into();
        f.configure();
        f.invoke(2, "invalid_input");
        assert!(!f.root.join("output-1").exists());
    }
    let mut f = Fixture::new("phlo_observed_version_conflation");
    f.payload("admission")["casper_protocol_version"] = observed(json!("8"));
    assert!(!f.invoke(1, "product_failure")["product_failures"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn phlo_signed_field_missing() {
    for field in ["phloLimit", "phloPrice"] {
        let mut f = Fixture::new(&format!("phlo_signed_field_missing_request_{field}"));
        f.request.as_object_mut().unwrap().remove(field);
        f.invoke(2, "invalid_input");
        for omitted in [false, true] {
            let mut f = Fixture::new(&format!(
                "phlo_signed_field_missing_capture_{field}_{omitted}"
            ));
            if omitted {
                f.payload("envelope").as_object_mut().unwrap().remove(field);
            } else {
                f.payload("envelope")[field] = missing();
            }
            let r = f.invoke(1, "incomplete");
            assert!(r["measurements"]["envelope"][field]["value"].is_null());
        }
    }
}
#[test]
fn phlo_refund_mismatch() {
    let mut f = Fixture::new("phlo_refund_mismatch");
    f.payload("settlement")["refund"] = observed(json!("13"));
    let r = f.invoke(1, "product_failure");
    assert!(r["product_failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["kind"] == "refund_mismatch"));
}
#[test]
fn independent_fields_retain_failures() {
    for field in [
        "execution_outcome",
        "prepayment",
        "charge",
        "refund",
        "exhausted",
    ] {
        let mut f = Fixture::new(&format!("phlo_settlement_mismatch_{field}"));
        let wrong = match field {
            "execution_outcome" => json!("exhausted"),
            "exhausted" => json!(true),
            _ => json!("1"),
        };
        f.payload("settlement")[field] = observed(wrong);
        f.invoke(1, "product_failure");
    }
    for malformed in [false, true] {
        let mut f = Fixture::new(&format!("phlo_independent_failure_{malformed}"));
        f.payload("settlement")["prepayment"] = if malformed {
            observed(json!("-1"))
        } else {
            missing()
        };
        f.payload("settlement")["refund"] = observed(json!("13"));
        let r = f.invoke(
            if malformed { 2 } else { 1 },
            if malformed {
                "invalid_input"
            } else {
                "product_failure"
            },
        );
        assert!(r["product_failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "refund_mismatch"));
    }
}
#[test]
fn minimum_price_and_exhaustion_cases() {
    for (case, price, expectation) in [
        (
            "minimum_equal",
            "1",
            json!({"acceptance":"accepted","execution_outcome":"completed","prepayment":"10","charge":"3","refund":"7","exhausted":false}),
        ),
        (
            "minimum_above",
            "2",
            json!({"acceptance":"accepted","execution_outcome":"completed","prepayment":"20","charge":"6","refund":"14","exhausted":false}),
        ),
        (
            "minimum_below",
            "0",
            json!({"acceptance":"rejected","execution_outcome":"not_executed","prepayment":"0","charge":"0","refund":"0","exhausted":false}),
        ),
        (
            "exhausted",
            "2",
            json!({"acceptance":"accepted","execution_outcome":"exhausted","prepayment":"20","charge":"20","refund":"0","exhausted":true}),
        ),
    ] {
        let mut f = Fixture::new(&format!("phlo_{case}"));
        f.request["phlo_case"] = case.into();
        f.request["phloPrice"] = price.into();
        f.expectation = expectation;
        f.pin();
        let r = f.invoke(0, "passed");
        assert_eq!(
            r["measurements"]["settlement"]["refund"],
            observed(f.expectation["refund"].clone())
        );
    }
    for version in ["6", "8", "9"] {
        let mut f = Fixture::new(&format!("phlo_version_rejected_{version}"));
        f.request["phlo_case"] = "version_rejected".into();
        f.request["proposed_protocol_version"] = version.into();
        f.rejected();
        f.pin();
        f.invoke(0, "passed");
    }
}
#[test]
fn signed_field_mutations_need_observed_receipts() {
    for field in ["phloLimit", "phloPrice"] {
        let mut f = Fixture::new(&format!("phlo_mutation_{field}"));
        f.mutation(field);
        let r = f.invoke(0, "passed");
        assert_eq!(r["coverage"]["acknowledged_faults"], 1);
        let g = record(&f.root.join("output-1/generation.json")).unwrap();
        assert_eq!(g["workloads"][1]["signed_fields"][field], f.request[field]);
        assert_eq!(g["workloads"][1]["submitted_fields"][field], "11");
    }
    for defect in [
        "missing",
        "status",
        "field",
        "value",
        "trigger",
        "digest",
        "order",
        "schedule",
        "capability",
    ] {
        let mut f = Fixture::new(&format!("phlo_mutation_{defect}"));
        f.mutation("phloLimit");
        match defect {
            "missing" => f.observations.retain(|v| v["event_kind"] != "fault_ack"),
            "status" => f.payload("fault_ack")["status"] = "unknown".into(),
            "field" => f.payload("fault_ack")["field"] = "phloPrice".into(),
            "value" => f.payload("fault_ack")["value"] = "12".into(),
            "trigger" => f.payload("fault_ack")["trigger_event"] = "before_signing".into(),
            "digest" => f.payload("fault_ack")["signed_envelope_digest"] = "f".repeat(64).into(),
            "order" => f.observations[1]["producer"] = "other".into(),
            "schedule" => f.request["fault_schedule"] = json!([]),
            _ => {
                f.manifest["capabilities"]["signed-field-mutation-receipts"]["status"] =
                    "unsupported".into();
                f.seal();
            }
        }
        let (code, verdict) = match defect {
            "schedule" => (2, "invalid_input"),
            "capability" => (3, "blocked"),
            _ => (1, "incomplete"),
        };
        f.invoke(code, verdict);
    }
}
#[test]
fn unknown_is_not_zero() {
    for field in ["prepayment", "charge", "refund"] {
        let mut f = Fixture::new(&format!("phlo_missing_{field}"));
        f.payload("settlement")[field] = missing();
        assert_eq!(
            f.invoke(1, "incomplete")["measurements"]["settlement"][field],
            missing()
        );
    }
    for field in ["phloLimit", "phloPrice", "prepayment", "charge", "refund"] {
        for value in [
            json!(0),
            json!("-1"),
            json!("01"),
            json!("1.5"),
            json!("18446744073709551616"),
        ] {
            let mut f = Fixture::new(&format!(
                "phlo_malformed_{field}_{}",
                hash(&encoded(&value).unwrap())
            ));
            let stage = if field.starts_with("phlo") {
                "envelope"
            } else {
                "settlement"
            };
            f.payload(stage)[field] = observed(value);
            f.invoke(2, "invalid_input");
        }
    }
}
#[test]
fn transport_identity_and_inventory() {
    for defect in [
        "duplicate",
        "conflict",
        "duplicate_record",
        "wrong_run",
        "wrong_member",
        "wrong_clock",
        "late",
        "empty",
        "extra_stage",
        "sequence",
        "clock_regression",
    ] {
        let mut f = Fixture::new(&format!("phlo_transport_{defect}"));
        match defect {
            "duplicate" | "conflict" | "duplicate_record" => {
                let mut v = f.observations[2].clone();
                if defect != "duplicate_record" {
                    v["record_id"] = "copy".into();
                }
                if defect == "conflict" {
                    v["payload"]["refund"] = observed(json!("12"));
                }
                f.observations.push(v);
            }
            "wrong_run" => f.observations[2]["run_id"] = "other".into(),
            "wrong_member" => f.observations[2]["member_id"] = "other".into(),
            "wrong_clock" => f.observations[2]["time"]["clock_id"] = "other".into(),
            "late" => f.observations[2]["time"]["monotonic_ns"] = "101".into(),
            "empty" => f.observations.clear(),
            "extra_stage" => {
                let mut v = f.observations[2].clone();
                v["record_id"] = "new".into();
                v["event_id"] = "new".into();
                v["producer_sequence"] = "4".into();
                f.observations.push(v);
            }
            "sequence" => f.observations[2]["producer_sequence"] = "2".into(),
            _ => f.observations[2]["time"]["monotonic_ns"] = "1".into(),
        }
        let (code, verdict) = match defect {
            "duplicate" => (0, "passed"),
            "conflict" | "duplicate_record" | "sequence" | "clock_regression" => {
                (2, "invalid_input")
            }
            _ => (1, "incomplete"),
        };
        f.invoke(code, verdict);
    }
}
#[test]
fn blocked_policy_and_live_requests() {
    for defect in ["live", "post_merge", "policy", "funding"] {
        let mut f = Fixture::new(&format!("phlo_blocked_{defect}"));
        match defect {
            "live" => f.manifest["evidence_kind"] = "node_observation".into(),
            "post_merge" => {
                f.manifest["phase"] = "post_pr216_merge".into();
                f.manifest["merge_gate"] = json!({});
            }
            "policy" => f.manifest["policy_variant"] = "experimental".into(),
            _ => f.manifest["funding_policy"] = "multi_wallet".into(),
        }
        f.configure();
        f.invoke(3, "blocked");
    }
}
#[test]
fn fault_receipts_bind_the_fixture_and_deploy() {
    for field in ["fixture_digest", "deploy_signature"] {
        let mut f = Fixture::new(&format!("phlo_receipt_binding_{field}"));
        f.mutation("phloLimit");
        f.payload("fault_ack")[field] = "f".repeat(64).into();
        f.invoke(1, "incomplete");
    }
}
#[test]
fn failure_survives_missing_and_conflicting_stages() {
    for defect in [
        "missing_envelope",
        "missing_admission",
        "conflicting_settlement",
        "unknown_payload",
    ] {
        let mut f = Fixture::new(&format!("phlo_retained_failure_{defect}"));
        f.payload("settlement")["refund"] = observed(json!("13"));
        match defect {
            "missing_envelope" => f.observations.retain(|v| v["event_kind"] != "envelope"),
            "missing_admission" => f.observations.retain(|v| v["event_kind"] != "admission"),
            "unknown_payload" => {
                f.observations[0]["presence"] = "error".into();
                f.observations[0]["payload"] = Value::Null;
                f.observations[0]["reason"] = "transport_failure".into();
            }
            _ => {
                let mut v = f.observations[2].clone();
                v["record_id"] = "copy".into();
                v["payload"]["refund"] = observed(json!("14"));
                f.observations.push(v);
            }
        }
        let invalid = defect == "conflicting_settlement";
        let r = f.invoke(
            if invalid { 2 } else { 1 },
            if invalid {
                "invalid_input"
            } else {
                "product_failure"
            },
        );
        assert!(r["product_failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "refund_mismatch"));
    }
}
#[test]
fn source_and_envelope_binding() {
    for defect in [
        "manifest_bytes",
        "source",
        "binary",
        "envelope_bytes",
        "envelope_fields",
        "expectation",
        "raw_digest",
        "raw_association",
    ] {
        let mut f = Fixture::new(&format!("phlo_binding_{defect}"));
        match defect {
            "source" => {
                f.manifest["source_digests"]["scripts/casper-soak/src/profiles/version_phlo.rs"] =
                    "0".repeat(64).into();
                f.seal();
            }
            "binary" => {
                f.manifest["profile_binary_digest"] = "0".repeat(64).into();
                f.seal();
            }
            "envelope_fields" => {
                f.request["phloLimit"] = "11".into();
            }
            "expectation" => {
                f.expectation["acceptance"] = "rejected".into();
                f.pin();
            }
            _ => {}
        }
        f.invoke_edited(2, "invalid_input", |root| match defect {
            "manifest_bytes" | "envelope_bytes" => {
                let name = if defect == "manifest_bytes" {
                    "manifest.json"
                } else {
                    "signed-envelope.json"
                };
                let mut bytes = fs::read(root.join(name)).unwrap();
                bytes.push(b' ');
                fs::write(root.join(name), bytes).unwrap();
            }
            "raw_digest" => {
                fs::write(root.join("raw/0.json"), b"{}").unwrap();
            }
            "raw_association" => {
                let mut v = record(&root.join("observations.json")).unwrap();
                v["records"][0]["observation_ids"] = json!(["other"]);
                fs::write(root.join("observations.json"), encoded(&v).unwrap()).unwrap();
            }
            _ => {}
        });
    }
}
