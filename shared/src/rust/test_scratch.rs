//! Process-scoped scratch directories that clean themselves up **without a `Drop` impl**.
//!
//! # The defect this module replaces
//!
//! Test support code across this workspace used to hold its scratch directory in a
//! lazily-initialised process-global:
//!
//! ```ignore
//! lazy_static! {
//!     // "Automatic cleanup when TempDir is dropped (at program exit)."
//!     static ref SHARED_LMDB_ENV: (PathBuf, TempDir) = { /* ... */ };
//! }
//! ```
//!
//! The comment is false. **Rust runs no destructors for `static`s.** `TempDir::drop` is the
//! only thing that removes a `tempfile` directory, so it never ran, and every test process
//! left its whole LMDB environment behind. Measured live during one `cargo nextest` run:
//! 285 directories / 824 MB accumulated in 145 seconds (~2 directories/s, ~5.7 MB/s), on a
//! `/tmp` that is `tmpfs` — that is resident memory, not disk.
//!
//! # Why this type has no `Drop`
//!
//! The defect is not "we forgot to drop it". It is *a type whose entire contract is `Drop`,
//! placed where `Drop` can never run*. A replacement that also carried `Drop` would be the
//! same lie with a new destructor. [`ProcessScratchDir`] therefore deliberately has **no**
//! `Drop` impl; it is built to be stored in a process-global `static` and to be correct
//! there. Cleanup is a property of the *process*, established by two independent layers.
//!
//! # Layer 1 — eager removal via `atexit`
//!
//! At acquisition, the final path is pushed onto a lock-free, append-only stack and an
//! `extern "C" fn()` is registered once with libc's `atexit(3)`. At normal process exit that
//! hook walks the stack and `remove_dir_all`s every registered path, ignoring errors.
//!
//! * `atexit` is **not** a signal context, so the hook may allocate, `opendir`, and recurse.
//! * The hook may run while tokio workers still hold LMDB `mmap`s. That is fine: POSIX
//!   `unlink(2)` removes the *name*; the inode survives until the last mapping is closed.
//! * It covers normal exit and panicking tests. No profile in this workspace sets
//!   `panic = "abort"`, so a panicking test unwinds and the harness still exits normally.
//! * It does **not** cover `SIGKILL`, `SIGSEGV`, `SIGTERM`, or `abort(3)`. That is layer 2's
//!   entire job.
//!
//! # Layer 2 — `reap_orphans`, backstopped by the kernel
//!
//! Before a process creates its own directory it reaps the ones left by processes that died
//! without running their `atexit` hook. **Safety comes from `flock(2)`, not from an age
//! threshold.** Every scratch directory contains an [`OWNER_LOCK_FILE_NAME`] file which its
//! owner opens and holds an exclusive `flock` on for the process's whole lifetime. Per
//! candidate the reaper opens that file and calls [`std::fs::File::try_lock`]:
//!
//! | `try_lock` result                | meaning                                    | action        |
//! |----------------------------------|--------------------------------------------|---------------|
//! | `Err(TryLockError::WouldBlock)`  | the owner is **alive** — a proof, not an estimate | skip     |
//! | `Ok(())`                         | the owner is **dead**, and we now hold the lock so no concurrent reaper can be removing this tree | `remove_dir_all` |
//! | no usable lock file              | undecidable — fall through to the age gate | see below     |
//!
//! The load-bearing kernel property is that **an `flock` is held by the open file
//! description, and the kernel closes every descriptor on process termination by any means,
//! including `SIGKILL` and `SIGSEGV`**. No cooperation from the dying process is required.
//! That is exactly why this works where `atexit` cannot, and it is why the owner's descriptor
//! is leaked on purpose (see [`acquire_in`]): *the kernel is the releaser*.
//!
//! # Closing the creation window with `rename(2)`, not with a threshold
//!
//! Between `mkdir` and `flock` a directory exists that is not yet locked. A reaper that saw
//! it would find no usable lock and could remove a live directory. So creation stages:
//!
//! ```text
//!   mkdir  <prefix><pid>-<stamp>.incoming
//!   create+flock  <prefix><pid>-<stamp>.incoming/.owner.lock      (descriptor leaked)
//!   rename <prefix><pid>-<stamp>.incoming  ->  <prefix><pid>-<stamp>
//! ```
//!
//! `rename(2)` within a single parent directory is atomic, and the `flock` rides the inode,
//! not the path. **After the rename, a directory is visible under a final name only if it
//! already holds a live lock.**
//!
//! The staging name is not decoration: the reaper treats the two namespaces by different
//! rules. A `.incoming` name is judged by **age alone** and never by `flock`, because in the
//! window between creating `.owner.lock` and locking it the file exists unlocked, and a reaper
//! that consulted `flock` there would take the lock, call the owner dead, and delete a
//! directory that is being born. A final name is judged by `flock` alone, which is exact,
//! because a final name never exists without a held lock. See [`reap_orphans_in`].
//!
//! # The age threshold is not safety-critical
//!
//! [`ORPHAN_MIN_AGE`] is six hours, and it gates only the *undecidable* case: entries with no
//! usable lock file. Those are `.incoming` orphans from the microsecond-wide creation window,
//! directories left by binaries built before this module existed, and manual meddling. A live
//! process in that state is at most milliseconds old, so six hours is enormously generous.
//!
//! Layer 1 is what earns the generosity. Because the `atexit` hook removes directories
//! promptly on every normal exit, nothing depends on the threshold to *bound accumulation* —
//! the threshold only has to be long enough that it can never race a live process, and short
//! enough that pre-existing junk eventually goes away. The number itself is not the point;
//! the argument is.
//!
//! # Why no signal handlers
//!
//! Ruled out with reasons, not by omission:
//!
//! * A `SIGSEGV` handler cannot do recursive tree removal. Async-signal-safety permits
//!   `unlink`/`rmdir` but not `opendir`/`readdir`/`malloc`, and the dominant `SIGSEGV` in this
//!   workspace is a guard-page hit from deep normalizer recursion — i.e. the handler would run
//!   on an exhausted stack.
//! * `SIGTERM` would need a self-pipe plus a reaper thread in *every* test process to cover a
//!   case layer 2 already handles from the outside, for free.
//!
//! # Where the directories live
//!
//! [`base_dir`] is [`std::env::temp_dir`] (which honours `TMPDIR`), overridable with
//! [`SCRATCH_BASE_DIR_ENV`]. RAM-backing via `tmpfs` is **deliberate**: it is what makes LMDB
//! commits in the test suite fast. Moving the scratch tree onto the repository's `target/`
//! directory would trade a bounded, self-clearing memory footprint for a permanent throughput
//! loss on every commit — and `target/` is 93% full. The two layers above are what make the
//! RAM-backing safe to keep.
//!
//! # A note on gating
//!
//! This module is compiled unconditionally rather than behind a `shared/test-utils` feature.
//! `rholang` exposes `rholang::rust::interpreter::test_utils` as an ungated `pub mod` and
//! calls into this module from it, so a feature gate here would need a matching gate there
//! (a 54-file change) plus an edit to `rholang/Cargo.toml`, which is a held-local overlay in
//! this worktree. The module's only dependency beyond `std` is `libc`, which `shared` already
//! links transitively through `heed` and `tokio`, so the cost of compiling it everywhere is
//! the object code alone. Gating `rholang`'s `test_utils` is tracked separately; once that
//! lands, this module can move behind `shared/test-utils` with a two-line change.

use std::collections::HashSet;
use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fmt, io, ptr};

/// Environment variable that overrides the scratch base directory.
///
/// When unset (or empty) [`base_dir`] falls back to [`std::env::temp_dir`], which itself
/// honours `TMPDIR`. Tests that need a private base point this at a directory they own and
/// genuinely drop.
pub const SCRATCH_BASE_DIR_ENV: &str = "F1R3FLY_TEST_SCRATCH_DIR";

/// Name of the `flock`-bearing sentinel file inside every scratch directory.
///
/// Its presence-and-lockedness is the *only* liveness signal the reaper trusts.
pub const OWNER_LOCK_FILE_NAME: &str = ".owner.lock";

/// Suffix of the staging name a directory carries between `mkdir` and `rename`.
///
/// See the module documentation: the staging name is what keeps the unlocked creation window
/// out of the final namespace.
pub const STAGING_SUFFIX: &str = ".incoming";

/// How old a *lock-less* candidate must be before the reaper will remove it.
///
/// Not safety-critical — see the module documentation. Only entries whose ownership cannot be
/// decided by `flock` ever reach this gate.
pub const ORPHAN_MIN_AGE: Duration = Duration::from_secs(6 * 60 * 60);

/// How many times [`acquire_in`] retries when a generated directory name collides.
///
/// A collision requires two `mkdir`s in the same process to produce the same nanosecond *and*
/// the same monotonic sequence number, which cannot happen; the retry exists so that an
/// adversarially pre-created name cannot wedge the process.
const MAX_CREATE_ATTEMPTS: u32 = 64;

// ---------------------------------------------------------------------------------------
// The handle
// ---------------------------------------------------------------------------------------

/// Handle to a scratch directory owned by this process.
///
/// This type **deliberately has no `Drop`**. It exists to be stored in a process-global
/// `static`, and Rust never drops `static`s. Cleanup is
///
/// 1. registered with `atexit(3)` at acquisition ([`acquire_in`]), and
/// 2. backstopped by [`reap_orphans_in`] in the next process that starts.
///
/// A `Drop` impl here would be unreachable in the only place this type is used, which is
/// exactly the defect this type replaces. If you find yourself wanting one, you want
/// [`tempfile::TempDir`](https://docs.rs/tempfile) in a genuinely scoped local instead.
pub struct ProcessScratchDir {
    path: PathBuf,
}

impl ProcessScratchDir {
    /// The directory's path. It exists and is owned by this process.
    pub fn path(&self) -> &Path { &self.path }

    /// A clone of the directory's path, for APIs that need an owned `PathBuf`.
    pub fn to_path_buf(&self) -> PathBuf { self.path.clone() }
}

impl AsRef<Path> for ProcessScratchDir {
    fn as_ref(&self) -> &Path { &self.path }
}

impl fmt::Debug for ProcessScratchDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessScratchDir")
            .field("path", &self.path)
            .finish()
    }
}

// ---------------------------------------------------------------------------------------
// Layer 1 — eager removal at exit
// ---------------------------------------------------------------------------------------

/// A node in the append-only, never-freed stack of paths to remove at exit.
///
/// `next` is written before the node is published by the release CAS in
/// [`register_for_exit_removal`] and is never mutated afterwards, so the `atexit` hook may
/// walk the chain with plain acquire loads.
struct ExitRemovalNode {
    path: PathBuf,
    next: *mut ExitRemovalNode,
}

/// Treiber stack head. Nodes are intentionally leaked: freeing them would introduce a
/// reclamation race with an `atexit` hook that can start at any moment, and the total is
/// bounded by the number of scratch directories a process acquires.
static EXIT_REMOVAL_STACK: AtomicPtr<ExitRemovalNode> = AtomicPtr::new(ptr::null_mut());

/// Guards the single `atexit` registration.
static EXIT_HOOK_REGISTERED: Once = Once::new();

/// Push `path` onto the exit-removal stack and make sure the `atexit` hook is installed.
///
/// Lock-free on purpose. A `Mutex` here would be a latent deadlock: a thread calling
/// `std::process::exit` while another thread held the lock would block the `atexit` hook
/// forever. A single CAS cannot deadlock against anything.
fn register_for_exit_removal(path: &Path) {
    let node = Box::into_raw(Box::new(ExitRemovalNode {
        path: path.to_path_buf(),
        next: ptr::null_mut(),
    }));

    loop {
        let head = EXIT_REMOVAL_STACK.load(Ordering::Acquire);
        // SAFETY: `node` was just allocated here and has not been published yet, so this
        // thread holds exclusive access to it until the CAS below succeeds.
        unsafe {
            (*node).next = head;
        }
        match EXIT_REMOVAL_STACK.compare_exchange_weak(
            head,
            node,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(_) => continue,
        }
    }

    EXIT_HOOK_REGISTERED.call_once(|| {
        // SAFETY: `atexit` is called at most once per process (guarded by `Once`) with a
        // `'static` `extern "C" fn` that takes no arguments, which is exactly the contract
        // libc requires.
        unsafe {
            libc::atexit(remove_registered_scratch_dirs);
        }
    });
}

/// The `atexit(3)` hook. Walks the exit-removal stack and unlinks every registered tree.
///
/// Panic-free by construction — every fallible call is discarded — because a panic escaping
/// an `extern "C"` function aborts the process. Errors are deliberately ignored: whatever this
/// hook fails to remove, [`reap_orphans_in`] removes from the next process that starts.
///
/// If a concurrent [`register_for_exit_removal`] wins the race with the `load` below, the new
/// path is simply not seen here; layer 2 covers it.
extern "C" fn remove_registered_scratch_dirs() {
    let mut cursor = EXIT_REMOVAL_STACK.load(Ordering::Acquire);
    while !cursor.is_null() {
        // SAFETY: nodes are leaked, never freed, and immutable after publication, so a
        // published pointer stays valid for the lifetime of the process.
        let node: &ExitRemovalNode = unsafe { &*cursor };
        let _ = fs::remove_dir_all(&node.path);
        cursor = node.next;
    }
}

// ---------------------------------------------------------------------------------------
// Layer 2 — reaping
// ---------------------------------------------------------------------------------------

/// What [`reap_orphans_in`] did, for tests and for logging.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReapReport {
    /// Directories removed because their owner was proved dead, or because they carried no
    /// usable lock file and were older than [`ORPHAN_MIN_AGE`].
    pub removed: usize,
    /// Directories skipped because `flock` proved the owner is still alive.
    pub skipped_live: usize,
    /// Directories skipped because they carried no usable lock file and were younger than
    /// [`ORPHAN_MIN_AGE`].
    pub skipped_young: usize,
    /// Directory entries that could not be read, stat'd, or removed. Always non-fatal.
    pub errors: usize,
}

/// The ownership verdict `flock` gives us for one candidate directory.
enum Ownership {
    /// `flock` said `WouldBlock`: some open file description still holds the lock, therefore
    /// the owning process is alive.
    Live,
    /// `flock` succeeded: the owner is gone. The `File` is carried along so the exclusive lock
    /// is held for the whole removal, which serialises concurrent reapers.
    Dead(File),
    /// No usable lock file. Ownership is undecidable and the age gate applies.
    Undecidable,
}

/// Decide who owns `dir` by trying to take its owner lock.
fn ownership_of(dir: &Path) -> Ownership {
    let lock_path = dir.join(OWNER_LOCK_FILE_NAME);
    match File::open(&lock_path) {
        Ok(lock_file) => match lock_file.try_lock() {
            // We took the lock, so nobody else holds it: the owner is dead.
            Ok(()) => Ownership::Dead(lock_file),
            // Somebody's open file description still holds it: the owner is alive.
            Err(TryLockError::WouldBlock) => Ownership::Live,
            // A real I/O failure (e.g. a filesystem that does not implement `flock`).
            // Refuse to guess; let the age gate decide.
            Err(TryLockError::Error(_)) => Ownership::Undecidable,
        },
        Err(_) => Ownership::Undecidable,
    }
}

/// `Some(true)` when `dir`'s mtime is at least `age` in the past, `Some(false)` when it is
/// not, `None` when the mtime cannot be read. `None` and a clock that runs backwards both
/// resolve conservatively to "do not remove".
fn is_older_than(dir: &Path, age: Duration) -> Option<bool> {
    let modified = fs::metadata(dir).ok()?.modified().ok()?;
    Some(
        SystemTime::now()
            .duration_since(modified)
            .map(|elapsed| elapsed >= age)
            .unwrap_or(false),
    )
}

/// Remove scratch directories under `base` whose names start with `prefix` and whose owning
/// process is gone.
///
/// This is layer 2. See the module documentation for the safety argument. Errors never
/// propagate: a reaper that cannot clean up must not break the process that is starting.
///
/// Two reapers racing on the same lock-less candidate can both call `remove_dir_all`; the
/// loser sees `NotFound`, which is counted as a removal rather than an error because the tree
/// is gone either way.
///
/// # Staging directories are judged by age alone
///
/// A name ending in [`STAGING_SUFFIX`] **never** goes through [`ownership_of`]; it goes
/// straight to the age gate. This is not a stylistic choice, it is the whole reason the
/// staging name exists.
///
/// [`acquire_in`] creates the directory, then creates `.owner.lock` inside it, then locks it.
/// Between the second and third steps the lock file **exists and is not yet held**, so a
/// concurrent reaper that consulted `flock` would take the lock, conclude the owner is dead,
/// and `remove_dir_all` a directory that is being born. That is not theoretical: it was
/// observed in a real casper run the first time this function applied one uniform rule to both
/// namespaces —
///
/// ```text
/// test_scratch: cannot rename .../casper-shared-lmdb-<pid>-<stamp>.incoming into place
///               at .../casper-shared-lmdb-<pid>-<stamp>: No such file or directory
/// ```
///
/// — two nextest processes starting within microseconds of each other, one reaping the other's
/// half-built directory out from under it.
///
/// Sending staging names to the age gate closes it completely, because a *live* staging
/// directory is at most microseconds old and [`ORPHAN_MIN_AGE`] is six hours. And it costs
/// nothing: after the `rename`, a directory under a final name always already holds its lock,
/// so the `flock` verdict remains exact everywhere it is actually used.
pub fn reap_orphans_in(base: &Path, prefix: &str) -> ReapReport {
    let mut report = ReapReport::default();

    let entries = match fs::read_dir(base) {
        Ok(entries) => entries,
        Err(_) => {
            report.errors += 1;
            return report;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                report.errors += 1;
                continue;
            }
        };

        let file_name = entry.file_name();
        let name = match file_name.to_str() {
            Some(name) => name,
            None => continue,
        };
        if !name.starts_with(prefix) {
            continue;
        }
        // `file_type` does not follow symlinks, so a symlink named like a scratch directory is
        // ignored rather than followed into somebody else's tree.
        match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => {}
            Ok(_) => continue,
            Err(_) => {
                report.errors += 1;
                continue;
            }
        }

        let path = entry.path();

        // A staging directory is mid-creation by construction: its lock file may exist without
        // yet being held. Consulting `flock` here would race `acquire_in` and delete a
        // directory that is being born, so age is the only admissible evidence. See this
        // function's documentation for the failure this prevents.
        let ownership = if name.ends_with(STAGING_SUFFIX) {
            Ownership::Undecidable
        } else {
            ownership_of(&path)
        };

        match ownership {
            Ownership::Live => {
                report.skipped_live += 1;
            }
            Ownership::Dead(lock_file) => {
                remove_tree(&path, &mut report);
                // Held until here on purpose: while we own the lock no other reaper can be
                // removing this same tree.
                drop(lock_file);
            }
            Ownership::Undecidable => match is_older_than(&path, ORPHAN_MIN_AGE) {
                Some(true) => remove_tree(&path, &mut report),
                Some(false) => report.skipped_young += 1,
                None => report.errors += 1,
            },
        }
    }

    report
}

/// `remove_dir_all` with the accounting the reaper wants.
fn remove_tree(path: &Path, report: &mut ReapReport) {
    match fs::remove_dir_all(path) {
        Ok(()) => report.removed += 1,
        Err(err) if err.kind() == io::ErrorKind::NotFound => report.removed += 1,
        Err(_) => report.errors += 1,
    }
}

/// [`reap_orphans_in`] against [`base_dir`].
pub fn reap_orphans(prefix: &str) -> ReapReport { reap_orphans_in(&base_dir(), prefix) }

// ---------------------------------------------------------------------------------------
// Acquisition
// ---------------------------------------------------------------------------------------

/// The base directory scratch directories are created in.
///
/// [`SCRATCH_BASE_DIR_ENV`] wins if it is set and non-empty; otherwise
/// [`std::env::temp_dir`], which honours `TMPDIR`.
pub fn base_dir() -> PathBuf {
    match std::env::var_os(SCRATCH_BASE_DIR_ENV) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => std::env::temp_dir(),
    }
}

/// Monotonic per-process counter, so two directories created in the same nanosecond still get
/// distinct names.
static NAME_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// `(base, prefix)` pairs this process has already reaped, so the scan runs once per pair.
static REAPED_PAIRS: OnceLock<Mutex<HashSet<(PathBuf, String)>>> = OnceLock::new();

fn unique_stamp() -> String {
    let sequence = NAME_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}-{sequence:x}")
}

/// Run [`reap_orphans_in`] at most once per `(base, prefix)` per process.
///
/// Deduplication is a cost optimisation, not a correctness requirement: a second scan would
/// find this process's own directory and get `WouldBlock` from `flock`, because an `flock` is
/// held per *open file description* and the reaper's `open` produces a different one even
/// within the owning process. (Verified: a second `try_lock` on a freshly `open`ed handle to
/// an already-locked file returns `WouldBlock` in the same process.)
fn reap_orphans_once(base: &Path, prefix: &str) {
    let key = (base.to_path_buf(), prefix.to_string());
    let seen = REAPED_PAIRS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut seen = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if seen.insert(key) {
        // Held across the scan so two threads racing into the same `OnceLock` initialiser do
        // not both scan. This mutex is never touched by the `atexit` hook, so it cannot
        // deadlock at exit.
        let _ = reap_orphans_in(base, prefix);
    }
}

/// Create this process's scratch directory under `base` and register it for cleanup.
///
/// The sequence — stage under [`STAGING_SUFFIX`], create and `flock` the owner lock, `rename`
/// into place — is what makes the reaper's rule sound; see the module documentation.
///
/// # Panics
///
/// Panics if the base directory cannot be created, if the scratch directory cannot be created
/// or renamed, or if the owner lock cannot be taken on a directory this call just created.
/// Every one of those means the test environment is unusable, and a silent fallback would
/// reintroduce exactly the class of bug this module exists to remove.
pub fn acquire_in(base: &Path, prefix: &str) -> ProcessScratchDir {
    fs::create_dir_all(base).unwrap_or_else(|err| {
        panic!(
            "test_scratch: cannot create scratch base directory {}: {err}",
            base.display()
        )
    });

    reap_orphans_once(base, prefix);

    let pid = std::process::id();
    let mut last_collision = None;

    for _ in 0..MAX_CREATE_ATTEMPTS {
        let final_name = format!("{prefix}{pid}-{}", unique_stamp());
        let staging_path = base.join(format!("{final_name}{STAGING_SUFFIX}"));

        match fs::create_dir(&staging_path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                last_collision = Some(staging_path);
                continue;
            }
            Err(err) => panic!(
                "test_scratch: cannot create scratch directory {}: {err}",
                staging_path.display()
            ),
        }

        // The directory exists but is not yet locked. It is invisible to the reaper's
        // "final name implies live lock" rule because of the staging suffix, and the age gate
        // protects it in the microseconds before the rename below.
        let lock_path = staging_path.join(OWNER_LOCK_FILE_NAME);
        let lock_file = File::create(&lock_path).unwrap_or_else(|err| {
            panic!(
                "test_scratch: cannot create owner lock {}: {err}",
                lock_path.display()
            )
        });
        if let Err(err) = lock_file.try_lock() {
            panic!(
                "test_scratch: cannot take the owner lock on {}, which this call just \
                 created: {err:?}",
                lock_path.display()
            );
        }

        // Deliberately leak the descriptor. `flock(2)` is released when the last descriptor
        // referring to its open file description is closed, and the kernel closes every
        // descriptor on process termination *by any means* — including `SIGKILL` and
        // `SIGSEGV`, where no user code of ours ever runs. Leaking it therefore makes the
        // kernel the releaser, which is the whole reason layer 2 can prove liveness.
        std::mem::forget(lock_file);

        let final_path = base.join(&final_name);
        fs::rename(&staging_path, &final_path).unwrap_or_else(|err| {
            panic!(
                "test_scratch: cannot rename {} into place at {}: {err}",
                staging_path.display(),
                final_path.display()
            )
        });

        register_for_exit_removal(&final_path);
        return ProcessScratchDir { path: final_path };
    }

    panic!(
        "test_scratch: could not find a free scratch directory name under {} after {} \
         attempts (last collision: {:?})",
        base.display(),
        MAX_CREATE_ATTEMPTS,
        last_collision
    );
}

/// [`acquire_in`] against [`base_dir`].
pub fn acquire(prefix: &str) -> ProcessScratchDir { acquire_in(&base_dir(), prefix) }

#[cfg(test)]
mod tests {
    use super::*;

    /// The name a directory ends up with must not carry the staging suffix, and the owner lock
    /// must be present and held.
    #[test]
    fn acquired_directory_is_final_named_and_locked() {
        let base = tempfile::tempdir().expect("base");
        let dir = acquire_in(base.path(), "unit-final-");

        let name = dir
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("name");
        assert!(
            !name.ends_with(STAGING_SUFFIX),
            "acquire must rename out of the staging namespace, got {name}"
        );
        assert!(dir.path().join(OWNER_LOCK_FILE_NAME).exists());

        // A second open file description must not be able to take the lock.
        let probe = File::open(dir.path().join(OWNER_LOCK_FILE_NAME)).expect("open lock");
        assert!(
            matches!(probe.try_lock(), Err(TryLockError::WouldBlock)),
            "the owner lock must still be held by this process"
        );
    }

    /// Distinct acquisitions must not collide, even back to back.
    #[test]
    fn acquisitions_are_distinct() {
        let base = tempfile::tempdir().expect("base");
        let first = acquire_in(base.path(), "unit-distinct-");
        let second = acquire_in(base.path(), "unit-distinct-");
        assert_ne!(first.path(), second.path());
    }
}
