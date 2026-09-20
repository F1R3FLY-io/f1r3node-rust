#![cfg(target_os = "linux")]

use std::fs::{self, File};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::TempDir;

fn binary() -> &'static str { env!("CARGO_BIN_EXE_casper-campaign-reservation") }

struct Fixture {
    temp: TempDir,
    config: Value,
}

impl Fixture {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
            config: json!({
                "schema_version": 1,
                "campaign_id": "task-017-12-reservation-test",
                "identity_digest": "a".repeat(64),
                "approval_digest": "b".repeat(64),
                "preflight_candidate_id": "dev-amd64"
            }),
        }
    }

    fn root(&self) -> std::path::PathBuf { self.temp.path().join("store") }

    fn invoke(&self, command: &str, value: &Value) -> Output {
        self.raw(command, &serde_json::to_vec(value).unwrap())
    }

    fn raw(&self, command: &str, bytes: &[u8]) -> Output {
        let path = self.temp.path().join("input.json");
        fs::write(&path, bytes).unwrap();
        Command::new(binary())
            .arg(command)
            .arg(self.root())
            .arg(path)
            .output()
            .unwrap()
    }

    fn init(&self) { passed(self.invoke("init", &self.config)); }

    fn request(&self, stage: &str, candidate: &str, run: &str) -> Value {
        json!({
            "schema_version": 1,
            "campaign_id": self.config["campaign_id"],
            "identity_digest": self.config["identity_digest"],
            "approval_digest": self.config["approval_digest"],
            "stage": stage,
            "candidate_id": candidate,
            "run_id": run,
            "run_attempt": 1
        })
    }

    fn preflight(&self) {
        passed(self.invoke("reserve", &self.request("preflight", "dev-amd64", "101")));
    }
}

fn passed(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["scope"], "local-baseline-reservation-store");
    assert_eq!(receipt["execution_enabled"], false);
    assert_eq!(receipt["approval_authenticated"], false);
    receipt
}

fn rejected(output: Output) {
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn initialization_is_exclusive_and_owner_only() {
    let fixture = Fixture::new();
    fixture.init();
    assert_eq!(
        fs::metadata(fixture.root()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let binding = fs::read(fixture.root().join("campaign.json")).unwrap();
    rejected(fixture.invoke("init", &fixture.config));
    assert_eq!(
        fs::read(fixture.root().join("campaign.json")).unwrap(),
        binding
    );
}

#[test]
fn exactly_three_slots_survive_separate_processes() {
    let fixture = Fixture::new();
    fixture.init();
    for (stage, candidate, run, slot) in [
        ("preflight", "dev-amd64", "101", "preflight.json"),
        ("baseline", "dev-amd64", "102", "baseline-dev-amd64.json"),
        ("baseline", "dev-arm64", "103", "baseline-dev-arm64.json"),
    ] {
        let request = fixture.request(stage, candidate, run);
        let receipt = passed(fixture.invoke("reserve", &request));
        assert_eq!(receipt["request"], request);
        let retained: Value =
            serde_json::from_slice(&fs::read(fixture.root().join(slot)).unwrap()).unwrap();
        assert_eq!(receipt, retained);
        rejected(fixture.invoke("reserve", &request));
        rejected(fixture.invoke("reserve", &fixture.request(stage, candidate, "999")));
    }
    assert_eq!(fs::read_dir(fixture.root()).unwrap().count(), 4);
}

#[test]
fn baseline_requires_preflight_and_run_ids_cannot_be_reused() {
    let fixture = Fixture::new();
    fixture.init();
    rejected(fixture.invoke("reserve", &fixture.request("baseline", "dev-amd64", "102")));
    fixture.preflight();
    rejected(fixture.invoke("reserve", &fixture.request("baseline", "dev-amd64", "101")));
    passed(fixture.invoke("reserve", &fixture.request("baseline", "dev-amd64", "102")));
    rejected(fixture.invoke("reserve", &fixture.request("baseline", "dev-arm64", "102")));
}

#[test]
fn preflight_uses_only_the_bound_candidate() {
    let mut fixture = Fixture::new();
    fixture.config["preflight_candidate_id"] = json!("dev-arm64");
    fixture.init();
    rejected(fixture.invoke("reserve", &fixture.request("preflight", "dev-amd64", "101")));
    passed(fixture.invoke("reserve", &fixture.request("preflight", "dev-arm64", "101")));
}

#[test]
fn drift_reruns_unknown_fields_and_extra_stages_are_rejected() {
    let fixture = Fixture::new();
    fixture.init();
    for (field, value) in [
        ("campaign_id", json!("task-017-12-other")),
        ("identity_digest", json!("c".repeat(64))),
        ("approval_digest", json!("c".repeat(64))),
        ("run_attempt", json!(2)),
        ("run_attempt", json!(1.0)),
        ("run_id", json!("01")),
        ("run_id", json!("0")),
        ("run_id", json!("1".repeat(21))),
        ("run_id", json!(102)),
        ("stage", json!("stability")),
        ("stage", json!("replacement")),
        ("candidate_id", json!("other")),
        ("max_launches", json!(10)),
        ("schema_version", json!(2)),
    ] {
        let mut request = fixture.request("preflight", "dev-amd64", "101");
        request[field] = value;
        rejected(fixture.invoke("reserve", &request));
    }
    fixture.preflight();
}

#[test]
fn invalid_configuration_never_creates_a_store() {
    for (field, value) in [
        ("campaign_id", json!("../escape")),
        ("campaign_id", json!("task-017-12-")),
        (
            "campaign_id",
            json!(format!("task-017-12-{}", "a".repeat(49))),
        ),
        ("identity_digest", json!("A".repeat(64))),
        ("approval_digest", json!("b".repeat(63))),
        ("preflight_candidate_id", json!("other")),
        ("memory_gb", json!(128)),
        ("schema_version", json!(0)),
    ] {
        let mut fixture = Fixture::new();
        fixture.config[field] = value;
        rejected(fixture.invoke("init", &fixture.config));
        assert!(!fixture.root().exists());
    }
}

#[test]
fn malformed_duplicate_and_oversized_inputs_are_rejected() {
    let fixture = Fixture::new();
    for bytes in [
        Vec::new(),
        b"[]".to_vec(),
        b"{} {}".to_vec(),
        b"{\"schema_version\":1,\"schema_version\":1}".to_vec(),
        vec![b' '; 4097],
    ] {
        rejected(fixture.raw("init", &bytes));
        assert!(!fixture.root().exists());
    }
    let mut exact = serde_json::to_vec(&fixture.config).unwrap();
    exact.resize(4096, b' ');
    passed(fixture.raw("init", &exact));
    let request = fixture.request("preflight", "dev-amd64", "101");
    let mut duplicate = serde_json::to_string(&request).unwrap();
    duplicate.insert_str(1, "\"run_attempt\":1,");
    rejected(fixture.raw("reserve", duplicate.as_bytes()));
    let mut oversized = serde_json::to_vec(&request).unwrap();
    oversized.resize(4097, b' ');
    rejected(fixture.raw("reserve", &oversized));
    fixture.preflight();
}

#[test]
fn input_links_directories_and_missing_files_are_rejected() {
    let fixture = Fixture::new();
    let real = fixture.temp.path().join("real.json");
    fs::write(&real, serde_json::to_vec(&fixture.config).unwrap()).unwrap();
    let link = fixture.temp.path().join("link.json");
    symlink(&real, &link).unwrap();
    for input in [
        link,
        fixture.temp.path().to_path_buf(),
        fixture.temp.path().join("missing"),
    ] {
        rejected(
            Command::new(binary())
                .arg("init")
                .arg(fixture.root())
                .arg(input)
                .output()
                .unwrap(),
        );
    }
    assert!(!fixture.root().exists());
}

#[test]
fn unsafe_roots_and_missing_bindings_are_rejected() {
    let fixture = Fixture::new();
    rejected(fixture.invoke("reserve", &fixture.request("preflight", "dev-amd64", "101")));
    fixture.init();
    fs::set_permissions(fixture.root(), fs::Permissions::from_mode(0o755)).unwrap();
    rejected(fixture.invoke("reserve", &fixture.request("preflight", "dev-amd64", "101")));
    fs::set_permissions(fixture.root(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(fixture.root().join("campaign.json"), b"{").unwrap();
    rejected(fixture.invoke("reserve", &fixture.request("preflight", "dev-amd64", "101")));
    fs::remove_file(fixture.root().join("campaign.json")).unwrap();
    rejected(fixture.invoke("reserve", &fixture.request("preflight", "dev-amd64", "101")));
    rejected(fixture.invoke("init", &fixture.config));
}

#[test]
fn root_links_and_noncanonical_roots_are_rejected() {
    let fixture = Fixture::new();
    fixture.init();
    let input = fixture.temp.path().join("request.json");
    fs::write(
        &input,
        serde_json::to_vec(&fixture.request("preflight", "dev-amd64", "101")).unwrap(),
    )
    .unwrap();
    let link = fixture.temp.path().join("link");
    symlink(fixture.root(), &link).unwrap();
    let parent = fixture.temp.path().join("parent-link");
    symlink(fixture.temp.path(), &parent).unwrap();
    for root in [
        link,
        parent.join("store"),
        fixture.root().join("../store"),
        "relative-store".into(),
    ] {
        rejected(
            Command::new(binary())
                .arg("reserve")
                .arg(root)
                .arg(&input)
                .output()
                .unwrap(),
        );
    }
    fixture.preflight();
}

#[test]
fn partial_wrong_and_linked_slot_records_block_further_reservations() {
    for damage in ["partial", "wrong", "link", "directory", "oversized"] {
        let fixture = Fixture::new();
        fixture.init();
        fixture.preflight();
        let slot = fixture.root().join("baseline-dev-amd64.json");
        match damage {
            "partial" => fs::write(&slot, b"{").unwrap(),
            "wrong" => {
                fs::copy(fixture.root().join("preflight.json"), &slot).unwrap();
            }
            "link" => symlink(fixture.root().join("preflight.json"), &slot).unwrap(),
            "directory" => fs::create_dir(&slot).unwrap(),
            _ => fs::write(&slot, vec![b' '; 4097]).unwrap(),
        }
        rejected(fixture.invoke("reserve", &fixture.request("baseline", "dev-arm64", "103")));
        assert!(!fixture.root().join("baseline-dev-arm64.json").exists());
    }
}

#[test]
fn missing_preflight_cannot_be_recreated_after_a_baseline() {
    let fixture = Fixture::new();
    fixture.init();
    fixture.preflight();
    passed(fixture.invoke("reserve", &fixture.request("baseline", "dev-amd64", "102")));
    fs::remove_file(fixture.root().join("preflight.json")).unwrap();
    rejected(fixture.invoke("reserve", &fixture.request("preflight", "dev-amd64", "103")));
    assert!(!fixture.root().join("preflight.json").exists());
}

#[test]
fn competing_processes_cannot_consume_one_slot_twice() {
    let fixture = Fixture::new();
    fixture.init();
    let input = fixture.temp.path().join("race.json");
    fs::write(
        &input,
        serde_json::to_vec(&fixture.request("preflight", "dev-amd64", "101")).unwrap(),
    )
    .unwrap();
    let children: Vec<_> = (0..16)
        .map(|_| {
            Command::new(binary())
                .arg("reserve")
                .arg(fixture.root())
                .arg(&input)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let mut successes = 0;
    for child in children {
        let output = child.wait_with_output().unwrap();
        if output.status.success() {
            passed(output);
            successes += 1;
        } else {
            rejected(output);
        }
    }
    assert_eq!(successes, 1);
    assert_eq!(fs::read_dir(fixture.root()).unwrap().count(), 2);
}

#[test]
fn competing_slots_cannot_reuse_one_run_id() {
    let fixture = Fixture::new();
    fixture.init();
    fixture.preflight();
    let children: Vec<_> = ["dev-amd64", "dev-arm64"]
        .into_iter()
        .map(|candidate| {
            let input = fixture.temp.path().join(format!("{candidate}.json"));
            fs::write(
                &input,
                serde_json::to_vec(&fixture.request("baseline", candidate, "102")).unwrap(),
            )
            .unwrap();
            Command::new(binary())
                .arg("reserve")
                .arg(fixture.root())
                .arg(input)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let mut successes = 0;
    for child in children {
        let output = child.wait_with_output().unwrap();
        if output.status.success() {
            passed(output);
            successes += 1;
        } else {
            rejected(output);
        }
    }
    assert_eq!(successes, 1);
}

#[test]
fn lost_acknowledgment_does_not_release_a_slot() {
    let fixture = Fixture::new();
    fixture.init();
    let input = fixture.temp.path().join("discarded.json");
    fs::write(
        &input,
        serde_json::to_vec(&fixture.request("preflight", "dev-amd64", "101")).unwrap(),
    )
    .unwrap();
    assert!(Command::new(binary())
        .arg("reserve")
        .arg(fixture.root())
        .arg(&input)
        .stdout(Stdio::null())
        .status()
        .unwrap()
        .success());
    rejected(fixture.invoke("reserve", &fixture.request("preflight", "dev-amd64", "101")));
    for command in ["reset", "release", "refund", "delete", "retry"] {
        rejected(fixture.invoke(command, &fixture.config));
    }
}

#[test]
#[ignore]
fn lock_holder_process() {
    let root = std::env::var_os("RESERVATION_TEST_ROOT").unwrap();
    let lock = File::open(&root).unwrap();
    lock.lock().unwrap();
    fs::write(Path::new(&root).join("ready"), b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(10));
}

#[test]
fn lock_contention_rejects_and_killed_holder_releases_the_lock() {
    let fixture = Fixture::new();
    fixture.init();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "lock_holder_process"])
        .env("RESERVATION_TEST_ROOT", fixture.root())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.root().join("ready").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let ready = fixture.root().join("ready").exists();
    let output = fixture.invoke("reserve", &fixture.request("preflight", "dev-amd64", "101"));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(ready);
    rejected(output);
    fixture.preflight();
}
