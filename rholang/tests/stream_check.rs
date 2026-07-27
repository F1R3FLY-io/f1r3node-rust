//! Compile-check + end-to-end runtime tests for Stream.rho.
//!
//! Tests supply an in-memory producer that pops values off a list
//! carried in a private cell, giving deterministic streams over
//! caller-controlled data without needing a File agent (Phase 5).

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::rholang::implicits::single_expr;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;
use rholang::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use rspace_plus_plus::rspace::rspace::RSpace;

const STREAM_RHO: &str = include_str!("../../casper/src/main/resources/Stream.rho");

fn rand() -> crypto::rust::hash::blake2b512_random::Blake2b512Random {
    crypto::rust::hash::blake2b512_random::Blake2b512Random::create_from_bytes(&Vec::new())
}

/// Extract the body of Stream.rho's outer `new Stream, paramsP,
/// stateP in { ... }` block, so tests can inject test code in the
/// same scope.
fn stream_lib_body() -> String {
    let src = STREAM_RHO;
    let in_pos = src.find("in {").expect("Stream.rho must contain `in {`");
    let body_start = in_pos + "in {".len();
    let last_brace = src.rfind('}').expect("Stream.rho must end with `}`");
    src[body_start..last_brace].to_string()
}

/// Wrap the Stream library body with a test snippet and the correct
/// outer `new` scope.
fn with_lib(test_snippet: &str) -> String {
    format!(
        "new Stream, paramsP, stateP, gatherN, foldLoop, forEachLoop in {{\n{}|\n{}\n}}",
        stream_lib_body(),
        test_snippet
    )
}

// ---------------------------------------------------------------------
// Compile checks
// ---------------------------------------------------------------------

#[test]
fn stream_rho_source_parses() {
    match ParBuilderUtil::mk_term(STREAM_RHO) {
        Ok(_) => {}
        Err(e) => panic!("Stream.rho failed to compile:\n{e:?}"),
    }
}

#[test]
fn with_lib_composes() {
    match ParBuilderUtil::mk_term(&with_lib("Nil")) {
        Ok(_) => {}
        Err(e) => panic!("with_lib(Nil) failed:\n{e:?}"),
    }
}

// ---------------------------------------------------------------------
// Runtime helpers
// ---------------------------------------------------------------------

/// Extract (ok_flag, second_string, second_int, second_bytes) from a
/// reply Par.  Mirrors the buffer_compile_check helper.
fn extract_reply(par: &Par) -> (bool, String, Option<i64>, Option<Vec<u8>>) {
    let list = match single_expr(par).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        other => panic!("expected list reply, got {other:?}"),
    };
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("head not Bool"),
    };
    let (s, i, b) = if list.ps.len() >= 2 {
        match single_expr(&list.ps[1]).unwrap().expr_instance {
            Some(ExprInstance::GString(v)) => (v, None, None),
            Some(ExprInstance::GInt(v)) => (String::new(), Some(v), None),
            Some(ExprInstance::GByteArray(v)) => (String::new(), None, Some(v)),
            _ => (String::new(), None, None),
        }
    } else {
        (String::new(), None, None)
    };
    (ok, s, i, b)
}

/// Evaluate `src` and read a single reply from `@"out"`.  If the
/// out-channel has no data (because a Rholang-side `for` blocked on a
/// shape mismatch, or the test's Rholang code never reached the send),
/// panic with a diagnostic that includes the offending source so the
/// failure isn't just "out channel" — Mi-4 test-quality fix.
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
    let row = match map.get(&vec![chan]) {
        Some(r) => r,
        None => panic!(
            "no reply on @\"out\" — a Rholang `for` likely blocked on a \
             shape mismatch or an unfulfilled receive.  Source (first \
             500 chars):\n{}\n\ntuplespace after eval (keys only):\n{:?}",
            &src.chars().take(500).collect::<String>(),
            map.keys().collect::<Vec<_>>()
        ),
    };
    row.data[0].a.pars[0].clone()
}

// ---------------------------------------------------------------------
// In-memory producer & identity chunk-builder used by all runtime tests.
//
// Producer: a contract that pops the head off a list held in a
// private cell.  Value goes back as [true, val]; when list is [],
// returns [false, "EOS", ""].
//
// ChunkBuilder: identity — returns the values-list wrapped as
// [true, list].  Suitable for an "EntryStream"-shaped test surface;
// CharStream / ByteStream would supply builders that concatenate.
// ---------------------------------------------------------------------

/// The Rholang bootstrap for an in-memory producer + identity chunk
/// builder, plus a Stream construction over them.  After this runs,
/// `stream` is bound to the constructed Stream handle.  Users of this
/// helper wrap their test code in a `for (@stream <- Stream!?(...))`.
///
/// `values` is a Rholang list literal in the caller's snippet — e.g.
/// `["a", "b", "c"]` or `[1, 2, 3]`.
fn stream_from_list(values_lit: &str) -> String {
    format!(
        r#"
        new listState, producer, identityBuilder in {{
          listState!({values_lit}) |
          contract producer(retCh) = {{
            for (@lst <- listState) {{
              match lst {{
                [] => {{
                  listState!([]) |
                  retCh!([false, "EOS", "no more values"])
                }}
                [head ...tail] => {{
                  listState!(tail) |
                  retCh!([true, head])
                }}
              }}
            }}
          }} |
          contract identityBuilder(@vals, retCh) = {{
            retCh!([true, vals])
          }} |
          for (@stream <- Stream!?(*producer, *identityBuilder)) {{
            @"streamOut"!(stream)
          }}
        }} |
        for (@stream <- @"streamOut") {{
          %TEST_SNIPPET%
        }}
        "#
    )
}

/// Compose stream_from_list + test snippet into a with_lib source.
fn test_over_stream(values_lit: &str, test_snippet: &str) -> String {
    let bootstrap = stream_from_list(values_lit).replace("%TEST_SNIPPET%", test_snippet);
    with_lib(&bootstrap)
}

// ---------------------------------------------------------------------
// Runtime tests
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn next_returns_first_value() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"["hello", "world"]"#,
        r#"for (@r <- @stream!?("next")) { @"out"!(r) }"#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, val, _, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(val, "hello");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn next_returns_eos_after_exhaustion() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"["only"]"#,
        r#"
        for (@_ <- @stream!?("next")) {
          for (@r <- @stream!?("next")) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eos_is_sticky() {
    // Spec: "once a stream has signaled EOS, subsequent calls return
    // the same EOS tuple".
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"[]"#,
        r#"
        for (@_ <- @stream!?("next")) {
          for (@r <- @stream!?("next")) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_makes_subsequent_next_return_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"["a", "b"]"#,
        r#"
        // Bind the close reply as a free variable rather than
        // pattern-matching [true] — a shape mismatch here would
        // block the outer `for` forever, and the test would only
        // fail via the eval_and_read_out "no reply" diagnostic
        // rather than pinpointing the wrong close reply.
        for (@closeReply <- @stream!?("close")) {
          for (@nextReply <- @stream!?("next")) {
            @"out"!([closeReply, nextReply])
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    // reply is [closeReply, nextReply].  Verify BOTH shapes in Rust.
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected outer list"),
    };
    let (close_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(close_ok, "close() must reply [true]");
    let (next_ok, next_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(!next_ok, "next after close must fail");
    assert_eq!(next_code, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chunk_returns_up_to_n_values() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"["a", "b", "c", "d", "e"]"#,
        r#"for (@r <- @stream!?("chunk", 3)) { @"out"!(r) }"#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    // Reply is [true, container].  With our identity builder,
    // container is the list of values: ["a", "b", "c"].
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let ok = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(ok);
    let inner = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        other => panic!("expected inner list, got {other:?}"),
    };
    assert_eq!(inner.ps.len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chunk_zero_returns_fserr_bad_arg() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"["a"]"#,
        r#"for (@r <- @stream!?("chunk", 0)) { @"out"!(r) }"#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chunk_non_int_returns_fserr_bad_arg() {
    // Type-guard regression, matching the Buffer.rho pattern.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"["a"]"#,
        r#"for (@r <- @stream!?("chunk", "not an int")) { @"out"!(r) }"#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fold_counts_stream_length() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"[10, 20, 30, 40, 50]"#,
        r#"
        new plus1 in {
          contract plus1(retCh, @acc, @_val) = { retCh!(acc + 1) } |
          for (@r <- @stream!?("fold", 0, *plus1)) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, count, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(count, Some(5));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fold_sums_int_stream() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"[1, 2, 3, 4]"#,
        r#"
        new plus in {
          contract plus(retCh, @acc, @val) = { retCh!(acc + val) } |
          for (@r <- @stream!?("fold", 0, *plus)) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, sum, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(sum, Some(10));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fold_on_empty_stream_returns_init() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"[]"#,
        r#"
        new plus in {
          contract plus(retCh, @acc, @val) = { retCh!(acc + val) } |
          for (@r <- @stream!?("fold", 42, *plus)) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, val, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(val, Some(42));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn for_each_visits_every_value() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Handler appends each value to a running counter cell.
    let src = test_over_stream(
        r#"[1, 2, 3]"#,
        r#"
        new counterCell, handler in {
          counterCell!(0) |
          contract handler(retCh, @val) = {
            for (@c <- counterCell) {
              counterCell!(c + val) |
              retCh!(Nil)
            }
          } |
          for (@r <- @stream!?("forEach", *handler)) {
            for (@c <- counterCell) {
              @"out"!([r, c])
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    // Reply is [[true], 6].
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (r_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(r_ok, "forEach returns [true]");
    let count = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GInt(v)) => v,
        _ => panic!(),
    };
    assert_eq!(count, 6);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_method_returns_fserr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = test_over_stream(
        r#"["x"]"#,
        r#"for (@r <- @stream!?("wibble")) { @"out"!(r) }"#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn line_stream_chunk_unsupported() {
    // LineStream specialization: pass a chunkBuilder that always
    // returns FSERR_UNSUPPORTED, per spec §chunk method for LineStream.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = r#"
        new listState, producer, lineChunkBuilder in {
          listState!(["line1", "line2"]) |
          contract producer(retCh) = {
            for (@lst <- listState) {
              match lst {
                [] => { listState!([]) | retCh!([false, "EOS", ""]) }
                [head ...tail] => { listState!(tail) | retCh!([true, head]) }
              }
            }
          } |
          contract lineChunkBuilder(@_vals, retCh) = {
            retCh!([false, "FSERR_UNSUPPORTED", "chunk unsupported on LineStream"])
          } |
          for (@stream <- Stream!?(*producer, *lineChunkBuilder)) {
            for (@r <- @stream!?("chunk", 2)) {
              @"out"!(r)
            }
          }
        }
    "#;
    let src = with_lib(bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// Mi-1 regression: a chunk-builder that returns malformed data
/// (neither `[true, container]` nor `[false, code, msg]`) must be
/// intercepted and surfaced as `FSERR_IO` rather than propagated as
/// a shape-mismatched reply that would break the caller's own pattern
/// match.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chunk_builder_malformed_reply_yields_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = r#"
        new listState, producer, badBuilder in {
          listState!(["a", "b"]) |
          contract producer(retCh) = {
            for (@lst <- listState) {
              match lst {
                [] => { listState!([]) | retCh!([false, "EOS", ""]) }
                [head ...tail] => { listState!(tail) | retCh!([true, head]) }
              }
            }
          } |
          // Malformed: replies with a bare Int rather than a reply-shaped
          // list.  Stream.chunk() must catch this and return FSERR_IO,
          // not propagate the garbage.
          contract badBuilder(@_vals, retCh) = {
            retCh!(42)
          } |
          for (@stream <- Stream!?(*producer, *badBuilder)) {
            for (@r <- @stream!?("chunk", 2)) {
              @"out"!(r)
            }
          }
        }
    "#;
    let src = with_lib(bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
}
