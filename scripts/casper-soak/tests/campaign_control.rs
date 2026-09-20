#[path = "../src/campaign_control/mod.rs"]
pub mod campaign_control;

use std::collections::BTreeMap;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use campaign_control::{operations, Config, Provider, Reply, SOURCE_PATHS};
use casper_soak::{encoded, hash};
use eyre::{eyre, Result};
use serde_json::{json, Value};

#[derive(Clone)]
struct Fake {
    config: Config,
    cloud: Arc<Mutex<Cloud>>,
    now: u64,
    run: String,
    request: Vec<u8>,
    plan: Value,
    role: String,
    review_override: Option<Value>,
    environment_override: Option<Value>,
    fail_at: Option<usize>,
    calls: usize,
    lost_write: Option<usize>,
    writes: usize,
    lose_launch_response: bool,
    no_termination: bool,
    first_read_barrier: Option<Arc<Barrier>>,
}

struct Cloud {
    state: Value,
    version: usize,
    schedules: BTreeMap<String, Value>,
    instances: BTreeMap<String, Value>,
    launches: usize,
    deletes: usize,
    trace: Vec<(String, String, String, Option<Value>)>,
}

fn config() -> Config {
    let mut sources = json!({});
    for path in SOURCE_PATHS {
        sources[path] = json!("a".repeat(64));
    }
    let candidate = json!({"boot_image_id":"ocid1.image.fixture","shape":"VM.Standard.A1.Flex",
        "ocpus":8,"subnet_id":"ocid1.subnet.fixture","availability_domain":"test:AD-1",
        "image_digest":format!("sha256:{}","b".repeat(64)),
        "image_config_digest":format!("sha256:{}","c".repeat(64)),
        "node_binary_digest":format!("sha256:{}","d".repeat(64)),
        "bootstrap_path":"bootstrap.base64","bootstrap_sha256":"e".repeat(64)});
    Config::new(json!({"schema_version":1,"repository":campaign_control::REPOSITORY,
        "control_revision":"f".repeat(40),"campaign_id":"task-017-12-fixture",
        "identity_digest":"1".repeat(64),"approval_digest":"2".repeat(64),
        "preflight_candidate_id":"dev-amd64",
        "environment":{"id":99,"name":"casper-campaign","branch":"formal/soak-casper-consensus"},
        "storage":{"region":"us-sanjose-1","namespace":"test","bucket":"campaigns","object":"budget.json",
            "policy_id":"ocid1.policy.fixture","policy_sha256":hash(&encoded(&json!(["reviewed-policy"])).unwrap())},
        "compute":{"runner_group_id":1,"compartment_id":"ocid1.compartment.fixture",
            "candidates":{"dev-amd64":candidate,"dev-arm64":candidate}},
        "supervisor":{"application_id":"ocid1.fnapp.fixture","function_id":"ocid1.fnfunc.fixture",
            "image_digest":format!("sha256:{}","3".repeat(64)),"verification_sha256":"4".repeat(64),
            "timing":{"schedule_seconds":30,"invocation_seconds":30,"api_seconds":47,
                "termination_seconds":300,"clock_seconds":5,"submission_seconds":47}},
        "source_digests":sources,"activation":{"path":"activation.json","sha256":"5".repeat(64)}})).unwrap()
}

impl Fake {
    fn new() -> Self {
        let config = config();
        let candidate = &config.value["compute"]["candidates"]["dev-amd64"];
        let request = encoded(
            &json!({"campaign_id":config.value["campaign_id"],"stage":"preflight",
            "candidate_id":"dev-amd64","preflight_run_id":null}),
        )
        .unwrap();
        let plan = json!({"campaign_id":config.value["campaign_id"],"stage":"preflight","candidate_id":"dev-amd64",
            "identity_digest":config.value["identity_digest"],"approval_digest":config.value["approval_digest"],
            "harness_revision":config.value["control_revision"],"platform":"linux/amd64",
            "memory_gb":64,"cleanup_reserve_seconds":600,"max_launches":1,"duration_seconds":0,
            "runner_max_seconds":14400,"image_digest":candidate["image_digest"],
            "image_config_digest":candidate["image_config_digest"],"node_binary_digest":candidate["node_binary_digest"],
            "image_reference":format!("docker.io/f1r3flyindustries/f1r3fly-rust@{}",candidate["image_digest"].as_str().unwrap())});
        let state = json!({"schema_version":1,"config_digest":config.digest,"campaign_id":config.value["campaign_id"],
            "sequence":0,"slots":{"preflight":null,"baseline-dev-amd64":null,"baseline-dev-arm64":null}});
        Self {
            config,
            cloud: Arc::new(Mutex::new(Cloud {
                state,
                version: 1,
                schedules: BTreeMap::new(),
                instances: BTreeMap::new(),
                launches: 0,
                deletes: 0,
                trace: Vec::new(),
            })),
            now: 1800000000,
            run: "101".into(),
            request,
            plan,
            role: "maintain".into(),
            review_override: None,
            environment_override: None,
            fail_at: None,
            calls: 0,
            lost_write: None,
            writes: 0,
            lose_launch_response: false,
            no_termination: false,
            first_read_barrier: None,
        }
    }

    fn dispatch(&mut self) -> Result<Value> {
        operations::dispatch(
            self,
            &self.config.clone(),
            &self.request.clone(),
            &self.plan.clone(),
            &self.run.clone(),
            "#!/bin/bash\nreservation=__CASPER_RESERVATION__\njit=__CASPER_JIT__\n",
        )
    }

    fn slot(&self) -> Value { self.cloud.lock().unwrap().state["slots"]["preflight"].clone() }

    fn due(&mut self) { self.now = self.slot()["termination_epoch"].as_u64().unwrap(); }

    fn baseline(&mut self, candidate: &str, run: &str) {
        self.run = run.into();
        self.plan["stage"] = json!("baseline");
        self.plan["candidate_id"] = json!(candidate);
        self.plan["platform"] = json!(if candidate == "dev-amd64" {
            "linux/amd64"
        } else {
            "linux/arm64"
        });
        self.plan["duration_seconds"] = json!(86400);
        self.plan["runner_max_seconds"] = json!(93600);
        self.request = encoded(
            &json!({"campaign_id":self.config.value["campaign_id"],"stage":"baseline",
            "candidate_id":candidate,"preflight_run_id":"101"}),
        )
        .unwrap();
    }
}

fn reply(data: Value) -> Reply {
    Reply {
        status: 200,
        data,
        etag: None,
        next_page: None,
    }
}

impl Provider for Fake {
    fn now(&self) -> u64 { self.now }
    fn pause(&mut self, seconds: u64) { self.now += seconds }
    fn github(&mut self, path: &str) -> Result<Value> {
        if path.ends_with("/approvals") {
            return Ok(self.review_override.clone().unwrap_or(json!([{"state":"approved",
                "comment":campaign_control::approval_comment(&self.config,&self.request,&self.plan,&self.run)?,
                "user":{"id":200,"login":"maintainer"},"environments":[{"id":99,"name":"casper-campaign"}]}])));
        }
        if path.contains("deployment-branch-policies") {
            return Ok(
                json!({"total_count":1,"branch_policies":[{"type":"branch","name":self.config.value["environment"]["branch"]}]}),
            );
        }
        if path.ends_with("environments/casper-campaign") {
            return Ok(self.environment_override.clone().unwrap_or(json!({"id":99,"name":"casper-campaign",
                "can_admins_bypass":false,"deployment_branch_policy":{"custom_branch_policies":true,"protected_branches":false},
                "protection_rules":[{"type":"required_reviewers","prevent_self_review":true,
                    "reviewers":[{"type":"User","reviewer":{"id":200}}]}]})));
        }
        if path.ends_with("/permission") {
            return Ok(
                json!({"role_name":self.role,"permission":"write","user":{"id":200,"login":"maintainer"}}),
            );
        }
        Ok(
            json!({"id":self.run.parse::<u64>()?,"run_attempt":1,"event":"workflow_dispatch",
            "path":".github/workflows/merge-recovery-soak.yml","head_sha":self.config.value["control_revision"],
            "head_branch":self.config.value["environment"]["branch"],"repository":{"full_name":campaign_control::REPOSITORY},
            "status":"in_progress","actor":{"id":100},"triggering_actor":{"id":100}}),
        )
    }

    fn github_post(&mut self, _path: &str, body: &Value) -> Result<Value> {
        Ok(json!({"runner":{"id":500,"name":body["name"],"labels":[
            {"name":"self-hosted"},{"name":body["name"]}]},"encoded_jit_config":"c2VjcmV0"}))
    }
    fn oci(
        &mut self,
        service: &str,
        method: &str,
        path: &str,
        body: Option<&Value>,
        etag: Option<&str>,
    ) -> Result<Reply> {
        self.calls += 1;
        if self.fail_at == Some(self.calls) {
            return Err(eyre!("Injected transport failure."));
        }
        if matches!(service, "invoke" | "invoke-detached") {
            let body = body.unwrap();
            if body.as_object().is_some_and(|v| v.is_empty()) {
                return Ok(reply(operations::supervise(self, &self.config.clone())?));
            }
            return Ok(reply(operations::arm(
                self,
                &self.config.clone(),
                body["slot"].as_str().unwrap(),
                body["reservation_id"].as_str().unwrap(),
            )?));
        }
        let mut cloud = self.cloud.lock().unwrap();
        cloud
            .trace
            .push((service.into(), method.into(), path.into(), body.cloned()));
        match (service, method) {
            ("object", "GET") => {
                let mut result = reply(cloud.state.clone());
                result.etag = Some(cloud.version.to_string());
                drop(cloud);
                if let Some(barrier) = self.first_read_barrier.take() {
                    barrier.wait();
                }
                Ok(result)
            }
            ("object", "PUT") => {
                self.writes += 1;
                if etag != Some(cloud.version.to_string().as_str()) {
                    let mut result = reply(json!({}));
                    result.status = 412;
                    return Ok(result);
                }
                cloud.state = body.unwrap().clone();
                cloud.version += 1;
                if self.lost_write == Some(self.writes) {
                    return Err(eyre!("Lost conditional-write acknowledgment."));
                }
                let mut result = reply(json!({}));
                result.etag = Some(cloud.version.to_string());
                Ok(result)
            }
            ("identity", "GET") => Ok(reply(
                json!({"lifecycleState":"ACTIVE","statements":["reviewed-policy"]}),
            )),
            ("functions", "GET") => Ok(reply(
                json!({"id":self.config.value["supervisor"]["function_id"],
                "applicationId":self.config.value["supervisor"]["application_id"],
                "imageDigest":self.config.value["supervisor"]["image_digest"],"lifecycleState":"ACTIVE",
                "detachedModeTimeoutInSeconds":3600,"config":{"CASPER_CAMPAIGN_CONFIG_SHA256":self.config.digest},
                "invokeEndpoint":"https://fixture.functions.us-sanjose-1.oci.oraclecloud.com"}),
            )),
            ("scheduler", "POST") => {
                let id = format!(
                    "ocid1.resourceschedule.fixture{}",
                    cloud.schedules.len() + 1
                );
                let mut schedule = body.unwrap().clone();
                schedule["id"] = json!(id);
                schedule["lifecycleState"] = json!("ACTIVE");
                cloud.schedules.insert(id, schedule.clone());
                Ok(reply(schedule))
            }
            ("scheduler", "GET") => Ok(reply(
                cloud.schedules[path.rsplit('/').next().unwrap()].clone(),
            )),
            ("compute", "POST") => {
                cloud.launches += 1;
                let body = body.unwrap();
                let id = format!("ocid1.instance.fixture{}", cloud.launches);
                let instance = json!({"id":id,"compartmentId":body["compartmentId"],"imageId":body["sourceDetails"]["imageId"],
                    "shape":body["shape"],"shapeConfig":body["shapeConfig"],"freeformTags":body["freeformTags"],
                    "timeCreated":campaign_control::stamp(self.now)?,"lifecycleState":"RUNNING"});
                cloud.instances.insert(id, instance.clone());
                if self.lose_launch_response {
                    Err(eyre!("The launch response was lost."))
                } else {
                    Ok(reply(instance))
                }
            }
            ("compute", "GET") if path.contains('?') => {
                Ok(reply(json!(cloud.instances.values().collect::<Vec<_>>())))
            }
            ("compute", "GET") => {
                let id = path.rsplit('/').next().unwrap();
                if let Some(instance) = cloud.instances.get_mut(id) {
                    if instance["lifecycleState"] == "TERMINATING" && !self.no_termination {
                        instance["lifecycleState"] = json!("TERMINATED");
                    }
                    Ok(reply(instance.clone()))
                } else {
                    let mut result = reply(json!({}));
                    result.status = 404;
                    Ok(result)
                }
            }
            ("compute", "DELETE") => {
                cloud.deletes += 1;
                let id = path.split('?').next().unwrap().rsplit('/').next().unwrap();
                cloud.instances.get_mut(id).unwrap()["lifecycleState"] =
                    json!(if self.no_termination {
                        "STOPPED"
                    } else {
                        "TERMINATING"
                    });
                let mut result = reply(json!({}));
                result.status = 204;
                Ok(result)
            }
            _ => Err(eyre!("Unexpected provider operation.")),
        }
    }
}

#[test]
fn approval_requires_current_maintainer_role_and_exact_request_bytes() {
    let mut fake = Fake::new();
    fake.role = "write".into();
    assert!(fake.dispatch().is_err());
    assert_eq!(fake.cloud.lock().unwrap().launches, 0);
    fake.role = "maintain".into();
    let approval =
        campaign_control::approval_comment(&fake.config, &fake.request, &fake.plan, &fake.run)
            .unwrap();
    fake.review_override = Some(
        json!([{"state":"approved","comment":approval,"user":{"id":200,"login":"maintainer"},
        "environments":[{"id":99}]}]),
    );
    fake.request.push(b' ');
    assert!(fake.dispatch().is_err());
    assert!(fake.slot().is_null());
}

#[test]
fn self_review_missing_protection_and_ambiguous_approvals_are_rejected() {
    for variant in 0..5 {
        let mut fake = Fake::new();
        let approval =
            campaign_control::approval_comment(&fake.config, &fake.request, &fake.plan, &fake.run)
                .unwrap();
        let mut review = json!({"state":"approved","comment":approval,"user":{"id":200,"login":"maintainer"},"environments":[{"id":99}]});
        match variant {
            0 => review["user"]["id"] = json!(100),
            1 => review["state"] = json!("rejected"),
            2 => review["environments"] = json!([{"id":98}]),
            3 => {
                fake.environment_override =
                    Some(json!({"id":99,"name":"casper-campaign","can_admins_bypass":true}));
            }
            _ => (),
        }
        fake.review_override = Some(if variant == 4 {
            json!([review, review])
        } else {
            json!([review])
        });
        assert!(fake.dispatch().is_err());
        assert!(fake.slot().is_null());
    }
}

#[test]
fn authority_binds_plan_configuration_revision_and_run() {
    let fake = Fake::new();
    let original =
        campaign_control::approval_comment(&fake.config, &fake.request, &fake.plan, &fake.run)
            .unwrap();
    let mut changed = fake.plan.clone();
    changed["image_digest"] = json!(format!("sha256:{}", "7".repeat(64)));
    assert_ne!(
        original,
        campaign_control::approval_comment(&fake.config, &fake.request, &changed, &fake.run)
            .unwrap()
    );
    assert_ne!(
        original,
        campaign_control::approval_comment(&fake.config, &fake.request, &fake.plan, "102").unwrap()
    );
    let mut config = fake.config.value.clone();
    config["control_revision"] = json!("8".repeat(40));
    assert_ne!(
        original,
        campaign_control::approval_comment(
            &Config::new(config).unwrap(),
            &fake.request,
            &fake.plan,
            &fake.run
        )
        .unwrap()
    );
}

#[test]
fn two_controllers_share_one_authoritative_slot() {
    let mut first = Fake::new();
    let mut second = first.clone();
    let barrier = Arc::new(Barrier::new(2));
    first.first_read_barrier = Some(barrier.clone());
    second.first_read_barrier = Some(barrier);
    let cloud = first.cloud.clone();
    let a = thread::spawn(move || first.dispatch());
    let b = thread::spawn(move || second.dispatch());
    let results = [a.join().unwrap(), b.join().unwrap()];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(cloud.lock().unwrap().launches, 1);
}

#[test]
fn lost_write_acknowledgments_consume_slots_without_launch_retries() {
    for write in 1..=5 {
        let mut fake = Fake::new();
        fake.lost_write = Some(write);
        let _ = fake.dispatch();
        fake.lost_write = None;
        assert!(fake.dispatch().is_err());
        assert!(fake.cloud.lock().unwrap().launches <= 1);
        assert!(!fake.slot().is_null());
    }
}

#[test]
fn every_provider_boundary_fails_without_duplicate_launch() {
    let mut complete = Fake::new();
    complete.dispatch().unwrap();
    let calls = complete.calls;
    assert!(calls > 15);
    for call in 1..=calls {
        let mut fake = Fake::new();
        fake.fail_at = Some(call);
        let _ = fake.dispatch();
        fake.fail_at = None;
        let _ = fake.dispatch();
        assert!(fake.cloud.lock().unwrap().launches <= 1, "boundary {call}");
    }
}

#[test]
fn lost_launch_response_is_reconciled_without_another_submission() {
    let mut fake = Fake::new();
    fake.lose_launch_response = true;
    fake.dispatch().unwrap();
    assert_eq!(fake.cloud.lock().unwrap().launches, 1);
    assert_eq!(fake.slot()["state"], "launched");
    assert_eq!(
        fake.slot()["infrastructure_failures"],
        json!(["launch_response_unknown"])
    );
    assert!(fake.dispatch().is_err());
    fake.due();
    operations::supervise(&mut fake, &config()).unwrap();
    assert_eq!(fake.slot()["state"], "terminated");
    assert_eq!(
        campaign_control::final_verdict(&fake.slot()).unwrap(),
        "infrastructure_failure"
    );
}

#[test]
fn schedule_is_individual_and_armed_before_submission() {
    let mut fake = Fake::new();
    fake.dispatch().unwrap();
    let cloud = fake.cloud.lock().unwrap();
    let schedule = cloud.schedules.values().next().unwrap();
    assert_eq!(schedule["recurrenceDetails"], "FREQ=DAILY;COUNT=1");
    assert_eq!(schedule["action"], "START_RESOURCE");
    assert_eq!(
        schedule["resources"][0]["id"],
        fake.config.value["supervisor"]["function_id"]
    );
    assert!(
        campaign_control::epoch(&schedule["timeStarts"]).unwrap()
            < cloud.state["slots"]["preflight"]["deadline_epoch"]
                .as_u64()
                .unwrap()
    );
    let before = cloud
        .trace
        .iter()
        .take_while(|(s, m, _, _)| !(s == "compute" && m == "POST"))
        .collect::<Vec<_>>();
    assert!(before.iter().any(|(s, m, _, b)| s == "object"
        && m == "PUT"
        && b.as_ref().unwrap()["slots"]["preflight"]["state"] == "submitting"));
    assert!(before.iter().any(|(s, m, _, b)| s == "object"
        && m == "PUT"
        && b.as_ref().unwrap()["slots"]["preflight"]["state"] == "armed"));
}

#[test]
fn supervisor_uses_storage_deadlines_and_observed_termination() {
    let mut fake = Fake::new();
    fake.dispatch().unwrap();
    let original_deadline = fake.slot()["termination_epoch"].clone();
    {
        let mut cloud = fake.cloud.lock().unwrap();
        cloud.instances.values_mut().next().unwrap()["freeformTags"]["deadline"] = json!(0);
    }
    let early = operations::supervise(&mut fake, &config()).unwrap();
    assert_eq!(early["reports"], json!([]));
    assert_eq!(fake.cloud.lock().unwrap().deletes, 0);
    fake.now = original_deadline.as_u64().unwrap();
    let report = operations::supervise(&mut fake, &config()).unwrap();
    assert_eq!(report["termination_confirmed"], true);
    assert_eq!(fake.slot()["termination_confirmed"], true);
    assert!(fake
        .cloud
        .lock()
        .unwrap()
        .trace
        .iter()
        .any(|(s, m, p, _)| s == "compute"
            && m == "DELETE"
            && p.contains("preserveBootVolume=false")));
}

#[test]
fn stopped_missing_and_unobservable_instances_never_count_as_terminated() {
    for mode in 0..3 {
        let mut fake = Fake::new();
        fake.dispatch().unwrap();
        fake.due();
        match mode {
            0 => fake.no_termination = true,
            1 => fake.cloud.lock().unwrap().instances.clear(),
            _ => fake.fail_at = Some(fake.calls + 2),
        }
        let result = operations::supervise(&mut fake, &config());
        assert!(result.is_err() || result.unwrap()["termination_confirmed"] == false);
        assert_ne!(fake.slot()["termination_confirmed"], true);
        assert_ne!(fake.slot()["state"], "terminated");
    }
}

#[test]
fn product_failures_survive_cleanup_failure() {
    let mut fake = Fake::new();
    fake.dispatch().unwrap();
    fake.due();
    fake.no_termination = true;
    fake.cloud.lock().unwrap().state["slots"]["preflight"]["product_failures"] =
        json!(["finality_failed"]);
    operations::supervise(&mut fake, &config()).unwrap();
    assert_eq!(
        campaign_control::final_verdict(&fake.slot()).unwrap(),
        "product_failure"
    );
    assert_eq!(fake.slot()["product_failures"], json!(["finality_failed"]));
    assert!(!fake.slot()["infrastructure_failures"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn three_fixed_slots_require_passing_preflight_and_never_refund() {
    let mut fake = Fake::new();
    fake.dispatch().unwrap();
    fake.baseline("dev-amd64", "102");
    assert!(fake.dispatch().is_err());
    fake.due();
    operations::supervise(&mut fake, &config()).unwrap();
    assert!(fake.dispatch().is_err());
    fake.cloud.lock().unwrap().state["slots"]["preflight"]["result"] = json!("passed");
    fake.dispatch().unwrap();
    fake.baseline("dev-arm64", "103");
    fake.dispatch().unwrap();
    assert_eq!(fake.cloud.lock().unwrap().launches, 3);
    for (candidate, run) in [("dev-amd64", "104"), ("dev-arm64", "105")] {
        fake.baseline(candidate, run);
        assert!(fake.dispatch().is_err());
    }
    assert_eq!(fake.cloud.lock().unwrap().launches, 3);
}

#[test]
fn missing_replaced_and_corrupt_authoritative_records_block_dispatch() {
    for field in ["config_digest", "campaign_id", "slots", "sequence"] {
        let mut fake = Fake::new();
        fake.cloud.lock().unwrap().state[field] = Value::Null;
        assert!(fake.dispatch().is_err());
        assert_eq!(fake.cloud.lock().unwrap().launches, 0);
    }
}

#[test]
fn plan_rejects_mutable_images_wrong_architecture_and_short_workloads() {
    let mut fake = Fake::new();
    for (field, value) in [
        ("image_reference", json!("node:latest")),
        ("platform", json!("linux/arm64")),
        ("memory_gb", json!(48)),
        ("runner_max_seconds", json!(20000)),
        ("max_launches", json!(2)),
    ] {
        let mut changed = fake.plan.clone();
        changed[field] = value;
        assert!(campaign_control::validate_plan(&fake.config, &fake.request, &changed).is_err());
    }
    fake.baseline("dev-amd64", "102");
    fake.plan["duration_seconds"] = json!(86000);
    assert!(campaign_control::validate_plan(&fake.config, &fake.request, &fake.plan).is_err());
}

#[test]
fn host_admission_requires_identity_protection_and_full_workload_window() {
    let mut fake = Fake::new();
    fake.dispatch().unwrap();
    let mut slot = fake.slot();
    slot["plan"]["duration_seconds"] = json!(86400);
    slot["termination_epoch"] = json!(fake.now + 90000);
    let mut host = json!({"instance_id":slot["instance_id"],"reservation_id":slot["reservation_id"],
        "platform":slot["plan"]["platform"],"image_digest":slot["plan"]["image_digest"],
        "image_config_digest":slot["plan"]["image_config_digest"],"node_binary_digest":slot["plan"]["node_binary_digest"],
        "runner_label":format!("casper-{}",slot["reservation_id"].as_str().unwrap()),"runner_exclusive":true,
        "memory_total_mib":64000,"memory_max_bytes":45056_u64*1024*1024,"memory_swap_max_bytes":0,
        "memory_available_mib":8192,"disk_free_mib":8192,"disk_floor_mib":4096,
        "disk_guardian_active":true,"memory_guardian_active":true,"metadata_access_blocked":true});
    campaign_control::host_admission(&fake.config, &slot, &host, fake.now).unwrap();
    assert!(campaign_control::host_admission(&fake.config, &slot, &host, fake.now + 4000).is_err());
    for field in [
        "runner_exclusive",
        "disk_guardian_active",
        "memory_guardian_active",
        "metadata_access_blocked",
    ] {
        let saved = host[field].clone();
        host[field] = json!(false);
        assert!(campaign_control::host_admission(&fake.config, &slot, &host, fake.now).is_err());
        host[field] = saved;
    }
    host["node_binary_digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    assert!(campaign_control::host_admission(&fake.config, &slot, &host, fake.now).is_err());
}

#[test]
fn unsupported_or_excessive_timing_bounds_reject_configuration() {
    for value in [Value::Null, json!(0), json!(601)] {
        let mut candidate = config().value;
        candidate["supervisor"]["timing"]["schedule_seconds"] = value;
        assert!(Config::new(candidate).is_err());
    }
}

#[test]
fn utc_timestamp_roundtrips_and_rejects_invalid_dates() {
    for now in [1, 1800000000, 4102444800] {
        assert_eq!(
            campaign_control::epoch(&json!(campaign_control::stamp(now).unwrap())).unwrap(),
            now
        );
    }
    for value in [
        "2026-02-30T12:00:00Z",
        "2026-01-01T25:00:00Z",
        "2026-01-01T00:00:00-08:00",
        "invalid",
    ] {
        assert!(campaign_control::epoch(&json!(value)).is_err());
    }
}
