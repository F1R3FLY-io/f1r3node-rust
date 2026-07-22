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

/// The native URNs the `File` agent + test harness need.
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
    env.insert(
        "rho:io:fs:native:1.0.0/readLine".to_string(),
        FixedChannels::native_read_line(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/readAllLines".to_string(),
        FixedChannels::native_read_all_lines(),
    );
    env.insert(
        "rho:io:fs:native:1.0.0/appendLines".to_string(),
        FixedChannels::native_append_lines(),
    );
    env
}

/// Wrap `body` in the outer `new`-scope + inject the `File` agent
/// block. Takes `mode` so writeable-fd tests can use `"w+"` or
/// `"r+"` while read-only tests use `"r"`. The body has `File`
/// (agent constructor) and a `@"sink"` channel to publish
/// observable results on.
fn wrap_with_mode(body: &str, path: &str, mode: &str) -> String {
    format!(
        r#"new
     File,
     nOpen(`rho:io:fs:native:1.0.0/open`),
     nRead(`rho:io:fs:native:1.0.0/read`),
     nWrite(`rho:io:fs:native:1.0.0/write`),
     nSeek(`rho:io:fs:native:1.0.0/seek`),
     nTell(`rho:io:fs:native:1.0.0/tell`),
     nSize(`rho:io:fs:native:1.0.0/size`),
     nTruncate(`rho:io:fs:native:1.0.0/truncate`),
     nFlush(`rho:io:fs:native:1.0.0/flush`),
     nClose(`rho:io:fs:native:1.0.0/close`),
     nReadLine(`rho:io:fs:native:1.0.0/readLine`),
     nReadAllLines(`rho:io:fs:native:1.0.0/readAllLines`),
     nAppendLines(`rho:io:fs:native:1.0.0/appendLines`),
     openAck
   in {{
     {FILE_AGENT_SRC}
     |
     nOpen!(*openAck, "{path}", "{mode}") |
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

/// Read-only convenience wrapper — opens `path` with mode "r".
fn wrap(body: &str, path: &str) -> String { wrap_with_mode(body, path, "r") }

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

/// `write(bytes)` writes at the current position and returns
/// `[true, nWritten]`. Follow-up `seek(0, "set")` + `read` reads
/// them back, proving the write reached disk and that fd-table
/// state persists across four method invocations
/// (write → seek → read → sink).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_write_then_readback_roundtrip() {
    let path = temp_path("write_roundtrip");
    // Precondition: file must exist for "w+" to succeed on all
    // platforms (macOS's `w+` is fine on missing files too, but
    // touching first keeps the test portable).
    std::fs::write(&path, b"").expect("precondition touch");

    // Write 5 bytes, seek to start, read them back. Publish the
    // read result on @"sink" for inspection.
    //
    // `"68656c6c6f".hexToBytes()` gives the ASCII bytes of
    // "hello". Using .hexToBytes() rather than
    // "hello".toByteArray() because the latter protobuf-encodes
    // the string as a Rholang Par -- not what we want on disk.
    let body = r#"
        new writeRet, seekRet, readRet in {
          fileAgent!(*writeRet, "write", "68656c6c6f".hexToBytes()) |
          for (@_ <- writeRet) {
            fileAgent!(*seekRet, "seek", 0, "set") |
            for (@_ <- seekRet) {
              fileAgent!(*readRet, "read", 5) |
              for (@result <- readRet) { @"sink"!(result) }
            }
          }
        }
    "#;
    let src = wrap_with_mode(body, &path, "w+");

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-write-roundtrip".to_string());
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
        sink.contains("GByteArray([104, 101, 108, 108, 111])"),
        "expected the ASCII bytes of \"hello\" round-tripped, got: {sink}"
    );
}

/// `seek(3, "set")` moves to position 3; `tell()` reports it.
/// Also exercises the "cur" whence: subsequent `seek(2, "cur")`
/// should land at position 5.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_seek_and_tell_report_position() {
    let path = temp_path("seek_tell");
    std::fs::write(&path, b"0123456789").expect("precondition write");

    let body = r#"
        new seekRet1, tellRet1, seekRet2, tellRet2 in {
          fileAgent!(*seekRet1, "seek", 3, "set") |
          for (@_ <- seekRet1) {
            fileAgent!(*tellRet1, "tell") |
            for (@r1 <- tellRet1) {
              @"sink"!(("after-set", r1)) |
              fileAgent!(*seekRet2, "seek", 2, "cur") |
              for (@_ <- seekRet2) {
                fileAgent!(*tellRet2, "tell") |
                for (@r2 <- tellRet2) { @"sink"!(("after-cur", r2)) }
              }
            }
          }
        }
    "#;
    let src = wrap(body, &path);

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-seek-tell".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    // Expected: two tuples on sink -- ("after-set", [true, 3]) and
    // ("after-cur", [true, 5]).
    assert!(
        sink.contains(r#"GString("after-set")"#) && sink.contains("GInt(3)"),
        "expected tell to report 3 after seek(3,\"set\"), got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("after-cur")"#) && sink.contains("GInt(5)"),
        "expected tell to report 5 after seek(2,\"cur\") from 3, got: {sink}"
    );
}

/// `truncate(3)` shrinks a 10-byte file to 3 bytes. `size()`
/// then reports 3. Exercises write mode + truncate reaching the
/// underlying `File::set_len`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_truncate_shrinks_file() {
    let path = temp_path("truncate");
    std::fs::write(&path, b"0123456789").expect("precondition write");

    let body = r#"
        new truncRet, sizeRet in {
          fileAgent!(*truncRet, "truncate", 3) |
          for (@truncResult <- truncRet) {
            @"sink"!(("trunc", truncResult)) |
            fileAgent!(*sizeRet, "size") |
            for (@sizeResult <- sizeRet) { @"sink"!(("size", sizeResult)) }
          }
        }
    "#;
    // "r+" opens read+write on existing file (no truncate on open,
    // unlike "w+"). Correct for a shrink test.
    let src = wrap_with_mode(body, &path, "r+");

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-truncate".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let disk_size = std::fs::metadata(&path)
        .map(|m| m.len())
        .unwrap_or(u64::MAX);
    let _ = std::fs::remove_file(&path);

    assert!(
        sink.contains(r#"GString("trunc")"#) && sink.contains("GBool(true)"),
        "expected truncate to succeed, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("size")"#) && sink.contains("GInt(3)"),
        "expected size=3 after truncate(3), got: {sink}"
    );
    assert_eq!(
        disk_size, 3,
        "expected the file on disk to be 3 bytes after truncate"
    );
}

/// `flush()` returns `[true]`. Sanity check that the wrapper
/// resolves the URN and the try/catch destructures the empty
/// success payload correctly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_flush_returns_success() {
    let path = temp_path("flush");
    std::fs::write(&path, b"").expect("precondition touch");

    // `"616263".hexToBytes()` = ASCII "abc" (3 bytes). Content
    // doesn't matter here -- test only cares that flush returns
    // a `[true]` success tuple.
    let body = r#"
        new writeRet, flushRet in {
          fileAgent!(*writeRet, "write", "616263".hexToBytes()) |
          for (@_ <- writeRet) {
            fileAgent!(*flushRet, "flush") |
            for (@result <- flushRet) { @"sink"!(result) }
          }
        }
    "#;
    let src = wrap_with_mode(body, &path, "w+");

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-flush".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    // Success reply is a bare `[true]` -- a 1-element list.
    assert!(
        sink.contains("GBool(true)"),
        "expected flush success tuple on sink, got: {sink}"
    );
}

/// End-to-end: file has three LF-terminated lines; `readLine`
/// returns the first line without the newline, advances the cursor
/// past the newline; a subsequent `tell` reports the byte position
/// just past `\n`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_read_line_returns_first_line_and_advances_cursor() {
    let path = temp_path("read_line");
    std::fs::write(&path, b"first line\nsecond line\nthird\n").expect("precondition write");

    let body = r#"
        new readRet, tellRet in {
          fileAgent!(*readRet, "readLine") |
          for (@lineResult <- readRet) {
            fileAgent!(*tellRet, "tell") |
            for (@posResult <- tellRet) {
              @"sink"!((lineResult, posResult))
            }
          }
        }
    "#;
    let src = wrap(body, &path);

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-read-line".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    assert!(
        sink.contains(r#"GString("first line")"#),
        "expected first line on sink, got: {sink}"
    );
    // Cursor should be at byte 11 -- 10 chars of "first line" + 1
    // newline byte. The rewind logic in native_read_line has to be
    // right for this to hold.
    assert!(
        sink.contains("GInt(11)"),
        "expected tell to report pos 11 after readLine, got: {sink}"
    );
}

/// CRLF handling: file with `\r\n` line terminators. `readLine`
/// strips both bytes, returning the bare line text.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_read_line_strips_crlf() {
    let path = temp_path("read_line_crlf");
    std::fs::write(&path, b"hello\r\nworld\r\n").expect("precondition write");

    let body = r#"
        new r1, r2 in {
          fileAgent!(*r1, "readLine") |
          for (@line1 <- r1) {
            fileAgent!(*r2, "readLine") |
            for (@line2 <- r2) {
              @"sink"!((line1, line2))
            }
          }
        }
    "#;
    let src = wrap(body, &path);

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-read-line-crlf".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    assert!(
        sink.contains(r#"GString("hello")"#),
        "expected 'hello' (CRLF stripped) on sink, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("world")"#),
        "expected 'world' (CRLF stripped) on sink, got: {sink}"
    );
}

/// `readLine` at EOF returns `[true, ""]`. Ambiguous with an empty
/// line at EOF, per documented convention; callers can compare
/// `tell()` against `size()` to disambiguate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_read_line_at_eof_returns_empty_string() {
    let path = temp_path("read_line_eof");
    std::fs::write(&path, b"only\n").expect("precondition write");

    let body = r#"
        new r1, r2 in {
          fileAgent!(*r1, "readLine") |
          for (@_ <- r1) {
            // second readLine now sits at EOF (byte 5)
            fileAgent!(*r2, "readLine") |
            for (@eofResult <- r2) { @"sink"!(eofResult) }
          }
        }
    "#;
    let src = wrap(body, &path);

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-read-line-eof".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    assert!(
        sink.contains("GBool(true)"),
        "expected success tuple even at EOF, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("")"#),
        "expected empty string on EOF, got: {sink}"
    );
}

/// `lines()` reads a full file into a list of Strings; trailing
/// newline is absorbed (no trailing empty element); CRLF is stripped
/// per line.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_lines_returns_all_lines_without_trailing_empty() {
    let path = temp_path("lines_all");
    std::fs::write(&path, b"alpha\nbeta\r\ngamma\n").expect("precondition write");

    let body = r#"
        new linesRet in {
          fileAgent!(*linesRet, "lines") |
          for (@result <- linesRet) { @"sink"!(result) }
        }
    "#;
    let src = wrap(body, &path);

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-lines".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    assert!(
        sink.contains("GBool(true)"),
        "expected success tuple, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("alpha")"#),
        "expected 'alpha' on sink, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("beta")"#),
        "expected 'beta' (CRLF stripped) on sink, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("gamma")"#),
        "expected 'gamma' on sink, got: {sink}"
    );
}

/// `appendLines(list)` writes each String followed by `\n` and
/// returns the total byte count. Verified end-to-end: write three
/// lines, seek back to 0, read them via `lines`, assert the round
/// trip.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_append_lines_writes_lines_then_readback_matches() {
    let path = temp_path("append_lines");
    // Create an empty file so we can open w+ and append.
    std::fs::write(&path, b"").expect("precondition create");

    let body = r#"
        new appendRet, seekRet, linesRet in {
          fileAgent!(*appendRet, "appendLines", ["one", "two", "three"]) |
          for (@writeResult <- appendRet) {
            fileAgent!(*seekRet, "seek", 0, "set") |
            for (@_ <- seekRet) {
              fileAgent!(*linesRet, "lines") |
              for (@readResult <- linesRet) {
                @"sink"!((writeResult, readResult))
              }
            }
          }
        }
    "#;
    let src = wrap_with_mode(body, &path, "w+");

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-append-lines".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    // Expected nBytes: len("one\n") + len("two\n") + len("three\n")
    // = 4 + 4 + 6 = 14.
    assert!(
        sink.contains("GInt(14)"),
        "expected 14 bytes written, got: {sink}"
    );
    // Roundtrip: the readback should list each line back.
    assert!(
        sink.contains(r#"GString("one")"#),
        "expected 'one' back from readback, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("two")"#),
        "expected 'two' back from readback, got: {sink}"
    );
    assert!(
        sink.contains(r#"GString("three")"#),
        "expected 'three' back from readback, got: {sink}"
    );
}

/// `appendLines` with a non-String element in the list returns
/// `FSERR_BAD_ARG` without writing anything. Type discipline at
/// the native boundary rather than a Rholang match failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_agent_append_lines_rejects_non_string_element() {
    let path = temp_path("append_lines_bad");
    std::fs::write(&path, b"").expect("precondition create");

    let body = r#"
        new appendRet, sizeRet in {
          fileAgent!(*appendRet, "appendLines", ["good", 42, "also good"]) |
          for (@writeResult <- appendRet) {
            fileAgent!(*sizeRet, "size") |
            for (@sizeResult <- sizeRet) {
              @"sink"!((writeResult, sizeResult))
            }
          }
        }
    "#;
    let src = wrap_with_mode(body, &path, "w+");

    let runtime = mk_runtime().await;
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let phlo = Cost::create(i64::MAX, "file-agent-append-lines-bad".to_string());
    let result = runtime
        .evaluate(&src, phlo, file_agent_env(), rand)
        .await
        .expect("evaluate");
    assert!(result.errors.is_empty(), "eval errors: {:?}", result.errors);

    let sink = observe_sink(&runtime).await;
    let _ = std::fs::remove_file(&path);

    assert!(
        sink.contains(r#"GString("FSERR_BAD_ARG")"#),
        "expected FSERR_BAD_ARG code on sink, got: {sink}"
    );
    // Size must still be 0 -- native rejected before any write.
    assert!(
        sink.contains("GInt(0)"),
        "expected file to still be empty (size 0), got: {sink}"
    );
}
