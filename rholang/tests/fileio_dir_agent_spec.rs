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

/// The native URNs the `Dir` agent + test harness need.
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
     nExists(`rho:io:fs:native:1.0.0/exists`),
     nRemoveFile(`rho:io:fs:native:1.0.0/removeFile`),
     nRemoveDir(`rho:io:fs:native:1.0.0/removeDir`),
     nRename(`rho:io:fs:native:1.0.0/rename`),
     nCopyFile(`rho:io:fs:native:1.0.0/copyFile`)
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
