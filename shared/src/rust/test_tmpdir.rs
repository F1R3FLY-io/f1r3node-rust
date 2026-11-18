//! Per-test `$TMPDIR` scoping — a `mktemp -d` equivalent that cleans up on
//! drop and also redirects the process's `TMPDIR` env var at the fresh dir
//! for the guard's lifetime, so any code the test exercises that calls
//! `std::env::temp_dir()` lands under this test's private tree.
//!
//! Safe under `cargo nextest`, which runs each test in its own subprocess
//! (one `TMPDIR` reader for the guard's lifetime). Under `cargo test`'s
//! thread-per-test model the `TMPDIR` mutation races with concurrent tests
//! in the same binary — reach for plain `tempfile::tempdir()` there.
//!
//! Lifecycle hardening beyond `TempDir::drop`:
//!
//! - Every scoped dir is named `f1r3node-scoped-*` and contains a
//!   `.owner.pid` file naming the process that created it, so an orphan
//!   left behind by a crashed run can always be matched back to its
//!   (likely dead) owner.
//! - A process-global registry tracks every live `ScopedTmpdir`. The first
//!   `ScopedTmpdir::new()` installs a handler on `SIGTERM`, `SIGINT`, and
//!   `SIGHUP` that iterates the registry and removes any entries still
//!   present, then re-raises the signal so the default handler runs.
//!   Removal is idempotent — `NotFound` is swallowed — so the signal path
//!   races harmlessly against the regular `Drop` path.
//! - The same first call sweeps any pre-existing `f1r3node-scoped-*` dirs
//!   whose `.owner.pid` is no longer a live process (checked via
//!   `kill(pid, 0)`). Dirs whose owner is alive — another concurrent test
//!   binary on the same host — are left alone, so the sweep is safe even
//!   when several test processes run at once.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::{fs, thread};

use tempfile::{Builder, TempDir};

const SCOPED_TMPDIR_PREFIX: &str = "f1r3node-scoped-";
const OWNER_PID_FILE: &str = ".owner.pid";

static REGISTRY: OnceLock<Mutex<HashMap<u64, PathBuf>>> = OnceLock::new();
static INIT_ONCE: OnceLock<()> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct ScopedTmpdir {
    id: u64,
    dir: Option<TempDir>,
    previous: Option<OsString>,
}

impl ScopedTmpdir {
    pub fn new() -> Self {
        ensure_initialized();

        let dir = Builder::new()
            .prefix(SCOPED_TMPDIR_PREFIX)
            .tempdir()
            .expect("create scoped tmpdir");

        // Best-effort owner stamp. If the write fails the sweep just
        // leaves this dir alone until its own Drop removes it, which is
        // the safe side of the fence.
        let _ = write_owner_pid(dir.path());

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut reg) = registry().lock() {
            reg.insert(id, dir.path().to_path_buf());
        }

        let previous = std::env::var_os("TMPDIR");
        // SAFETY: Under nextest's subprocess-per-test model, this test is the
        // sole reader of `TMPDIR` for the guard's lifetime. The guard restores
        // the prior value on drop before any later test observes it.
        unsafe {
            std::env::set_var("TMPDIR", dir.path());
        }

        Self {
            id,
            dir: Some(dir),
            previous,
        }
    }

    pub fn path(&self) -> &Path {
        self.dir
            .as_ref()
            .expect("ScopedTmpdir dir is populated until drop")
            .path()
    }
}

impl Default for ScopedTmpdir {
    fn default() -> Self { Self::new() }
}

impl Drop for ScopedTmpdir {
    fn drop(&mut self) {
        unsafe {
            match self.previous.take() {
                Some(v) => std::env::set_var("TMPDIR", v),
                None => std::env::remove_var("TMPDIR"),
            }
        }
        if let Ok(mut reg) = registry().lock() {
            reg.remove(&self.id);
        }
        // TempDir drops here, removing the directory and everything under it.
        // If the signal handler raced us and already removed the path, the
        // NotFound inside TempDir::close() is logged by tempfile but harmless.
    }
}

fn registry() -> &'static Mutex<HashMap<u64, PathBuf>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn ensure_initialized() {
    INIT_ONCE.get_or_init(|| {
        sweep_orphans();
        install_signal_handler();
    });
}

fn write_owner_pid(dir: &Path) -> io::Result<()> {
    let mut f = fs::File::create(dir.join(OWNER_PID_FILE))?;
    writeln!(f, "{}", std::process::id())
}

fn sweep_orphans() {
    let base = std::env::temp_dir();
    let entries = match fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.starts_with(SCOPED_TMPDIR_PREFIX) {
            continue;
        }
        let path = entry.path();
        let Ok(pid_str) = fs::read_to_string(path.join(OWNER_PID_FILE)) else {
            // No owner stamp: a hand-created dir or a tempfile that crashed
            // mid-setup before write_owner_pid ran. Err on the side of not
            // touching it — a future sweep can revisit if we add a mtime
            // age cutoff, but we won't delete something we can't prove is
            // ours.
            continue;
        };
        let Ok(pid) = pid_str.trim().parse::<i32>() else {
            continue;
        };
        if !pid_alive(pid) {
            remove_quietly(&path);
        }
    }
}

fn pid_alive(pid: i32) -> bool {
    // kill(pid, 0) returns 0 if the process exists and we have permission,
    // -1/ESRCH if it does not, -1/EPERM if it exists but we cannot signal
    // it. Treat EPERM as alive — someone else owns the PID, not safe to
    // reclaim their dir.
    unsafe {
        if libc::kill(pid as libc::pid_t, 0) == 0 {
            return true;
        }
        io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

fn remove_quietly(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

static SIGNAL_RECEIVED: AtomicI32 = AtomicI32::new(0);
static SIGNAL_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

fn install_signal_handler() {
    // A pipe carries the "signal fired, come clean up" wake from the (async-
    // signal-safe) handler to a normal thread where we can lock a Mutex and
    // call std::fs::remove_dir_all.
    let mut fds: [libc::c_int; 2] = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return;
    }
    let read_fd = fds[0];
    let write_fd = fds[1];
    SIGNAL_WRITE_FD.store(write_fd, Ordering::SeqCst);

    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: the closure only touches atomics and calls libc::write,
        // both async-signal-safe. signal_hook_registry::register preserves
        // any previously-installed handler, so we compose with the tempfile
        // crate's own SIGINT cleanup rather than replacing it.
        let _ = unsafe {
            signal_hook_registry::register(sig, move || {
                SIGNAL_RECEIVED.store(sig, Ordering::SeqCst);
                let fd = SIGNAL_WRITE_FD.load(Ordering::SeqCst);
                if fd >= 0 {
                    let byte: u8 = 1;
                    libc::write(fd, &byte as *const u8 as *const _, 1);
                }
            })
        };
    }

    let _ = thread::Builder::new()
        .name("scoped-tmpdir-signal".into())
        .spawn(move || {
            let mut buf = [0u8; 1];
            // One signal is enough; we clean up and re-raise so the default
            // handler terminates the process on the next trip.
            let n = unsafe { libc::read(read_fd, buf.as_mut_ptr() as *mut _, 1) };
            if n <= 0 {
                return;
            }
            let paths: Vec<PathBuf> = registry()
                .lock()
                .map(|r| r.values().cloned().collect())
                .unwrap_or_default();
            for p in &paths {
                remove_quietly(p);
            }
            let sig = SIGNAL_RECEIVED.load(Ordering::SeqCst);
            // Restore the default handler and re-raise so the process exits
            // with the normal signal-indicated status (and the kernel tears
            // down its fds, semids, etc).
            unsafe {
                libc::signal(sig, libc::SIG_DFL);
                libc::raise(sig);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_tmpdir_redirects_env_temp_dir_and_cleans_up_on_drop() {
        let outside = std::env::temp_dir();
        let path_copy;
        {
            let scope = ScopedTmpdir::new();
            path_copy = scope.path().to_path_buf();
            assert_eq!(std::env::temp_dir(), path_copy);
            assert!(path_copy.exists());
            assert_ne!(path_copy, outside);
        }
        assert!(!path_copy.exists(), "tmpdir should be removed on drop");
        assert_eq!(std::env::temp_dir(), outside);
    }

    #[test]
    fn scoped_tmpdir_stamps_owner_pid_and_uses_the_f1r3node_prefix() {
        let scope = ScopedTmpdir::new();
        let name = scope
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(
            name.starts_with(SCOPED_TMPDIR_PREFIX),
            "tempdir name should start with {SCOPED_TMPDIR_PREFIX}; got {name}"
        );

        let pid_file = scope.path().join(OWNER_PID_FILE);
        let pid_str = fs::read_to_string(&pid_file).expect("owner pid file exists");
        let pid: u32 = pid_str.trim().parse().expect("pid parses");
        assert_eq!(pid, std::process::id(), "owner pid file names this process");
    }

    #[test]
    fn dropping_a_scoped_tmpdir_removes_it_from_the_registry() {
        let id;
        {
            let scope = ScopedTmpdir::new();
            id = scope.id;
            assert!(registry().lock().unwrap().contains_key(&id));
        }
        assert!(!registry().lock().unwrap().contains_key(&id));
    }

    #[test]
    fn sweep_orphans_removes_dirs_owned_by_dead_pids_and_leaves_live_ones() {
        let base = std::env::temp_dir();

        // Fabricate a stale orphan: a dir with our prefix whose .owner.pid
        // names a pid that could not possibly be live.
        let stale = Builder::new()
            .prefix(SCOPED_TMPDIR_PREFIX)
            .tempdir_in(&base)
            .expect("create stale tempdir")
            .keep();
        fs::write(stale.join(OWNER_PID_FILE), "2147483647\n").unwrap();

        // And a live orphan: owner is this process.
        let live = Builder::new()
            .prefix(SCOPED_TMPDIR_PREFIX)
            .tempdir_in(&base)
            .expect("create live tempdir")
            .keep();
        fs::write(
            live.join(OWNER_PID_FILE),
            format!("{}\n", std::process::id()),
        )
        .unwrap();

        sweep_orphans();

        assert!(!stale.exists(), "sweep should remove the dead-pid orphan");
        assert!(live.exists(), "sweep must not remove a live-pid orphan");

        // Clean up the live one ourselves.
        remove_quietly(&live);
    }

    #[test]
    fn remove_quietly_is_idempotent_on_a_missing_path() {
        let gone = std::env::temp_dir().join("f1r3node-scoped-does-not-exist");
        // Just ensure no panic / propagated error.
        remove_quietly(&gone);
        remove_quietly(&gone);
    }
}
