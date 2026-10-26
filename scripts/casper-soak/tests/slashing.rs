use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use casper_soak::{encoded, hash, record};
use serde_json::{json, Value};

fn binary() -> &'static str { env!("CARGO_BIN_EXE_casper-slashing") }
fn observed(v: Value) -> Value { json!({"presence":"observed","value":v,"reason":null}) }
fn missing() -> Value { json!({"presence":"missing","value":null,"reason":"unavailable"}) }
fn reference(root: &Path, path: &str, v: &Value, ids: Vec<Value>) -> Value {
    let bytes = encoded(v).unwrap();
    fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    fs::write(root.join(path), &bytes).unwrap();
    json!({"path":path,"bytes":bytes.len(),"sha256":hash(&bytes),"producer":"fixture","capture_state":"captured","observation_ids":ids})
}
const COMPATIBILITY: &[&str] = &[
    "evidence_schema",
    "protocol_version",
    "authorization_rule",
    "economic_neglect_slashing",
];
const FIELDS: &[&str] = &[
    "authorization",
    "recovery_outcome",
    "offender_deduplicated",
    "invalid_hash_seed",
];
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
        let (root, temporary) = if let Ok(base) = std::env::var("SOAK_SLASHING_EVIDENCE") {
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
        let manifest = json!({"schema_version":1,"run_id":name,"phase":"pre_pr216_merge","candidate_id":"fixture-node","node_revision":"2".repeat(40),"node_binary_digest":"1".repeat(64),"image_digest":null,"image_digest_reason":"subprocess fixture","harness_revision":"3".repeat(40),"external_harness_revision":"4".repeat(40),"source_digests":own["source_digests"],"profile_id":"slashing","profile_digest":own["profile_digest"],"profile_binary_digest":own["executable_sha256"],"seed":"7","provider":"subprocess","policy_variant":"baseline","evidence_kind":"synthetic_fixture","capabilities":{},"tool_versions":{"profile":own["version"]},"bounds":{"scenarios":2,"observations":3},"assumptions":["controlled transcript"],"resource_limits":{"artifact_bytes":1048576},"required_scenarios":["scenario-1"],"deadline":{"clock_id":"observer-clock","monotonic_ns":"1000"},"merge_gate":null});
        let evidence: Vec<_> = ["a","b"].into_iter().map(|s| json!({"evidence_id":s,"evidence_digest":s.repeat(64),"invalid_block_hash":s.repeat(64),"offender":"offender-1","evidence_epoch":"6","stake":"100","validator":"validator-1","sequence":"1","invalid_hash_seed":"d".repeat(64),"evidence_status":"current","origin":"invalid_latest_message"})).collect();
        let request = json!({"schema_version":1,"profile_id":"slashing","scenario_id":"scenario-1","scenario_family":"delivery_permutation","pair_id":"pair-1","member_id":"member-1","node_id":"node-1","incarnation":"incarnation-1","segment":"1","iteration":"1","evidence_schema":"synthetic-dev-unary-v1","protocol_version":"6","authorization_rule":"dev_rejected_slash_loop","economic_neglect_slashing":false,"epoch":"6","rebond_epoch":"5","parent_prestate_digest":"c".repeat(64),"evidence_fixture":evidence,"schedule":[{"step_id":"deliver-a","action":"deliver_evidence","evidence_id":"a","incarnation":"incarnation-1","depends_on":[]},{"step_id":"deliver-b","action":"deliver_evidence","evidence_id":"b","incarnation":"incarnation-1","depends_on":[]}],"observation_deadline":{"clock_id":"observer-clock","monotonic_ns":"1000"},"inputs":{}});
        let mut f = Self {
            root,
            _temporary: temporary,
            manifest,
            request,
            expectation: json!({}),
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
        for key in COMPATIBILITY {
            self.manifest[*key] = self.request[*key].clone();
        }
        self.manifest["capabilities"] = json!({});
        for name in [
            "slashing-delivery-receipts",
            "slashing-prestate",
            "slashing-epoch-authorization",
            "slashing-recovery",
            "slashing-linked-restart",
            "slashing-rebond-receipts",
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
            ]
            .into_iter()
            .chain(COMPATIBILITY.iter().copied())
            {
                proof[key] = self.manifest[key].clone();
            }
            self.manifest["capabilities"][name] = json!({"status":"qualified","provider":self.manifest["provider"],"node_revision":self.manifest["node_revision"],"qualification":reference(&self.root,&format!("qualification/{name}.json"),&proof,vec![])});
        }
        let outcomes: Vec<_> = self.request["evidence_fixture"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let mut e = d.clone();
                e["authorization"] = (d["evidence_status"] == "current"
                    && d["stake"] != "0"
                    && d["evidence_epoch"] == self.request["epoch"]
                    && d["evidence_epoch"]
                        .as_str()
                        .unwrap()
                        .parse::<u64>()
                        .unwrap()
                        >= self.request["rebond_epoch"]
                            .as_str()
                            .unwrap()
                            .parse::<u64>()
                            .unwrap())
                .into();
                e["recovery_outcome"] = if d["origin"] == "merge_rejected_slash" {
                    "reissued"
                } else if e["authorization"] == false {
                    "rejected"
                } else {
                    "not_required"
                }
                .into();
                e["offender_deduplicated"] = (i > 0).into();
                e
            })
            .collect();
        self.expectation = json!({"outcomes":outcomes});
        self.pin();
    }
    fn pin(&mut self) {
        let mut configuration = json!({"policy_variant":self.request["policy_variant"]});
        for key in COMPATIBILITY {
            configuration[*key] = self.request[*key].clone();
        }
        let mut fixture = json!({});
        for key in [
            "evidence_fixture",
            "schedule",
            "parent_prestate_digest",
            "epoch",
            "rebond_epoch",
            "scenario_family",
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
        self.observations.clear();
        let mut payloads: Vec<_> = self.request["schedule"]
            .as_array()
            .unwrap()
            .iter()
            .map(|step| {
                let mut p = step.clone();
                p["status"] = "applied".into();
                p["observed"] = true.into();
                match step["action"].as_str().unwrap() {
                    "deliver_evidence" => {
                        p["evidence_digest"] = self.request["evidence_fixture"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|e| e["evidence_id"] == step["evidence_id"])
                            .unwrap()["evidence_digest"]
                            .clone();
                    }
                    "restart" => {
                        p["exit_observed"] = true.into();
                        p["exit_code"] = 137.into();
                        p["restart_ready"] = true.into();
                    }
                    "rebond" => {
                        p["rebond_observed"] = true.into();
                    }
                    _ => (),
                }
                ("step_ack", p)
            })
            .collect();
        let entries: Vec<_> = self.expectation["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let mut row = e.clone();
                for k in FIELDS {
                    row[*k] = observed(e[*k].clone());
                }
                row
            })
            .collect();
        let target = self.request["schedule"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|s| s["action"] == "restart")
            .map_or(&self.request["incarnation"], |s| &s["to_incarnation"]);
        let mut p = json!({"fixture_digest":self.request["inputs"]["fixture"]["sha256"],"applied_steps":["load_evidence_fixture","capture_authorization"],"target_incarnation":target,"entries":observed(json!(entries))});
        for key in COMPATIBILITY {
            p[*key] = self.request[*key].clone();
        }
        payloads.push(("slashing_snapshot", p));
        for (i, (kind, payload)) in payloads.into_iter().enumerate() {
            let mut v = self.request.clone();
            v["record_id"] = format!("record-{i}").into();
            v["event_id"] = format!("event-{i}").into();
            v["producer"] = "fixture".into();
            v["producer_sequence"] = (i + 1).to_string().into();
            v["event_kind"] = kind.into();
            v["time"] = json!({"clock_id":"observer-clock","monotonic_ns":((i+1)*10).to_string(),"utc":"2026-01-01T00:00:00Z"});
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
        fs::write(self.root.join(format!("invocation-{}.json",self.invocation)),encoded(&json!({"command":"casper-slashing","arguments":args,"expected_exit":code,"actual_exit":result.status.code(),"expected_verdict":verdict,"stdout":String::from_utf8_lossy(&result.stdout),"stderr":String::from_utf8_lossy(&result.stderr)})).unwrap()).unwrap();
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
    fn restart(&mut self) {
        self.request["scenario_family"] = "restart_during_delivery".into();
        self.request["schedule"].as_array_mut().unwrap().insert(1,json!({"step_id":"restart-1","action":"restart","from_incarnation":"incarnation-1","to_incarnation":"incarnation-2","depends_on":["deliver-a"]}));
        self.request["schedule"][2]["incarnation"] = "incarnation-2".into();
        self.request["schedule"][2]["depends_on"] = json!(["restart-1"]);
        self.configure();
    }
}
#[test]
fn required_slashing_fixtures() {
    Fixture::new("slashing_complete").invoke(0, "passed");
    let mut f = Fixture::new("slashing_generation");
    f.invoke(0, "passed");
    f.invoke(0, "passed");
    assert_eq!(
        fs::read(f.root.join("output-1/generation.json")).unwrap(),
        fs::read(f.root.join("output-2/generation.json")).unwrap()
    );
    let mut f = Fixture::new("slashing_delivery_order");
    let a = f.observations[0]["payload"].clone();
    f.observations[0]["payload"] = f.observations[1]["payload"].clone();
    f.observations[1]["payload"] = a;
    let v = f.invoke(1, "incomplete");
    assert_eq!(
        v["measurements"]["delivery_order"]["value"],
        json!(["deliver-b", "deliver-a"])
    );
    assert_eq!(
        v["requested_delivery_order"],
        json!(["deliver-a", "deliver-b"])
    );
    let mut f = Fixture::new("slashing_epoch_mismatch");
    f.observations.last_mut().unwrap()["epoch"] = "7".into();
    let v = f.invoke(1, "incomplete");
    assert!(v["measurements"]["counts"]["value"].is_null());
    assert_eq!(v["rejected_observations"][0]["fatal"], false);
    let mut f = Fixture::new("slashing_authorization_mismatch");
    f.payload()["entries"]["value"][0]["authorization"] = observed(json!(false));
    let v = f.invoke(1, "product_failure");
    assert_eq!(v["product_failures"][0]["kind"], "authorization_mismatch");
}
#[test]
fn scenario_families_and_expected_rejections() {
    let mut f = Fixture::new("merge_lost_slash");
    f.request["scenario_family"] = "merge_lost_slash".into();
    f.request["evidence_fixture"][0]["origin"] = "merge_rejected_slash".into();
    f.configure();
    f.invoke(0, "passed");
    for (name, status) in [
        ("missing_evidence", "missing"),
        ("forged_deploy", "forged"),
        ("stale_epoch", "stale"),
    ] {
        let mut f = Fixture::new(name);
        f.request["scenario_family"] = name.into();
        f.request["evidence_fixture"][0]["evidence_status"] = status.into();
        if status == "stale" {
            f.request["evidence_fixture"][0]["evidence_epoch"] = "4".into();
        }
        f.configure();
        assert_eq!(f.expectation["outcomes"][0]["authorization"], false);
        f.invoke(0, "passed");
        f.payload()["entries"]["value"][0]["authorization"] = observed(json!(true));
        f.invoke(1, "product_failure");
    }
    let mut f = Fixture::new("same_key_rebond");
    f.request["scenario_family"] = "same_key_rebond".into();
    f.request["epoch"] = "7".into();
    f.request["rebond_epoch"] = "7".into();
    f.request["schedule"].as_array_mut().unwrap().insert(0,json!({"step_id":"rebond-1","action":"rebond","offender":"offender-1","from_epoch":"5","to_epoch":"7","depends_on":[]}));
    f.configure();
    f.invoke(0, "passed");
    f.observations[0]["payload"]["rebond_observed"] = false.into();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("duplicate_evidence");
    f.request["scenario_family"] = "duplicate_evidence".into();
    let mut step = f.request["schedule"][0].clone();
    step["step_id"] = "deliver-a-again".into();
    f.request["schedule"].as_array_mut().unwrap().push(step);
    f.configure();
    let row = f.payload()["entries"]["value"][0].clone();
    f.payload()["entries"]["value"]
        .as_array_mut()
        .unwrap()
        .push(row);
    let v = f.invoke(0, "passed");
    assert_eq!(
        v["measurements"]["counts"]["value"],
        json!({"evidence":2,"offenders":1,"duplicate_observations":1})
    );
    let mut f = Fixture::new("restart_during_delivery");
    f.restart();
    f.invoke(0, "passed");
    f.observations[1]["payload"]["exit_observed"] = false.into();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("zero_stake_rejection");
    f.request["evidence_fixture"][0]["stake"] = "0".into();
    f.configure();
    f.invoke(0, "passed");
}
#[test]
fn independent_failures_and_unknown_measurements() {
    for name in FIELDS {
        let mut f = Fixture::new(&format!("missing-{name}"));
        f.payload()["entries"]["value"][0][*name] = missing();
        let v = f.invoke(1, "incomplete");
        assert!(v["measurements"]["entries"]["value"][0][*name]["value"].is_null());
    }
    for name in FIELDS {
        let mut f = Fixture::new(&format!("mismatch-{name}"));
        let value = match *name {
            "authorization" => json!(false),
            "offender_deduplicated" => json!(true),
            "recovery_outcome" => json!("pending"),
            _ => json!("f".repeat(64)),
        };
        f.payload()["entries"]["value"][0][*name] = observed(value);
        f.observations.remove(0);
        assert!(!f.invoke(1, "product_failure")["product_failures"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    let mut f = Fixture::new("malformed-sibling-keeps-authorization");
    f.payload()["entries"]["value"][0]["offender_deduplicated"] = observed(json!("true"));
    f.payload()["entries"]["value"][0]["authorization"] = observed(json!(false));
    assert!(!f.invoke(2, "invalid_input")["product_failures"]
        .as_array()
        .unwrap()
        .is_empty());
    let mut f = Fixture::new("missing-inventory");
    f.payload()["entries"] = missing();
    assert!(f.invoke(1, "incomplete")["measurements"]["counts"]["value"].is_null());
    let mut f = Fixture::new("empty-inventory");
    f.payload()["entries"] = observed(json!([]));
    assert!(f.invoke(1, "incomplete")["measurements"]["counts"]["value"].is_null());
    let mut f = Fixture::new("conflicting-evidence-counts-unknown");
    let mut row = f.payload()["entries"]["value"][0].clone();
    row["authorization"] = observed(json!(false));
    f.payload()["entries"]["value"]
        .as_array_mut()
        .unwrap()
        .push(row);
    assert!(f.invoke(2, "invalid_input")["measurements"]["counts"]["value"].is_null());
}
#[test]
fn immutable_transport_and_correlation() {
    let mut f = Fixture::new("duplicate-transport");
    let mut copy = f.observations[0].clone();
    copy["record_id"] = "copy".into();
    f.observations.push(copy);
    f.invoke(0, "passed");
    for reverse in [false, true] {
        for field in ["epoch", "time"] {
            let mut f = Fixture::new(&format!("conflicting-{field}-{reverse}"));
            let mut copy = f.observations[0].clone();
            copy["record_id"] = "copy".into();
            if field == "epoch" {
                copy["epoch"] = "8".into();
            } else {
                copy["time"]["monotonic_ns"] = "1001".into();
            }
            if reverse {
                f.observations.insert(0, copy);
            } else {
                f.observations.push(copy);
            }
            f.invoke(2, "invalid_input");
        }
    }
    for field in [
        "node_revision",
        "parent_prestate_digest",
        "epoch",
        "rebond_epoch",
        "run_id",
    ] {
        let mut f = Fixture::new(&format!("foreign-{field}"));
        let v = f.observations.last_mut().unwrap();
        v[field] = match field {
            "node_revision" => "a".repeat(40).into(),
            "parent_prestate_digest" => "a".repeat(64).into(),
            "epoch" | "rebond_epoch" => "99".into(),
            _ => "another-run".into(),
        };
        f.invoke(1, "incomplete");
    }
    let mut f = Fixture::new("known-failure-plus-foreign");
    f.payload()["entries"]["value"][0]["authorization"] = observed(json!(false));
    let mut foreign = f.observations.last().unwrap().clone();
    foreign["run_id"] = "another".into();
    foreign["record_id"] = "foreign".into();
    f.observations.push(foreign);
    f.invoke(1, "product_failure");
}
#[test]
fn scheduling_receipts_and_incarnations() {
    for field in ["observed", "status", "evidence_digest"] {
        let mut f = Fixture::new(&format!("unapplied-{field}"));
        f.observations[0]["payload"][field] = match field {
            "observed" => false.into(),
            "status" => "requested".into(),
            _ => "f".repeat(64).into(),
        };
        f.invoke(1, "incomplete");
    }
    let mut f = Fixture::new("boolean-receipt-refusal");
    f.observations[0]["payload"]["observed"] = "true".into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("cross-producer-order-unknown");
    f.observations[1]["producer"] = "another".into();
    f.invoke_edited(1, "incomplete", |root| {
        let mut inv = record(&root.join("observations.json")).unwrap();
        inv["records"][1]["producer"] = "another".into();
        fs::write(root.join("observations.json"), encoded(&inv).unwrap()).unwrap();
    });
    let mut f = Fixture::new("restart-wrong-target");
    f.restart();
    f.payload()["target_incarnation"] = "incarnation-1".into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("restart-boolean-exit");
    f.restart();
    f.observations[1]["payload"]["exit_code"] = true.into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("snapshot-before-delivery");
    f.observations.last_mut().unwrap()["time"]["monotonic_ns"] = "1".into();
    f.invoke(1, "incomplete");
    let mut f = Fixture::new("missing-restart-capability");
    f.restart();
    f.manifest["capabilities"]["slashing-linked-restart"]["status"] = "unknown".into();
    f.seal();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("false-scenario-label");
    f.request["scenario_family"] = "restart_during_delivery".into();
    f.pin();
    f.invoke(2, "invalid_input");
}
#[test]
fn pinned_bounds_and_expectation_prerequisites() {
    let mut f = Fixture::new("request-deadline-refusal");
    f.request["observation_deadline"]["monotonic_ns"] = "2000".into();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("future-rebond-epoch-refusal");
    f.request["rebond_epoch"] = "7".into();
    f.configure();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("invalid-expected-authorization");
    f.request["evidence_fixture"][0]["stake"] = "0".into();
    f.configure();
    f.expectation["outcomes"][0]["authorization"] = true.into();
    f.pin();
    f.invoke(2, "invalid_input");
    let mut f = Fixture::new("conflicting-snapshots-keep-failure");
    let mut copy = f.observations.last().unwrap().clone();
    copy["record_id"] = "other".into();
    copy["event_id"] = "other".into();
    copy["producer_sequence"] = "4".into();
    copy["time"]["monotonic_ns"] = "40".into();
    copy["payload"]["entries"]["value"][0]["authorization"] = observed(json!(false));
    f.observations.push(copy);
    let v = f.invoke(1, "product_failure");
    assert!(v["measurements"]["counts"]["value"].is_null());
}
#[test]
fn qualification_policy_and_raw_refusals() {
    let mut f = Fixture::new("slashing_capability_missing");
    f.manifest["capabilities"]["slashing-prestate"]["status"] = "unknown".into();
    f.seal();
    f.invoke(3, "blocked");
    for field in [
        "protocol_version",
        "authorization_rule",
        "economic_neglect_slashing",
        "evidence_schema",
    ] {
        let mut f = Fixture::new(&format!("blocked-{field}"));
        f.request[field] = match field {
            "protocol_version" => "7".into(),
            "economic_neglect_slashing" => true.into(),
            _ => "unapproved".into(),
        };
        f.configure();
        f.invoke(3, "blocked");
    }
    let mut f = Fixture::new("live-blocked");
    f.manifest["evidence_kind"] = "node_observation".into();
    f.configure();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("post-merge-blocked");
    f.manifest["phase"] = "post_pr216_merge".into();
    f.manifest["merge_gate"] = json!({"status":"confirmed","merge_commit":"5".repeat(40)});
    f.configure();
    f.invoke(3, "blocked");
    let mut f = Fixture::new("boolean-neglect-refusal");
    f.request["economic_neglect_slashing"] = "false".into();
    f.configure();
    f.invoke(2, "invalid_input");
    for value in [
        json!(true),
        json!("01"),
        json!("18446744073709551616"),
        json!(-1),
    ] {
        let mut f = Fixture::new(&format!("bad-epoch-{}", hash(value.to_string().as_bytes())));
        f.request["epoch"] = value;
        f.pin();
        f.invoke(2, "invalid_input");
    }
    let mut f = Fixture::new("pinned-expectation-refusal");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::write(root.join("expectation.json"), b"{}").unwrap();
    });
    let mut f = Fixture::new("corrupt-artifact");
    f.invoke_edited(2, "invalid_input", |root| {
        fs::write(root.join("raw/0.json"), b"{}").unwrap();
    });
    let mut f = Fixture::new("duplicate-json-key");
    f.invoke_edited(2, "invalid_input", |root| {
        let path = root.join("request.json");
        let s = fs::read_to_string(&path).unwrap();
        fs::write(path, s.replacen('{', "{\"schema_version\":1,", 1)).unwrap();
    });
    #[cfg(unix)]
    {
        let mut f = Fixture::new("symlink-artifact");
        f.invoke_edited(2, "invalid_input", |root| {
            fs::rename(root.join("raw/0.json"), root.join("raw/source.json")).unwrap();
            std::os::unix::fs::symlink("source.json", root.join("raw/0.json")).unwrap();
        });
    }
}
