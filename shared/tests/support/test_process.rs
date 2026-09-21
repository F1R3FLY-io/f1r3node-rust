use super::{
    classify_probe_status, monotone_ceiling, run_child_probe, suppress_expected_probe_core_dump,
    MonotoneCeiling, ProbeFailure, ProbeFailureKind, ProbeOutcome,
};

const CLASSIFIER_MODE: &str = "F1R3_TEST_PROCESS_CLASSIFIER_MODE";

fn stack_failure() -> ProbeOutcome {
    ProbeOutcome::Failed(ProbeFailure {
        kind: ProbeFailureKind::StackExhausted,
        exit_code: None,
        signal: Some(6),
        stderr: "fatal runtime error: stack overflow".to_string(),
    })
}

fn panic_failure() -> ProbeOutcome {
    ProbeOutcome::Failed(ProbeFailure {
        kind: ProbeFailureKind::Panicked,
        exit_code: Some(101),
        signal: None,
        stderr: "panicked at synthetic probe".to_string(),
    })
}

#[inline(never)]
fn grow_native_stack(depth: usize, next: fn(usize)) {
    let frame = [depth as u8; 4096];
    std::hint::black_box(&frame);
    next(depth.wrapping_add(1));
    std::hint::black_box(frame[0]);
}

#[inline(never)]
fn overflow_native_stack(depth: usize) { grow_native_stack(depth, overflow_native_stack) }

#[test]
#[ignore]
fn classified_probe_child() {
    let Ok(mode) = std::env::var(CLASSIFIER_MODE) else {
        return;
    };
    match mode.as_str() {
        "pass" => {}
        "panic" => panic!("synthetic classified child panic"),
        "panic-stack-text" => {
            panic!("fatal runtime error: stack overflow; has overflowed its stack")
        }
        "stack" => {
            let _ = suppress_expected_probe_core_dump();
            std::thread::Builder::new()
                .stack_size(64 * 1024)
                .spawn(|| overflow_native_stack(0))
                .expect("spawn stack-exhaustion child")
                .join()
                .expect("stack-exhaustion thread unexpectedly returned");
        }
        "timeout" => std::thread::sleep(std::time::Duration::from_secs(10)),
        "memory" => {
            let _ = suppress_expected_probe_core_dump();
            use std::io::Write as _;
            let mut stderr = std::io::stderr().lock();
            stderr
                .write_all(b"memory allocation of 18446744073709551615 bytes failed\n")
                .expect("write allocation-failure signature");
            stderr.flush().expect("flush allocation-failure signature");
            std::process::abort();
        }
        "signal" => {
            let _ = suppress_expected_probe_core_dump();
            std::process::abort();
        }
        other => panic!("unknown classifier child mode {other:?}"),
    }
}

fn process_outcome(mode: &str, timeout: std::time::Duration) -> ProbeOutcome {
    let executable = std::env::current_exe().expect("shared unit-test executable");
    let mut command = std::process::Command::new(executable);
    command
        .args([
            "--ignored",
            "--exact",
            "rust::test_process::tests::classified_probe_child",
        ])
        .env(CLASSIFIER_MODE, mode);
    run_child_probe(&mut command, timeout).expect("run classifier child")
}

#[test]
fn cap_success_needs_only_the_anti_vacuity_and_cap_probes() {
    let mut probes = Vec::new();
    let result = monotone_ceiling(262_144, |depth| {
        probes.push(depth);
        ProbeOutcome::Passed
    })
    .expect("a passing cap is a valid monotone probe");

    assert_eq!(result, MonotoneCeiling::AtLeast(262_144));
    assert_eq!(probes, [1, 262_144]);
}

#[test]
fn a_failing_cap_is_bisected_to_the_exact_boundary() {
    for greatest_survivor in [1usize, 2, 347, 1023] {
        let result = monotone_ceiling(1024, |depth| {
            if depth <= greatest_survivor {
                ProbeOutcome::Passed
            } else {
                stack_failure()
            }
        })
        .expect("stack exhaustion is the expected boundary");
        assert!(matches!(
            result,
            MonotoneCeiling::Exact {
                greatest_survivor: actual,
                ..
            } if actual == greatest_survivor
        ));
    }
}

#[test]
fn cap_first_search_matches_the_monotone_ceiling_spec_exhaustively() {
    for cap in 1usize..=256 {
        // Zero models a broken depth-one probe; `cap` models survival of
        // the whole inclusive range.  Every interior boundary is exact.
        for greatest_survivor in 0usize..=cap {
            let actual = monotone_ceiling(cap, |depth| {
                if depth <= greatest_survivor {
                    ProbeOutcome::Passed
                } else {
                    stack_failure()
                }
            })
            .expect("stack exhaustion is the expected boundary");
            let expected = match greatest_survivor {
                0 => MonotoneCeiling::BaseFailed(match stack_failure() {
                    ProbeOutcome::Failed(failure) => failure,
                    ProbeOutcome::Passed => unreachable!(),
                }),
                boundary if boundary == cap => MonotoneCeiling::AtLeast(cap),
                boundary => MonotoneCeiling::Exact {
                    greatest_survivor: boundary,
                    first_failure: match stack_failure() {
                        ProbeOutcome::Failed(failure) => failure,
                        ProbeOutcome::Passed => unreachable!(),
                    },
                },
            };
            assert_eq!(actual, expected, "cap={cap}, boundary={greatest_survivor}");
        }
    }
}

#[test]
fn a_broken_base_probe_is_not_called_a_ceiling() {
    let mut probes = Vec::new();
    let result = monotone_ceiling(1024, |depth| {
        probes.push(depth);
        stack_failure()
    })
    .expect("a failing base has its own typed result");

    assert!(matches!(result, MonotoneCeiling::BaseFailed(_)));
    assert_eq!(probes, [1]);
}

#[test]
fn a_unit_cap_is_both_the_base_and_cap_witness() {
    let mut probes = Vec::new();
    let result = monotone_ceiling(1, |depth| {
        probes.push(depth);
        ProbeOutcome::Passed
    })
    .expect("a passing unit cap is a valid monotone probe");

    assert_eq!(result, MonotoneCeiling::AtLeast(1));
    assert_eq!(probes, [1]);
}

#[test]
fn a_non_stack_failure_cannot_be_misreported_as_a_ceiling() {
    let error = monotone_ceiling(1024, |depth| {
        if depth < 700 {
            ProbeOutcome::Passed
        } else {
            panic_failure()
        }
    })
    .expect_err("a panic is not evidence of a native-stack boundary");

    assert_eq!(error.depth, 1024);
    assert_eq!(error.failure.kind, ProbeFailureKind::Panicked);
}

#[test]
fn child_process_outcomes_are_classified_distinctly() {
    let ordinary = std::time::Duration::from_secs(10);
    assert_eq!(process_outcome("pass", ordinary), ProbeOutcome::Passed);

    for (mode, expected) in [
        ("stack", ProbeFailureKind::StackExhausted),
        ("panic", ProbeFailureKind::Panicked),
        ("memory", ProbeFailureKind::MemoryExhausted),
        (
            "signal",
            if cfg!(unix) {
                ProbeFailureKind::Signaled
            } else {
                ProbeFailureKind::ExitFailure
            },
        ),
    ] {
        let ProbeOutcome::Failed(failure) = process_outcome(mode, ordinary) else {
            panic!("{mode} classifier child unexpectedly passed");
        };
        assert_eq!(failure.kind, expected, "classifier mode {mode}");
    }

    let ProbeOutcome::Failed(timeout) =
        process_outcome("timeout", std::time::Duration::from_millis(25))
    else {
        panic!("timeout classifier child unexpectedly passed");
    };
    assert_eq!(timeout.kind, ProbeFailureKind::TimedOut);
}

#[test]
fn panic_with_stack_text_is_not_stack_exhaustion() {
    let ProbeOutcome::Failed(failure) =
        process_outcome("panic-stack-text", std::time::Duration::from_secs(10))
    else {
        panic!("deceptive panic unexpectedly passed");
    };
    assert_eq!(failure.kind, ProbeFailureKind::Panicked);
}

#[cfg(unix)]
#[test]
fn classification_matches_termination_evidence_exhaustively() {
    use std::os::unix::process::ExitStatusExt as _;
    for raw in [
        0,
        1 << 8,
        101 << 8,
        libc::SIGABRT,
        libc::SIGSEGV,
        libc::SIGKILL,
    ] {
        for flags in 0u8..16 {
            let status = std::process::ExitStatus::from_raw(raw);
            let timeout = flags & 1 != 0;
            let panic = flags & 2 != 0;
            let stack = flags & 4 != 0;
            let memory = flags & 8 != 0;
            let mut diagnostic = String::new();
            if panic {
                diagnostic.push_str("thread 'probe' panicked at test\n");
            }
            if stack {
                diagnostic.push_str("fatal runtime error: stack overflow\n");
            }
            if memory {
                diagnostic.push_str("memory allocation of 123 bytes failed\n");
            }
            let expected = match (timeout, raw, panic, stack, memory) {
                (true, _, _, _, _) => Some(ProbeFailureKind::TimedOut),
                (false, 0, _, _, _) => None,
                (false, _, true, _, _) => Some(ProbeFailureKind::Panicked),
                (false, code, _, _, _) if code == 101 << 8 => Some(ProbeFailureKind::Panicked),
                (false, libc::SIGABRT, _, true, _) => Some(ProbeFailureKind::StackExhausted),
                (false, libc::SIGABRT, _, _, true) => Some(ProbeFailureKind::MemoryExhausted),
                (false, signal, _, _, _) if signal < 128 => Some(ProbeFailureKind::Signaled),
                _ => Some(ProbeFailureKind::ExitFailure),
            };
            let actual = classify_probe_status(status, timeout, diagnostic.clone());
            match (actual, expected) {
                (ProbeOutcome::Passed, None) => {}
                (ProbeOutcome::Failed(failure), Some(kind)) => {
                    assert_eq!(failure.kind, kind, "raw={raw}, flags={flags}");
                    assert_eq!(failure.exit_code, status.code());
                    assert_eq!(failure.signal, status.signal());
                    assert_eq!(failure.stderr, diagnostic);
                }
                (actual, expected) => {
                    panic!("raw={raw}, flags={flags}: {actual:?} != {expected:?}")
                }
            }
        }
    }
}

#[cfg(windows)]
#[test]
fn windows_exception_status_requires_compatible_diagnostics() {
    use std::os::windows::process::ExitStatusExt as _;
    for (code, diagnostic, expected) in [
        (0xC000_00FD, "", ProbeFailureKind::StackExhausted),
        (
            0xC000_0409,
            "fatal runtime error: stack overflow",
            ProbeFailureKind::StackExhausted,
        ),
        (
            0xC000_0409,
            "memory allocation of 123 bytes failed",
            ProbeFailureKind::MemoryExhausted,
        ),
        (
            0xC000_0409,
            "panicked at: has overflowed its stack",
            ProbeFailureKind::Panicked,
        ),
        (0xC000_0409, "", ProbeFailureKind::ExitFailure),
        (
            0xC000_0005,
            "has overflowed its stack",
            ProbeFailureKind::ExitFailure,
        ),
        (1, "has overflowed its stack", ProbeFailureKind::ExitFailure),
        (101, "has overflowed its stack", ProbeFailureKind::Panicked),
    ] {
        let ProbeOutcome::Failed(failure) = classify_probe_status(
            std::process::ExitStatus::from_raw(code),
            false,
            diagnostic.to_owned(),
        ) else {
            panic!("failed status unexpectedly passed");
        };
        assert_eq!(
            failure.kind, expected,
            "code={code:x}, diagnostic={diagnostic}"
        );
    }
}
