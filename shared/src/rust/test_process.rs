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
