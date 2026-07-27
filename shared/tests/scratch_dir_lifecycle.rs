//! Behavioural guards for [`shared::rust::test_scratch`] — G3.
//!
//! Every case runs in its **own private base directory**, owned by a [`tempfile::TempDir`]
//! that this test genuinely drops. **No case scans `/tmp`.** Scanning a shared base is racy
//! under parallel execution and this codebase has been bitten repeatedly by checks that could
//! not fail; a private base makes each assertion about a world this test fully controls.
//!
//! The cases, and the piece of the design each one pins:
//!
//! | case | pins |
//! |------|------|
//! | [`reaps_an_orphan_whose_owner_lock_is_unlocked`] | the reaper removes a directory whose owner is provably gone |
//! | [`reaping_is_confined_to_the_requested_prefix`] | the prefix filter filters, so case 1 cannot pass by removing everything |
//! | [`does_not_reap_a_live_directory_whose_lock_is_held`] | **the safety property** — a held `flock` is never overridden |
//! | [`sigkill_releases_the_owner_lock_across_processes`] | ★ **the property the whole design rests on**: the kernel closes descriptors on `SIGKILL`, so the lock is released with no cooperation from the dying process |
//! | [`eager_layer_removes_scratch_dir_on_normal_exit`] | layer 1 — a reintroduced never-dropped `TempDir` fails this |
//! | [`incoming_staging_is_spared_while_young_and_reaped_when_old`] | the `rename(2)` creation window and the age gate |
//! | [`young_staging_with_an_unlocked_lock_file_is_never_reaped`] | ★ the staging window itself — a real, observed production failure |
//! | [`concurrent_acquisition_and_reaping_never_collide`] | a live process's own directories survive its own reaper, under load |
//! | [`lockless_leftovers_are_spared_while_young_and_reaped_when_old`] | the age gate on pre-fix leftovers |
//! | [`age_never_overrides_a_held_lock`] | an mtime is not evidence about a process; a lock is |
//!
//! The two cross-process cases spawn **this very test binary** again, selecting the single
//! entry point [`eager_layer_removes_scratch_dir_on_normal_exit`] with libtest's `--exact`,
//! and switch it into a child role with [`CHILD_ROLE_ENV`]. That is deliberate: the `SIGKILL`
//! property is about a real process being destroyed by the kernel, and asserting it from
//! documentation instead of from a corpse would be exactly the kind of check that cannot fail.

use std::fs::{self, File, FileTimes, TryLockError};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, SystemTime};

use shared::rust::test_scratch::{
    acquire, reap_orphans_in, ORPHAN_MIN_AGE, OWNER_LOCK_FILE_NAME, SCRATCH_BASE_DIR_ENV,
    STAGING_SUFFIX,
};

/// Selects the child role when this binary re-executes itself. Absent in an ordinary run.
const CHILD_ROLE_ENV: &str = "F1R3FLY_SCRATCH_TEST_CHILD_ROLE";

/// Prefix the child should acquire under, so the parent knows what to reap.
const CHILD_PREFIX_ENV: &str = "F1R3FLY_SCRATCH_TEST_CHILD_PREFIX";

/// Line prefix the child writes its acquired path on, for the parent to parse.
const CHILD_PATH_MARKER: &str = "SCRATCH_PATH=";

/// libtest name of the single test the child is asked to run. Kept in one place so a rename
/// cannot silently break the cross-process cases into no-ops.
const CHILD_ENTRY_POINT: &str = "eager_layer_removes_scratch_dir_on_normal_exit";

/// Upper bound on how long a `hold` child parks before exiting on its own. Purely a safety
/// net against a parent that dies before killing it; far longer than any assertion window.
const CHILD_HOLD_TIMEOUT: Duration = Duration::from_secs(120);

// ---------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------

/// Build a scratch-shaped directory containing an owner lock that **nobody holds**.
fn make_dir_with_unlocked_owner_lock(base: &Path, name: &str) -> PathBuf {
    let path = base.join(name);
    fs::create_dir_all(&path).expect("create candidate directory");
    File::create(path.join(OWNER_LOCK_FILE_NAME)).expect("create owner lock");
    // Payload, so the assertions are about removing a *tree*, not an empty directory.
    fs::create_dir_all(path.join("rspace")).expect("create payload subdirectory");
    fs::write(path.join("rspace").join("data.mdb"), b"payload").expect("write payload");
    path
}

/// Build a scratch-shaped directory with **no** owner lock at all — the shape a pre-fix
/// binary leaves behind, and the shape the age gate exists for.
fn make_dir_without_owner_lock(base: &Path, name: &str) -> PathBuf {
    let path = base.join(name);
    fs::create_dir_all(&path).expect("create candidate directory");
    fs::write(path.join("data.mdb"), b"payload").expect("write payload");
    path
}

/// Push a directory's mtime `by` into the past, so the age gate sees it as old.
fn age_directory(path: &Path, by: Duration) {
    let when = SystemTime::now()
        .checked_sub(by)
        .expect("clock far enough from the epoch");
    let handle = File::open(path).expect("open directory for set_times");
    handle
        .set_times(FileTimes::new().set_accessed(when).set_modified(when))
        .expect("set directory times");
}

// ---------------------------------------------------------------------------------------
// 1. The reaper removes a directory whose owner is provably gone
// ---------------------------------------------------------------------------------------

#[test]
fn reaps_an_orphan_whose_owner_lock_is_unlocked() {
    let base = tempfile::tempdir().expect("private base");
    let prefix = "guard-orphan-";
    let orphan = make_dir_with_unlocked_owner_lock(base.path(), &format!("{prefix}dead-owner"));

    let report = reap_orphans_in(base.path(), prefix);

    assert_eq!(
        report.removed, 1,
        "an unlocked owner lock proves the owner is gone; report was {report:?}"
    );
    assert_eq!(report.skipped_live, 0, "report was {report:?}");
    assert_eq!(report.skipped_young, 0, "report was {report:?}");
    assert!(
        !orphan.exists(),
        "the orphan tree must be gone, but {} still exists",
        orphan.display()
    );
}

/// The prefix filter must actually filter: an unrelated directory in the same base is not
/// collateral damage. Without this, case 1 would pass even if the reaper removed everything.
#[test]
fn reaping_is_confined_to_the_requested_prefix() {
    let base = tempfile::tempdir().expect("private base");
    let mine = make_dir_with_unlocked_owner_lock(base.path(), "guard-confined-dead");
    let theirs = make_dir_with_unlocked_owner_lock(base.path(), "somebody-elses-directory");

    let report = reap_orphans_in(base.path(), "guard-confined-");

    assert_eq!(report.removed, 1, "report was {report:?}");
    assert!(!mine.exists());
    assert!(
        theirs.exists(),
        "the reaper must not touch names outside its prefix"
    );
}

// ---------------------------------------------------------------------------------------
// 2. THE SAFETY PROPERTY — a held lock is never overridden
// ---------------------------------------------------------------------------------------

/// If this ever fails, the reaper has been weakened into something that can delete a live
/// process's storage out from under it. It must fail loudly.
#[test]
fn does_not_reap_a_live_directory_whose_lock_is_held() {
    let base = tempfile::tempdir().expect("private base");
    let prefix = "guard-live-";
    let live = make_dir_with_unlocked_owner_lock(base.path(), &format!("{prefix}live-owner"));

    // Stand in for the owning process: hold the exclusive lock for the whole case. `flock` is
    // held by the open file description, so the reaper's own `open` of the same path is a
    // different description and must be refused.
    let owner_handle = File::open(live.join(OWNER_LOCK_FILE_NAME)).expect("open owner lock");
    owner_handle.try_lock().expect("take the owner lock");

    let report = reap_orphans_in(base.path(), prefix);

    assert_eq!(
        report.skipped_live, 1,
        "a held flock must be reported as a live owner; report was {report:?}"
    );
    assert_eq!(
        report.removed, 0,
        "nothing may be removed while its lock is held; report was {report:?}"
    );
    assert!(
        live.join("rspace").join("data.mdb").exists(),
        "the live tree must be untouched, payload and all"
    );

    // And once the owner lets go, the very same directory becomes reapable — proving the
    // skip above was caused by the lock and not by some unrelated filter.
    owner_handle.unlock().expect("release the owner lock");
    drop(owner_handle);

    let report = reap_orphans_in(base.path(), prefix);
    assert_eq!(
        report.removed, 1,
        "releasing the lock must make the directory reapable; report was {report:?}"
    );
    assert!(!live.exists());
}

// ---------------------------------------------------------------------------------------
// 3. ★ SIGKILL RELEASES THE LOCK, CROSS-PROCESS
// ---------------------------------------------------------------------------------------

/// Kills the child on drop, so a panicking assertion cannot leave a parked process behind.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Spawn this test binary again, running only [`CHILD_ENTRY_POINT`], in the given role.
///
/// The base directory is handed over through [`SCRATCH_BASE_DIR_ENV`], which exercises the
/// documented environment override end to end: the child calls plain `acquire`, and the parent
/// asserts the result landed inside the base it chose.
fn spawn_child(role: &str, base: &Path, prefix: &str) -> Child {
    let exe = std::env::current_exe().expect("current test binary");
    Command::new(exe)
        .args([
            "--exact",
            CHILD_ENTRY_POINT,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ROLE_ENV, role)
        .env(CHILD_PREFIX_ENV, prefix)
        .env(SCRATCH_BASE_DIR_ENV, base)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn child test process")
}

/// Take ownership of the child's stdout.
///
/// The returned reader must stay alive until after the child has exited. Dropping it closes
/// the read end of the pipe, and libtest writes its `ok` / `test result:` epilogue *after* the
/// test body returns — into a pipe with no reader, which is `EPIPE`, which panics the child
/// and turns a passing case into a spurious exit-101 failure.
fn child_stdout(child: &mut Child) -> BufReader<ChildStdout> {
    BufReader::new(child.stdout.take().expect("child stdout is piped"))
}

/// Read the child's stdout until it announces its acquired path.
///
/// The marker is searched for *within* the line rather than at its start: under `--nocapture`
/// libtest writes `test <name> ... ` with no trailing newline before handing stdout to the
/// test, so the child's first line arrives with that banner glued to its front.
fn read_child_scratch_path(reader: &mut BufReader<ChildStdout>) -> PathBuf {
    let mut transcript = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).expect("read child stdout") == 0 {
            break;
        }
        if let Some(offset) = line.find(CHILD_PATH_MARKER) {
            return PathBuf::from(line[offset + CHILD_PATH_MARKER.len()..].trim());
        }
        transcript.push_str(&line);
    }
    panic!("child never announced its scratch path. Transcript:\n{transcript}");
}

/// Whatever the child wrote to stderr, for failure messages. Read only after the child has
/// exited, so it cannot block.
fn child_stderr(child: &mut Child) -> String {
    let mut text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut text);
    }
    text
}

/// ★ **The single property the whole design rests on.**
///
/// `atexit` cannot help a process that is destroyed without running user code. What makes
/// layer 2 sound is that an `flock` lives on the *open file description*, and the kernel
/// closes every descriptor when a task dies **by any means**, `SIGKILL` included. This case
/// proves that against a real corpse:
///
/// 1. a child acquires a scratch directory and parks, holding its owner lock;
/// 2. the parent's reaper sees `WouldBlock` and skips it — the directory survives;
/// 3. the parent `SIGKILL`s the child and reaps its exit status;
/// 4. the parent's reaper now takes the lock and removes the directory.
///
/// Step 2 is what makes step 4 meaningful: without it the case would pass even if the reaper
/// ignored locks entirely.
#[test]
fn sigkill_releases_the_owner_lock_across_processes() {
    let base = tempfile::tempdir().expect("private base");
    let prefix = "guard-sigkill-";

    let mut child = KillOnDrop(spawn_child("hold", base.path(), prefix));
    let mut transcript = child_stdout(&mut child.0);
    let child_path = read_child_scratch_path(&mut transcript);

    assert!(
        child_path.starts_with(base.path()),
        "the child must have honoured {SCRATCH_BASE_DIR_ENV}: {} is not under {}",
        child_path.display(),
        base.path().display()
    );
    assert!(child_path.is_dir(), "the child's directory must exist");
    assert!(
        child_path.join(OWNER_LOCK_FILE_NAME).is_file(),
        "the child's directory must carry an owner lock"
    );

    // (2) Alive: the lock is held by a live process, so the reaper must refuse.
    let report = reap_orphans_in(base.path(), prefix);
    assert_eq!(
        report.skipped_live, 1,
        "a live child's directory must be skipped; report was {report:?}"
    );
    assert_eq!(report.removed, 0, "report was {report:?}");
    assert!(
        child_path.is_dir(),
        "the live child's directory must survive the reaper"
    );

    // (3) Destroy it with no chance to clean up after itself.
    child.0.kill().expect("SIGKILL the child");
    let status = child.0.wait().expect("reap the child");
    assert!(
        !status.success(),
        "a SIGKILLed process must not report success"
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            status.signal(),
            Some(libc_sigkill()),
            "the child must have died from SIGKILL, not from a cooperative shutdown"
        );
    }

    // The corpse ran no code of ours. Only the kernel released that lock.
    drop(transcript);
    let report = reap_orphans_in(base.path(), prefix);
    assert_eq!(
        report.removed, 1,
        "SIGKILL must release the owner lock and make the directory reapable; report was \
         {report:?}"
    );
    assert_eq!(report.skipped_live, 0, "report was {report:?}");
    assert!(
        !child_path.exists(),
        "the dead child's directory must be gone, but {} still exists",
        child_path.display()
    );
}

/// `SIGKILL`'s signal number. Spelled out rather than pulled from a dependency so this test
/// crate needs nothing beyond `std` and `tempfile`.
#[cfg(unix)]
fn libc_sigkill() -> i32 { 9 }

// ---------------------------------------------------------------------------------------
// 4. Layer 1 — the eager `atexit` removal (also the child entry point)
// ---------------------------------------------------------------------------------------

/// Layer 1: a process that exits normally leaves nothing behind.
///
/// **A reintroduced never-dropped `TempDir` fails this case.** That is the direct answer to
/// "what breaks if someone puts the old `lazy_static!` back": the child would exit, its
/// `TempDir` would never be dropped, and the directory would still be on disk here.
///
/// This function doubles as the entry point the cross-process cases re-execute. In an
/// ordinary run [`CHILD_ROLE_ENV`] is unset and it takes the parent branch below; when the
/// parent spawns it with `--exact` and a role, it takes the child branch instead. There is no
/// third path, so the function is never a no-op in either role.
#[test]
fn eager_layer_removes_scratch_dir_on_normal_exit() {
    if let Ok(role) = std::env::var(CHILD_ROLE_ENV) {
        run_child_role(&role);
        return;
    }

    let base = tempfile::tempdir().expect("private base");
    let prefix = "guard-eager-";

    let mut child = KillOnDrop(spawn_child("exit", base.path(), prefix));
    let mut transcript = child_stdout(&mut child.0);
    let child_path = read_child_scratch_path(&mut transcript);

    assert!(
        child_path.starts_with(base.path()),
        "the child must have honoured {SCRATCH_BASE_DIR_ENV}"
    );

    // `transcript` is deliberately still alive: the child's libtest epilogue is written after
    // the test body returns and must have somewhere to go.
    let status = child.0.wait().expect("await the child");
    let stderr = child_stderr(&mut child.0);
    drop(transcript);
    assert!(
        status.success(),
        "the child must exit cleanly; it exited with {status:?}. stderr:\n{stderr}"
    );

    assert!(
        !child_path.exists(),
        "the atexit hook must remove {} on normal exit — it is still there, which is exactly \
         what a never-dropped TempDir looks like",
        child_path.display()
    );

    // And nothing else was left in the base either.
    let leftovers: Vec<_> = fs::read_dir(base.path())
        .expect("read base")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(prefix))
        .collect();
    assert!(
        leftovers.is_empty(),
        "normal exit must leave no scratch directories behind, found {leftovers:?}"
    );
}

/// The child half of the cross-process cases.
///
/// `acquire` (not `acquire_in`) on purpose: the parent hands the base over through
/// [`SCRATCH_BASE_DIR_ENV`], so this exercises the documented override rather than a test-only
/// back door.
fn run_child_role(role: &str) {
    let prefix = std::env::var(CHILD_PREFIX_ENV).expect("child prefix");
    let scratch = acquire(&prefix);

    // Announce, then flush: the parent is blocked reading this line. The leading newline
    // separates the marker from libtest's `test <name> ... ` banner in the transcript.
    println!("\n{CHILD_PATH_MARKER}{}", scratch.path().display());
    use std::io::Write;
    std::io::stdout().flush().expect("flush child stdout");

    match role {
        // Park, holding the owner lock, until the parent kills us. The bounded wait is a
        // safety net for a parent that dies first; it is far beyond any assertion window.
        "hold" => {
            std::thread::sleep(CHILD_HOLD_TIMEOUT);
        }
        // Return, so libtest finishes and the process exits normally, firing the atexit hook.
        "exit" => {}
        other => panic!("unknown child role {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------
// 5. The `rename(2)` creation window and the age gate
// ---------------------------------------------------------------------------------------

/// A `.incoming` directory is what exists for the microseconds between `mkdir` and the
/// `rename` that publishes it. It has no usable lock yet, so only the age gate protects it —
/// and a live one is milliseconds old, never hours.
#[test]
fn incoming_staging_is_spared_while_young_and_reaped_when_old() {
    let base = tempfile::tempdir().expect("private base");
    let prefix = "guard-incoming-";
    let staging =
        make_dir_without_owner_lock(base.path(), &format!("{prefix}4242-cafe-0{STAGING_SUFFIX}"));

    let report = reap_orphans_in(base.path(), prefix);
    assert_eq!(
        report.skipped_young, 1,
        "a fresh staging directory must be spared; report was {report:?}"
    );
    assert_eq!(report.removed, 0, "report was {report:?}");
    assert!(staging.is_dir(), "the staging directory must survive");

    age_directory(&staging, ORPHAN_MIN_AGE + Duration::from_secs(3600));

    let report = reap_orphans_in(base.path(), prefix);
    assert_eq!(
        report.removed, 1,
        "a staging directory older than ORPHAN_MIN_AGE must be reaped; report was {report:?}"
    );
    assert_eq!(report.skipped_young, 0, "report was {report:?}");
    assert!(!staging.exists());
}

/// ★ The exact shape of a real, observed production failure.
///
/// `acquire_in` creates the staging directory, then creates `.owner.lock`, then locks it.
/// **Between the last two steps the lock file exists and is not yet held.** A reaper that
/// consults `flock` in that window takes the lock, concludes the owner is dead, and deletes a
/// directory that is being born — after which `acquire_in`'s `rename` fails with `ENOENT`.
///
/// This is not hypothetical. The first version of the reaper applied one uniform rule to both
/// namespaces and two casper tests died on it within seconds of each other:
///
/// ```text
/// test_scratch: cannot rename .../casper-shared-lmdb-<pid>-<stamp>.incoming into place
///               at .../casper-shared-lmdb-<pid>-<stamp>: No such file or directory
/// ```
///
/// The earlier staging case above cannot catch this, because it builds a staging directory
/// with *no* lock file at all — which the age gate spares for a different reason. Only an
/// **unlocked but present** lock file exercises the window.
#[test]
fn young_staging_with_an_unlocked_lock_file_is_never_reaped() {
    let base = tempfile::tempdir().expect("private base");
    let prefix = "guard-staging-window-";
    let staging = make_dir_with_unlocked_owner_lock(
        base.path(),
        &format!("{prefix}4242-cafe-0{STAGING_SUFFIX}"),
    );

    // The lock is takeable, exactly as it is mid-`acquire_in`.
    {
        let probe = File::open(staging.join(OWNER_LOCK_FILE_NAME)).expect("open owner lock");
        probe
            .try_lock()
            .expect("the lock must be free in this window");
        probe.unlock().expect("release");
    }

    let report = reap_orphans_in(base.path(), prefix);

    assert_eq!(
        report.skipped_young, 1,
        "a staging directory must be judged by age, never by flock; report was {report:?}"
    );
    assert_eq!(
        report.removed, 0,
        "reaping a directory mid-creation makes its rename fail with ENOENT; report was \
         {report:?}"
    );
    assert!(
        staging.is_dir(),
        "the half-built directory must survive the reaper"
    );
}

/// Many acquisitions racing a reaper that never stops scanning.
///
/// What this pins: **a live process's own directories survive its own concurrent reaper.**
/// That rests on `flock` being held per *open file description*, so the reaper's fresh `open`
/// of a lock this same process already holds still returns `WouldBlock`. The single-threaded
/// cases cannot show that, because they never have 200 live locks in flight at once.
///
/// What this does **not** pin, stated plainly: the staging-window race above. Measured — with
/// the `.incoming` rule reverted, this case still passes, because the window between
/// `File::create` and `try_lock` is too narrow to land on reliably from another thread. The
/// deterministic case [`young_staging_with_an_unlocked_lock_file_is_never_reaped`] is what
/// actually guards that, and it does fail on reversion. This one is a stress test, and
/// claiming more for it would be exactly the kind of check that cannot fail.
#[test]
fn concurrent_acquisition_and_reaping_never_collide() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let base = tempfile::tempdir().expect("private base");
    let prefix = "guard-concurrent-";
    let base_path = base.path().to_path_buf();
    let reaping = Arc::new(AtomicBool::new(true));

    let reaper = {
        let base_path = base_path.clone();
        let reaping = Arc::clone(&reaping);
        std::thread::spawn(move || {
            let mut scans = 0usize;
            while reaping.load(Ordering::Relaxed) {
                reap_orphans_in(&base_path, "guard-concurrent-");
                scans += 1;
            }
            scans
        })
    };

    let acquirers: Vec<_> = (0..4)
        .map(|_| {
            let base_path = base_path.clone();
            std::thread::spawn(move || {
                for _ in 0..50 {
                    // Panics if the reaper removed this directory mid-creation.
                    let scratch = shared::rust::test_scratch::acquire_in(&base_path, prefix);
                    assert!(
                        scratch.path().is_dir(),
                        "acquired directory vanished: {}",
                        scratch.path().display()
                    );
                }
            })
        })
        .collect();

    for acquirer in acquirers {
        acquirer.join().expect("acquirer thread");
    }
    reaping.store(false, Ordering::Relaxed);
    let scans = reaper.join().expect("reaper thread");

    assert!(
        scans > 0,
        "the reaper thread never scanned, so this case proved nothing"
    );

    // Every acquisition is still there: this process holds all 200 locks.
    let survivors = fs::read_dir(base.path())
        .expect("read base")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .count();
    assert_eq!(
        survivors, 200,
        "a live process's own directories must survive its own reaper"
    );
}

/// The same gate applies to a final-named directory with no lock file at all — the shape a
/// pre-fix binary left behind. Six hours is the only thing standing between it and removal,
/// which is why the safety argument must not depend on it.
#[test]
fn lockless_leftovers_are_spared_while_young_and_reaped_when_old() {
    let base = tempfile::tempdir().expect("private base");
    let prefix = "guard-legacy-";
    let legacy = make_dir_without_owner_lock(base.path(), &format!("{prefix}aBcDeF"));

    let report = reap_orphans_in(base.path(), prefix);
    assert_eq!(report.skipped_young, 1, "report was {report:?}");
    assert!(legacy.is_dir());

    age_directory(&legacy, ORPHAN_MIN_AGE + Duration::from_secs(1));

    let report = reap_orphans_in(base.path(), prefix);
    assert_eq!(report.removed, 1, "report was {report:?}");
    assert!(!legacy.exists());
}

/// The age gate must never override a held lock, no matter how old the directory looks. An
/// mtime is not evidence about a process; a lock is.
#[test]
fn age_never_overrides_a_held_lock() {
    let base = tempfile::tempdir().expect("private base");
    let prefix = "guard-old-live-";
    let live = make_dir_with_unlocked_owner_lock(base.path(), &format!("{prefix}ancient"));

    let owner_handle = File::open(live.join(OWNER_LOCK_FILE_NAME)).expect("open owner lock");
    owner_handle.try_lock().expect("take the owner lock");
    age_directory(&live, ORPHAN_MIN_AGE * 10);

    let report = reap_orphans_in(base.path(), prefix);

    assert_eq!(
        report.skipped_live, 1,
        "an ancient mtime must not defeat a held lock; report was {report:?}"
    );
    assert_eq!(report.removed, 0, "report was {report:?}");
    assert!(live.is_dir());

    assert!(matches!(
        File::open(live.join(OWNER_LOCK_FILE_NAME))
            .expect("reopen owner lock")
            .try_lock(),
        Err(TryLockError::WouldBlock)
    ));
}
