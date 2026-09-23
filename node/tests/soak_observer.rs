#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use node::rust::configuration::{builder, NodeConf, Options};
use node::rust::soak_observer::{
    process_start_ticks, validate_config, Observer, MAX_CONFIG_BYTES, MAX_FRAME_BYTES,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use uuid::Uuid;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("soak-observer-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }

    fn socket(&self) -> PathBuf { self.0.join("observer.sock") }
}

impl Drop for Directory {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

fn configuration(directory: &Directory, enabled: bool) -> NodeConf {
    let mut args = vec![
        "node".to_owned(),
        "run".to_owned(),
        "--config-file".to_owned(),
        directory.0.join("absent.conf").to_str().unwrap().to_owned(),
        "--data-dir".to_owned(),
        directory.0.to_str().unwrap().to_owned(),
    ];
    if enabled {
        args.extend([
            "--soak-observer".to_owned(),
            json!({
                "directory": directory.0,
                "source-revision": "a".repeat(40),
                "approved-request-sha256": "b".repeat(64),
                "peer-pid": std::process::id(),
                "peer-start-ticks": process_start_ticks(std::process::id()).unwrap(),
                "session-timeout-ms": 500,
                "max-sessions": 128
            })
            .to_string(),
        ]);
    }
    let options = Options::try_parse_from(args).unwrap();
    builder::build(options).unwrap().0
}

async fn receive(stream: &mut UnixStream) -> Value {
    let size = stream.read_u32().await.unwrap() as usize;
    assert!(size > 0 && size <= MAX_FRAME_BYTES);
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn connect(path: &Path) -> (UnixStream, Value) {
    let mut stream = UnixStream::connect(path).await.unwrap();
    let hello = receive(&mut stream).await;
    (stream, hello)
}

fn request(hello: &Value) -> Value {
    json!({
        "schema_version": 1,
        "request_id": Uuid::new_v4().to_string(),
        "incarnation": hello["identity"]["incarnation"],
        "challenge": hello["challenge"],
        "executable_sha256": hello["identity"]["executable_sha256"],
        "configuration_sha256": hello["identity"]["configuration_sha256"],
        "approved_request_sha256": hello["approved_request_sha256"],
        "operation": "capabilities"
    })
}

async fn send(stream: &mut UnixStream, value: &Value) {
    let bytes = serde_json::to_vec(value).unwrap();
    stream.write_u32(bytes.len() as u32).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
}

async fn closed(stream: &mut UnixStream) {
    let result = tokio::time::timeout(Duration::from_secs(3), stream.read_u8())
        .await
        .unwrap();
    assert!(result.is_err(), "A rejected session returned bytes.");
}

#[test]
fn runtime_source_returns_before_observer_cleanup_and_exit() {
    let source = include_str!("../src/rust/runtime/node_runtime.rs");
    let main = source
        .split_once("pub async fn main(&self)")
        .unwrap()
        .1
        .split_once("async fn node_program")
        .unwrap()
        .0;
    assert!(!main.contains("handle_unrecoverable_errors("));
    assert!(!main.contains("std::process::exit("));
    assert!(main.contains("\n        program.await\n    }"));

    let start = source
        .split_once("pub async fn start(node_conf: NodeConf)")
        .unwrap()
        .1;
    let returned = start.find("let result = runtime.main().await;").unwrap();
    let stopped = start.find("observer.stop().await;").unwrap();
    let exited = start
        .find("handle_unrecoverable_errors(async { result }).await")
        .unwrap();
    assert!(returned < stopped && stopped < exited);
}

#[test]
fn default_configuration_has_no_observer() {
    let directory = Directory::new();
    let conf = configuration(&directory, false);
    assert!(conf.soak_observer.is_none());
    assert!(Observer::bind(&conf).unwrap().is_none());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[tokio::test]
async fn capabilities_are_bound_and_never_claim_live_qualification() {
    let directory = Directory::new();
    let mut conf = configuration(&directory, true);
    conf.casper.validator_private_key = Some("synthetic-secret-sentinel".to_owned());
    let executable = fs::read("/proc/self/exe").unwrap();
    let executable_sha256 =
        hex::encode(crypto::rust::hash::sha_256::Sha256Hasher::hash(executable));
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    let (mut stream, hello) = connect(&directory.socket()).await;
    assert_eq!(hello["permission"], "read-only");
    assert_eq!(
        hello["identity"]["configuration_scope"],
        "batch-a-public-config-v1"
    );
    assert_eq!(hello["identity"]["executable_sha256"], executable_sha256);
    assert_eq!(hello["identity"]["pid"], std::process::id());
    assert_eq!(
        hello["identity"]["process_start_ticks"],
        process_start_ticks(std::process::id()).unwrap()
    );
    let request = request(&hello);
    send(&mut stream, &request).await;
    let response = receive(&mut stream).await;
    assert_eq!(response["request_id"], request["request_id"]);
    assert_eq!(response["identity"], hello["identity"]);
    assert_eq!(response["live_profile_qualified"], false);
    let capabilities = response["capabilities"].as_array().unwrap();
    assert_eq!(capabilities.len(), 5);
    assert!(capabilities.iter().all(|item| item["supported"] == false));
    assert!(response["sequence"].as_u64().unwrap() > hello["sequence"].as_u64().unwrap());
    assert!(response["monotonic_ns"].as_u64().unwrap() >= hello["monotonic_ns"].as_u64().unwrap());
    assert!(!response.to_string().contains("synthetic-secret-sentinel"));
    assert!(!hello.to_string().contains("synthetic-secret-sentinel"));
    let expected =
        crypto::rust::hash::sha_256::Sha256Hasher::hash(serde_json::to_vec(&request).unwrap());
    assert_eq!(response["request_sha256"], hex::encode(expected));
    closed(&mut stream).await;
    observer.stop().await;
    assert!(!directory.socket().exists());
}

#[tokio::test]
async fn mismatched_identities_and_fault_commands_are_rejected() {
    let directory = Directory::new();
    let observer = Observer::bind(&configuration(&directory, true))
        .unwrap()
        .unwrap()
        .spawn();
    for field in [
        "incarnation",
        "challenge",
        "executable_sha256",
        "configuration_sha256",
        "approved_request_sha256",
        "operation",
        "request_id",
    ] {
        let (mut stream, hello) = connect(&directory.socket()).await;
        let mut value = request(&hello);
        value[field] = json!("invalid");
        send(&mut stream, &value).await;
        closed(&mut stream).await;
    }
    let (mut stream, hello) = connect(&directory.socket()).await;
    let mut value = request(&hello);
    value["arm_fault"] = json!(true);
    send(&mut stream, &value).await;
    closed(&mut stream).await;
    observer.stop().await;
}

#[tokio::test]
async fn session_sequences_match_an_independent_event_oracle() {
    let directory = Directory::new();
    let observer = Observer::bind(&configuration(&directory, true))
        .unwrap()
        .unwrap()
        .spawn();
    let mut expected_sequence = 0u64;
    let mut old_request = None;
    let mut challenges = std::collections::BTreeSet::new();
    for action in ["accept", "replay", "disconnect", "accept", "replay"] {
        let (mut stream, hello) = connect(&directory.socket()).await;
        expected_sequence += 1;
        assert_eq!(hello["sequence"], expected_sequence);
        let challenge = hello["challenge"].as_str().unwrap();
        let (nonce, sequence) = challenge.rsplit_once(':').unwrap();
        assert_eq!(Uuid::parse_str(nonce).unwrap().to_string(), nonce);
        assert_eq!(sequence, expected_sequence.to_string());
        assert!(challenges.insert(challenge.to_owned()));
        match action {
            "accept" => {
                let current = request(&hello);
                send(&mut stream, &current).await;
                let response = receive(&mut stream).await;
                expected_sequence += 1;
                assert_eq!(response["sequence"], expected_sequence);
                assert_eq!(response["kind"], "capabilities");
                old_request = Some(current);
                closed(&mut stream).await;
            }
            "replay" => {
                send(&mut stream, old_request.as_ref().unwrap()).await;
                closed(&mut stream).await;
            }
            "disconnect" => drop(stream),
            _ => unreachable!(),
        }
    }
    observer.stop().await;
}

#[tokio::test]
async fn replayed_challenges_and_previous_incarnations_are_rejected() {
    let directory = Directory::new();
    let conf = configuration(&directory, true);
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    let (mut first, hello) = connect(&directory.socket()).await;
    let old = request(&hello);
    send(&mut first, &old).await;
    receive(&mut first).await;
    let (mut second, next) = connect(&directory.socket()).await;
    assert_ne!(hello["challenge"], next["challenge"]);
    send(&mut second, &old).await;
    closed(&mut second).await;
    observer.stop().await;
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    let (mut stream, next) = connect(&directory.socket()).await;
    assert_ne!(
        hello["identity"]["incarnation"],
        next["identity"]["incarnation"]
    );
    let mut stale = request(&next);
    stale["incarnation"] = hello["identity"]["incarnation"].clone();
    send(&mut stream, &stale).await;
    closed(&mut stream).await;
    observer.stop().await;
}

#[tokio::test]
async fn frame_limits_and_session_deadlines_close_connections() {
    let directory = Directory::new();
    let mut conf = configuration(&directory, true);
    conf.soak_observer.as_mut().unwrap().session_timeout_ms = 100;
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    for size in [0, MAX_FRAME_BYTES as u32 + 1, u32::MAX] {
        let (mut stream, _) = connect(&directory.socket()).await;
        stream.write_u32(size).await.unwrap();
        closed(&mut stream).await;
    }
    let (mut stream, _) = connect(&directory.socket()).await;
    stream.write_u32(100).await.unwrap();
    stream.write_all(b"{").await.unwrap();
    closed(&mut stream).await;
    let (mut idle, _) = connect(&directory.socket()).await;
    closed(&mut idle).await;
    let (mut healthy, hello) = connect(&directory.socket()).await;
    send(&mut healthy, &request(&hello)).await;
    assert_eq!(receive(&mut healthy).await["kind"], "capabilities");
    observer.stop().await;
}

#[tokio::test]
async fn unsafe_directories_links_and_existing_paths_are_rejected() {
    let directory = Directory::new();
    let mut conf = configuration(&directory, true);
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Observer::bind(&conf).is_err());
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700)).unwrap();
    let other = Directory::new();
    let link = other.0.join("link");
    std::os::unix::fs::symlink(&directory.0, &link).unwrap();
    conf.soak_observer.as_mut().unwrap().directory = link;
    assert!(Observer::bind(&conf).is_err());
    conf.soak_observer.as_mut().unwrap().directory = directory.0.clone();
    fs::write(directory.socket(), b"retained").unwrap();
    assert!(Observer::bind(&conf).is_err());
    assert_eq!(fs::read(directory.socket()).unwrap(), b"retained");
}

#[tokio::test]
async fn cleanup_preserves_a_replacement_file() {
    let directory = Directory::new();
    let observer = Observer::bind(&configuration(&directory, true))
        .unwrap()
        .unwrap()
        .spawn();
    assert_eq!(
        fs::metadata(directory.socket()).unwrap().mode() & 0o777,
        0o600
    );
    fs::remove_file(directory.socket()).unwrap();
    fs::write(directory.socket(), b"replacement").unwrap();
    observer.stop().await;
    assert_eq!(fs::read(directory.socket()).unwrap(), b"replacement");
}

#[tokio::test]
async fn peer_start_identity_must_match() {
    let directory = Directory::new();
    let mut conf = configuration(&directory, true);
    conf.soak_observer.as_mut().unwrap().peer_start_ticks += 1;
    assert!(Observer::bind(&conf).is_err());
}

#[test]
fn configuration_limits_and_unknown_fields_are_rejected() {
    let directory = Directory::new();
    let config = configuration(&directory, true).soak_observer.unwrap();
    let base = serde_json::to_value(config).unwrap();
    for (field, invalid) in [
        ("session-timeout-ms", json!(0)),
        ("session-timeout-ms", json!(30_001)),
        ("max-sessions", json!(0)),
        ("max-sessions", json!(4097)),
        ("peer-pid", json!(0)),
        ("peer-pid", json!(u32::MAX)),
        ("peer-start-ticks", json!(0)),
        ("source-revision", json!("A".repeat(40))),
        ("approved-request-sha256", json!("b".repeat(63))),
        ("directory", json!("relative")),
        ("directory", json!("/tmp/../unsafe")),
        ("directory", json!(format!("/{}", "a".repeat(90)))),
        ("fault-control", json!(true)),
    ] {
        let mut value = base.clone();
        value[field] = invalid;
        assert!(
            Options::try_parse_from(["node", "run", "--soak-observer", &value.to_string()])
                .is_err()
        );
    }
    for raw in ["{} {}".to_owned(), " ".repeat(MAX_CONFIG_BYTES + 1)] {
        assert!(Options::try_parse_from(["node", "run", "--soak-observer", &raw]).is_err());
    }
    for (timeout, sessions) in [(50, 1), (30_000, 4096)] {
        let mut value = base.clone();
        value["session-timeout-ms"] = json!(timeout);
        value["max-sessions"] = json!(sessions);
        let config = serde_json::from_value(value).unwrap();
        assert!(validate_config(&config).is_ok());
    }
}

#[test]
fn hocon_configuration_is_validated_and_cli_takes_precedence() {
    let directory = Directory::new();
    let mut value =
        serde_json::to_value(configuration(&directory, true).soak_observer.unwrap()).unwrap();
    value["max-sessions"] = json!(3);
    fs::write(
        directory.0.join("absent.conf"),
        format!("soak-observer = {value}"),
    )
    .unwrap();
    assert_eq!(
        configuration(&directory, false)
            .soak_observer
            .unwrap()
            .max_sessions,
        3
    );
    assert_eq!(
        configuration(&directory, true)
            .soak_observer
            .unwrap()
            .max_sessions,
        128
    );
    value["max-sessions"] = json!(0);
    fs::write(
        directory.0.join("absent.conf"),
        format!("soak-observer = {value}"),
    )
    .unwrap();
    let options = Options::try_parse_from([
        "node",
        "run",
        "--config-file",
        directory.0.join("absent.conf").to_str().unwrap(),
    ])
    .unwrap();
    assert!(builder::build(options).is_err());
}

#[tokio::test]
async fn exact_frame_limit_succeeds_and_malformed_json_fails() {
    let directory = Directory::new();
    let mut conf = configuration(&directory, true);
    conf.soak_observer.as_mut().unwrap().session_timeout_ms = 3000;
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    let (mut stream, hello) = connect(&directory.socket()).await;
    let mut bytes = serde_json::to_vec(&request(&hello)).unwrap();
    bytes.resize(MAX_FRAME_BYTES, b' ');
    stream.write_u32(bytes.len() as u32).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
    assert_eq!(
        receive(&mut stream).await["request_sha256"],
        hex::encode(crypto::rust::hash::sha_256::Sha256Hasher::hash(bytes))
    );
    for malformed in [
        b"[]".to_vec(),
        b"{} {}".to_vec(),
        vec![0xff],
        b"{\"schema_version\":1,\"schema_version\":1}".to_vec(),
    ] {
        let (mut stream, _) = connect(&directory.socket()).await;
        stream.write_u32(malformed.len() as u32).await.unwrap();
        stream.write_all(&malformed).await.unwrap();
        closed(&mut stream).await;
    }
    for operation in ["pause", "terminate", "evaluate", "publication_snapshot"] {
        let (mut stream, hello) = connect(&directory.socket()).await;
        let mut value = request(&hello);
        value["operation"] = json!(operation);
        send(&mut stream, &value).await;
        closed(&mut stream).await;
    }
    let (mut stream, hello) = connect(&directory.socket()).await;
    let mut value = request(&hello);
    value["schema_version"] = json!(2);
    send(&mut stream, &value).await;
    closed(&mut stream).await;
    observer.stop().await;
}

#[tokio::test]
async fn only_one_session_is_active_and_disconnect_releases_it() {
    let directory = Directory::new();
    let mut conf = configuration(&directory, true);
    conf.soak_observer.as_mut().unwrap().session_timeout_ms = 3000;
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    let (first, _) = connect(&directory.socket()).await;
    let mut second = UnixStream::connect(directory.socket()).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), second.read_u8())
            .await
            .is_err()
    );
    drop(first);
    let hello = receive(&mut second).await;
    send(&mut second, &request(&hello)).await;
    assert_eq!(receive(&mut second).await["kind"], "capabilities");
    observer.stop().await;
}

#[tokio::test]
async fn session_budget_stops_only_the_observer() {
    let directory = Directory::new();
    let mut conf = configuration(&directory, true);
    conf.soak_observer.as_mut().unwrap().max_sessions = 1;
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    let (mut stream, hello) = connect(&directory.socket()).await;
    send(&mut stream, &request(&hello)).await;
    receive(&mut stream).await;
    closed(&mut stream).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while directory.socket().exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    observer.stop().await;
    assert!(UnixStream::connect(directory.socket()).await.is_err());
}

#[tokio::test]
async fn dropping_the_owner_cancels_a_pending_session() {
    let directory = Directory::new();
    let observer = Observer::bind(&configuration(&directory, true))
        .unwrap()
        .unwrap()
        .spawn();
    let (mut stream, _) = connect(&directory.socket()).await;
    drop(observer);
    closed(&mut stream).await;
    assert!(!directory.socket().exists());
}

#[tokio::test]
async fn duplicate_observer_does_not_remove_the_active_socket() {
    let directory = Directory::new();
    let conf = configuration(&directory, true);
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    let inode = fs::metadata(directory.socket()).unwrap().ino();
    assert!(Observer::bind(&conf).is_err());
    assert_eq!(fs::metadata(directory.socket()).unwrap().ino(), inode);
    let (mut stream, hello) = connect(&directory.socket()).await;
    send(&mut stream, &request(&hello)).await;
    receive(&mut stream).await;
    observer.stop().await;
}

#[tokio::test]
async fn secret_changes_do_not_change_the_public_configuration_digest() {
    let directory = Directory::new();
    let mut conf = configuration(&directory, true);
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    let (first, hello) = connect(&directory.socket()).await;
    drop(first);
    observer.stop().await;
    conf.casper.validator_private_key = Some("another-synthetic-sentinel".to_owned());
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    let (second, unchanged) = connect(&directory.socket()).await;
    assert_eq!(
        hello["identity"]["configuration_sha256"],
        unchanged["identity"]["configuration_sha256"]
    );
    drop(second);
    observer.stop().await;
    conf.casper.max_parent_depth += 1;
    let observer = Observer::bind(&conf).unwrap().unwrap().spawn();
    let (_, changed) = connect(&directory.socket()).await;
    assert_ne!(
        hello["identity"]["configuration_sha256"],
        changed["identity"]["configuration_sha256"]
    );
    observer.stop().await;
}

#[tokio::test]
async fn a_different_process_cannot_receive_the_handshake() {
    let directory = Directory::new();
    let observer = Observer::bind(&configuration(&directory, true))
        .unwrap()
        .unwrap()
        .spawn();
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "foreign_peer_process",
                "--ignored",
                "--nocapture",
            ])
            .env("SOAK_OBSERVER_TEST_SOCKET", directory.socket())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    observer.stop().await;
}

#[tokio::test]
#[ignore]
async fn foreign_peer_process() {
    let path = std::env::var_os("SOAK_OBSERVER_TEST_SOCKET")
        .expect("The parent must supply its socket path.");
    let mut stream = UnixStream::connect(path).await.unwrap();
    closed(&mut stream).await;
}
