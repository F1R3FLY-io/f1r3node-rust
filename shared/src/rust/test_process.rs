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

pub fn stack_survived<E>(
    observation: Result<ProbeOutcome, E>,
) -> Result<bool, ProbeFailureCause<E>> {
    match observation.map_err(ProbeFailureCause::Transport)? {
        ProbeOutcome::Passed => Ok(true),
        ProbeOutcome::Failed(failure) if failure.kind == ProbeFailureKind::StackExhausted => {
            Ok(false)
        }
        ProbeOutcome::Failed(failure) => Err(ProbeFailureCause::Outcome(failure)),
    }
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
    run_child_probe_with_cleanup(command, timeout, std::time::Duration::from_secs(1))
}

fn run_child_probe_with_cleanup(
    command: &mut std::process::Command,
    timeout: std::time::Duration,
    cleanup_timeout: std::time::Duration,
) -> std::io::Result<ProbeOutcome> {
    let started = std::time::Instant::now();
    if timeout.is_zero() || started.checked_add(timeout).is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "depth-probe timeout must be positive and representable",
        ));
    }
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    let capture = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("depth-probe stderr pipe is missing"))
        .and_then(ProbeStderr::new);
    let (stderr, error) = match capture {
        Ok(stderr) => (Some(stderr), None),
        Err(error) => (None, Some(error)),
    };
    drive_child_probe(
        &mut child,
        stderr,
        timeout,
        cleanup_timeout,
        &started,
        error,
    )
}

const PROBE_STDERR_LIMIT: usize = 64 * 1024;
const PROBE_READ_ATTEMPTS: usize = 16;

#[derive(Debug, Clone, Copy, Default)]
struct ProbeLifecycle {
    reaped: bool,
    eof: bool,
    failed: bool,
    timed_out: bool,
    cleanup_started: Option<std::time::Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeDecision {
    Poll,
    Classify(bool),
    HarnessError,
}

impl ProbeLifecycle {
    fn observe(
        &mut self,
        timeout: std::time::Duration,
        now: std::time::Duration,
        exited: bool,
        closed: bool,
        error: bool,
    ) {
        self.reaped |= exited;
        self.eof |= closed;
        self.failed |= error;
        self.timed_out |= !self.reaped && self.cleanup_started.is_none() && now >= timeout;
        if self.cleanup_started.is_none() && (self.reaped || self.failed || self.timed_out) {
            self.cleanup_started = Some(now);
        }
    }

    fn can_read(&self) -> bool { !self.eof && !self.failed }

    fn decide(&self, grace: std::time::Duration, now: std::time::Duration) -> ProbeDecision {
        if self.reaped && (self.eof || self.failed) {
            if self.failed {
                ProbeDecision::HarnessError
            } else {
                ProbeDecision::Classify(self.timed_out)
            }
        } else if self
            .cleanup_started
            .is_some_and(|start| now.saturating_sub(start) >= grace)
        {
            ProbeDecision::HarnessError
        } else {
            ProbeDecision::Poll
        }
    }
}

fn retain_probe_stderr(tail: &mut std::collections::VecDeque<u8>, bytes: &[u8]) {
    if bytes.len() >= PROBE_STDERR_LIMIT {
        tail.clear();
        tail.extend(&bytes[bytes.len() - PROBE_STDERR_LIMIT..]);
    } else {
        let discard = tail
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(PROBE_STDERR_LIMIT);
        tail.drain(..discard);
        tail.extend(bytes);
    }
}

#[derive(Default)]
struct CaptureProgress {
    eof: bool,
    made_progress: bool,
}

fn read_probe_quantum(
    reader: &mut impl std::io::Read,
    tail: &mut std::collections::VecDeque<u8>,
) -> std::io::Result<CaptureProgress> {
    let mut result = CaptureProgress::default();
    let mut chunk = [0u8; 4096];
    for _ in 0..PROBE_READ_ATTEMPTS {
        match reader.read(&mut chunk) {
            Ok(0) => {
                result.eof = true;
                break;
            }
            Ok(count) => {
                retain_probe_stderr(tail, &chunk[..count]);
                result.made_progress = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error),
        }
    }
    Ok(result)
}

struct ProbeStderr(std::process::ChildStderr);

impl ProbeStderr {
    fn new(stderr: std::process::ChildStderr) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let flags = unsafe { libc::fcntl(stderr.as_raw_fd(), libc::F_GETFL) };
            if flags == -1
                || unsafe {
                    libc::fcntl(stderr.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK)
                } == -1
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        #[cfg(not(any(unix, windows)))]
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "nonblocking depth-probe capture is unsupported on this platform",
        ));

        #[cfg(any(unix, windows))]
        Ok(Self(stderr))
    }
}

impl std::io::Read for ProbeStderr {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            #[link(name = "kernel32")]
            extern "system" {
                fn PeekNamedPipe(
                    handle: *mut std::ffi::c_void,
                    buffer: *mut std::ffi::c_void,
                    buffer_size: u32,
                    bytes_read: *mut u32,
                    total_available: *mut u32,
                    bytes_left: *mut u32,
                ) -> i32;
            }
            if buffer.is_empty() {
                return Ok(0);
            }
            let mut available = 0;
            let success = unsafe {
                PeekNamedPipe(
                    self.0.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if success == 0 {
                let error = std::io::Error::last_os_error();
                return if error.raw_os_error() == Some(109) {
                    Ok(0)
                } else {
                    Err(error)
                };
            }
            if available == 0 {
                return Err(std::io::ErrorKind::WouldBlock.into());
            }
            let length = buffer.len().min(available as usize);
            return self.0.read(&mut buffer[..length]);
        }
        #[cfg(not(windows))]
        self.0.read(buffer)
    }
}

trait ProbeChildControl {
    fn id(&self) -> u32;
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>>;
    fn kill(&mut self) -> std::io::Result<()>;
}

impl ProbeChildControl for std::process::Child {
    fn id(&self) -> u32 { self.id() }
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> { self.try_wait() }
    fn kill(&mut self) -> std::io::Result<()> { self.kill() }
}

trait ProbeClock {
    fn elapsed(&self) -> std::time::Duration;
    fn sleep(&self, duration: std::time::Duration);
}

impl ProbeClock for std::time::Instant {
    fn elapsed(&self) -> std::time::Duration { self.elapsed() }
    fn sleep(&self, duration: std::time::Duration) { std::thread::sleep(duration); }
}

#[derive(Debug)]
struct ProbeIoFailure {
    primary: Option<std::io::Error>,
    termination: Option<std::io::Error>,
    child: u32,
    reaped: bool,
    eof: bool,
}

impl std::fmt::Display for ProbeIoFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "depth-probe child {}: reaped={}, stderr_eof={}",
            self.child, self.reaped, self.eof
        )?;
        if let Some(error) = &self.primary {
            write!(f, "; primary error: {error}")?;
        }
        if let Some(error) = &self.termination {
            write!(f, "; termination error: {error}")?;
        }
        if !self.reaped || (!self.eof && self.primary.is_none()) {
            write!(f, "; cleanup deadline expired")?;
        }
        Ok(())
    }
}

impl std::error::Error for ProbeIoFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.primary
            .as_ref()
            .or(self.termination.as_ref())
            .map(|error| error as _)
    }
}

fn drive_child_probe(
    child: &mut impl ProbeChildControl,
    mut stderr: Option<impl std::io::Read>,
    timeout: std::time::Duration,
    cleanup_timeout: std::time::Duration,
    clock: &impl ProbeClock,
    mut error: Option<std::io::Error>,
) -> std::io::Result<ProbeOutcome> {
    let mut state = ProbeLifecycle::default();
    let mut status = None;
    let mut termination = None;
    let mut tail = std::collections::VecDeque::with_capacity(PROBE_STDERR_LIMIT);
    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(observed) => status = observed,
                Err(observed) => {
                    error.get_or_insert(observed);
                }
            }
        }
        let now = clock.elapsed();
        let was_running = state.cleanup_started.is_none();
        state.observe(timeout, now, status.is_some(), false, error.is_some());
        if was_running && state.cleanup_started.is_some() && !state.reaped {
            termination = child.kill().err();
        }
        if state.failed {
            stderr.take();
        }
        match state.decide(cleanup_timeout, clock.elapsed()) {
            ProbeDecision::Classify(timed_out) => {
                let stderr = String::from_utf8_lossy(tail.make_contiguous()).into_owned();
                return Ok(classify_probe_status(
                    status.expect("reaped probe has exit status"),
                    timed_out,
                    stderr,
                ));
            }
            ProbeDecision::HarnessError => {
                let kind = error
                    .as_ref()
                    .map_or(std::io::ErrorKind::TimedOut, std::io::Error::kind);
                return Err(std::io::Error::new(kind, ProbeIoFailure {
                    primary: error,
                    termination: if state.reaped { None } else { termination },
                    child: child.id(),
                    reaped: state.reaped,
                    eof: state.eof,
                }));
            }
            ProbeDecision::Poll => {}
        }
        if state.can_read() {
            if let Some(reader) = stderr.as_mut() {
                match read_probe_quantum(reader, &mut tail) {
                    Ok(progress) => {
                        state.eof |= progress.eof;
                        if progress.eof || progress.made_progress {
                            continue;
                        }
                    }
                    Err(observed) => {
                        error.get_or_insert(observed);
                        continue;
                    }
                }
            }
        }
        let now = clock.elapsed();
        let remaining = match state.cleanup_started {
            Some(start) => cleanup_timeout.saturating_sub(now.saturating_sub(start)),
            None => timeout.saturating_sub(now),
        };
        clock.sleep(remaining.min(std::time::Duration::from_millis(5)));
    }
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
