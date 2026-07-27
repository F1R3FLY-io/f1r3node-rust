//! Compile-check + end-to-end runtime tests for File.rho and Dir.rho.
//!
//! Tests supply MOCK syscall contracts (fsRead, fsWrite, fsSeek, etc.)
//! that simulate an in-memory file, so the runtime never touches the
//! real filesystem.  Phase 6's Fs agent will bind these names to the
//! native URNs (rho:io:fs:native:1.0.0/*) at genesis time.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::rholang::implicits::single_expr;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;
use rholang::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use rspace_plus_plus::rspace::rspace::RSpace;

const FILE_RHO: &str = include_str!("../../casper/src/main/resources/File.rho");
const DIR_RHO: &str = include_str!("../../casper/src/main/resources/Dir.rho");

fn rand() -> crypto::rust::hash::blake2b512_random::Blake2b512Random {
    crypto::rust::hash::blake2b512_random::Blake2b512Random::create_from_bytes(&Vec::new())
}

/// Extract the body between `new ... in {` and the final `}` of a
/// `.rho` library file.  Skips commented-out `in {` occurrences (which
/// appear in the file's doc-comment header) by requiring the match to
/// be on a line NOT starting with `//` (possibly after whitespace).
fn lib_body(src: &str) -> String {
    let mut in_pos = None;
    let mut cursor = 0;
    while let Some(rel) = src[cursor..].find("in {") {
        let abs = cursor + rel;
        // Find the start of this line.
        let line_start = src[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line_head = src[line_start..abs].trim_start();
        if !line_head.starts_with("//") {
            in_pos = Some(abs);
            break;
        }
        cursor = abs + "in {".len();
    }
    let in_pos = in_pos.expect("must contain a non-comment `in {`");
    let body_start = in_pos + "in {".len();
    let last_brace = src.rfind('}').expect("must end with `}`");
    src[body_start..last_brace].to_string()
}

/// Compose File.rho + Dir.rho + mock syscall dispatchers + a test
/// snippet all in the same top-level `new` scope so agent bodies can
/// capture the syscall names.
fn with_libs(test_snippet: &str) -> String {
    format!(
        r#"
        new File, fdP, stateP, Dir, rootP, openFileImpl,
            fsRead, fsWrite, fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsStat, fsExists, fsOpen,
            mockFdCell
        in {{
          // -- Mock in-memory file "syscalls" ------------------------
          //
          // A single "file" state cell holds (bytes, cursor).  All
          // syscalls take a fake fd (ignored) plus their args.  Tests
          // that need multi-fd support would extend this to a Map
          // keyed by fd; for MVP a single file is enough.

          mockFdCell!(("".hexToBytes(), 0)) |

          contract fsRead(@_fd, @n, ret) = {{
            for (@state <- mockFdCell) {{
              match state {{
                (bytes, cur) => {{
                  match bytes.length() - cur {{
                    unread => {{
                      match unread <= 0 {{
                        true => {{
                          mockFdCell!(state) |
                          ret!([true, "".hexToBytes()])
                        }}
                        false => {{
                          match n <= unread {{
                            true => {{
                              mockFdCell!((bytes, cur + n)) |
                              ret!([true, bytes.slice(cur, cur + n)])
                            }}
                            false => {{
                              mockFdCell!((bytes, cur + unread)) |
                              ret!([true, bytes.slice(cur, cur + unread)])
                            }}
                          }}
                        }}
                      }}
                    }}
                  }}
                }}
              }}
            }}
          }} |

          contract fsWrite(@_fd, @xs, ret) = {{
            for (@state <- mockFdCell) {{
              match state {{
                (bytes, cur) => {{
                  // Overwrite [cur, cur+|xs|) with xs, extending
                  // bytes if needed.
                  match bytes.length() {{
                    blen => {{
                      match cur >= blen {{
                        true => {{
                          // Append (padding with 0s not needed if cur ==
                          // blen; but if cur > blen we'd need zero-fill.
                          // For MVP tests, always cur == blen when
                          // appending, so simple concat.
                          mockFdCell!(([bytes, xs].concatBytes(), cur + xs.length())) |
                          ret!([true, xs.length()])
                        }}
                        false => {{
                          // Overwrite in the middle.  Compute:
                          // newBytes = bytes[0..cur] ++ xs ++ bytes[cur+|xs|..]
                          match cur + xs.length() >= blen {{
                            true => {{
                              // xs extends past current end.
                              mockFdCell!(([bytes.slice(0, cur), xs].concatBytes(), cur + xs.length())) |
                              ret!([true, xs.length()])
                            }}
                            false => {{
                              mockFdCell!(([bytes.slice(0, cur), xs, bytes.slice(cur + xs.length(), blen)].concatBytes(), cur + xs.length())) |
                              ret!([true, xs.length()])
                            }}
                          }}
                        }}
                      }}
                    }}
                  }}
                }}
              }}
            }}
          }} |

          contract fsSeek(@_fd, @off, @whence, ret) = {{
            for (@state <- mockFdCell) {{
              match state {{
                (bytes, cur) => {{
                  match whence {{
                    "set" => {{
                      mockFdCell!((bytes, off)) |
                      ret!([true, off])
                    }}
                    "cur" => {{
                      mockFdCell!((bytes, cur + off)) |
                      ret!([true, cur + off])
                    }}
                    "end" => {{
                      mockFdCell!((bytes, bytes.length() + off)) |
                      ret!([true, bytes.length() + off])
                    }}
                    _ => {{
                      mockFdCell!(state) |
                      ret!([false, "FSERR_BAD_ARG", "bad whence"])
                    }}
                  }}
                }}
              }}
            }}
          }} |

          contract fsTell(@_fd, ret) = {{
            for (@state <<- mockFdCell) {{
              match state {{
                (_bytes, cur) => ret!([true, cur])
              }}
            }}
          }} |

          contract fsSize(@_fd, ret) = {{
            for (@state <<- mockFdCell) {{
              match state {{
                (bytes, _cur) => ret!([true, bytes.length()])
              }}
            }}
          }} |

          contract fsFlush(@_fd, ret) = {{
            ret!([true])
          }} |

          contract fsClose(@_fd, ret) = {{
            ret!([true])
          }} |

          contract fsOpen(@_root, @_rel, @_mode, ret) = {{
            // Return a fake fd = 1.
            ret!([true, 1])
          }} |

          contract fsStat(@_root, @_rel, ret) = {{
            ret!([true, {{"kind": "file", "size": 100}}])
          }} |

          contract fsExists(@_root, @_rel, ret) = {{
            ret!([true, true])
          }} |

          // -- Library bodies ---------------------------------------
          {file_body}
          |
          {dir_body}
          |
          // -- Test snippet -----------------------------------------
          {snippet}
        }}
        "#,
        file_body = lib_body(FILE_RHO),
        dir_body = lib_body(DIR_RHO),
        snippet = test_snippet,
    )
}

// ---------------------------------------------------------------------
// Compile checks
//
// File.rho and Dir.rho are library FRAGMENTS designed to be composed
// into a larger `new` scope that binds the fsRead/fsWrite/fsSeek/…
// syscall dispatchers (mock contracts in tests; native URN lookups
// in the Fs agent for genesis).  Standalone they have free variables
// by design.  Only the composed form is expected to parse.
// ---------------------------------------------------------------------

#[test]
fn with_libs_composes() {
    match ParBuilderUtil::mk_term(&with_libs("Nil")) {
        Ok(_) => {}
        Err(e) => panic!("with_libs(Nil) failed:\n{e:?}"),
    }
}

// ---------------------------------------------------------------------
// Runtime helpers
// ---------------------------------------------------------------------

fn extract_reply(par: &Par) -> (bool, String, Option<i64>, Option<Vec<u8>>) {
    let list = match single_expr(par).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        other => panic!("expected list reply, got {other:?}"),
    };
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("head not Bool"),
    };
    // The second element may be a ground value (String/Int/ByteArray)
    // OR an unforgeable name (a bundled File/Dir handle).  Fall through
    // to empty defaults on non-ground shapes so callers that only care
    // about the ok discriminator don't crash.
    let (s, i, b) = if list.ps.len() >= 2 {
        match single_expr(&list.ps[1]).and_then(|e| e.expr_instance) {
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
            "no reply on @\"out\".  Source (first 500 chars):\n{}\n\ntuplespace keys:\n{:?}",
            &src.chars().take(500).collect::<String>(),
            map.keys().collect::<Vec<_>>()
        ),
    };
    row.data[0].a.pars[0].clone()
}

// ---------------------------------------------------------------------
// File runtime tests
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_then_read_roundtrips() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@[true, k] <- @f!?("writeByteArray", "hello".toUtf8Bytes())) {
            // Seek back to start.
            for (@[true, _] <- @f!?("seek", 0, "set")) {
              for (@[true, bytes] <- @f!?("readN", 100)) {
                @"out"!([k, bytes])
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
    let k = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GInt(v)) => v,
        _ => panic!(),
    };
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(v)) => v,
        _ => panic!(),
    };
    assert_eq!(k, 5);
    assert_eq!(bytes, b"hello".to_vec());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_close_then_read_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@_ <- @f!?("close")) {
            for (@r <- @f!?("readN", 10)) {
              @"out"!(r)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_on_read_only_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "r")) {
          for (@r <- @f!?("writeByteArray", "x".toUtf8Bytes())) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_non_bytearray_rejects_cleanly() {
    // Type-guard regression — non-ByteArray must not brick the file.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@bad <- @f!?("writeByteArray", 42)) {
            // Follow-up query verifies the file is still responsive.
            for (@sz <- @f!?("size")) {
              @"out"!([bad, sz])
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
    assert_eq!(bad_code, "FSERR_BAD_ARG");
    let (sz_ok, _, _, _) = extract_reply(&outer.ps[1]);
    assert!(sz_ok, "file still responsive after bad arg");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_size_and_tell_after_write() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@[true, _] <- @f!?("writeByteArray", "abcde".toUtf8Bytes())) {
            for (@[true, sz] <- @f!?("size")) {
              for (@[true, pos] <- @f!?("tell")) {
                @"out"!([sz, pos])
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
    let sz = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GInt(v)) => v,
        _ => panic!(),
    };
    let pos = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GInt(v)) => v,
        _ => panic!(),
    };
    assert_eq!(sz, 5);
    assert_eq!(pos, 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_unknown_method_returns_fserr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("wibble")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

// ---------------------------------------------------------------------
// Dir runtime tests
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_stat_returns_record() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "r", *File)) {
          for (@r <- @d!?("stat", "some/file.txt")) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    // Reply is [true, {"kind": "file", "size": 100}].
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_exists_returns_bool() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "r", *File)) {
          for (@r <- @d!?("exists", "some/file.txt")) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_file_mints_a_file_agent() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("openFile", "some/file.txt", "r")) {
            match r {
              [true, f] => {
                // Verify we got a usable File — call tell() to check.
                for (@tellReply <- @f!?("tell")) {
                  @"out"!([r, tellReply])
                }
              }
              _ => @"out"!([r, [false, "openFile failed"]])
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
    let (open_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(open_ok, "openFile must succeed");
    let (tell_ok, _, _, _) = extract_reply(&outer.ps[1]);
    assert!(tell_ok, "tell on the minted File must work");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_file_rejects_mode_upgrade() {
    // Dir mode "r"; requesting File mode "rw" must fail with FSERR_UNSUPPORTED.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "r", *File)) {
          for (@r <- @d!?("openFile", "some/file.txt", "rw")) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_stat_non_string_rel_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "r", *File)) {
          for (@r <- @d!?("stat", 42)) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_unknown_method_returns_fserr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "r", *File)) {
          for (@r <- @d!?("wibble")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}
