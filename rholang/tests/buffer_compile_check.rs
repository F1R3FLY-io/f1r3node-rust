//! Compile-check + end-to-end runtime tests for Buffer.rho.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::rholang::implicits::single_expr;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;
use rholang::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;

const BUFFER_RHO: &str = include_str!("../../casper/src/main/resources/Buffer.rho");

fn rand() -> crypto::rust::hash::blake2b512_random::Blake2b512Random {
    crypto::rust::hash::blake2b512_random::Blake2b512Random::create_from_bytes(&Vec::new())
}

/// Extract Buffer.rho's body (between the outermost `new ... in {` and the
/// final matching `}`) so tests can concatenate their own test code with
/// the same shared scope.
///
/// The library's shape is `new Buffer, Allocator, metaP, chunkP,
/// gatherChunks in { ... }` — we strip that wrapper.
fn buffer_lib_body() -> String {
    let src = BUFFER_RHO;
    // Find "in {" following the outermost `new`.
    let in_pos = src
        .find("in {")
        .expect("Buffer.rho must contain `in {` at the outer new");
    let body_start = in_pos + "in {".len();
    let last_brace = src.rfind('}').expect("Buffer.rho must end with `}`");
    src[body_start..last_brace].to_string()
}

/// Build a `.rho` source that includes the Buffer library and a test
/// snippet, sharing the same `new` scope so the test can reference
/// `Buffer`, `Allocator`, `metaP`, `chunkP`, and `gatherChunks`.
fn with_lib(test_snippet: &str) -> String {
    format!(
        "new Buffer, Allocator, metaP, chunkP, gatherChunks, drainChunks in {{\n{}|\n{}\n}}",
        buffer_lib_body(),
        test_snippet
    )
}

// ---------------------------------------------------------------------
// Compile checks
// ---------------------------------------------------------------------

#[test]
fn buffer_rho_source_parses() {
    match ParBuilderUtil::mk_term(BUFFER_RHO) {
        Ok(_) => {}
        Err(e) => panic!("Buffer.rho failed to compile:\n{e:?}"),
    }
}

#[test]
fn with_lib_composes_and_parses() {
    let src = with_lib("Nil");
    match ParBuilderUtil::mk_term(&src) {
        Ok(_) => {}
        Err(e) => panic!("with_lib(Nil) failed to compile:\n{e:?}"),
    }
}

#[test]
fn minimal_buffer_capacity_only_compiles() {
    let src = r#"
        new Buffer, metaP in {
            agent Buffer {
                constructor(@capBytes) {
                    @[*private, *metaP]!((0, 0, capBytes, "bytes", "none", 0, 0))
                } |
                method capacity() {
                    for (@meta <- @[*private, *metaP]) {
                        match meta {
                            "REVOKED" => {
                                @[*private, *metaP]!("REVOKED") |
                                return!([false, "BUFERR_REVOKED", "closed"])
                            }
                            (_ell, _rho, cap, _unit, _lease, _lo, _hi) => {
                                @[*private, *metaP]!(meta) |
                                return!([true, cap])
                            }
                            _ => {
                                @[*private, *metaP]!(meta) |
                                return!([false, "BUFERR_REVOKED", "bad state"])
                            }
                        }
                    }
                } |
                default(...@args) {
                    match args {
                        [*ret, ..._] => ret!([false, "BUFERR_UNSUPPORTED", "unknown method"])
                    }
                }
            }
        }
    "#;
    match ParBuilderUtil::mk_term(src) {
        Ok(_) => {}
        Err(e) => panic!("minimal buffer failed to compile:\n{e:?}"),
    }
}

// ---------------------------------------------------------------------
// End-to-end runtime tests (drive through the reducer).
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn allocator_alloc_returns_a_bundled_buffer_handle() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@r <- @alloc!?("allocBytes", 32)) {
            @"out"!(r)
          }
        }
        "#,
    );
    let par = ParBuilderUtil::mk_term(&src).expect("compile");
    let env: Env<Par> = Env::new();
    let res = reducer.eval(par, &env, rand().split_byte(0)).await;
    assert!(res.is_ok(), "eval failed: {:?}", res);
    let map = space.to_map().await;
    let chan = new_gstring_par("out".to_string(), Vec::new(), false);
    let row = map.get(&vec![chan]).expect("out channel has data");
    assert_eq!(row.data.len(), 1, "expected one reply on out");
    // The reply is a list [true, bufHandle].  We only check the true
    // discriminator here; runtime tests that exercise the handle come
    // in the write/read roundtrip test below.
    let reply = &row.data[0].a.pars[0];
    // Extract the first element of the EList reply.
    let list_expr = single_expr(reply).unwrap();
    match list_expr.expr_instance {
        Some(ExprInstance::EListBody(list)) => {
            assert!(!list.ps.is_empty());
            let head = single_expr(&list.ps[0]).unwrap();
            match head.expr_instance {
                Some(ExprInstance::GBool(b)) => assert!(b, "expected [true, buf]"),
                other => panic!("expected bool head, got {other:?}"),
            }
        }
        other => panic!("expected list reply, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_then_read_roundtrips_bytes() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Alloc a 16-byte buffer, writeBytes "hello!", read all 6, send bytes to @"out".
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 16)) {
            for (@[true, k] <- @buf!?("writeBytes", "hello!".toUtf8Bytes())) {
              for (@[true, bytes] <- @buf!?("read", 100)) {
                @"out"!([k, bytes])
              }
            }
          }
        }
        "#,
    );
    let par = ParBuilderUtil::mk_term(&src).expect("compile");
    let res = reducer.eval(par, &Env::new(), rand().split_byte(0)).await;
    assert!(res.is_ok(), "eval failed: {:?}", res);
    let map = space.to_map().await;
    let chan = new_gstring_par("out".to_string(), Vec::new(), false);
    let row = map.get(&vec![chan]).expect("out channel has data");
    // Reply is [k, bytes] where k = 6 and bytes = "hello!".
    let reply = &row.data[0].a.pars[0];
    let list = match single_expr(reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        other => panic!("expected list, got {other:?}"),
    };
    assert_eq!(list.ps.len(), 2);
    let k = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GInt(v)) => v,
        other => panic!("expected int k, got {other:?}"),
    };
    let bytes = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        other => panic!("expected bytes, got {other:?}"),
    };
    assert_eq!(k, 6);
    assert_eq!(bytes, b"hello!".to_vec());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capacity_query_after_alloc() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 128)) {
            for (@r <- @buf!?("capacity")) {
              @"out"!(r)
            }
          }
        }
        "#,
    );
    let par = ParBuilderUtil::mk_term(&src).expect("compile");
    reducer
        .eval(par, &Env::new(), rand().split_byte(0))
        .await
        .expect("eval");
    let map = space.to_map().await;
    let chan = new_gstring_par("out".to_string(), Vec::new(), false);
    let row = map.get(&vec![chan]).expect("out channel");
    let reply = &row.data[0].a.pars[0];
    let list = match single_expr(reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        other => panic!("expected list, got {other:?}"),
    };
    // Reply is [true, 128]
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("expected bool"),
    };
    assert!(ok);
    let cap = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GInt(v)) => v,
        _ => panic!("expected int"),
    };
    assert_eq!(cap, 128);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_then_capacity_returns_revoked() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 8)) {
            for (@[true] <- @buf!?("close")) {
              for (@r <- @buf!?("capacity")) {
                @"out"!(r)
              }
            }
          }
        }
        "#,
    );
    let par = ParBuilderUtil::mk_term(&src).expect("compile");
    reducer
        .eval(par, &Env::new(), rand().split_byte(0))
        .await
        .expect("eval");
    let map = space.to_map().await;
    let chan = new_gstring_par("out".to_string(), Vec::new(), false);
    let row = map.get(&vec![chan]).expect("out channel");
    let reply = &row.data[0].a.pars[0];
    let list = match single_expr(reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list"),
    };
    // [false, "BUFERR_REVOKED", "..."]
    assert_eq!(list.ps.len(), 3);
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    let code = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert!(!ok);
    assert_eq!(code, "BUFERR_REVOKED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_method_returns_buferr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 8)) {
            for (@r <- @buf!?("wibble")) {
              @"out"!(r)
            }
          }
        }
        "#,
    );
    let par = ParBuilderUtil::mk_term(&src).expect("compile");
    reducer
        .eval(par, &Env::new(), rand().split_byte(0))
        .await
        .expect("eval");
    let map = space.to_map().await;
    let chan = new_gstring_par("out".to_string(), Vec::new(), false);
    let row = map.get(&vec![chan]).expect("out channel");
    let reply = &row.data[0].a.pars[0];
    let list = match single_expr(reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let code = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert_eq!(code, "BUFERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_short_when_exceeds_capacity() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Cap 4 bytes; try to write 6 → should return k = 4.
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
            for (@r <- @buf!?("writeBytes", "abcdef".toUtf8Bytes())) {
              @"out"!(r)
            }
          }
        }
        "#,
    );
    let par = ParBuilderUtil::mk_term(&src).expect("compile");
    reducer
        .eval(par, &Env::new(), rand().split_byte(0))
        .await
        .expect("eval");
    let map = space.to_map().await;
    let chan = new_gstring_par("out".to_string(), Vec::new(), false);
    let row = map.get(&vec![chan]).expect("out channel");
    let reply = &row.data[0].a.pars[0];
    let list = match single_expr(reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let k = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GInt(v)) => v,
        _ => panic!(),
    };
    assert_eq!(k, 4);
}

// ---------------------------------------------------------------------
// Regression tests for the fixes to B-1, B-3, B-4, M-1, Mi-3.
//
// Prior versions could BRICK the buffer by triggering an uncatchable
// MethodNotDefined inside the metadata-token critical section: any
// caller-supplied arg of the wrong type raised on `.length()`/`<=`,
// consuming the token and never re-parking.  These tests exercise
// the griefing calls that used to brick and verify the buffer is
// still responsive after.
// ---------------------------------------------------------------------

/// Helper: extract [ok, code_or_val, msg?] from a reply Par.
fn extract_reply(par: &Par) -> (bool, String, Option<i64>, Option<Vec<u8>>) {
    let list = match single_expr(par).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        other => panic!("expected list reply, got {other:?}"),
    };
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("head not Bool"),
    };
    let (second_str, second_int, second_bytes) = if list.ps.len() >= 2 {
        match single_expr(&list.ps[1]).unwrap().expr_instance {
            Some(ExprInstance::GString(s)) => (s, None, None),
            Some(ExprInstance::GInt(i)) => (String::new(), Some(i), None),
            Some(ExprInstance::GByteArray(b)) => (String::new(), None, Some(b)),
            _ => (String::new(), None, None),
        }
    } else {
        (String::new(), None, None)
    };
    (ok, second_str, second_int, second_bytes)
}

async fn eval_and_read_out(
    space: &impl rspace_plus_plus::rspace::rspace_interface::ISpace<
        Par,
        BindPattern,
        ListParWithRandom,
        TaggedContinuation,
    >,
    reducer: &rholang::rust::interpreter::reduce::DebruijnInterpreter,
    src: &str,
) -> Par {
    let par = ParBuilderUtil::mk_term(src).expect("compile");
    reducer
        .eval(par, &Env::new(), rand().split_byte(0))
        .await
        .expect("eval");
    let map = space.to_map().await;
    let chan = new_gstring_par("out".to_string(), Vec::new(), false);
    let row = map.get(&vec![chan]).expect("out channel");
    row.data[0].a.pars[0].clone()
}

/// B-1 fix regression: writeBytes on a non-ByteArray must reply with
/// BUFERR_INVALID_ARGUMENT and NOT brick the buffer.  After the bad
/// call, a subsequent valid call must still work.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_bytes_non_bytearray_returns_invalid_argument_and_does_not_brick() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Send an Int as `xs` — should reject cleanly.  Then verify the
    // buffer still works via a follow-up capacity() call.
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 16)) {
            for (@bad <- @buf!?("writeBytes", 42)) {
              for (@cap <- @buf!?("capacity")) {
                @"out"!([bad, cap])
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    // Reply is [bad, cap] where bad = [false, "BUFERR_INVALID_ARGUMENT", ...]
    // and cap = [true, 16].
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected outer list"),
    };
    let (bad_ok, bad_code, _, _) = extract_reply(&outer.ps[0]);
    assert!(!bad_ok, "writeBytes(42) should fail");
    assert_eq!(bad_code, "BUFERR_INVALID_ARGUMENT");
    let (cap_ok, _, cap_v, _) = extract_reply(&outer.ps[1]);
    assert!(cap_ok, "capacity() should still work after bad writeBytes");
    assert_eq!(cap_v, Some(16), "buffer still has correct capacity");
}

/// B-3 fix regression: read on a non-Int must reply with
/// BUFERR_INVALID_ARGUMENT and NOT brick.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_non_int_returns_invalid_argument_and_does_not_brick() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 8)) {
            for (@bad <- @buf!?("read", "not an int")) {
              for (@cap <- @buf!?("capacity")) {
                @"out"!([bad, cap])
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (bad_ok, bad_code, _, _) = extract_reply(&outer.ps[0]);
    assert!(!bad_ok);
    assert_eq!(bad_code, "BUFERR_INVALID_ARGUMENT");
    let (cap_ok, _, cap_v, _) = extract_reply(&outer.ps[1]);
    assert!(cap_ok);
    assert_eq!(cap_v, Some(8));
}

/// M-1 fix regression: endFill("none") on a buffer with no lease held
/// must fail with BUFERR_FILLING, not falsely succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn end_fill_none_on_no_lease_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
            for (@r <- @buf!?("endFill", "none")) {
              @"out"!(r)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "endFill on no-lease-held must reject");
    assert_eq!(code, "BUFERR_FILLING");
}

/// Full lease roundtrip: beginFill, endFill with correct token, verify
/// read then succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lease_acquire_and_release_permits_subsequent_read() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 8)) {
            for (@[true, tok] <- @buf!?("beginFill")) {
              // Read under lease should be refused with BUFERR_FILLING.
              for (@refused <- @buf!?("read", 4)) {
                for (@[true] <- @buf!?("endFill", tok)) {
                  // After endFill, read succeeds (empty buffer → 0 bytes).
                  for (@r <- @buf!?("read", 4)) {
                    @"out"!([refused, r])
                  }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (refused_ok, refused_code, _, _) = extract_reply(&outer.ps[0]);
    assert!(!refused_ok, "read under held lease must fail");
    assert_eq!(refused_code, "BUFERR_FILLING");
    let (read_ok, _, _, read_bytes) = extract_reply(&outer.ps[1]);
    assert!(read_ok, "read after endFill must succeed");
    assert_eq!(
        read_bytes,
        Some(vec![]),
        "empty buffer read yields empty bytes"
    );
}

/// Wrong endFill token must reject and NOT release the lease.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn end_fill_with_wrong_token_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        new fakeToken in {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              for (@[true, _realTok] <- @buf!?("beginFill")) {
                for (@r <- @buf!?("endFill", *fakeToken)) {
                  @"out"!(r)
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "endFill with wrong token must reject");
    assert_eq!(code, "BUFERR_FILLING");
}

/// M-2 fix regression: clear() releases a held lease AND resets the
/// buffer so reads work again.  Verifies the non-destructive escape
/// hatch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clear_releases_lease_and_resets_state() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 8)) {
            for (@[true, _tok] <- @buf!?("beginFill")) {
              // Abandon the token.  Now clear() to recover.
              for (@[true] <- @buf!?("clear")) {
                // After clear, buffer is empty and no lease is held.
                for (@len <- @buf!?("length")) {
                  for (@r <- @buf!?("read", 4)) {
                    @"out"!([len, r])
                  }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (len_ok, _, len_v, _) = extract_reply(&outer.ps[0]);
    assert!(len_ok);
    assert_eq!(len_v, Some(0), "length after clear() is 0");
    let (read_ok, _, _, read_bytes) = extract_reply(&outer.ps[1]);
    assert!(read_ok, "read after clear() succeeds (no lease held)");
    assert_eq!(read_bytes, Some(vec![]));
}

/// Monotonic-chunk-index invariant: after clear(), a fresh write goes
/// to a chunk index STRICTLY GREATER than any used before.  We verify
/// indirectly by writing 3 bytes, clearing, writing 3 different bytes,
/// then reading — the read must return the new content.  If clear
/// rewound `lo` to 0 it would either observe stale chunk 0 or fail.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clear_then_write_then_read_yields_new_content_not_stale() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, buf] <- @alloc!?("allocBytes", 8)) {
            for (@[true, _k1] <- @buf!?("writeBytes", "AAA".toUtf8Bytes())) {
              for (@[true] <- @buf!?("clear")) {
                for (@[true, _k2] <- @buf!?("writeBytes", "BBB".toUtf8Bytes())) {
                  for (@r <- @buf!?("read", 8)) {
                    @"out"!(r)
                  }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, bytes) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(
        bytes,
        Some(b"BBB".to_vec()),
        "read must return new content, not stale"
    );
}

/// Mi-3 fix regression: allocator on a non-Int rejects cleanly (caller
/// does not hang).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn allocator_non_int_rejects_cleanly() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_lib(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@r <- @alloc!?("allocBytes", "not an int")) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "BUFERR_INVALID_ARGUMENT");
}
