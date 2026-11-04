#![cfg(target_os = "linux")]

#[path = "../src/authority_process.rs"]
#[allow(dead_code)]
mod process;

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use casper_soak::{encoded, file_hash, record};
use serde_json::{json, Value};

fn config(root: &Path, executable: &Path) -> Value {
    let executable = fs::canonicalize(executable).unwrap();
    let pinned = root.join("sleep");
    fs::copy(executable, &pinned).unwrap();
    json!({"path":pinned,"sha256":file_hash(&pinned).unwrap(),"arguments":["60"],"working_directory":root})
}

fn request(child: Value, action: &str) -> Value {
    json!({"schema_version":1,"action":action,"hold_ms":10,"process":child,
        "clock_id":format!("linux-monotonic:{}",fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap().trim()),
        "deadline_monotonic_ns":process::now().unwrap()+2_000_000_000})
}

struct Server {
    child: std::process::Child,
    owner: Value,
    root: tempfile::TempDir,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Server {
    fn new() -> Self { Self::with_executable(Path::new("/usr/bin/sleep")) }

    fn with_executable(executable: &Path) -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.json");
        fs::write(&path, encoded(&config(root.path(), executable)).unwrap()).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_casper-authority-process"))
            .args([
                "--config",
                path.to_str().unwrap(),
                "--output",
                root.path().join("owner").to_str().unwrap(),
                "--lifetime-ms",
                "5000",
            ])
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        let owner = loop {
            if let Ok(owner) = record(&root.path().join("owner/owner.json")) {
                break owner;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        };
        Self { child, owner, root }
    }
}

#[test]
fn symlinked_system_executable_can_be_pinned_and_started() {
    let source = tempfile::tempdir().unwrap();
    let link = source.path().join("sleep");
    std::os::unix::fs::symlink("/usr/bin/sleep", &link).unwrap();
    assert!(file_hash(&link).is_err());
    let server = Server::with_executable(&link);
    let configured = record(&server.root.path().join("config.json")).unwrap();
    let executable = Path::new(configured["path"].as_str().unwrap());
    assert!(fs::symlink_metadata(executable).unwrap().is_file());
    assert_eq!(
        server.owner["child"]["executable_sha256"],
        configured["sha256"]
    );
}

#[test]
fn owned_pause_resumes_and_restart_replaces_the_child() {
    let server = Server::new();
    let path = server.root.path().join("pause.json");
    fs::write(
        &path,
        encoded(&request(server.owner["child"].clone(), "pause")).unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_casper-authority-process"))
        .arg("--owner")
        .arg(server.root.path().join("owner/owner.json"))
        .arg("--request")
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pause = casper_soak::parse(&output.stdout).unwrap();
    assert_eq!(pause["observed_state"], "stopped");
    assert_eq!(pause["resumed"], true);
    assert_eq!(pause["before"]["pid"], pause["after"]["pid"]);
    let restart = process::call(
        &server.owner,
        &request(pause["after"].clone(), "restart"),
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(restart["prior_exit"], true);
    assert_ne!(restart["before"]["pid"], restart["after"]["pid"]);
    assert!(!std::path::Path::new(&format!("/proc/{}", restart["before"]["pid"])).exists());
    assert!(server.root.path().join("owner/receipt-0002.json").exists());
}

#[test]
fn stale_foreign_replayed_and_expired_requests_cannot_signal_a_child() {
    let server = Server::new();
    let valid = request(server.owner["child"].clone(), "pause");
    let mut stale = valid.clone();
    stale["process"]["process_start_ticks"] = 1.into();
    let mut foreign = valid.clone();
    foreign["clock_id"] = "foreign".into();
    let mut expired = valid.clone();
    expired["deadline_monotonic_ns"] = 1.into();
    for invalid in [stale, foreign, expired] {
        assert!(process::call(&server.owner, &invalid, Duration::from_secs(1)).is_err());
    }
    let mut impostor = server.owner.clone();
    impostor["identity"]["executable_sha256"] = "0".repeat(64).into();
    assert!(process::call(&impostor, &valid, Duration::from_secs(1)).is_err());
    assert!(process::call(&server.owner, &valid, Duration::from_secs(2)).is_ok());
    assert!(process::call(&server.owner, &valid, Duration::from_secs(1)).is_err());
}

#[test]
fn owner_exit_kills_its_child() {
    let server = Server::new();
    let pid = server.owner["child"]["pid"].as_u64().unwrap() as u32;
    drop(server);
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if process::identity(pid).is_err() {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
}
