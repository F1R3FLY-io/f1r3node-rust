//! End-to-end tests for the `Dir` agent (Phase 2 of the File I/O
//! implementation).
//!
//! Same full-stack shape as `fileio_file_agent_spec.rs`, but for
//! the `Dir` class: agent-block sugar → dispatch → NormalizerEnv
//! injection → native handlers → replay guards. `Dir` differs
//! from `File` in that it stashes a canonical absolute PATH
//! (not an fd) in per-instance state, and each path-taking
//! method routes through `nativeQuarantine(root, relPath)`
//! before dispatching to the underlying native.
//!
//! Tests cover:
//! - `entries()` — lists the root; no quarantine step.
//! - `stat(relPath)` / `exists(relPath)` — quarantine + native.
//! - Quarantine failure surfaces as `FSERR_QUARANTINE` on the
//!   caller's ack channel (not a deploy-abort).
//! - Default arm replies `FSERR_UNSUPPORTED` per the reply idiom
//!   shared with `File`.

use std::collections::HashMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::io::agents::DIR_AGENT_SRC;
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

/// Create a fresh temp dir with a stable name for this test run.
/// Returns the canonicalized absolute path (Dir's constructor
/// requires the root to already be canonical, per
/// `nativeQuarantine`'s precondition).
fn temp_dir(tag: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "fileio_dir_agent_spec_{tag}_{pid}_{ts}",
        pid = std::process::id(),
        ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).expect("create temp dir");
    // Canonicalize so macOS /tmp -> /private/tmp translation
    // is applied; native_quarantine matches against the
    // canonical form.
    p.canonicalize().expect("canonicalize temp dir")
}

/// The four native URNs the `Dir` agent + test harness need.
fn dir_agent_env() -> HashMap<String, Par> {
    let mut env: HashMap<String, Par> = HashMap::new();
    env.insert(
        "rho:io:fs:native:1.0.0/quarantine".to_string(),
        FixedChannels::native_quarantine(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/entries".to_string(),
        FixedChannels::native_entries(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/stat".to_string(),
        FixedChannels::native_stat(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/exists".to_string(),
        FixedChannels::native_exists(),
    );
    env
}

/// Wrap `body` in the outer `new`-scope + inject the `Dir` agent
/// block. Constructs a Dir instance rooted at `root_path` and
/// binds `dirAgent` for the body to invoke.
fn wrap(body: &str, root_path: &str) -> String {
    format!(
        r#"new
     Dir,
     nQuarantine(`rho:io:fs:native:1.0.0/quarantine`),
     nEntries(`rho:io:fs:native:1.0.0/entries`),
     nStat(`rho:io:fs:native:1.0.0/stat`),
     nExists(`rho:io:fs:native:1.0.0/exists`)
   in {{
     {DIR_AGENT_SRC}
     |
     new dirRet in {{
       // Dir constructor: `Dir!(replyChan_as_process, rootPath)`.
       // rootPath is a String Process; auto-quoted on send,
       // unquoted back via `@rootPath` in the constructor formal.
       Dir!(*dirRet, "{root_path}") |
       for (dirAgent <- dirRet) {{
         {body}
       }}
     }}
   }}"#
    )
}

async fn observe_sink(runtime: &RhoRuntimeImpl) -> String {
    let sink_channel = Par::default().with_exprs(vec![models::rhoapi::Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::GString(
            "sink".to_string(),
        )),
    }]);
    let data = runtime.get_data(&sink_channel).await;
    data.into_iter()
        .flat_map(|d| d.a.pars)
        .map(|p| format!("{p:?}"))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// `entries()` on a directory with two files returns a list
/// containing both entries. Native sorts lexicographically, so
/// the order is deterministic.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_entries_lists_root_children() {
    let root = temp_dir("entries");
    std::fs::write(root.join("alpha.txt"), b"a").expect("write alpha");
    std::fs::write(root.join("beta.txt"), b"bb").expect("write beta");

    let body = r#"
        new entRet in {
          dirAgent!(*entRet, "entries") |
          for (@result <- entRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-entries".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        sink.contains("GBool(true)"),
        "expected success tuple on sink, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("alpha.txt")"#),
        "expected alpha.txt in entries, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("beta.txt")"#),
        "expected beta.txt in entries, got: {sink}"
    );
}

/// `stat(relPath)` on a present child returns a stat record.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_stat_reports_child_metadata() {
    let root = temp_dir("stat");
    std::fs::write(root.join("target.txt"), b"12345").expect("write target");

    let body = r#"
        new statRet in {
          dirAgent!(*statRet, "stat", "target.txt") |
          for (@result <- statRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-stat".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_dir_all(&root);

    // Expected: [true, {"name": "target.txt", "size": 5, "kind": "file", ...}]
    assert!(
        sink.contains("GBool(true)"),
        "expected success tuple on sink, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("target.txt")"#),
        "expected the file name in the stat record, got: {sink}"
    );
    assert!(
        sink.contains("GInt(5)"),
        "expected size=5 in the stat record, got: {sink}"
    );
}

/// `exists(relPath)` returns `[true, true]` when the child is
/// present.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_exists_true_for_present_child() {
    let root = temp_dir("exists_true");
    std::fs::write(root.join("here.txt"), b"").expect("touch here");

    let body = r#"
        new existRet in {
          dirAgent!(*existRet, "exists", "here.txt") |
          for (@result <- existRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-exists-true".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_dir_all(&root);

    // Expected: [true, true] -- both booleans on the sink.
    let true_count = sink.matches("GBool(true)").count();
    assert!(
        true_count >= 2,
        "expected two `true` booleans (success + exists), got: {sink}"
    );
}

/// `exists(relPath)` returns `[true, false]` when the child is
/// absent. Native `nativeExists` translates `NotFound` to
/// `Ok(false)`, so the outer tuple is still `[true, ...]`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_exists_false_for_absent_child() {
    let root = temp_dir("exists_false");

    let body = r#"
        new existRet in {
          dirAgent!(*existRet, "exists", "nope.txt") |
          for (@result <- existRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-exists-false".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_dir_all(&root);

    // Expected: [true, false] -- outer success (query succeeded),
    // inner false (file doesn't exist).
    assert!(
        sink.contains("GBool(true)") && sink.contains("GBool(false)"),
        "expected both true and false on sink, got: {sink}"
    );
}

/// A `..` escape attempt is rejected by `nativeQuarantine`, which
/// returns `FSERR_QUARANTINE`. The `Dir` agent forwards the
/// tuple verbatim; no `nativeStat`/`nativeExists` call happens.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_stat_rejects_parent_escape() {
    let root = temp_dir("escape");
    // Create a sibling file OUTSIDE the root to prove the escape
    // is denied even when the target exists.
    std::fs::write(root.parent().unwrap().join("outside.txt"), b"secret").expect("write outside");

    let body = r#"
        new statRet in {
          dirAgent!(*statRet, "stat", "../outside.txt") |
          for (@result <- statRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-escape".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(root.parent().unwrap().join("outside.txt"));
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        sink.contains("GBool(false)"),
        "expected error tuple on sink, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("FSERR_QUARANTINE")"#),
        "expected FSERR_QUARANTINE code on sink, got: {sink}"
    );
}

/// Unknown methods hit the default arm and get
/// `FSERR_UNSUPPORTED` with the method name in the payload.
/// Same reply idiom as `File.default`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_unknown_method_returns_unsupported() {
    let root = temp_dir("unknown");

    let body = r#"
        new unkRet in {
          dirAgent!(*unkRet, "nonexistent", "arg1") |
          for (@result <- unkRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-unknown".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        sink.contains("GBool(false)"),
        "expected error tuple on sink, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("FSERR_UNSUPPORTED")"#),
        "expected FSERR_UNSUPPORTED code on sink, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("nonexistent")"#),
        "expected the method name in the error payload, got: {sink}"
    );
}
