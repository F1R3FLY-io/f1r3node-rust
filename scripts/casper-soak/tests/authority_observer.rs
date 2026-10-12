#[path = "../src/authority_observer.rs"]
mod observer;

use casper_soak::{hash, parse};
use observer::Binding;
use serde_json::{json, Value};

fn binding() -> Binding {
    Binding {
        socket: "/tmp/observer.sock".into(),
        node_pid: 100,
        process_start_ticks: 123,
        source_revision: "a".repeat(40),
        executable_sha256: "b".repeat(64),
        configuration_sha256: "c".repeat(64),
        approved_request_sha256: "d".repeat(64),
        request_id: "01234567-89ab-cdef-0123-456789abcdef".into(),
        timeout_ms: 10_000,
    }
}

fn hello(b: &Binding) -> Value {
    json!({
        "kind":"hello","permission":"read-only","clock":"observer-monotonic",
        "max_frame_bytes":1_048_576,"approved_request_sha256":b.approved_request_sha256,
        "sequence":1,"monotonic_ns":10,
        "challenge":"12345678-1234-1234-1234-123456789abc:1",
        "identity":{
            "schema_version":1,"incarnation":"23456789-1234-1234-1234-123456789abc",
            "pid":b.node_pid,"process_start_ticks":b.process_start_ticks,
            "declared_source_revision":b.source_revision,"executable_sha256":b.executable_sha256,
            "configuration_sha256":b.configuration_sha256,"configuration_scope":"batch-a-public-config-v1"
        }
    })
}

fn response(h: &Value, request: &[u8]) -> Value {
    let r = parse(request).unwrap();
    let work = json!({"traversal":0,"metadata":0,"oracle":0,"clique":0,"signature":0,
        "allocation":0,"operations":0,"allocated_bytes":0,"clique_expansions":0,"maximum_depth":0});
    json!({
        "kind":"authority_snapshot","identity":h["identity"],"request_id":r["request_id"],
        "request_sha256":hash(request),"approved_request_sha256":r["approved_request_sha256"],
        "clock":"observer-monotonic","sequence":2,"monotonic_ns":11,"live_profile_qualified":false,
        "result":{"availability":"available","value":{
            "scope":"batch-b2-detached-authority-evaluation","live_profile_qualified":false,
            "request":r["authority"],"snapshot_digest":"e".repeat(64),"authority_digest":"f".repeat(64),
            "targets":[],"floor_result":{"availability":"not_requested"},
            "floor_comparison":{"availability":"not_requested"},"events":[],"coverage":null,
            "work":{"aggregate":work,"preparation":work,"measured":work,
                "original":{"availability":"not_requested"},"reference":{"availability":"not_requested"},
                "complete":true,"failure":null}
        }}
    })
}

#[test]
fn binding_rejects_invalid_identities_and_limits() {
    let baseline = binding();
    baseline.validate().unwrap();
    let variants: [fn(&mut Binding); 12] = [
        |b| b.socket = "relative.sock".into(),
        |b| b.socket = "/tmp/../observer.sock".into(),
        |b| b.node_pid = 0,
        |b| b.node_pid = u32::MAX,
        |b| b.process_start_ticks = 0,
        |b| b.source_revision = "A".repeat(40),
        |b| b.executable_sha256.clear(),
        |b| b.configuration_sha256 = "z".repeat(64),
        |b| b.approved_request_sha256 = "d".repeat(63),
        |b| b.request_id = "not-a-uuid".into(),
        |b| b.timeout_ms = 49,
        |b| b.timeout_ms = 30_001,
    ];
    for (i, mutate) in variants.into_iter().enumerate() {
        let mut b = baseline.clone();
        mutate(&mut b);
        assert!(b.validate().is_err(), "variant {i}");
    }
}

#[test]
fn greeting_must_match_pinned_identity_and_challenge() {
    let b = binding();
    let original = hello(&b);
    let authority = json!({"targets":[],"reference":true});
    let request = observer::request(&b, &original, &authority).unwrap();
    assert_eq!(request["authority"], authority);
    assert_eq!(request["challenge"], original["challenge"]);
    for (pointer, value) in [
        ("/kind", json!("other")),
        ("/permission", json!("write")),
        ("/clock", json!("wall")),
        ("/max_frame_bytes", json!(2_097_152)),
        ("/approved_request_sha256", json!("e".repeat(64))),
        ("/sequence", json!(0)),
        ("/monotonic_ns", json!(-1)),
        (
            "/challenge",
            json!("12345678-1234-1234-1234-123456789abc:2"),
        ),
        ("/identity/schema_version", json!(2)),
        ("/identity/incarnation", json!("invalid")),
        ("/identity/pid", json!(101)),
        ("/identity/process_start_ticks", json!(124)),
        ("/identity/declared_source_revision", json!("b".repeat(40))),
        ("/identity/executable_sha256", json!("a".repeat(64))),
        ("/identity/configuration_sha256", json!("a".repeat(64))),
        ("/identity/configuration_scope", json!("different")),
    ] {
        let mut h = original.clone();
        *h.pointer_mut(pointer).unwrap() = value;
        assert!(observer::request(&b, &h, &authority).is_err(), "{pointer}");
    }
}

#[test]
fn response_rejects_replay_changed_inputs_and_false_qualification() {
    let h = hello(&binding());
    let r = observer::request(&binding(), &h, &json!({"reference":true})).unwrap();
    let bytes = serde_json::to_vec(&r).unwrap();
    let original = response(&h, &bytes);
    observer::validate_response(&h, &bytes, &original).unwrap();
    for (pointer, value) in [
        ("/kind", json!("capabilities")),
        ("/identity/pid", json!(101)),
        ("/request_id", json!("another")),
        ("/request_sha256", json!("0".repeat(64))),
        ("/approved_request_sha256", json!("0".repeat(64))),
        ("/clock", json!("other")),
        ("/sequence", json!(1)),
        ("/monotonic_ns", json!(9)),
        ("/live_profile_qualified", json!(true)),
        ("/result/availability", json!("passed")),
        ("/result/value/request/reference", json!(false)),
        ("/result/value/scope", json!("other")),
        ("/result/value/live_profile_qualified", json!(true)),
        ("/result/value/snapshot_digest", json!("bad")),
        ("/result/value/authority_digest", json!(null)),
    ] {
        let mut v = original.clone();
        *v.pointer_mut(pointer).unwrap() = value;
        assert!(
            observer::validate_response(&h, &bytes, &v).is_err(),
            "{pointer}"
        );
    }
    let mut unavailable = original;
    unavailable["result"] =
        json!({"availability":"unavailable","reason":"not_attached","input_digest":null});
    observer::validate_response(&h, &bytes, &unavailable).unwrap();
    unavailable["result"]["reason"] = json!("");
    assert!(observer::validate_response(&h, &bytes, &unavailable).is_err());
}

#[test]
fn invalid_input_does_not_create_output() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("capture");
    assert!(observer::collect(b"{}", b"{}", &output).is_err());
    assert!(!output.exists());
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    fn local_binding(socket: &std::path::Path) -> Value {
        let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
        let ticks: u64 = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap()
            .parse()
            .unwrap();
        let exe = std::fs::read("/proc/self/exe").unwrap();
        json!({
            "socket":socket,"node_pid":std::process::id(),"process_start_ticks":ticks,
            "source_revision":"a".repeat(40),"executable_sha256":hash(&exe),
            "configuration_sha256":"c".repeat(64),"approved_request_sha256":"d".repeat(64),
            "request_id":"01234567-89ab-cdef-0123-456789abcdef","timeout_ms":10_000
        })
    }

    fn exercise(case: &str) -> (Value, Vec<String>) {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("observer.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let mut b = local_binding(&socket);
        let h = hello(&Binding::from_value(&b).unwrap());
        match case {
            "peer_pid" => b["node_pid"] = json!(std::process::id() + 1),
            "birth" => b["process_start_ticks"] = json!(1),
            "executable" => b["executable_sha256"] = json!("0".repeat(64)),
            "timeout" | "trickle" => b["timeout_ms"] = json!(500),
            _ => {}
        }
        let timeout_case = matches!(case, "timeout" | "trickle");
        let case = case.to_string();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            if case == "timeout" {
                thread::sleep(Duration::from_millis(800));
                return;
            }
            if case == "oversize" || case == "empty" {
                let size = if case == "empty" { 0u32 } else { 1_048_577 };
                let _ = stream.write_all(&size.to_be_bytes());
                return;
            }
            let mut bytes = serde_json::to_vec(&h).unwrap();
            if case == "duplicate" {
                bytes = b"{\"kind\":\"hello\",\"kind\":\"hello\"}".to_vec();
            }
            if stream
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .is_err()
            {
                return;
            }
            if case == "truncated" {
                let _ = stream.write_all(&bytes[..5]);
                return;
            }
            if case == "trickle" {
                for byte in &bytes {
                    thread::sleep(Duration::from_millis(100));
                    if stream.write_all(&[*byte]).is_err() {
                        return;
                    }
                }
                return;
            }
            for chunk in bytes.chunks(7) {
                if stream.write_all(chunk).is_err() {
                    return;
                }
            }
            let mut prefix = [0; 4];
            if stream.read_exact(&mut prefix).is_err() {
                return;
            }
            let mut request = vec![0; u32::from_be_bytes(prefix) as usize];
            stream.read_exact(&mut request).unwrap();
            let mut value = response(&h, &request);
            if case == "unavailable" {
                value["result"] = json!({"availability":"unavailable","reason":"not_attached","input_digest":null});
            }
            if case == "replay" {
                value["request_sha256"] = json!("0".repeat(64));
            }
            if case == "mapping_defect" {
                value["result"]["value"]["work"]["measured"]["traversal"] = Value::Null;
            }
            let bytes = serde_json::to_vec(&value).unwrap();
            let _ = stream.write_all(&(bytes.len() as u32).to_be_bytes());
            let _ = stream.write_all(&bytes);
        });
        let output = directory.path().join("capture");
        let started = Instant::now();
        let report = observer::collect(
            &serde_json::to_vec(&b).unwrap(),
            b"{\"targets\":[],\"strict\":false,\"reference\":true}",
            &output,
        )
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(10));
        if timeout_case {
            assert!(started.elapsed() < Duration::from_secs(5));
        }
        server.join().unwrap();
        for reference in report["artifacts"].as_array().unwrap() {
            let bytes = std::fs::read(output.join(reference["path"].as_str().unwrap())).unwrap();
            assert_eq!(hash(&bytes), reference["sha256"]);
            assert_eq!(bytes.len() as u64, reference["bytes"]);
        }
        assert_eq!(report["qualification"], "pending");
        assert_eq!(report["profile_verdict"], "pending");
        assert_eq!(report["soak_verdict"], "non_passing");
        assert_eq!(report["node_launch_count"], 0);
        assert!(observer::collect(&serde_json::to_vec(&b).unwrap(), b"{}", &output).is_err());
        let paths = report["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["path"].as_str().unwrap().to_owned())
            .collect();
        (report, paths)
    }

    #[test]
    fn real_socket_capture_retains_correlated_bytes_without_qualification() {
        let (report, paths) = exercise("valid");
        assert_eq!(report["status"], "captured", "{report}");
        assert_eq!(report["availability"], "available");
        assert_eq!(report["mapping_status"], "mapped");
        assert_eq!(paths, [
            "binding.json",
            "authority.json",
            "hello.json",
            "request.json",
            "response.json",
            "mapping.json"
        ]);
        let (unavailable, _) = exercise("unavailable");
        assert_eq!(unavailable["status"], "captured");
        assert_eq!(unavailable["availability"], "unavailable");
        assert_eq!(unavailable["mapping_status"], "mapped");
    }

    #[test]
    fn mapping_failure_preserves_successful_transport_and_raw_evidence() {
        let (report, paths) = exercise("mapping_defect");
        assert_eq!(report["status"], "captured");
        assert_eq!(report["mapping_status"], "rejected");
        assert!(paths.contains(&"response.json".to_string()));
        assert!(paths.contains(&"mapping.json".to_string()));
    }

    #[test]
    fn real_socket_rejects_peer_replacement_and_wrong_executable() {
        for case in ["peer_pid", "birth", "executable"] {
            let (report, paths) = exercise(case);
            assert_eq!(report["status"], "rejected", "{case}: {report}");
            assert_eq!(paths, ["binding.json", "authority.json"]);
        }
    }

    #[test]
    fn real_socket_rejects_frame_defects_and_retains_replay_evidence() {
        for case in ["oversize", "empty", "truncated", "duplicate", "replay"] {
            let (report, paths) = exercise(case);
            assert_eq!(report["status"], "rejected", "{case}: {report}");
            if case == "replay" {
                assert!(paths.contains(&"response.json".to_string()));
            }
            if case == "duplicate" {
                assert!(paths.contains(&"hello.json".to_string()));
            }
        }
    }

    #[test]
    fn real_socket_timeout_is_bounded() {
        for case in ["timeout", "trickle"] {
            let (report, _) = exercise(case);
            assert_eq!(report["status"], "rejected");
            assert!(
                report["error"].as_str().unwrap().contains("deadline"),
                "{report}"
            );
        }
    }
}
