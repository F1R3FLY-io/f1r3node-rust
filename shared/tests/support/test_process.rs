use super::{
    classify_probe_status, prepare_expected_probe_core_dump, run_child_probe, MonotoneCeiling,
    MonotoneCeilingError, ProbeFailure, ProbeFailureCause, ProbeFailureKind, ProbeOutcome,
};

const CLASSIFIER_MODE: &str = "F1R3_TEST_PROCESS_CLASSIFIER_MODE";

#[test]
fn core_policy_opt_in_skips_installation() {
    assert_eq!(
        super::probe_core_policy_with(true, || panic!("installer called during opt-in")).unwrap(),
        super::ExpectedProbeCorePolicy::ExplicitlyAllowed
    );
}

#[test]
fn core_policy_preserves_installed_unsupported_and_original_errors() {
    use super::{CoreSuppression, ExpectedProbeCorePolicy};
    for (installed, expected) in [
        (
            CoreSuppression::Installed,
            ExpectedProbeCorePolicy::SuppressionInstalled,
        ),
        (
            CoreSuppression::Unsupported,
            ExpectedProbeCorePolicy::Unsupported,
        ),
    ] {
        let mut calls = 0;
        let actual = super::probe_core_policy_with(false, || {
            calls += 1;
            Ok(installed)
        })
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(calls, 1);
    }
    let error = std::io::Error::other("retained policy error");
    let identity = error.get_ref().unwrap() as *const _;
    let actual = super::probe_core_policy_with(false, || Err(error)).unwrap_err();
    assert!(std::ptr::eq(
        actual.get_ref().unwrap() as *const _,
        identity
    ));
    let actual =
        super::probe_core_policy_with(false, || Err(std::io::Error::from_raw_os_error(13)))
            .unwrap_err();
    assert_eq!(actual.raw_os_error(), Some(13));
}

proptest::proptest! {
    #[test]
    fn core_policy_requires_exact_opt_in(value in proptest::option::of(".{0,30}")) {
        let actual = super::probe_core_opt_in(value.as_deref().map(std::ffi::OsStr::new));
        proptest::prop_assert_eq!(actual, value.as_deref() == Some("1"));
    }
}

#[test]
fn failed_core_policy_exits_before_probe_body() {
    let ProbeOutcome::Failed(failure) =
        process_outcome("core-policy-error", std::time::Duration::from_secs(10))
    else {
        panic!("failed core policy ran the body");
    };
    assert_eq!(failure.kind, ProbeFailureKind::ExitFailure);
    assert_eq!(failure.exit_code, Some(2));
    assert!(failure.stderr.contains("depth-probe core policy failed"));
    assert!(!failure.stderr.contains("probe body reached"));
}

#[test]
fn unsupported_core_policy_reports_without_claiming_suppression() {
    let ProbeOutcome::Failed(failure) = process_outcome(
        "core-policy-unsupported",
        std::time::Duration::from_secs(10),
    ) else {
        panic!("unsupported-policy diagnostic fixture did not finish");
    };
    assert_eq!(failure.kind, ProbeFailureKind::ExitFailure);
    assert_eq!(failure.exit_code, Some(7));
    assert!(failure.stderr.contains("core suppression is unsupported"));
}

#[test]
fn classifier_child_without_selector_is_a_noop() {
    let mut command = classifier_command("unused");
    command.env_remove(CLASSIFIER_MODE);
    assert_eq!(
        run_child_probe(&mut command, std::time::Duration::from_secs(10)).unwrap(),
        ProbeOutcome::Passed
    );
}

#[cfg(target_os = "linux")]
#[test]
fn core_policy_is_child_scoped_and_explicit_opt_in_preserves_dumpability() {
    let before = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
    assert!(before >= 0);
    for allow in [None, Some("0"), Some("1"), Some("true"), Some(" 1")] {
        let mut command = classifier_command("core-policy-inspect");
        if let Some(value) = allow {
            command.env(super::ALLOW_EXPECTED_PROBE_CORE, value);
        }
        assert_eq!(
            run_child_probe(&mut command, std::time::Duration::from_secs(10)).unwrap(),
            ProbeOutcome::Passed,
            "allow={allow:?}"
        );
        assert_eq!(
            unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) },
            before
        );
    }
}

#[test]
fn automated_classifier_children_remove_inherited_core_opt_in() {
    for mode in ["stack", "memory", "signal"] {
        let command = classifier_command(mode);
        assert!(command
            .get_envs()
            .any(|(key, value)| key == super::ALLOW_EXPECTED_PROBE_CORE && value.is_none()));
    }
}

#[test]
fn stack_survival_rejects_all_nonstack_failures_without_losing_evidence() {
    assert_eq!(
        super::stack_survived::<Box<u64>>(Ok(ProbeOutcome::Passed)),
        Ok(true)
    );
    for kind in [
        ProbeFailureKind::StackExhausted,
        ProbeFailureKind::TimedOut,
        ProbeFailureKind::MemoryExhausted,
        ProbeFailureKind::Panicked,
        ProbeFailureKind::ExitFailure,
        ProbeFailureKind::Signaled,
    ] {
        let failure = ProbeFailure {
            kind,
            exit_code: Some(17),
            signal: Some(6),
            stderr: "original diagnostic allocation".to_owned(),
        };
        let text_identity = failure.stderr.as_ptr();
        let expected = failure.clone();
        let result = super::stack_survived::<Box<u64>>(Ok(ProbeOutcome::Failed(failure)));
        if kind == ProbeFailureKind::StackExhausted {
            assert_eq!(result, Ok(false));
        } else {
            let Err(ProbeFailureCause::Outcome(actual)) = result else {
                panic!("non-stack failure became a measurement: {kind:?}");
            };
            assert_eq!(actual, expected);
            assert_eq!(actual.stderr.as_ptr(), text_identity);
        }
    }
    let error = Box::new(5917u64);
    let identity = error.as_ref() as *const u64;
    let Err(ProbeFailureCause::Transport(actual)) = super::stack_survived(Err(error)) else {
        panic!("transport error became a measurement");
    };
    assert_eq!(actual.as_ref() as *const u64, identity);
}

#[test]
fn watchdog_expiry_cannot_satisfy_an_expected_stack_failure() {
    let observation = process_outcome("timeout", std::time::Duration::from_millis(25));
    let Err(ProbeFailureCause::Outcome(failure)) = super::stack_survived::<()>(Ok(observation))
    else {
        panic!("watchdog expiry became a stack measurement");
    };
    assert_eq!(failure.kind, ProbeFailureKind::TimedOut);
}

#[test]
fn lifecycle_decisions_match_complete_capture_and_cleanup_contract() {
    use std::time::Duration as D;

    use super::{ProbeDecision, ProbeLifecycle};
    for flags in 0u8..16 {
        for cleanup in [None, Some(D::from_millis(5))] {
            for now in [0, 5, 14, 15, 16] {
                let state = ProbeLifecycle {
                    reaped: flags & 1 != 0,
                    eof: flags & 2 != 0,
                    failed: flags & 4 != 0,
                    timed_out: flags & 8 != 0,
                    cleanup_started: cleanup,
                };
                let decision = state.decide(D::from_millis(10), D::from_millis(now));
                let expected = if state.reaped && state.failed {
                    ProbeDecision::HarnessError
                } else if state.reaped && state.eof {
                    ProbeDecision::Classify(state.timed_out)
                } else if cleanup.is_some() && now >= 15 {
                    ProbeDecision::HarnessError
                } else {
                    ProbeDecision::Poll
                };
                assert_eq!(
                    decision, expected,
                    "flags={flags}, cleanup={cleanup:?}, now={now}"
                );
                assert_eq!(state.can_read(), !state.eof && !state.failed);
            }
        }
    }
}

#[test]
fn lifecycle_observations_latch_evidence_without_renewing_cleanup() {
    use std::time::Duration as D;

    use super::ProbeLifecycle;
    for flags in 0u8..16 {
        for observations in 0u8..8 {
            for cleanup in [None, Some(D::from_millis(2))] {
                for now in [4, 5, 6, 100] {
                    let before = ProbeLifecycle {
                        reaped: flags & 1 != 0,
                        eof: flags & 2 != 0,
                        failed: flags & 4 != 0,
                        timed_out: flags & 8 != 0,
                        cleanup_started: cleanup,
                    };
                    let mut after = before;
                    after.observe(
                        D::from_millis(5),
                        D::from_millis(now),
                        observations & 1 != 0,
                        observations & 2 != 0,
                        observations & 4 != 0,
                    );
                    assert_eq!(after.reaped, before.reaped || observations & 1 != 0);
                    assert_eq!(after.eof, before.eof || observations & 2 != 0);
                    assert_eq!(after.failed, before.failed || observations & 4 != 0);
                    assert_eq!(
                        after.timed_out,
                        before.timed_out || (!after.reaped && cleanup.is_none() && now >= 5)
                    );
                    let expected_cleanup = cleanup.or_else(|| {
                        (after.reaped || after.failed || after.timed_out)
                            .then_some(D::from_millis(now))
                    });
                    assert_eq!(after.cleanup_started, expected_cleanup);
                }
            }
        }
    }
}

#[test]
fn diagnostic_tail_is_exact_across_chunk_boundaries() {
    for length in [0usize, 1, 65535, 65536, 65537, 262177] {
        let source: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
        for chunk_size in [1, 31, 4096, 65536, 70000] {
            let mut tail = std::collections::VecDeque::new();
            for chunk in source.chunks(chunk_size) {
                super::retain_probe_stderr(&mut tail, chunk);
                assert!(tail.len() <= super::PROBE_STDERR_LIMIT);
            }
            let retained = tail.clone();
            super::retain_probe_stderr(&mut tail, &[]);
            assert_eq!(tail, retained);
            let actual: Vec<_> = tail.into_iter().collect();
            assert_eq!(actual, source[length.saturating_sub(65536)..]);
        }
    }
}

proptest::proptest! {
    #[test]
    fn diagnostic_tail_matches_suffix_spec(
        source in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..150_000),
        chunks in proptest::collection::vec(1usize..80_000, 1..12),
    ) {
        let mut tail = std::collections::VecDeque::new();
        let mut position = 0;
        for chunk in chunks.iter().cycle() {
            let end = (position + chunk).min(source.len());
            super::retain_probe_stderr(&mut tail, &source[position..end]);
            position = end;
            if position == source.len() { break; }
        }
        let actual: Vec<_> = tail.into_iter().collect();
        proptest::prop_assert_eq!(actual, &source[source.len().saturating_sub(65536)..]);
    }
}

#[test]
fn interrupted_reads_consume_the_quantum_and_pending_stops_it() {
    struct Interrupted(usize);
    impl std::io::Read for Interrupted {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            self.0 += 1;
            Err(std::io::ErrorKind::Interrupted.into())
        }
    }
    let mut reader = Interrupted(0);
    let mut tail = std::collections::VecDeque::new();
    let progress = super::read_probe_quantum(&mut reader, &mut tail).unwrap();
    assert_eq!(reader.0, super::PROBE_READ_ATTEMPTS);
    assert!(!progress.eof);
    assert!(!progress.made_progress);

    assert!(tail.is_empty());
    struct Continuous(usize);
    impl std::io::Read for Continuous {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.0 += 1;
            assert_eq!(buffer.len(), 4096);
            buffer.fill(b'x');
            Ok(buffer.len())
        }
    }
    let mut reader = Continuous(0);
    let progress = super::read_probe_quantum(&mut reader, &mut tail).unwrap();
    assert_eq!(reader.0, super::PROBE_READ_ATTEMPTS);
    assert!(progress.made_progress);
    assert!(!progress.eof);
    assert_eq!(tail.len(), super::PROBE_STDERR_LIMIT);

    struct Pending(usize);
    impl std::io::Read for Pending {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            self.0 += 1;
            Err(std::io::ErrorKind::WouldBlock.into())
        }
    }
    let mut reader = Pending(0);
    let progress = super::read_probe_quantum(&mut reader, &mut tail).unwrap();
    assert_eq!(reader.0, 1);
    assert!(!progress.eof);
    assert!(!progress.made_progress);
}

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
        "core-policy-unsupported" => {
            assert_eq!(
                super::require_probe_core_policy(Ok(super::ExpectedProbeCorePolicy::Unsupported)),
                super::ExpectedProbeCorePolicy::Unsupported
            );
            std::process::exit(7);
        }
        "core-policy-error" => {
            super::require_probe_core_policy(Err(std::io::Error::from_raw_os_error(13)));
            panic!("probe body reached");
        }
        #[cfg(target_os = "linux")]
        "core-policy-inspect" => {
            let before = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
            assert!(before >= 0);
            let policy = super::prepare_expected_probe_core_dump();
            let after = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
            if std::env::var(super::ALLOW_EXPECTED_PROBE_CORE).as_deref() == Ok("1") {
                assert_eq!(policy, super::ExpectedProbeCorePolicy::ExplicitlyAllowed);
                assert_eq!(after, before);
            } else {
                assert_eq!(policy, super::ExpectedProbeCorePolicy::SuppressionInstalled);
                assert_eq!(after, 0);
            }
        }
        "pass" => {}
        "panic" => panic!("synthetic classified child panic"),
        "panic-stack-text" => {
            panic!("fatal runtime error: stack overflow; has overflowed its stack")
        }
        "stack" => {
            prepare_expected_probe_core_dump();
            std::thread::Builder::new()
                .stack_size(64 * 1024)
                .spawn(|| overflow_native_stack(0))
                .expect("spawn stack-exhaustion child")
                .join()
                .expect("stack-exhaustion thread unexpectedly returned");
        }
        "timeout" => std::thread::sleep(std::time::Duration::from_secs(10)),
        "large-output" => {
            use std::io::Write as _;
            let bytes: Vec<_> = (0..262177).map(|i| (i % 251) as u8).collect();
            std::io::stderr().write_all(&bytes).unwrap();
            std::process::exit(7);
        }
        "flood" | "hold-flood" => {
            use std::io::Write as _;
            let started = std::time::Instant::now();
            let mut stderr = std::io::stderr().lock();
            while started.elapsed() < std::time::Duration::from_secs(3) {
                if stderr.write_all(&[b'x'; 4096]).is_err() {
                    break;
                }
            }
            std::process::exit(7);
        }
        "hold-silent" => {
            std::thread::sleep(std::time::Duration::from_secs(1));
            std::process::exit(7);
        }
        "descendant-silent" | "descendant-flood" => {
            let mut command = classifier_command(if mode == "descendant-silent" {
                "hold-silent"
            } else {
                "hold-flood"
            });
            let _descendant = command
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
                .unwrap();
            std::process::exit(7);
        }
        #[cfg(unix)]
        "closed-stderr-live-child" => {
            assert_eq!(unsafe { libc::close(libc::STDERR_FILENO) }, 0);
            std::thread::sleep(std::time::Duration::from_secs(3));
            std::process::exit(7);
        }
        "memory" => {
            prepare_expected_probe_core_dump();
            use std::io::Write as _;
            let mut stderr = std::io::stderr().lock();
            stderr
                .write_all(b"memory allocation of 18446744073709551615 bytes failed\n")
                .expect("write allocation-failure signature");
            stderr.flush().expect("flush allocation-failure signature");
            std::process::abort();
        }
        "signal" => {
            prepare_expected_probe_core_dump();
            std::process::abort();
        }
        other => panic!("unknown classifier child mode {other:?}"),
    }
}

fn process_outcome(mode: &str, timeout: std::time::Duration) -> ProbeOutcome {
    run_child_probe(&mut classifier_command(mode), timeout).expect("run classifier child")
}

fn classifier_command(mode: &str) -> std::process::Command {
    let executable = std::env::current_exe().expect("shared unit-test executable");
    let mut command = std::process::Command::new(executable);
    command
        .args([
            "--ignored",
            "--exact",
            "rust::test_process::tests::classified_probe_child",
        ])
        .env(CLASSIFIER_MODE, mode)
        .env_remove(super::ALLOW_EXPECTED_PROBE_CORE);
    command
}

#[test]
fn finite_output_larger_than_pipe_capacity_has_exact_raw_tail() {
    let ProbeOutcome::Failed(failure) =
        process_outcome("large-output", std::time::Duration::from_secs(10))
    else {
        panic!("large-output child unexpectedly passed");
    };
    let bytes: Vec<_> = (0..262177).map(|i| (i % 251) as u8).collect();
    assert_eq!(failure.exit_code, Some(7));
    assert_eq!(
        failure.stderr,
        String::from_utf8_lossy(&bytes[bytes.len() - 65536..])
    );
    assert!(failure.stderr.len() > 65536);
}

#[test]
fn continuous_output_cannot_starve_the_execution_deadline() {
    let ProbeOutcome::Failed(failure) =
        process_outcome("flood", std::time::Duration::from_millis(100))
    else {
        panic!("continuous writer unexpectedly passed");
    };
    assert_eq!(failure.kind, ProbeFailureKind::TimedOut);
}

#[test]
fn inherited_stderr_writers_cannot_hold_cleanup_open() {
    for mode in ["descendant-silent", "descendant-flood"] {
        let error = super::run_child_probe_with_cleanup(
            &mut classifier_command(mode),
            std::time::Duration::from_secs(10),
            std::time::Duration::from_millis(100),
        )
        .unwrap_err();
        let failure = error
            .get_ref()
            .unwrap()
            .downcast_ref::<super::ProbeIoFailure>()
            .unwrap();
        assert!(failure.reaped, "{mode}: {failure}");
        assert!(!failure.eof, "{mode}: {failure}");
        assert!(failure.primary.is_none());
        assert!(failure.termination.is_none());
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }
}

#[cfg(unix)]
#[test]
fn stderr_eof_does_not_mean_the_child_exited() {
    let ProbeOutcome::Failed(failure) = process_outcome(
        "closed-stderr-live-child",
        std::time::Duration::from_millis(100),
    ) else {
        panic!("live child with closed stderr unexpectedly passed");
    };
    assert_eq!(failure.kind, ProbeFailureKind::TimedOut);
}

#[test]
fn invalid_deadlines_are_rejected_before_spawn() {
    for timeout in [std::time::Duration::ZERO, std::time::Duration::MAX] {
        let mut command = std::process::Command::new("this-depth-probe-executable-does-not-exist");
        let error = run_child_probe(&mut command, timeout).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}

#[cfg(any(unix, windows))]
mod lifecycle_faults {
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::io;
    use std::process::ExitStatus;
    use std::rc::Rc;
    use std::time::Duration;

    use super::*;

    fn success() -> ExitStatus {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(0)
    }

    #[derive(Default)]
    struct Clock(Cell<Duration>);
    impl super::super::ProbeClock for Clock {
        fn elapsed(&self) -> Duration {
            let now = self.0.get() + Duration::from_millis(1);
            self.0.set(now);
            now
        }
        fn sleep(&self, time: Duration) { self.0.set(self.0.get() + time); }
    }

    struct Child {
        statuses: VecDeque<io::Result<Option<ExitStatus>>>,
        kill_error: Option<io::Error>,
        waits: usize,
        kills: usize,
    }
    impl Child {
        fn new(statuses: impl IntoIterator<Item = io::Result<Option<ExitStatus>>>) -> Self {
            Self {
                statuses: statuses.into_iter().collect(),
                kill_error: None,
                waits: 0,
                kills: 0,
            }
        }
    }
    impl super::super::ProbeChildControl for Child {
        fn id(&self) -> u32 { 5917 }
        fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            self.waits += 1;
            self.statuses.pop_front().unwrap_or(Ok(None))
        }
        fn kill(&mut self) -> io::Result<()> {
            self.kills += 1;
            self.kill_error.take().map_or(Ok(()), Err)
        }
    }

    enum Mode {
        Eof,
        Pending,
        Flood,
        Interrupted,
        Error,
    }
    struct Reader {
        mode: Mode,
        reads: Rc<Cell<usize>>,
        dropped: Rc<Cell<bool>>,
    }
    impl Reader {
        fn new(mode: Mode) -> Self {
            Self {
                mode,
                reads: Rc::new(Cell::new(0)),
                dropped: Rc::new(Cell::new(false)),
            }
        }
    }
    impl io::Read for Reader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.reads.set(self.reads.get() + 1);
            match self.mode {
                Mode::Eof => Ok(0),
                Mode::Pending => Err(io::ErrorKind::WouldBlock.into()),
                Mode::Interrupted => Err(io::ErrorKind::Interrupted.into()),
                Mode::Error => Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "capture-original",
                )),
                Mode::Flood => {
                    buffer.fill(b'x');
                    Ok(buffer.len())
                }
            }
        }
    }
    impl Drop for Reader {
        fn drop(&mut self) { self.dropped.set(true); }
    }

    fn run(
        child: &mut Child,
        reader: Reader,
        initial: Option<io::Error>,
    ) -> (io::Result<ProbeOutcome>, Duration) {
        let clock = Clock::default();
        let result = super::super::drive_child_probe(
            child,
            Some(reader),
            Duration::from_millis(5),
            Duration::from_millis(10),
            &clock,
            initial,
        );
        (result, clock.0.get())
    }

    #[test]
    fn first_error_survives_cleanup_and_capture_is_disabled() {
        for initial in [false, true] {
            let reader = Reader::new(Mode::Error);
            let reads = reader.reads.clone();
            let dropped = reader.dropped.clone();
            let primary =
                initial.then(|| io::Error::new(io::ErrorKind::InvalidData, "setup-original"));
            let mut child = Child::new([Ok(None), Ok(None), Ok(Some(success()))]);
            let (result, _) = run(&mut child, reader, primary);
            let error = result.unwrap_err();
            let failure = error
                .get_ref()
                .unwrap()
                .downcast_ref::<super::super::ProbeIoFailure>()
                .unwrap();
            assert_eq!(
                failure.primary.as_ref().unwrap().to_string(),
                if initial {
                    "setup-original"
                } else {
                    "capture-original"
                }
            );
            assert!(failure.reaped);
            assert!(!failure.eof);
            assert!(dropped.get());
            assert_eq!(reads.get(), if initial { 0 } else { 1 });
            assert_eq!(child.kills, 1);
        }
    }

    #[test]
    fn wait_failure_preserves_original_error_after_reaping() {
        let reader = Reader::new(Mode::Pending);
        let reads = reader.reads.clone();
        let mut child = Child::new([
            Err(io::Error::new(io::ErrorKind::Other, "wait-original")),
            Err(io::Error::new(io::ErrorKind::Other, "wait-secondary")),
            Ok(Some(success())),
        ]);
        let (result, _) = run(&mut child, reader, None);
        let error = result.unwrap_err();
        let failure = error
            .get_ref()
            .unwrap()
            .downcast_ref::<super::super::ProbeIoFailure>()
            .unwrap();
        assert_eq!(
            failure.primary.as_ref().unwrap().to_string(),
            "wait-original"
        );
        assert!(failure.reaped);
        assert_eq!(reads.get(), 0);
        assert_eq!(child.kills, 1);
    }

    #[test]
    fn timeout_survives_failed_kill_racing_with_successful_exit() {
        let clock = Clock(Cell::new(Duration::from_millis(5)));
        let mut child = Child::new([Ok(None), Ok(Some(success()))]);
        child.kill_error = Some(io::ErrorKind::InvalidInput.into());
        let result = super::super::drive_child_probe(
            &mut child,
            Some(Reader::new(Mode::Eof)),
            Duration::from_millis(5),
            Duration::from_millis(10),
            &clock,
            None,
        );
        let ProbeOutcome::Failed(failure) = result.unwrap() else {
            panic!("timeout disappeared");
        };
        assert_eq!(failure.kind, ProbeFailureKind::TimedOut);
        assert_eq!(failure.exit_code, Some(0));
        assert_eq!(child.kills, 1);
    }

    #[test]
    fn failed_termination_reports_unresolved_child_with_bounded_work() {
        for mode in [Mode::Pending, Mode::Flood, Mode::Interrupted] {
            let reader = Reader::new(mode);
            let reads = reader.reads.clone();
            let mut child = Child::new([]);
            child.kill_error = Some(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "kill-original",
            ));
            let (result, elapsed) = run(&mut child, reader, None);
            let error = result.unwrap_err();
            let failure = error
                .get_ref()
                .unwrap()
                .downcast_ref::<super::super::ProbeIoFailure>()
                .unwrap();
            assert!(!failure.reaped);
            assert_eq!(
                failure.termination.as_ref().unwrap().to_string(),
                "kill-original"
            );
            assert_eq!(child.kills, 1);
            assert!(elapsed <= Duration::from_millis(20));
            assert!(reads.get() <= 20 * super::super::PROBE_READ_ATTEMPTS);
        }
    }

    #[test]
    fn output_never_renews_cleanup_and_reaped_status_is_not_polled_again() {
        for mode in [Mode::Pending, Mode::Flood, Mode::Interrupted] {
            let mut child = Child::new([Ok(Some(success()))]);
            let (result, elapsed) = run(&mut child, Reader::new(mode), None);
            assert!(result.is_err());
            assert_eq!(child.waits, 1);
            assert_eq!(child.kills, 0);
            assert!(elapsed <= Duration::from_millis(14));
        }
    }

    #[test]
    fn observed_exit_wins_at_deadline_and_eof_is_not_read_again() {
        let clock = Clock(Cell::new(Duration::from_secs(10)));
        let mut child = Child::new([Ok(Some(success()))]);
        let reader = Reader::new(Mode::Eof);
        let reads = reader.reads.clone();
        let result = super::super::drive_child_probe(
            &mut child,
            Some(reader),
            Duration::from_millis(5),
            Duration::from_millis(10),
            &clock,
            None,
        )
        .unwrap();
        assert_eq!(result, ProbeOutcome::Passed);
        assert_eq!(child.waits, 1);
        assert_eq!(child.kills, 0);
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn slow_termination_cannot_start_a_read_after_cleanup_expiry() {
        struct SlowKill<'a>(&'a Clock);
        impl super::super::ProbeChildControl for SlowKill<'_> {
            fn id(&self) -> u32 { 5917 }
            fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> { Ok(None) }
            fn kill(&mut self) -> io::Result<()> {
                self.0 .0.set(Duration::from_secs(1));
                Ok(())
            }
        }
        let clock = Clock::default();
        let mut child = SlowKill(&clock);
        let reader = Reader::new(Mode::Flood);
        let reads = reader.reads.clone();
        let error = super::super::drive_child_probe(
            &mut child,
            Some(reader),
            Duration::from_millis(1),
            Duration::from_millis(10),
            &clock,
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn original_io_error_allocation_survives_secondary_failures() {
        #[derive(Debug)]
        struct Original(u64);
        impl std::fmt::Display for Original {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "original-{}", self.0)
            }
        }
        impl std::error::Error for Original {}
        let original = io::Error::new(io::ErrorKind::InvalidData, Original(5917));
        let identity = original.get_ref().unwrap() as *const _;
        let mut child = Child::new([Err(io::Error::other("secondary")), Ok(Some(success()))]);
        let (result, _) = run(&mut child, Reader::new(Mode::Pending), Some(original));
        let error = result.unwrap_err();
        let failure = error
            .get_ref()
            .unwrap()
            .downcast_ref::<super::super::ProbeIoFailure>()
            .unwrap();
        assert!(std::ptr::eq(
            failure.primary.as_ref().unwrap().get_ref().unwrap(),
            identity
        ));
    }
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
