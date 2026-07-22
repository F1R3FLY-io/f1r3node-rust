//! End-to-end tests for the `File` agent (Phase 2 of the File I/O
//! implementation).
//!
//! The `agent File { ... }` block in `io/agents/file.rho` is
//! self-contained syntactically but expects the enclosing scope to
//! bind the native-URN names it references (`nRead`, `nSize`,
//! `nClose`). These tests wrap the agent block in that scope,
//! construct a `File` instance from a real fd returned by
//! `nativeOpen`, and invoke each method through the dispatch
//! machinery — exercising the full stack:
//!
//! - `agent` block sugar (rholang-rs PR #94) → dispatch
//!   for-comprehension.
//! - Native URN injection via `NormalizerEnv` (f1r3node-rust PR
//!   #132) → fixed-channel `Par` resolution.
//! - The 19 native handlers themselves (PR #120) → `tokio::fs`.
//! - The `is_replay` guard on every handler (PR #137) → replay
//!   safety.
//! - Shared `FileHandleTable` across all handlers (PR #137
//!   `0ac2a4df`) → fd allocated by `nativeOpen` visible to
//!   `nativeRead` / `nativeSize` / `nativeClose`.
//!
//! Any regression in any of those layers would surface here.
//!
//! Also exercises the `!?` sync-send + `try/catch` sugar
//! composition against native handlers: the handlers now
//! destructure args as ack-FIRST (see PR #139 commit history),
//! matching what `!?` / SendReceive generate. File.rho method
//! bodies use the FIP-style
//! `try @<pat> <- nX!?(args) { ok } catch @[code, msg] { err }`
//! form directly, no send-then-for-then-match workaround.
//!
//! And the `default(...@args) { body }` reply idiom: `args` is
//! in scope via the outer `for(...@args <= this)` binding, its
//! first element is the caller's return channel (as a Process
//! from send's auto-deref), and the default arm destructures it
//! to reply with FSERR_UNSUPPORTED.

use std::collections::HashMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::io::agents::FILE_AGENT_SRC;
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
        "fileio_file_agent_spec_{tag}_{pid}_{ts}",
        pid = std::process::id(),
        ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    p.to_string_lossy().into_owned()
}

/// The four native URNs the `File` agent + test harness need.
/// Populated as a `NormalizerEnv` so the enclosing `new`-scope's
/// URN bindings resolve to the fixed-channel `Par`s.
fn file_agent_env() -> HashMap<String, Par> {
    let mut env: HashMap<String, Par> = HashMap::new();
    env.insert(
        "rho:io:fs:native:1.0.0/open".to_string(),
        FixedChannels::native_open(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/read".to_string(),
        FixedChannels::native_read(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/size".to_string(),
        FixedChannels::native_size(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/close".to_string(),
        FixedChannels::native_close(),
    );
    env
}

/// Wrap `body` in the outer `new`-scope + inject the `File` agent
/// block. The body has `File` (agent constructor), `nOpen` (native
/// open URN — separate from the ones the agent needs), and a
/// `@"sink"` channel to publish observable results on.
fn wrap(body: &str, path: &str) -> String {
    format!(
        r#"new
     File,
     nOpen(`rho:io:fs:native:1.0.0/open`),
     nRead(`rho:io:fs:native:1.0.0/read`),
     nSize(`rho:io:fs:native:1.0.0/size`),
     nClose(`rho:io:fs:native:1.0.0/close`),
     openAck
   in {{
     {FILE_AGENT_SRC}
     |
     nOpen!(*openAck, "{path}", "r") |
     for (@[true, fd] <- openAck) {{
       // File's constructor uses `@fd` (process pattern), so we
       // pass the fd Process directly. Agent constructor desugars
       // to `for(__r, @fd <= File)`; caller sends
       // `File!(replyChan_as_process, fd)` -- return channel
       // first (as Process via η-deref), then user-declared
       // formals. Send args are Processes throughout.
       new fileRet in {{
         File!(*fileRet, fd) |
         // Bind `fileAgent` as a Name (no `@` in pattern) so the
         // body can use it directly as a send channel:
         // `fileAgent!(...)`. File's constructor published
         // `bundle+{{*this}}` on __r; the received value is a
         // bundled name that behaves as a normal send target.
         for (fileAgent <- fileRet) {{
           {body}
         }}
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

/// End-to-end: open a file, wrap the fd in a `File` agent, call
/// `read(n)`, publish the result to `@"sink"`, verify the returned
/// bytes match the file contents.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_read_returns_file_contents() {
    let path = temp_path("read");
    std::fs::write(&path, b"hello, file agent").expect("precondition write");

    // Agent dispatch pattern is [*return, "method", ...formals], so
    // raw send order is: (returnChan_process, methodName, args...).
    let body = r#"
        new readRet in {
          fileAgent!(*readRet, "read", 17) |
          for (@result <- readRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &path);

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-read".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    // Expected: [true, GByteArray(...)] where the bytes spell out
    // "hello, file agent" (ASCII 104,101,108,108,111,44,32,102,...)
    assert!(
        sink.contains("GBool(true)"),
        "expected success tuple on sink, got: {sink}"
    );
    assert!(
        sink.contains("GByteArray([104, 101, 108, 108, 111, 44, 32, 102, 105, 108, 101, 32, 97, 103, 101, 110, 116])"),
        "expected the ASCII bytes of \"hello, file agent\" on sink, got: {sink}"
    );
}

/// `size()` returns the file's byte length via `nativeSize`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_size_returns_byte_length() {
    let path = temp_path("size");
    std::fs::write(&path, b"1234567890").expect("precondition write");

    let body = r#"
        new sizeRet in {
          fileAgent!(*sizeRet, "size") |
          for (@result <- sizeRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &path);

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-size".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    assert!(
        sink.contains("GBool(true)"),
        "expected success tuple on sink, got: {sink}"
    );
    assert!(
        sink.contains("GInt(10)"),
        "expected size=10 on sink, got: {sink}"
    );
}

/// `close()` produces `[true]` on the ack channel; a subsequent
/// `read()` on the same agent should surface `FSERR_CLOSED` (the
/// native handler's response when the fd is no longer in the
/// `FileHandleTable`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_close_then_read_reports_closed() {
    let path = temp_path("close");
    std::fs::write(&path, b"gone soon").expect("precondition write");

    let body = r#"
        new closeRet in {
          fileAgent!(*closeRet, "close") |
          for (@closeResult <- closeRet) {
            @"sink"!(("close", closeResult)) |
            new readRet in {
              fileAgent!(*readRet, "read", 10) |
              for (@readResult <- readRet) {
                @"sink"!(("read-after-close", readResult))
              }
            }
          }
        }
    "#;
    let src = wrap(body, &path);

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-close".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    assert!(
        sink.contains(r#"GString("close")"#),
        "expected 'close' tuple on sink, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("read-after-close")"#),
        "expected 'read-after-close' tuple on sink, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("FSERR_CLOSED")"#),
        "expected FSERR_CLOSED on the second read, got: {sink}"
    );
}

/// Unknown methods hit the `default` arm, which destructures
/// `args` (in scope via the outer `for(...@args <= this)` of the
/// desugared dispatch loop) to peel out the caller's return
/// channel and reply with `FSERR_UNSUPPORTED`. Locks in the
/// idiom for default-arm replies plus the observable
/// FSERR_UNSUPPORTED payload.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_unknown_method_returns_unsupported() {
    let path = temp_path("unknown");
    std::fs::write(&path, b"anything").expect("precondition write");

    let body = r#"
        new unkRet in {
          fileAgent!(*unkRet, "nonexistent", "arg1") |
          for (@result <- unkRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &path);

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-unknown".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

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
