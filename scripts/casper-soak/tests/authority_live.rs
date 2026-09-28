use std::fs;
use std::path::Path;

use casper_soak::{artifact, encoded, hash, parse};
use serde_json::{json, Value};

fn save(root: &Path, name: &str, bytes: &[u8]) -> Value {
    fs::write(root.join(name), bytes).unwrap();
    json!({"path":name,"bytes":bytes.len(),"sha256":hash(bytes)})
}

#[test]
#[ignore]
fn workload_driver_fixture() {
    let request_path = std::env::var("CASPER_AUTHORITY_STEP_REQUEST").unwrap();
    let output_path = std::env::var("CASPER_AUTHORITY_STEP_OUTPUT").unwrap();
    let output = Path::new(&output_path);
    let bytes = fs::read(request_path).unwrap();
    let request = parse(&bytes).unwrap();
    let input = Path::new(request["input_root"].as_str().unwrap());
    let fixture = parse(&artifact(input, &request["inputs"]["fixture"]).unwrap()).unwrap();
    let mode = fixture["mode"].as_str().unwrap();
    if mode == "timeout" {
        save(
            output,
            "pid.json",
            &encoded(&json!({"pid":std::process::id()})).unwrap(),
        );
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
    if mode == "exit" || (mode == "late_exit" && request["step"]["index"] == 2) {
        std::process::exit(17);
    }
    let mut exports = json!({});
    for role in ["dag", "electorate", "justification"] {
        let mut data = artifact(input, &request["inputs"][role]).unwrap();
        if mode == "changed_input" && role == "dag" {
            data.push(b' ');
        }
        exports[role] = save(output, &format!("{role}.json"), &data);
    }
    let mut result = json!({"schema_version":1,"request_sha256":hash(&bytes),"step":request["step"],
        "status":"applied","observed_inputs":exports,"snapshot_digest":"a".repeat(64)});
    match mode {
        "replay" => result["request_sha256"] = json!("0".repeat(64)),
        "reorder" => result["step"]["index"] = json!(999),
        "snapshot" => result["snapshot_digest"] = json!("0".repeat(64)),
        "unknown" => result["status"] = json!("unknown"),
        "missing_export" => {
            result["observed_inputs"]
                .as_object_mut()
                .unwrap()
                .remove("dag");
        }
        _ => {}
    }
    save(output, "result.json", &encoded(&result).unwrap());
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use casper_soak::record;

    use super::*;

    fn now() -> u64 {
        let mut value = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        assert_eq!(
            unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut value) },
            0
        );
        value.tv_sec as u64 * 1_000_000_000 + value.tv_nsec as u64
    }

    fn clock_id() -> String {
        format!(
            "linux-monotonic:{}",
            fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .unwrap()
                .trim()
        )
    }

    fn available(value: Value, digest: char) -> Value {
        json!({"availability":"available","input_digest":digest.to_string().repeat(64),"value":value})
    }

    fn absent(reason: &str, digest: char) -> Value {
        json!({"availability":"unavailable","input_digest":digest.to_string().repeat(64),"reason":reason})
    }

    fn send(stream: &mut UnixStream, value: &Value) -> std::io::Result<()> {
        let bytes = encoded(value).unwrap();
        stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
        stream.write_all(&bytes)
    }

    fn serve(mut stream: UnixStream, binding: &Value, mode: &str) -> std::io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let identity = json!({"schema_version":1,"incarnation":if mode == "incarnation" {"23456789-1234-1234-1234-123456789abc"} else {"12345678-1234-1234-1234-123456789abc"},
            "pid":binding["node_pid"],"process_start_ticks":binding["process_start_ticks"],
            "declared_source_revision":binding["source_revision"],"executable_sha256":binding["executable_sha256"],
            "configuration_sha256":binding["configuration_sha256"],"configuration_scope":"batch-a-public-config-v1"});
        send(
            &mut stream,
            &json!({"kind":"hello","permission":"read-only","clock":"observer-monotonic",
            "max_frame_bytes":1048576,"approved_request_sha256":binding["approved_request_sha256"],
            "sequence":1,"monotonic_ns":10,"challenge":"12345678-1234-1234-1234-123456789abc:1","identity":identity}),
        )?;
        let mut size = [0; 4];
        stream.read_exact(&mut size)?;
        let mut bytes = vec![0; u32::from_be_bytes(size) as usize];
        stream.read_exact(&mut bytes)?;
        let request = parse(&bytes).unwrap();
        let work = json!({"traversal":9,"metadata":8,"oracle":7,"clique":6,"signature":5,
            "allocation":4,"operations":39,"allocated_bytes":128,"clique_expansions":2,"maximum_depth":1});
        let mut response = json!({"kind":"authority_snapshot","identity":identity,"request_id":request["request_id"],
            "request_sha256":hash(&bytes),"approved_request_sha256":request["approved_request_sha256"],
            "sequence":2,"monotonic_ns":11,"clock":"observer-monotonic","live_profile_qualified":false,
            "result":{"availability":"available","value":{
                "scope":"batch-b2-detached-authority-evaluation","live_profile_qualified":false,
                "request":request["authority"],"snapshot_digest":"a".repeat(64),"authority_digest":"b".repeat(64),
                "targets":[{"target":"d".repeat(64),"input_scope":"captured_latest_messages","comparator":"ge",
                    "threshold_numerator":1,"threshold_denominator":3,
                    "oracle_decision":available(json!(true),'c'),
                    "oracle_witness":available(json!({"decision":true,"total_stake":3,"agreeing_stake":3,"clique_weight":2,"early_return":null}),'c'),
                    "original_fault_tolerance":available(json!((1.0f32/3.0).to_bits()),'c'),
                    "display_projection":absent("equivocation_snapshot_unavailable",'c'),
                    "persisted_fault_tolerance":absent("not_finalized",'a'),
                    "reference_comparison":{"availability":"not_requested"}}],
                "floor_result":available(json!({"outcome":"advance","hash":"e".repeat(64),"block_number":7}),'f'),
                "floor_comparison":{"availability":"not_requested"},"events":[],"coverage":null,
                "work":{"aggregate":work,"preparation":work,"measured":work,
                    "original":{"availability":"not_requested"},"reference":{"availability":"not_requested"},"complete":true,"failure":null}}}});
        if mode == "reference_mismatch" {
            response["result"]["value"]["targets"][0]["reference_comparison"] = available(
                json!({
                "algorithm":"immutable-subset-reference-v1","decision_matches":false,"measured_decision":true,
                "original_matches":false,"reference":{"decision":false,"original_bits":0,"total_stake":3,
                    "agreeing_stake":3,"clique_weight":2}}),
                'c',
            );
        }
        send(&mut stream, &response)
    }

    struct Fixture {
        root: tempfile::TempDir,
        envelope: Value,
        stop: Arc<AtomicBool>,
        captures: Arc<AtomicUsize>,
        server: Option<thread::JoinHandle<()>>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            self.server.take().unwrap().join().unwrap();
            if let Ok(base) = std::env::var("SOAK_AUTHORITY_LIVE_EVIDENCE") {
                let destination = Path::new(&base).join(self.root.path().file_name().unwrap());
                for path in casper_soak::walk(self.root.path()).unwrap() {
                    if matches!(
                        path.extension().and_then(|s| s.to_str()),
                        Some("json" | "txt")
                    ) {
                        let retained =
                            destination.join(path.strip_prefix(self.root.path()).unwrap());
                        fs::create_dir_all(retained.parent().unwrap()).unwrap();
                        fs::copy(path, retained).unwrap();
                    }
                }
            }
        }
    }

    impl Fixture {
        fn new(mode: &str) -> Self {
            let root = tempfile::tempdir().unwrap();
            let socket = root.path().join("observer.sock");
            let listener = UnixListener::bind(&socket).unwrap();
            listener.set_nonblocking(true).unwrap();
            let stat = fs::read_to_string("/proc/self/stat").unwrap();
            let ticks: u64 = stat
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .nth(19)
                .unwrap()
                .parse()
                .unwrap();
            let exe = fs::read("/proc/self/exe").unwrap();
            let binding = json!({"socket":socket,"node_pid":std::process::id(),"process_start_ticks":ticks,
                "source_revision":"2".repeat(40),"executable_sha256":hash(&exe),"configuration_sha256":"3".repeat(64),
                "approved_request_sha256":"4".repeat(64),"request_id":"01234567-89ab-cdef-0123-456789abcdef","timeout_ms":2000});
            let mut inputs = json!({});
            for role in ["dag", "electorate", "justification", "fixture"] {
                inputs[role] = save(
                    root.path(),
                    &format!("{role}.json"),
                    &encoded(&json!({"role":role,"mode":mode})).unwrap(),
                );
            }
            let members:Vec<_> = ["bounded", "reference"].into_iter().map(|mode|
                json!({"member_id":mode,"evaluation_mode":mode,"candidate_id":"fixture","node_id":mode,
                    "node_revision":"2".repeat(40),"node_binary_digest":hash(&exe),"incarnation":"12345678-1234-1234-1234-123456789abc",
                    "dag_digest":inputs["dag"]["sha256"],"electorate_digest":inputs["electorate"]["sha256"],"justification_digest":inputs["justification"]["sha256"]})).collect();
            let authority =
                json!({"targets":["d".repeat(64)],"original":true,"reference":true,"strict":false});
            let member_config = json!({"binding":binding,"authority":authority,"target":"d".repeat(64),"configuration_sha256":"3".repeat(64)});
            let manifest = json!({"manifest_digest":"5".repeat(64),"run_id":"fixture-live","phase":"pre_pr216_merge","evidence_kind":"node_observation","policy_variant":"baseline",
                "candidate_id":"fixture","node_revision":"2".repeat(40),"node_binary_digest":hash(&exe),
                "runtime":{"authority_live":{"driver":{"path":std::env::current_exe().unwrap(),"sha256":hash(&exe),"bytes":exe.len(),
                    "arguments":["--ignored","--exact","workload_driver_fixture","--nocapture"]},
                    "members":{"bounded":member_config,"reference":member_config}}}});
            let mut operations = Vec::new();
            for member in &members {
                for operation in ["load_fixture", "evaluate"] {
                    operations.push(
                        json!({"index":operations.len(),"operation":operation,"member":member}),
                    );
                }
            }
            let request = json!({"manifest_digest":"5".repeat(64),"run_id":"fixture-live","phase":"pre_pr216_merge","evidence_kind":"node_observation",
                "policy_variant":"baseline","scenario_kind":"threshold_boundary","scenario_id":"scenario","pair_id":"pair","seed":"1","segment":"1","iteration":"1",
                "protocol_context":{},"metadata_availability":"complete","threshold_inputs":{"q":"2","S":"3","agreeing_stake":"3","n":"1","d":"3"},
                "members":members,"inputs":inputs,"fault_schedule":[],"observation_deadline":{"clock_id":clock_id(),"monotonic_ns":(now()+30_000_000_000).to_string()}});
            let envelope = json!({"schema_version":1,"execution_nonce":"6".repeat(64),"manifest":manifest,"request":request,
                "operations":operations,"input_root":root.path(),"output_root":root.path().join("output"),"timeout_ms":20_000});
            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = stop.clone();
            let captures = Arc::new(AtomicUsize::new(0));
            let thread_captures = captures.clone();
            let server_mode = mode.to_owned();
            let server = thread::spawn(move || {
                while !thread_stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            thread_captures.fetch_add(1, Ordering::SeqCst);
                            let _ = serve(stream, &binding, &server_mode);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(e) => panic!("{e}"),
                    }
                }
            });
            Self {
                root,
                envelope,
                stop,
                captures,
                server: Some(server),
            }
        }

        fn run(&self, code: i32) -> Value {
            let request = self.root.path().join("envelope.json");
            fs::write(&request, encoded(&self.envelope).unwrap()).unwrap();
            let result = Command::new(env!("CARGO_BIN_EXE_casper-authority-live"))
                .arg("--request")
                .arg(request)
                .arg("--output")
                .arg(self.root.path().join("output"))
                .output()
                .unwrap();
            assert_eq!(
                result.status.code(),
                Some(code),
                "{}",
                String::from_utf8_lossy(&result.stdout)
            );
            parse(&result.stdout).unwrap()
        }

        fn receipts(&self) -> Vec<Value> {
            let output = self.root.path().join("output");
            let inventory = record(&output.join("execution.json")).unwrap();
            inventory["receipts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| parse(&artifact(&output, r).unwrap()).unwrap())
                .collect()
        }
    }

    #[test]
    fn live_driver_and_observer_bind_inputs_without_inventing_measurements() {
        let fixture = Fixture::new("valid");
        let report = fixture.run(0);
        assert_eq!(report["qualification"], "pending");
        assert_eq!(report["profile_verdict"], "blocked");
        assert_eq!(fixture.captures.load(Ordering::SeqCst), 8);
        let receipts = fixture.receipts();
        assert_eq!(receipts.len(), 4);
        let output = fixture.root.path().join("output");
        let mut previous = Value::Null;
        let inventory = record(&output.join("execution.json")).unwrap();
        for (i, receipt) in receipts.iter().enumerate() {
            assert_eq!(receipt["status"], "applied");
            assert_eq!(receipt["previous_receipt_sha256"], previous);
            assert_eq!(
                receipt["request_sha256"],
                hash(&encoded(&fixture.envelope).unwrap())
            );
            previous = inventory["receipts"][i]["sha256"].clone();
        }
        let observation =
            parse(&artifact(&output, &receipts[1]["observations"][0]).unwrap()).unwrap();
        assert_eq!(observation["payload"]["head"]["presence"], "missing");
        assert_eq!(observation["payload"]["work"]["presence"], "missing");
        assert_eq!(
            observation["payload"]["finality"]["value"]["decision"],
            "not_finalized"
        );
        assert_eq!(
            observation["payload"]["finality"]["value"]["threshold_pass"],
            true
        );
        assert_eq!(
            observation["payload"]["finality"]["value"]["projection"]["presence"],
            "missing"
        );
        let reference =
            parse(&artifact(&output, &receipts[3]["observations"][0]).unwrap()).unwrap();
        assert_eq!(reference["payload"]["finality"]["presence"], "missing");
    }

    #[test]
    fn live_driver_rejects_replays_changed_inputs_and_snapshot_mismatch() {
        for mode in [
            "replay",
            "reorder",
            "changed_input",
            "missing_export",
            "snapshot",
            "incarnation",
            "exit",
        ] {
            let fixture = Fixture::new(mode);
            let report = fixture.run(1);
            assert_eq!(report["status"], "incomplete", "{mode}");
            assert_eq!(fixture.receipts()[0]["status"], "unknown", "{mode}");
            assert_eq!(report["errors"].as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn missing_driver_and_unapplied_steps_remain_unknown() {
        for absent_driver in [true, false] {
            let mut fixture = Fixture::new("unknown");
            if absent_driver {
                fixture.envelope["manifest"]["runtime"]["authority_live"]["driver"] = Value::Null;
            }
            fixture.run(1);
            let receipts = fixture.receipts();
            assert!(receipts.iter().all(|r| r["status"] == "unknown"));
            let observation = parse(
                &artifact(
                    &fixture.root.path().join("output"),
                    &receipts[1]["observations"][0],
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                observation["payload"]["evaluation_receipt"]["value"]["status"],
                "unknown"
            );
        }
    }

    #[test]
    fn deadlines_preserve_partial_inventory_and_stop_the_driver() {
        let mut fixture = Fixture::new("timeout");
        fixture.envelope["timeout_ms"] = json!(1200);
        let start = Instant::now();
        let report = fixture.run(1);
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(report["status"], "incomplete");
        assert!(fixture.receipts().is_empty());
        let pid = record(
            &fixture
                .root
                .path()
                .join("output/step-00/application/pid.json"),
        )
        .unwrap()["pid"]
            .as_u64()
            .unwrap();
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }

    #[test]
    fn invalid_schedule_scope_clock_and_faults_do_not_launch() {
        for mode in ["schedule", "scope", "clock", "fault"] {
            let mut fixture = Fixture::new("valid");
            match mode {
                "schedule" => fixture.envelope["operations"][0]["operation"] = json!("evaluate"),
                "scope" => {
                    fixture.envelope["request"]["evidence_kind"] = json!("synthetic_fixture")
                }
                "clock" => {
                    fixture.envelope["request"]["observation_deadline"]["clock_id"] =
                        json!("foreign")
                }
                "fault" => {
                    fixture.envelope["request"]["fault_schedule"] = json!([{"action":"pause"}])
                }
                _ => unreachable!(),
            }
            fixture.run(2);
            assert!(!fixture.root.path().join("output").exists());
            assert_eq!(fixture.captures.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn bad_driver_pin_prevents_execution() {
        let mut fixture = Fixture::new("valid");
        fixture.envelope["manifest"]["runtime"]["authority_live"]["driver"]["sha256"] =
            json!("0".repeat(64));
        fixture.run(1);
        assert!(!fixture
            .root
            .path()
            .join("output/step-00/application/driver")
            .exists());
        assert_eq!(fixture.captures.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn reference_observations_preserve_a_real_comparison_disagreement() {
        let fixture = Fixture::new("reference_mismatch");
        fixture.run(0);
        let receipts = fixture.receipts();
        let output = fixture.root.path().join("output");
        let bounded = parse(&artifact(&output, &receipts[1]["observations"][0]).unwrap()).unwrap();
        let reference =
            parse(&artifact(&output, &receipts[3]["observations"][0]).unwrap()).unwrap();
        assert_eq!(
            bounded["payload"]["finality"]["value"]["threshold_pass"],
            true
        );
        assert_eq!(
            reference["payload"]["finality"]["value"]["threshold_pass"],
            false
        );
        assert_eq!(
            reference["payload"]["finality"]["value"]["original_ft"]["value"]["numerator"],
            "0"
        );
    }

    #[test]
    fn driver_failure_retains_prior_evaluations() {
        let fixture = Fixture::new("late_exit");
        fixture.run(1);
        let receipts = fixture.receipts();
        assert_eq!(receipts.len(), 3);
        assert_eq!(receipts[0]["status"], "applied");
        assert_eq!(receipts[1]["observations"].as_array().unwrap().len(), 1);
        assert_eq!(receipts[2]["status"], "unknown");
    }
}
