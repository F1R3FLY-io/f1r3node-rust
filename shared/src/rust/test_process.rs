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
pub enum ProbeFailureCause<E> {
    Outcome(ProbeFailure),
    Transport(E),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonotoneCeilingError<E> {
    pub depth: usize,
    pub failure: ProbeFailureCause<E>,
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
pub fn monotone_ceiling<E>(
    cap: usize,
    mut probe: impl FnMut(usize) -> Result<ProbeOutcome, E>,
) -> Result<MonotoneCeiling, MonotoneCeilingError<E>> {
    assert!(cap >= 1, "a depth-probe cap must include depth one");
    let mut run_probe = |depth| {
        probe(depth).map_err(|error| MonotoneCeilingError {
            depth,
            failure: ProbeFailureCause::Transport(error),
        })
    };

    if let ProbeOutcome::Failed(failure) = run_probe(1)? {
        return Ok(MonotoneCeiling::BaseFailed(failure));
    }
    if cap == 1 {
        return Ok(MonotoneCeiling::AtLeast(cap));
    }
    let mut first_failure = match run_probe(cap)? {
        ProbeOutcome::Passed => return Ok(MonotoneCeiling::AtLeast(cap)),
        ProbeOutcome::Failed(failure) if failure.kind == ProbeFailureKind::StackExhausted => {
            failure
        }
        ProbeOutcome::Failed(failure) => {
            return Err(MonotoneCeilingError {
                depth: cap,
                failure: ProbeFailureCause::Outcome(failure),
            });
        }
    };

    let mut lo = 1usize;
    let mut hi = cap;
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        match run_probe(mid)? {
            ProbeOutcome::Passed => lo = mid,
            ProbeOutcome::Failed(failure) if failure.kind == ProbeFailureKind::StackExhausted => {
                hi = mid;
                first_failure = failure;
            }
            ProbeOutcome::Failed(failure) => {
                return Err(MonotoneCeilingError {
                    depth: mid,
                    failure: ProbeFailureCause::Outcome(failure),
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

    Ok(classify_probe_status(status, timed_out, stderr))
}

fn classify_probe_status(
    status: std::process::ExitStatus,
    timed_out: bool,
    stderr: String,
) -> ProbeOutcome {
    if status.success() && !timed_out {
        return ProbeOutcome::Passed;
    }

    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt as _;
        status.signal()
    };
    #[cfg(not(unix))]
    let signal = None;

    #[cfg(unix)]
    let runtime_abort = signal == Some(libc::SIGABRT);
    #[cfg(windows)]
    let runtime_abort = status.code().map(|code| code as u32) == Some(0xC000_0409);
    #[cfg(not(any(unix, windows)))]
    let runtime_abort = false;

    #[cfg(windows)]
    let native_stack = status.code().map(|code| code as u32) == Some(0xC000_00FD);
    #[cfg(not(windows))]
    let native_stack = false;

    let lower = stderr.to_ascii_lowercase();
    let kind = if timed_out {
        ProbeFailureKind::TimedOut
    } else if status.code() == Some(101) || lower.contains("panicked at") {
        ProbeFailureKind::Panicked
    } else if native_stack
        || (runtime_abort
            && (lower.contains("fatal runtime error: stack overflow")
                || lower.contains("has overflowed its stack")))
    {
        ProbeFailureKind::StackExhausted
    } else if runtime_abort
        && (lower.contains("memory allocation of")
            || lower.contains("out of memory")
            || lower.contains("cannot allocate memory"))
    {
        ProbeFailureKind::MemoryExhausted
    } else if signal.is_some() {
        ProbeFailureKind::Signaled
    } else {
        ProbeFailureKind::ExitFailure
    };

    ProbeOutcome::Failed(ProbeFailure {
        kind,
        exit_code: status.code(),
        signal,
        stderr,
    })
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
#[path = "../../tests/support/test_process.rs"]
mod tests;
