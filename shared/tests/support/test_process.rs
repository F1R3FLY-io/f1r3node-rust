use super::{
    classify_probe_status, run_child_probe, suppress_expected_probe_core_dump, MonotoneCeiling,
    MonotoneCeilingError, ProbeFailure, ProbeFailureCause, ProbeFailureKind, ProbeOutcome,
};

const CLASSIFIER_MODE: &str = "F1R3_TEST_PROCESS_CLASSIFIER_MODE";

fn monotone_ceiling(
    cap: usize,
    mut probe: impl FnMut(usize) -> ProbeOutcome,
) -> Result<MonotoneCeiling, MonotoneCeilingError<std::convert::Infallible>> {
    super::monotone_ceiling(cap, |depth| Ok(probe(depth)))
}

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
    assert!(matches!(
        error.failure,
        ProbeFailureCause::Outcome(ProbeFailure {
            kind: ProbeFailureKind::Panicked,
            ..
        })
    ));
}

#[test]
fn transport_errors_preserve_identity_and_stop_at_every_search_phase() {
    for (at, expected_trace) in [(1, vec![1]), (17, vec![1, 17]), (9, vec![1, 17, 9])] {
        let error_value = Box::new(at);
        let address = std::ptr::from_ref(error_value.as_ref());
        let mut error_value = Some(error_value);
        let mut trace = Vec::new();
        let error = super::monotone_ceiling(17, |depth| {
            trace.push(depth);
            if depth == at {
                Err(error_value.take().expect("transport error requested once"))
            } else if depth <= 8 {
                Ok(ProbeOutcome::Passed)
            } else {
                Ok(stack_failure())
            }
        })
        .expect_err("a transport error is not a stack ceiling");
        assert_eq!(error.depth, at);
        let ProbeFailureCause::Transport(original) = error.failure else {
            panic!("transport error was normalized into an outcome");
        };
        assert_eq!(std::ptr::from_ref(original.as_ref()), address);
        assert_eq!(trace, expected_trace);
    }
}

#[test]
fn every_nonstack_outcome_stops_without_a_false_boundary() {
    for kind in [
        ProbeFailureKind::TimedOut,
        ProbeFailureKind::MemoryExhausted,
        ProbeFailureKind::Panicked,
        ProbeFailureKind::ExitFailure,
        ProbeFailureKind::Signaled,
    ] {
        for (at, expected_trace) in [(1, vec![1]), (17, vec![1, 17]), (9, vec![1, 17, 9])] {
            let failure = ProbeFailure {
                kind,
                exit_code: Some(37),
                signal: None,
                stderr: format!("sentinel at depth {at}"),
            };
            let mut trace = Vec::new();
            let result = monotone_ceiling(17, |depth| {
                trace.push(depth);
                if depth == at {
                    ProbeOutcome::Failed(failure.clone())
                } else if depth <= 8 {
                    ProbeOutcome::Passed
                } else {
                    stack_failure()
                }
            });
            if at == 1 {
                assert_eq!(result.unwrap(), MonotoneCeiling::BaseFailed(failure));
            } else {
                let error = result.expect_err("nonstack outcome cannot close a bracket");
                assert_eq!(error.depth, at);
                assert_eq!(error.failure, ProbeFailureCause::Outcome(failure));
            }
            assert_eq!(trace, expected_trace);
        }
    }
}

fn exponential_reference(cap: usize, boundary: usize) -> MonotoneCeiling {
    let failure = || match stack_failure() {
        ProbeOutcome::Failed(failure) => failure,
        ProbeOutcome::Passed => unreachable!(),
    };
    if boundary == 0 {
        return MonotoneCeiling::BaseFailed(failure());
    }
    if cap == 1 {
        return MonotoneCeiling::AtLeast(cap);
    }
    let mut lo = 1;
    let mut hi = 2;
    while hi < cap && hi <= boundary {
        lo = hi;
        hi *= 2;
    }
    if hi <= boundary {
        return MonotoneCeiling::AtLeast(cap);
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if mid <= boundary {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    MonotoneCeiling::Exact {
        greatest_survivor: lo,
        first_failure: failure(),
    }
}

#[test]
fn cap_first_matches_frozen_exponential_search_on_power_of_two_caps() {
    for exponent in 0..=10 {
        let cap = 1 << exponent;
        for boundary in 0..=cap {
            let actual = monotone_ceiling(cap, |depth| {
                if depth <= boundary {
                    ProbeOutcome::Passed
                } else {
                    stack_failure()
                }
            })
            .unwrap();
            assert_eq!(
                actual,
                exponential_reference(cap, boundary),
                "cap={cap}, boundary={boundary}"
            );
        }
    }
}

#[test]
fn maximum_machine_caps_preserve_bounds_and_terminate() {
    for cap in [usize::MAX - 2, usize::MAX - 1, usize::MAX] {
        for boundary in [1, cap / 2, cap - 1, cap] {
            let mut trace = Vec::new();
            let result = monotone_ceiling(cap, |depth| {
                assert!((1..=cap).contains(&depth));
                assert!(!trace.contains(&depth), "a probe was repeated");
                trace.push(depth);
                if depth <= boundary {
                    ProbeOutcome::Passed
                } else {
                    stack_failure()
                }
            })
            .unwrap();
            assert!(trace.len() <= usize::BITS as usize + 2);
            match result {
                MonotoneCeiling::AtLeast(actual) => {
                    assert_eq!(actual, cap);
                    assert_eq!(boundary, cap);
                    assert_eq!(trace, [1, cap]);
                }
                MonotoneCeiling::Exact {
                    greatest_survivor, ..
                } => assert_eq!(greatest_survivor, boundary),
                MonotoneCeiling::BaseFailed(_) => panic!("base was successful"),
            }
        }
    }
}

#[test]
fn bisection_trace_preserves_bracket_and_first_failing_witness() {
    for cap in 2..=128 {
        for boundary in 1..cap {
            let mut count = 0;
            let (mut lo, mut hi) = (1, cap);
            let result = monotone_ceiling(cap, |depth| {
                let expected = match count {
                    0 => 1,
                    1 => cap,
                    _ => lo + (hi - lo) / 2,
                };
                assert_eq!(depth, expected);
                if count >= 2 {
                    let previous_width = hi - lo;
                    assert!(lo < depth && depth < hi);
                    if depth <= boundary {
                        lo = depth;
                    } else {
                        hi = depth;
                    }
                    assert!(hi - lo < previous_width);
                    assert!(lo <= boundary && boundary < hi);
                }
                count += 1;
                if depth <= boundary {
                    ProbeOutcome::Passed
                } else {
                    ProbeOutcome::Failed(ProbeFailure {
                        kind: ProbeFailureKind::StackExhausted,
                        exit_code: None,
                        signal: Some(6),
                        stderr: format!("failure at {depth}"),
                    })
                }
            })
            .unwrap();
            let MonotoneCeiling::Exact {
                greatest_survivor,
                first_failure,
            } = result
            else {
                panic!("bracketed boundary must be exact");
            };
            assert_eq!(greatest_survivor, boundary);
            assert_eq!(hi, lo + 1);
            assert_eq!(first_failure.stderr, format!("failure at {hi}"));
        }
    }
}

proptest::proptest! {
    #[test]
    fn arbitrary_machine_caps_match_threshold_spec(
        cap in 1usize..=usize::MAX,
        boundary in proptest::prelude::any::<usize>(),
    ) {
        let mut calls = 0;
        let actual = monotone_ceiling(cap, |depth| {
            assert!((1..=cap).contains(&depth));
            calls += 1;
            if depth <= boundary { ProbeOutcome::Passed } else { stack_failure() }
        }).unwrap();
        proptest::prop_assert!(calls <= usize::BITS as usize + 2);
        match actual {
            MonotoneCeiling::BaseFailed(_) => proptest::prop_assert_eq!(boundary, 0),
            MonotoneCeiling::AtLeast(actual_cap) => {
                proptest::prop_assert_eq!(actual_cap, cap);
                proptest::prop_assert!(cap <= boundary);
            },
            MonotoneCeiling::Exact { greatest_survivor, .. } => {
                proptest::prop_assert_eq!(greatest_survivor, boundary);
                proptest::prop_assert!(boundary < cap);
            },
        }
    }
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
