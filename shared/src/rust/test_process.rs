//! Process controls for deliberately failing test children.
//!
//! Stack-boundary probes distinguish a successful child from one terminated by
//! stack overflow.  On Linux, an expected fatal signal is otherwise piped to
//! `systemd-coredump` even when `RLIMIT_CORE` is zero, turning a short bisection
//! into minutes of collector work and disk traffic.  The parent test remains
//! dumpable; callers invoke this helper only after proving that the process is
//! the specifically requested probe child.

/// Environment escape hatch for a developer who explicitly wants a core from
/// one expected-failure probe child.
pub const ALLOW_EXPECTED_PROBE_CORE: &str = "F1R3_DEPTH_PROBE_ALLOW_CORE";

/// Result of locating the boundary of a monotone child-process probe.
///
/// Stack-ceiling tests use a predicate that is expected to be `true` through
/// some depth and `false` thereafter.  Keeping the three outcomes distinct
/// prevents an anti-vacuity failure at depth one from being misreported as a
/// ceiling, and prevents a successful cap probe from being misreported as an
/// exact maximum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonotoneCeiling {
    /// The probe failed at depth one, so the subject or harness is broken.
    BaseFailed(ProbeFailure),
    /// The subject survived the inclusive search cap; no ceiling was found.
    AtLeast(usize),
    /// The exact greatest surviving depth below a failing cap.
    Exact {
        greatest_survivor: usize,
        first_failure: ProbeFailure,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeFailureKind {
    StackExhausted,
    TimedOut,
    MemoryExhausted,
    Panicked,
    ExitFailure,
    Signaled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeFailure {
    pub kind: ProbeFailureKind,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Passed,
    Failed(ProbeFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonotoneCeilingError {
    pub depth: usize,
    pub failure: ProbeFailure,
}

/// Find the boundary of a monotone depth predicate, probing the cap first.
///
/// The depth-one call is the anti-vacuity witness.  If the inclusive `cap`
/// also survives, those two calls are a complete proof that no boundary was
/// observed in the requested range; walking every intervening power of two
/// adds no evidence under the same monotonicity premise that bisection needs.
/// If the cap fails, ordinary bisection returns the exact greatest survivor.
///
/// This ordering matters for converted stack-safe traversals: their intended
/// result is [`MonotoneCeiling::AtLeast`], and constructing exponentially
/// larger deep fixtures merely to rediscover that fact can dominate an entire
/// workspace test run.
pub fn monotone_ceiling(
    cap: usize,
    mut probe: impl FnMut(usize) -> ProbeOutcome,
) -> Result<MonotoneCeiling, MonotoneCeilingError> {
    assert!(cap >= 1, "a depth-probe cap must include depth one");

    if let ProbeOutcome::Failed(failure) = probe(1) {
        return Ok(MonotoneCeiling::BaseFailed(failure));
    }
    if cap == 1 {
        return Ok(MonotoneCeiling::AtLeast(cap));
    }
    let mut first_failure = match probe(cap) {
        ProbeOutcome::Passed => return Ok(MonotoneCeiling::AtLeast(cap)),
        ProbeOutcome::Failed(failure) if failure.kind == ProbeFailureKind::StackExhausted => {
            failure
        }
        ProbeOutcome::Failed(failure) => {
            return Err(MonotoneCeilingError {
                depth: cap,
                failure,
            });
        }
    };

    let mut lo = 1usize;
    let mut hi = cap;
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        match probe(mid) {
            ProbeOutcome::Passed => lo = mid,
            ProbeOutcome::Failed(failure) if failure.kind == ProbeFailureKind::StackExhausted => {
                hi = mid;
                first_failure = failure;
            }
            ProbeOutcome::Failed(failure) => {
                return Err(MonotoneCeilingError {
                    depth: mid,
                    failure,
                });
            }
        }
    }
    Ok(MonotoneCeiling::Exact {
        greatest_survivor: lo,
        first_failure,
    })
}

pub fn run_child_probe(
    command: &mut std::process::Command,
    timeout: std::time::Duration,
) -> std::io::Result<ProbeOutcome> {
    use std::io::Read as _;

    const STDERR_LIMIT: usize = 64 * 1024;
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    let mut stderr = child
        .stderr
        .take()
        .expect("run_child_probe requested a piped stderr");
    let stderr_reader = std::thread::spawn(move || {
        let mut retained = Vec::with_capacity(STDERR_LIMIT);
        let mut chunk = [0u8; 4096];
        loop {
            let read = match stderr.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            if read >= STDERR_LIMIT {
                retained.clear();
                retained.extend_from_slice(&chunk[read - STDERR_LIMIT..read]);
            } else {
                let overflow = retained
                    .len()
                    .saturating_add(read)
                    .saturating_sub(STDERR_LIMIT);
                if overflow != 0 {
                    retained.drain(..overflow);
                }
                retained.extend_from_slice(&chunk[..read]);
            }
        }
        retained
    });

    let deadline = std::time::Instant::now() + timeout;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if std::time::Instant::now() >= deadline {
            child.kill()?;
            break (child.wait()?, true);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    let stderr = String::from_utf8_lossy(
        &stderr_reader
            .join()
            .expect("run_child_probe stderr reader panicked"),
    )
    .into_owned();

    if status.success() && !timed_out {
        return Ok(ProbeOutcome::Passed);
    }

    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt as _;
        status.signal()
    };
    #[cfg(not(unix))]
    let signal = None;

    let lower = stderr.to_ascii_lowercase();
    let kind = if timed_out {
        ProbeFailureKind::TimedOut
    } else if lower.contains("fatal runtime error: stack overflow")
        || lower.contains("has overflowed its stack")
    {
        ProbeFailureKind::StackExhausted
    } else if lower.contains("memory allocation of")
        || lower.contains("out of memory")
        || lower.contains("cannot allocate memory")
    {
        ProbeFailureKind::MemoryExhausted
    } else if status.code() == Some(101) || lower.contains("panicked at") {
        ProbeFailureKind::Panicked
    } else if signal.is_some() {
        ProbeFailureKind::Signaled
    } else {
        ProbeFailureKind::ExitFailure
    };

    Ok(ProbeOutcome::Failed(ProbeFailure {
        kind,
        exit_code: status.code(),
        signal,
        stderr,
    }))
}

/// Best-effort suppression of core collection for an expected-failure child.
///
/// Returns `true` only when suppression was installed.  Failure is deliberately
/// non-fatal: a setup error must not masquerade as the stack-limit failure the
/// parent is measuring.  The child still terminates with the same signal and
/// [`std::process::ExitStatus`], so the probe's anti-vacuity contract is
/// unchanged.
pub fn suppress_expected_probe_core_dump() -> bool {
    if std::env::var(ALLOW_EXPECTED_PROBE_CORE).is_ok_and(|value| value == "1") {
        return false;
    }

    #[cfg(target_os = "linux")]
    {
        // SAFETY: `prctl(PR_SET_DUMPABLE, 0)` takes no pointer arguments.  It
        // changes only the calling process's dumpable flag and is documented
        // to preserve signal delivery and exit status.
        unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) == 0 }
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
        // SAFETY: `limit` is valid writable storage for `getrlimit`; on
        // success it is fully initialized before it is read by `setrlimit`.
        if unsafe { libc::getrlimit(libc::RLIMIT_CORE, limit.as_mut_ptr()) } != 0 {
            false
        } else {
            // SAFETY: established by the successful `getrlimit` above.
            let mut limit = unsafe { limit.assume_init() };
            limit.rlim_cur = 0;
            // SAFETY: `limit` is a fully initialized value for the current process.
            unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) == 0 }
        }
    }

    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{
        monotone_ceiling, run_child_probe, suppress_expected_probe_core_dump, MonotoneCeiling,
        ProbeFailure, ProbeFailureKind, ProbeOutcome,
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
        let mode = std::env::var(CLASSIFIER_MODE).expect("classifier child mode");
        match mode.as_str() {
            "pass" => {}
            "panic" => panic!("synthetic classified child panic"),
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
                use std::io::Write as _;
                let mut stderr = std::io::stderr().lock();
                stderr
                    .write_all(b"memory allocation of 18446744073709551615 bytes failed\n")
                    .expect("write allocation-failure signature");
                stderr.flush().expect("flush allocation-failure signature");
                std::process::abort();
            }
            "signal" => std::process::abort(),
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
            ("signal", ProbeFailureKind::Signaled),
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
}
