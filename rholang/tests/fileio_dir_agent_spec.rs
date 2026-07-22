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
use rholang::rust::interpreter::io::agents::{DIR_AGENT_SRC, FILE_AGENT_SRC};
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

/// The native URNs the `Dir` agent + test harness need.
///
/// Includes all URNs referenced by both `Dir` and (transitively)
/// `File`, since `Dir.openFile` composes with `File` and Dir's
/// agent block references `nOpen` + the `File` constructor
/// channel. The normalizer catches unbound names regardless of
/// whether the referring method is invoked, so every test's
/// enclosing scope must supply the full set.
fn dir_agent_env() -> HashMap<String, Par> {
    let mut env: HashMap<String, Par> = HashMap::new();
    // Dir's own natives.
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
    env.insert(
        "rho:io:fs:native:1.0.0/removeFile".to_string(),
        FixedChannels::native_remove_file(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/removeDir".to_string(),
        FixedChannels::native_remove_dir(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/rename".to_string(),
        FixedChannels::native_rename(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/copyFile".to_string(),
        FixedChannels::native_copy_file(),
    );
    // `openFile` needs the open primitive + the File agent's
    // full native set (since embedding File also requires
    // resolving its own URN references at normalization time).
    env.insert(
        "rho:io:fs:native:1.0.0/open".to_string(),
        FixedChannels::native_open(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/read".to_string(),
        FixedChannels::native_read(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/write".to_string(),
        FixedChannels::native_write(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/seek".to_string(),
        FixedChannels::native_seek(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/tell".to_string(),
        FixedChannels::native_tell(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/size".to_string(),
        FixedChannels::native_size(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/truncate".to_string(),
        FixedChannels::native_truncate(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/flush".to_string(),
        FixedChannels::native_flush(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/close".to_string(),
        FixedChannels::native_close(),
    );
    env
}

/// Wrap `body` in the outer `new`-scope + inject BOTH the `Dir`
/// and `File` agent blocks (Dir.openFile constructs File
/// instances). Constructs a Dir instance rooted at `root_path`
/// and binds `dirAgent` for the body to invoke.
fn wrap(body: &str, root_path: &str) -> String {
    format!(
        r#"new
     Dir, File,
     nQuarantine(`rho:io:fs:native:1.0.0/quarantine`),
     nEntries(`rho:io:fs:native:1.0.0/entries`),
     nStat(`rho:io:fs:native:1.0.0/stat`),
     nExists(`rho:io:fs:native:1.0.0/exists`),
     nRemoveFile(`rho:io:fs:native:1.0.0/removeFile`),
     nRemoveDir(`rho:io:fs:native:1.0.0/removeDir`),
     nRename(`rho:io:fs:native:1.0.0/rename`),
     nCopyFile(`rho:io:fs:native:1.0.0/copyFile`),
     nOpen(`rho:io:fs:native:1.0.0/open`),
     nRead(`rho:io:fs:native:1.0.0/read`),
     nWrite(`rho:io:fs:native:1.0.0/write`),
     nSeek(`rho:io:fs:native:1.0.0/seek`),
     nTell(`rho:io:fs:native:1.0.0/tell`),
     nSize(`rho:io:fs:native:1.0.0/size`),
     nTruncate(`rho:io:fs:native:1.0.0/truncate`),
     nFlush(`rho:io:fs:native:1.0.0/flush`),
     nClose(`rho:io:fs:native:1.0.0/close`)
   in {{
     {DIR_AGENT_SRC}
     |
     {FILE_AGENT_SRC}
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

/// `removeFile(relPath)` unlinks the child; a follow-up
/// `exists()` confirms it's gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_remove_file_deletes_child() {
    let root = temp_dir("remove_file");
    let target = root.join("doomed.txt");
    std::fs::write(&target, b"bye").expect("write target");
    assert!(target.exists(), "precondition: target must exist");

    let body = r#"
        new rmRet, existRet in {
          dirAgent!(*rmRet, "removeFile", "doomed.txt") |
          for (@rmResult <- rmRet) {
            @"sink"!(("remove", rmResult)) |
            dirAgent!(*existRet, "exists", "doomed.txt") |
            for (@existResult <- existRet) {
              @"sink"!(("exists", existResult))
            }
          }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-remove-file".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    assert!(!target.exists(), "file should be gone from disk");
    let _ = std::fs::remove_dir_all(&root);

    // Success tuple on remove ([true]) + [true, false] on exists.
    assert!(
        sink.contains(r#"GString("remove")"#) && sink.contains("GBool(true)"),
        "expected remove success, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("exists")"#) && sink.contains("GBool(false)"),
        "expected exists=false after remove, got: {sink}"
    );
}

/// `removeDir(relPath, true)` recursively deletes a non-empty
/// subtree.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_remove_dir_recursive_deletes_tree() {
    let root = temp_dir("remove_dir");
    let subdir = root.join("sub");
    std::fs::create_dir(&subdir).expect("mkdir sub");
    std::fs::write(subdir.join("nested.txt"), b"x").expect("write nested");

    let body = r#"
        new rmRet in {
          dirAgent!(*rmRet, "removeDir", "sub", true) |
          for (@result <- rmRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-remove-dir".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    assert!(!subdir.exists(), "subdir should be gone from disk");
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        sink.contains("GBool(true)"),
        "expected removeDir success, got: {sink}"
    );
}

/// `removeDir(relPath, false)` on a non-empty directory returns
/// an error (native surfaces the OS's `DirectoryNotEmpty`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_remove_dir_nonrecursive_on_nonempty_errors() {
    let root = temp_dir("remove_dir_nonempty");
    let subdir = root.join("sub");
    std::fs::create_dir(&subdir).expect("mkdir sub");
    std::fs::write(subdir.join("holds.txt"), b"x").expect("write holds");

    let body = r#"
        new rmRet in {
          dirAgent!(*rmRet, "removeDir", "sub", false) |
          for (@result <- rmRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-remove-dir-nonempty".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    assert!(subdir.exists(), "subdir should still be on disk");
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        sink.contains("GBool(false)"),
        "expected error tuple on non-empty non-recursive remove, got: {sink}"
    );
}

/// `rename(from, to)` moves a file within the root. Both source
/// and destination are quarantined.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_rename_moves_child_within_root() {
    let root = temp_dir("rename");
    let source = root.join("old.txt");
    std::fs::write(&source, b"payload").expect("write source");
    let dest = root.join("new.txt");

    let body = r#"
        new renRet in {
          dirAgent!(*renRet, "rename", "old.txt", "new.txt") |
          for (@result <- renRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-rename".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    assert!(!source.exists(), "source should be gone");
    assert!(dest.exists(), "dest should exist");
    assert_eq!(
        std::fs::read(&dest).expect("read dest"),
        b"payload",
        "dest contents should match source"
    );
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        sink.contains("GBool(true)"),
        "expected rename success, got: {sink}"
    );
}

/// `rename` where the SOURCE escapes root — quarantine catches
/// it, no filesystem call. Escape target file exists to prove
/// rejection is on principle.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_rename_rejects_escape_in_source() {
    let root = temp_dir("rename_escape_src");
    let outside = root.parent().unwrap().join("outsider.txt");
    std::fs::write(&outside, b"external").expect("write outside");

    let body = r#"
        new renRet in {
          dirAgent!(*renRet, "rename", "../outsider.txt", "captured.txt") |
          for (@result <- renRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-rename-escape-src".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    assert!(outside.exists(), "outside file should be untouched");
    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        sink.contains(r#"GString("FSERR_QUARANTINE")"#),
        "expected FSERR_QUARANTINE on rejected source, got: {sink}"
    );
}

/// `rename` where the DESTINATION escapes root — same rejection
/// via the second quarantine call, no filesystem side effect.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_rename_rejects_escape_in_destination() {
    let root = temp_dir("rename_escape_dst");
    let source = root.join("valid.txt");
    std::fs::write(&source, b"stayput").expect("write source");
    let out_of_root = root.parent().unwrap().join("escaped.txt");

    let body = r#"
        new renRet in {
          dirAgent!(*renRet, "rename", "valid.txt", "../escaped.txt") |
          for (@result <- renRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-rename-escape-dst".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    assert!(source.exists(), "source should be untouched");
    assert!(!out_of_root.exists(), "escaped destination must NOT exist");
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        sink.contains(r#"GString("FSERR_QUARANTINE")"#),
        "expected FSERR_QUARANTINE on rejected destination, got: {sink}"
    );
}

/// `copyFile(from, to)` duplicates a file within the root. The
/// reply carries the number of bytes copied.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_copy_file_duplicates_child() {
    let root = temp_dir("copyfile");
    let source = root.join("orig.txt");
    std::fs::write(&source, b"duplicate me").expect("write source");
    let dest = root.join("copy.txt");

    let body = r#"
        new cpRet in {
          dirAgent!(*cpRet, "copyFile", "orig.txt", "copy.txt") |
          for (@result <- cpRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-agent-copyfile".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    assert!(source.exists(), "source must still exist");
    assert!(dest.exists(), "dest must exist");
    assert_eq!(std::fs::read(&dest).expect("read dest"), b"duplicate me");
    let _ = std::fs::remove_dir_all(&root);

    // Reply is [true, 12] (12 = len("duplicate me")).
    assert!(
        sink.contains("GBool(true)"),
        "expected copyFile success, got: {sink}"
    );
    assert!(
        sink.contains("GInt(12)"),
        "expected 12 bytes copied, got: {sink}"
    );
}

/// Regression guard for a HIGH-severity finding from the security
/// review of PR #141's mutating-methods commit: `removeDir("", true)`
/// (or `"."`, `"./"`, `"/"`) used to resolve through
/// `canonicalize_and_quarantine` to the root path itself, letting
/// a caller who legitimately holds only a `Dir` handle wipe the
/// entire sandbox via `tokio::fs::remove_dir_all(root)`.
///
/// Fix landed in `path.rs`: empty and `.`-only tails now surface
/// `FSERR_BAD_ARG` before any filesystem op. This test asserts
/// the reply is that FSERR and the root directory still exists
/// on disk after the call.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_remove_dir_empty_relpath_is_rejected() {
    let root = temp_dir("empty_relpath_reject");
    // Populate the root with a child so we can prove it wasn't
    // wiped (root existence alone would prove it, but a child
    // makes the test intent unambiguous).
    let canary = root.join("canary.txt");
    std::fs::write(&canary, b"i am still here").expect("write canary");

    // Exercise all four spelling variants of "the root itself":
    // "", ".", "./", "/". Each must be rejected with the same
    // FSERR_BAD_ARG code.
    for rel in ["", ".", "./", "/"] {
        let body = format!(
            r#"
            new rmRet in {{
              dirAgent!(*rmRet, "removeDir", "{rel}", true) |
              for (@result <- rmRet) {{ @"sink"!(("attempt", "{rel}", result)) }}
            }}
        "#
        );
        let src = wrap(&body, &root.to_string_lossy());

        let runtime = mk_runtime().await;
        let rand = Blake2b512Random::create_from_bytes(&[]);
        let phlo = Cost::create(i64::MAX, "dir-agent-empty-relpath-reject".to_string());
        let result = runtime
            .evaluate(&src, phlo, dir_agent_env(), rand)
            .await
            .expect("evaluate");
        assert!(
            result.errors.is_empty(),
            "eval errors for rel={rel:?}: {:?}",
            result.errors
        );

        let sink = observe_sink(&runtime).await;

        // Reply must be a false-tuple carrying FSERR_BAD_ARG.
        assert!(
            sink.contains("GBool(false)") && sink.contains(r#"GString("FSERR_BAD_ARG")"#),
            "expected FSERR_BAD_ARG on rel={rel:?}, got: {sink}"
        );
        // Root directory and canary must still exist on disk.
        assert!(
            root.exists() && canary.exists(),
            "root or canary was destroyed by removeDir({rel:?}, true)"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

// --- openFile / openDir: agent-composition tests ------------------
//
// `Dir.openFile(relPath, mode)` and `Dir.openDir(relPath)` are the
// first agent-composing methods in the fileio stack. They quarantine
// the caller-supplied path, then construct a `File` (or nested `Dir`)
// instance around the resolved handle/path and hand back the bundle
// via the reply tuple. Downstream callers use the returned agent as
// a plain send channel via Rholang's implicit process->name η.
//
// These tests exercise the full composition: `Dir.openFile` -> get a
// File bundle -> call `File.read` on it -> assert content. Any bug in
// the composition (wrong reply shape, missing agent binding,
// mis-bundled `*this`) surfaces here.

/// `openFile` composes correctly: caller receives a File bundle
/// and uses it to `read` the file's bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_open_file_returns_functional_file() {
    let root = temp_dir("open_file_ok");
    std::fs::write(root.join("hello.txt"), b"hello").expect("write hello");

    // openFile -> get file bundle -> file!?("read", 5) via raw send
    // (return-first shape, as in fileio_file_agent_spec.rs). We
    // bind `file` as a Process via `@[true, file]`; using it as a
    // send channel relies on Rholang's implicit process->name
    // quoting.
    let body = r#"
        new openRet in {
          dirAgent!(*openRet, "openFile", "hello.txt", "r") |
          for (@openResult <- openRet) {
            match openResult {
              [true, file] => {
                new readRet in {
                  @file!(*readRet, "read", 5) |
                  for (@readResult <- readRet) { @"sink"!(readResult) }
                }
              }
              _ => @"sink"!(("openFile-failed", openResult))
            }
          }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-open-file-ok".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_dir_all(&root);

    // Expected: [true, GByteArray([104, 101, 108, 108, 111])]
    // = "hello" ASCII.
    assert!(
        sink.contains("GBool(true)"),
        "expected read success tuple on sink, got: {sink}"
    );
    assert!(
        sink.contains("GByteArray([104, 101, 108, 108, 111])"),
        "expected ASCII bytes of \"hello\", got: {sink}"
    );
}

/// `openFile` on a `..` escape is caught by quarantine before
/// any `nativeOpen` fires. Escape target file exists on disk to
/// prove the rejection is on principle.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_open_file_rejects_escape() {
    let root = temp_dir("open_file_escape");
    let outside = root.parent().unwrap().join("secret.txt");
    std::fs::write(&outside, b"secret").expect("write outside");

    let body = r#"
        new openRet in {
          dirAgent!(*openRet, "openFile", "../secret.txt", "r") |
          for (@result <- openRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-open-file-escape".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    assert!(outside.exists(), "outside file must be untouched");
    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        sink.contains(r#"GString("FSERR_QUARANTINE")"#),
        "expected FSERR_QUARANTINE on escape, got: {sink}"
    );
}

/// `openFile` on a missing file with mode "r" surfaces the
/// native's `NotFound` error (mapped to FSERR_NOT_FOUND). Proves
/// native errors bubble through the openFile composition without
/// being masked by the agent-composition layer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_open_file_missing_returns_not_found() {
    let root = temp_dir("open_file_missing");

    let body = r#"
        new openRet in {
          dirAgent!(*openRet, "openFile", "nope.txt", "r") |
          for (@result <- openRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-open-file-missing".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_dir_all(&root);

    // FSERR_NOT_FOUND (not FSERR_QUARANTINE — path resolves inside
    // root but doesn't exist).
    assert!(
        sink.contains(r#"GString("FSERR_NOT_FOUND")"#),
        "expected FSERR_NOT_FOUND for missing file, got: {sink}"
    );
}

/// `openDir` composes correctly: returned nested Dir handles
/// `entries()` on the subdirectory.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_open_dir_returns_functional_dir() {
    let root = temp_dir("open_dir_ok");
    let sub = root.join("sub");
    std::fs::create_dir(&sub).expect("mkdir sub");
    std::fs::write(sub.join("child.txt"), b"c").expect("write child");

    let body = r#"
        new openRet in {
          dirAgent!(*openRet, "openDir", "sub") |
          for (@openResult <- openRet) {
            match openResult {
              [true, subDir] => {
                new entRet in {
                  @subDir!(*entRet, "entries") |
                  for (@entResult <- entRet) { @"sink"!(entResult) }
                }
              }
              _ => @"sink"!(("openDir-failed", openResult))
            }
          }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-open-dir-ok".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_dir_all(&root);

    // Expected: entries() on `sub` returns a list containing
    // child.txt.
    assert!(
        sink.contains("GBool(true)"),
        "expected entries success, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("child.txt")"#),
        "expected child.txt in subdir entries, got: {sink}"
    );
}

/// `openDir` on a regular file returns `[false, "FSERR_BAD_ARG", ...]`.
/// Proves the kind-check fires before constructing a bogus Dir.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_open_dir_on_regular_file_returns_bad_arg() {
    let root = temp_dir("open_dir_notdir");
    std::fs::write(root.join("file.txt"), b"not a dir").expect("write file");

    let body = r#"
        new openRet in {
          dirAgent!(*openRet, "openDir", "file.txt") |
          for (@result <- openRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-open-dir-notdir".to_string());
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
        sink.contains(r#"GString("FSERR_BAD_ARG")"#),
        "expected FSERR_BAD_ARG code on sink, got: {sink}"
    );
}

/// `openDir` on a `..` escape is caught by quarantine.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_agent_open_dir_rejects_escape() {
    let root = temp_dir("open_dir_escape");
    // Create an outside dir so the escape target exists.
    let outside_dir = root.parent().unwrap().join("outside_dir");
    std::fs::create_dir_all(&outside_dir).expect("mkdir outside");

    let body = r#"
        new openRet in {
          dirAgent!(*openRet, "openDir", "../outside_dir") |
          for (@result <- openRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &root.to_string_lossy());

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "dir-open-dir-escape".to_string());
    let result = runtime
        .evaluate(&src, phlo, dir_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    assert!(outside_dir.exists(), "outside dir must be untouched");
    let _ = std::fs::remove_dir_all(&outside_dir);
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        sink.contains(r#"GString("FSERR_QUARANTINE")"#),
        "expected FSERR_QUARANTINE on escape, got: {sink}"
    );
}
