//! File-I/O fd table lifecycle: sharing across handlers, and
//! rollback around the soft-checkpoint boundary.
//!
//! Two invariants covered here that the primitive-layer unit
//! tests (in `interpreter::io::handle_table`) can't reach on
//! their own:
//!
//! 1. **Cross-handler sharing.** `nativeOpen` and every
//!    fd-consuming primitive (read/write/seek/close/...) must
//!    all see the SAME `FileHandleTable`. Each `Definition`'s
//!    handler closure closes over its own `ProcessContext`, so
//!    if the table isn't threaded in as a shared value, each
//!    Definition lands with `FileHandleTable::new()` and the
//!    entire native primitive layer is non-functional through
//!    the runtime: `nativeOpen` returns `fd=N` but every
//!    subsequent `native*(N, ...)` looks up in a different
//!    (empty) table and returns `FSERR_CLOSED`.
//!
//! 2. **Deploy-scope rollback.** `evaluate_with_env_and_phlo`
//!    wraps every deploy in a soft-checkpoint boundary that
//!    reverts rspace state on error. The `FileHandleTable`
//!    must participate in the same boundary: fds allocated by
//!    a deploy that later errors must be dropped, otherwise
//!    the OS-level `tokio::fs::File` objects sit in the table
//!    unreachable from Rholang (fds are monotonic, and any
//!    produce event that mentioned the reverted fd was rolled
//!    back by the rspace restore) but consuming an OS file
//!    descriptor until the runtime drops. A hostile deploy
//!    that repeatedly opens-then-errors could exhaust the
//!    process's `ulimit -n`.

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

#[allow(clippy::type_complexity)]
async fn mk_runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (runtime, _, _): (
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
    runtime
}

fn temp_path(tag: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "fileio_lifecycle_{tag}_{pid}_{ts}",
        pid = std::process::id(),
        ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    p.to_string_lossy().into_owned()
}

fn open_read_env() -> HashMap<String, Par> {
    let mut env: HashMap<String, Par> = HashMap::new();
    env.insert(
        "rho:io:fs:native:1.0.0/open".to_string(),
        FixedChannels::native_open(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/read".to_string(),
        FixedChannels::native_read(),
    );
    env
}

/// Open a file, then immediately read from the returned fd.
/// Regression guard for the cross-handler sharing invariant: if
/// each Definition's handler had its own `FileHandleTable::new()`
/// (which is what the old `SystemProcesses::create` did before
/// the shared-table plumbing), `nativeRead(1)` would see an empty
/// table and return `[false, FSERR_CLOSED, "fd 1 is not open"]`
/// instead of `[true, <bytes>]`. Verified by grepping for the
/// success-tuple shape on the sink channel.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn open_then_read_shares_fd_table() {
    let path = temp_path("open_read");
    std::fs::write(&path, b"hello").expect("precondition write");

    let env = open_read_env();
    let term = format!(
        r#"new nOpen(`rho:io:fs:native:1.0.0/open`),
                nRead(`rho:io:fs:native:1.0.0/read`),
                openAck, readAck in {{
             nOpen!(*openAck, "{path}", "r") |
             for (@[true, fd] <- openAck) {{
               nRead!(*readAck, fd, 5) |
               for (@result <- readAck) {{ @"sink"!(result) }}
             }}
           }}"#
    );

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "lifecycle-sharing".to_string());
    let result = runtime
        .evaluate(&term, phlo, env, rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink_channel = Par::default().with_exprs(vec![models::rhoapi::Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::GString(
            "sink".to_string(),
        )),
    }]);
    let data = runtime.get_data(&sink_channel).await;
    let joined: String = data
        .into_iter()
        .flat_map(|d| d.a.pars)
        .map(|p| format!("{p:?}"))
        .collect::<Vec<_>>()
        .join(" | ");

    let _ = std::fs::remove_file(&path);

    // nativeRead returns the ASCII bytes of "hello" as a
    // `GByteArray([104, 101, 108, 108, 111])`. Verify both the
    // success-tuple shape and the expected byte content.
    assert!(
        joined.contains("GBool(true)"),
        "expected [true, ...] success tuple on sink, got: {joined}"
    );
    assert!(
        joined.contains("GByteArray([104, 101, 108, 108, 111])"),
        "expected the ASCII bytes of \"hello\" on sink, got: {joined}"
    );
}

/// Deploy successfully allocates two fds; verify the table holds
/// them and `next_fd` advanced. Baseline for the rollback test
/// that follows.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn successful_deploy_leaves_fds_in_table() {
    let path_a = temp_path("commit_a");
    let path_b = temp_path("commit_b");
    std::fs::write(&path_a, b"aaa").expect("precondition a");
    std::fs::write(&path_b, b"bbb").expect("precondition b");

    let mut env: HashMap<String, Par> = HashMap::new();
    env.insert(
        "rho:io:fs:native:1.0.0/open".to_string(),
        FixedChannels::native_open(),
    );

    let term = format!(
        r#"new nOpen(`rho:io:fs:native:1.0.0/open`), ack1, ack2 in {{
             nOpen!(*ack1, "{path_a}", "r") |
             for (@_ <- ack1) {{
               nOpen!(*ack2, "{path_b}", "r") |
               for (@_ <- ack2) {{ Nil }}
             }}
           }}"#
    );

    let mut runtime = mk_runtime().await;
    let fd_snapshot_before = runtime.file_handles.snapshot_next_fd();
    let phlo = Cost::create(i64::MAX, "lifecycle-commit".to_string());
    let result = runtime
        .evaluate_with_env_and_phlo(&term, phlo, env)
        .await
        .expect("evaluate_with_env_and_phlo");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let fd_snapshot_after = runtime.file_handles.snapshot_next_fd();
    assert_eq!(
        fd_snapshot_after,
        fd_snapshot_before + 2,
        "successful deploy should advance next_fd by 2 (two nativeOpens)"
    );

    let _ = std::fs::remove_file(&path_a);
    let _ = std::fs::remove_file(&path_b);
}

/// Deploy calls nativeOpen (successfully) and then triggers a
/// deploy-level error via `1 / 0`. `evaluate_with_env_and_phlo`
/// reverts rspace to the soft checkpoint AND truncates the
/// FileHandleTable back to the pre-deploy `next_fd`, so the
/// opened fd is dropped. Without the M1 fix, the fd would linger
/// in the table (its OS-level `tokio::fs::File` still open) with
/// no Rholang-reachable way to close it -- a resource leak that
/// a hostile deploy could weaponize into fd exhaustion.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn erroring_deploy_rolls_back_fd_allocations() {
    let path = temp_path("rollback");
    std::fs::write(&path, b"oops").expect("precondition");

    let mut env: HashMap<String, Par> = HashMap::new();
    env.insert(
        "rho:io:fs:native:1.0.0/open".to_string(),
        FixedChannels::native_open(),
    );

    // Open the file, then divide by zero. The div-by-zero fires
    // AFTER nativeOpen has succeeded and inserted the fd; the
    // resulting eval-level error causes evaluate_with_env_and_phlo
    // to revert rspace + truncate the fd table.
    let term = format!(
        r#"new nOpen(`rho:io:fs:native:1.0.0/open`), ack in {{
             nOpen!(*ack, "{path}", "r") |
             for (@[true, _] <- ack) {{ @0!(1 / 0) }}
           }}"#
    );

    let mut runtime = mk_runtime().await;
    let fd_snapshot_before = runtime.file_handles.snapshot_next_fd();
    let phlo = Cost::create(i64::MAX, "lifecycle-rollback".to_string());
    let result = runtime
        .evaluate_with_env_and_phlo(&term, phlo, env)
        .await
        .expect("evaluate_with_env_and_phlo (produces errors, but doesn't hard-fail)");
    assert!(
        !result.errors.is_empty(),
        "deploy should have errored (div-by-zero); got no errors"
    );

    let fd_snapshot_after = runtime.file_handles.snapshot_next_fd();
    assert_eq!(
        fd_snapshot_after,
        fd_snapshot_before,
        "erroring deploy should truncate next_fd back to pre-deploy snapshot; \
         table has {} entries before revert-check",
        fd_snapshot_after - fd_snapshot_before
    );

    let _ = std::fs::remove_file(&path);
}
