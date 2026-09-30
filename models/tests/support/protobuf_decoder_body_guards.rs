use super::*;

#[test]
fn missing_public_guard_is_detected_before_canonical_fast_import() { assert_guard_refusal("fast"); }

#[test]
fn missing_public_guard_is_detected_before_machine_execution() { assert_guard_refusal("machine"); }

fn assert_guard_refusal(case: &str) {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "rust::rholang::protobuf_decoder::oracle_body_guards::missing_public_guard_child",
            "--nocapture",
        ])
        .env("MODEL_DECODER_GUARD_CASE", case)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run the decoder guard child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(
                failure.kind,
                shared::rust::test_process::ProbeFailureKind::Panicked
            );
            assert!(
                failure
                    .stderr
                    .contains("successor body bypassed decoder routing"),
                "{}",
                failure.stderr
            );
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("unguarded decoder entry passed its child probe")
        }
    }
}

#[test]
#[ignore = "isolated decoder guard child"]
fn missing_public_guard_child() {
    let Ok(case) = std::env::var("MODEL_DECODER_GUARD_CASE") else {
        return;
    };
    match case.as_str() {
        "fast" => {
            let _ = crate::protobuf_decoder_oracle::legacy(|| {
                decode_par_with_epathmap_validation(&[][..], true)
            });
        }
        "machine" => {
            let _ = crate::protobuf_decoder_oracle::legacy(|| {
                Machine::new(
                    &[][..],
                    Node::Par {
                        value: Par::default(),
                        limit: 0,
                    },
                    true,
                )
                .run()
            });
        }
        _ => panic!("unknown decoder guard case"),
    }
}
