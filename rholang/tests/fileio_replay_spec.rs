//! Non-deterministic-replay tests for the File I/O native primitives.
//!
//! Every fileio native handler wraps its syscall in an
//! `if is_replay { produce(&previous_output, ack).await?; return
//! Ok(previous_output); }` guard so followers replay the lead
//! node's captured output rather than re-running the syscall.
//! Without the guard, a follower whose disk state has drifted
//! from the lead's would observe different bytes / entries /
//! metadata / existence, and consensus would break.
//!
//! Each test below:
//!   1. Plays a Rholang term that invokes a fileio native URN
//!      against a specific disk state (e.g. a file with known
//!      contents).
//!   2. **Mutates the disk** between play and replay -- e.g.
//!      removes the file the test just read, or writes different
//!      bytes on top of it.
//!   3. Rigs a replay runtime with the play's event log and
//!      re-evaluates the same term.
//!   4. Verifies `check_replay_data` succeeds. If the guard were
//!      missing, the follower would call the syscall again and
//!      produce output derived from the *mutated* disk state --
//!      that output would not match the log's captured entry, and
//!      the rspace replay would flag the mismatch.
//!
//! The tests use `FixedChannels::native_*()` to build the
//! per-deploy `NormalizerEnv` in the same shape the FS-agent's
//! genesis deploy will (via `compile_fileio_genesis_source` in
//! `interpreter::io::injections`). Handing the tests the raw
//! bundle keeps them free of the `pub(crate)` visibility of the
//! wrapper helper.

use std::collections::HashMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::FixedChannels;
use rholang::rust::interpreter::test_utils::resources::create_runtimes_with_services;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

/// Build a `NormalizerEnv` containing only the fileio native URNs
/// the test needs. Mirrors what `compile_fileio_genesis_source`
/// does internally in the `interpreter::io::injections` module.
fn fileio_env(urns: &[(&str, Par)]) -> HashMap<String, Par> {
    urns.iter()
        .map(|(u, p)| ((*u).to_string(), p.clone()))
        .collect()
}

#[allow(clippy::type_complexity)]
async fn mk_pair() -> (RhoRuntimeImpl, RhoRuntimeImpl) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, replay_runtime, _): (
        RhoRuntimeImpl,
        RhoRuntimeImpl,
        Arc<
            Box<
                dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
                    + Send
                    + Sync
                    + 'static,
            >,
        >,
    ) = create_runtimes_with_services(store, false, &mut Vec::new(), ExternalServices::noop())
        .await;
    (runtime, replay_runtime)
}

/// Play + replay + `check_replay_data` in the standard shape.
/// Between the play evaluation and the rig, `mutate` runs so the
/// disk state visible to the replay handler differs from the play
/// handler. If any handler cheats -- runs the syscall instead of
/// consulting `previous_output` -- the replay's produce output
/// won't match the log entry recorded during play, and
/// `check_replay_data` fails.
async fn play_mutate_replay(term: &str, env: HashMap<String, Par>, mutate: impl FnOnce()) {
    let (mut runtime, mut replay_runtime) = mk_pair().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "fileio-replay-test".to_string());

    let play = runtime
        .evaluate(term, phlo.clone(), env.clone(), rand.clone())
        .await
        .expect("play should compile+evaluate");
    assert!(
        play.errors.is_empty(),
        "play produced errors: {:?}",
        play.errors
    );

    let checkpoint = runtime.create_checkpoint().await;

    mutate();

    replay_runtime
        .reset(&checkpoint.root)
        .await
        .expect("replay reset");
    replay_runtime
        .rig(checkpoint.log)
        .await
        .expect("replay rig");

    let replay = replay_runtime
        .evaluate(term, phlo, env, rand)
        .await
        .expect("replay should compile+evaluate");
    assert!(
        replay.errors.is_empty(),
        "replay produced errors (guard likely absent): {:?}",
        replay.errors
    );

    replay_runtime
        .check_replay_data()
        .await
        .expect("check_replay_data (unconsumed events => guard didn't fire)");

    // Same charged cost too -- non-deterministic ops cost the same on
    // both sides because the log-driven produce is bookkeeping-only.
    assert_eq!(
        play.cost.value, replay.cost.value,
        "play/replay cost divergence"
    );
}

/// Absolute path to a temp-file name unique to this test process
/// so parallel test runs don't stomp on each other.
fn temp_path(tag: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "fileio_replay_spec_{tag}_{pid}_{ts}",
        pid = std::process::id(),
        ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    p.to_string_lossy().into_owned()
}

/// Play `nativeExists(path)` where `path` does not exist -->
/// `[true, false]`. Between play and replay, CREATE the file.
/// A follower that reruns the syscall would produce `[true, true]`
/// and mismatch the log. The guard forces `[true, false]` on both.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exists_replays_false_after_file_is_created_between_runs() {
    let path = temp_path("exists_false");
    // Precondition: the path must not exist during play.
    let _ = std::fs::remove_file(&path);

    let env = fileio_env(&[(
        "rho:io:fs:native:1.0.0/exists",
        FixedChannels::native_exists(),
    )]);

    let path_for_term = path.clone();
    let path_for_mutate = path.clone();
    let term = format!(
        r#"new nExists(`rho:io:fs:native:1.0.0/exists`), ret in {{
             nExists!(*ret, "{path_for_term}") |
             for (@_ <- ret) {{ Nil }}
           }}"#
    );

    play_mutate_replay(&term, env, move || {
        std::fs::write(&path_for_mutate, b"appeared after play")
            .expect("create file between play and replay");
    })
    .await;

    // Cleanup.
    let _ = std::fs::remove_file(&path);
}

/// Play `nativeExists(path)` where the file exists --> `[true, true]`.
/// Between play and replay, DELETE the file. A follower rerunning the
/// syscall would produce `[true, false]`; the guard forces `[true, true]`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exists_replays_true_after_file_is_deleted_between_runs() {
    let path = temp_path("exists_true");
    std::fs::write(&path, b"present at play time").expect("precondition write");

    let env = fileio_env(&[(
        "rho:io:fs:native:1.0.0/exists",
        FixedChannels::native_exists(),
    )]);

    let path_for_term = path.clone();
    let path_for_mutate = path.clone();
    let term = format!(
        r#"new nExists(`rho:io:fs:native:1.0.0/exists`), ret in {{
             nExists!(*ret, "{path_for_term}") |
             for (@_ <- ret) {{ Nil }}
           }}"#
    );

    play_mutate_replay(&term, env, move || {
        std::fs::remove_file(&path_for_mutate).expect("remove file between play and replay");
    })
    .await;
}

/// Play `nativeStat(path)` on a file with known contents/mtime. Between
/// play and replay, overwrite the file (changing mtime AND size). A
/// follower rerunning the syscall would observe a stat record with the
/// new size/mtime; the guard forces the play's original record.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stat_replays_original_metadata_after_file_is_modified() {
    let path = temp_path("stat_meta");
    std::fs::write(&path, b"original").expect("precondition write");

    let env = fileio_env(&[("rho:io:fs:native:1.0.0/stat", FixedChannels::native_stat())]);

    let path_for_term = path.clone();
    let path_for_mutate = path.clone();
    let term = format!(
        r#"new nStat(`rho:io:fs:native:1.0.0/stat`), ret in {{
             nStat!(*ret, "{path_for_term}") |
             for (@_ <- ret) {{ Nil }}
           }}"#
    );

    play_mutate_replay(&term, env, move || {
        // Overwrite with different bytes so size changes; sleep first
        // so mtime is guaranteed to differ on filesystems with 1s
        // mtime resolution (HFS+ and some FAT variants).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&path_for_mutate, b"different contents entirely")
            .expect("mutate file between play and replay");
    })
    .await;

    let _ = std::fs::remove_file(&path);
}
