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
        new File, fdP, stateP, Dir, rootP,
            openFileImpl, openDirImpl, parseRwxToBits, parseRwxLoop,
            fsRead, fsWrite, fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsStat, fsExists, fsOpen, fsTruncate, fsChmod, fsChown,
            fsRemoveFile, fsRemoveDir, fsRename, fsCopyFile,
            mockFdCell, chmodLog, chownLog, truncLog,
            rmFileLog, rmDirLog, renameLog, copyLog
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

          // fs_truncate — records n in truncLog for test verification.
          truncLog!([]) |
          contract fsTruncate(@_fd, @n, ret) = {{
            for (@log <- truncLog) {{
              truncLog!(log ++ [n]) |
              ret!([true])
            }}
          }} |

          // fs_chmod — records (root, rel, bits) in chmodLog.
          chmodLog!([]) |
          contract fsChmod(@root, @rel, @bits, ret) = {{
            for (@log <- chmodLog) {{
              chmodLog!(log ++ [(root, rel, bits)]) |
              ret!([true])
            }}
          }} |

          // fs_chown — records (root, rel, owner, group) in chownLog.
          chownLog!([]) |
          contract fsChown(@root, @rel, @owner, @group, ret) = {{
            for (@log <- chownLog) {{
              chownLog!(log ++ [(root, rel, owner, group)]) |
              ret!([true])
            }}
          }} |

          // fs_removeFile — records (root, rel) in rmFileLog.
          rmFileLog!([]) |
          contract fsRemoveFile(@root, @rel, ret) = {{
            for (@log <- rmFileLog) {{
              rmFileLog!(log ++ [(root, rel)]) |
              ret!([true])
            }}
          }} |

          // fs_removeDir — records (root, rel, recursive) in rmDirLog.
          rmDirLog!([]) |
          contract fsRemoveDir(@root, @rel, @recursive, ret) = {{
            for (@log <- rmDirLog) {{
              rmDirLog!(log ++ [(root, rel, recursive)]) |
              ret!([true])
            }}
          }} |

          // fs_rename — records (fromRoot, from, toRoot, to).
          renameLog!([]) |
          contract fsRename(@fromRoot, @from, @toRoot, @to, ret) = {{
            for (@log <- renameLog) {{
              renameLog!(log ++ [(fromRoot, from, toRoot, to)]) |
              ret!([true])
            }}
          }} |

          // fs_copyFile — records (fromRoot, from, toRoot, to).  Reply
          // includes a fake nBytes = 42 to exercise the [true, nBytes]
          // shape from spec §Dir.copyFile.
          copyLog!([]) |
          contract fsCopyFile(@fromRoot, @from, @toRoot, @to, ret) = {{
            for (@log <- copyLog) {{
              copyLog!(log ++ [(fromRoot, from, toRoot, to)]) |
              ret!([true, 42])
            }}
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
    // Bind every reply as a free variable and forward the collection
    // of replies to @"out" — a shape mismatch on any step would
    // otherwise block the outer `for` silently.  Assert on shapes in
    // Rust via extract_reply (m-2 fix).
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@writeReply <- @f!?("writeByteArray", "hello".toUtf8Bytes())) {
            for (@seekReply <- @f!?("seek", 0, "set")) {
              for (@readReply <- @f!?("readN", 100)) {
                @"out"!([writeReply, seekReply, readReply])
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
    let (write_ok, _, k, _) = extract_reply(&outer.ps[0]);
    assert!(write_ok, "writeByteArray must succeed");
    assert_eq!(k, Some(5));
    let (seek_ok, _, _, _) = extract_reply(&outer.ps[1]);
    assert!(seek_ok, "seek to start must succeed");
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[2]);
    assert!(read_ok, "readN must succeed");
    assert_eq!(bytes, Some(b"hello".to_vec()));
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
          for (@writeReply <- @f!?("writeByteArray", "abcde".toUtf8Bytes())) {
            for (@sizeReply <- @f!?("size")) {
              for (@tellReply <- @f!?("tell")) {
                @"out"!([writeReply, sizeReply, tellReply])
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
    let (write_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(write_ok);
    let (size_ok, _, sz, _) = extract_reply(&outer.ps[1]);
    assert!(size_ok);
    assert_eq!(sz, Some(5));
    let (tell_ok, _, pos, _) = extract_reply(&outer.ps[2]);
    assert!(tell_ok);
    assert_eq!(pos, Some(5));
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

// ---------------------------------------------------------------------
// Review-fix regression tests
// ---------------------------------------------------------------------

/// m-1 regression: File.close must propagate fsClose failures.
/// Constructs a bespoke source where fsClose returns [false, ...];
/// verifies File.close forwards it AND still marks the file closed
/// (so a subsequent readN returns FSERR_CLOSED).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_close_propagates_fs_close_error() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = format!(
        r#"
        new File, fdP, stateP, parseRwxToBits, parseRwxLoop,
            fsRead, fsWrite, fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsTruncate, fsChmod, fsChown
        in {{
          contract fsRead(@_fd, @_n, ret)  = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsWrite(@_fd, @_xs, ret) = {{ ret!([true, 0]) }} |
          contract fsSeek(@_fd, @_o, @_w, ret) = {{ ret!([true, 0]) }} |
          contract fsTell(@_fd, ret) = {{ ret!([true, 0]) }} |
          contract fsSize(@_fd, ret) = {{ ret!([true, 0]) }} |
          contract fsFlush(@_fd, ret) = {{ ret!([true]) }} |
          contract fsClose(@_fd, ret) = {{ ret!([false, "FSERR_IO", "simulated"]) }} |
          contract fsTruncate(@_fd, @_n, ret) = {{ ret!([true]) }} |
          contract fsChmod(@_r, @_p, @_b, ret) = {{ ret!([true]) }} |
          contract fsChown(@_r, @_p, @_o, @_g, ret) = {{ ret!([true]) }} |
          // parseRwxToBits stub — this test doesn't exercise it, but
          // File.rho's chmod method captures it as a free var so it
          // needs to be in scope.  A minimal identity stub suffices.
          contract parseRwxToBits(@_s, ret) = {{ ret!([true, 0]) }} |

{}

          |
          for (@f <- File!?(1, "/root", "rw")) {{
            for (@closeReply <- @f!?("close")) {{
              for (@subReply <- @f!?("readN", 8)) {{
                @"out"!([closeReply, subReply])
              }}
            }}
          }}
        }}
        "#,
        lib_body(FILE_RHO)
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (close_ok, close_code, _, _) = extract_reply(&outer.ps[0]);
    assert!(!close_ok, "close must propagate native failure");
    assert_eq!(close_code, "FSERR_IO");
    // State still transitioned — readN after close → FSERR_CLOSED.
    let (sub_ok, sub_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(!sub_ok);
    assert_eq!(sub_code, "FSERR_CLOSED");
}

/// readN(0) fix regression: n=0 is a valid no-op returning
/// [true, empty ByteArray] (matches Unix read(2)).  Cursor unmoved.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_n_zero_returns_empty_bytes() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@writeReply <- @f!?("writeByteArray", "abc".toUtf8Bytes())) {
            for (@readReply <- @f!?("readN", 0)) {
              for (@tellReply <- @f!?("tell")) {
                @"out"!([writeReply, readReply, tellReply])
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
    let (write_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(write_ok);
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(read_ok, "readN(0) must succeed");
    assert_eq!(bytes, Some(Vec::<u8>::new()), "readN(0) yields empty bytes");
    let (tell_ok, _, pos, _) = extract_reply(&outer.ps[2]);
    assert!(tell_ok);
    assert_eq!(pos, Some(3), "readN(0) must not advance the cursor");
}

/// Negative n still rejected.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_n_negative_returns_bad_arg() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("readN", -1)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

/// m-3 regression: Dir.openFile with a mode string outside the
/// whitelist (r/rw/w/w+/wx/w+x/a/a+) returns FSERR_BAD_ARG.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_file_rejects_unknown_mode() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("openFile", "some/file.txt", "xyzzy")) {
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

/// m-3 sanity: all whitelisted modes are accepted on a "rw" Dir.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_file_accepts_all_whitelisted_modes() {
    for mode in &["r", "rw", "w", "w+", "wx", "w+x", "a", "a+"] {
        let (space, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let src = with_libs(&format!(
            r#"
            for (@d <- Dir!?("/root", "rw", *File)) {{
              for (@r <- @d!?("openFile", "some/file.txt", "{}")) {{
                @"out"!(r)
              }}
            }}
            "#,
            mode
        ));
        let reply = eval_and_read_out(&space, &reducer, &src).await;
        let (ok, _, _, _) = extract_reply(&reply);
        assert!(ok, "mode {:?} must be accepted on a rw Dir", mode);
    }
}

// ---------------------------------------------------------------------
// Second-slice tests: File.truncate/chmod/chown + parseRwxToBits +
// Dir.openDir + 6 Dir mutation methods.
// ---------------------------------------------------------------------

// -- File.truncate ----------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_truncate_rw_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("truncate", 100)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "truncate must succeed on rw file");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_truncate_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "r")) {
          for (@r <- @f!?("truncate", 100)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_truncate_negative_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("truncate", -1)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_truncate_non_int_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("truncate", "not int")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

// -- File.chmod -------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_valid_rwx_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("chmod", "rwxr-xr-x")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_octal_string_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("chmod", "0755")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_symbolic_delta_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("chmod", "u+x")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_wrong_char_at_position_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Position 0 must be 'r' or '-'; 'w' is invalid.
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("chmod", "wwxr-xr-x")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_all_dashes_yields_zero_bits() {
    // ---------  → 0 bits.  Verifies parseRwxToBits handles the
    // all-'-' case cleanly.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("chmod", "---------")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

// -- File.chown -------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_both_strings_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("chown", "alice", "wheel")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_nil_group_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("chown", "alice", Nil)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_empty_owner_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("chown", "", Nil)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_non_string_non_nil_owner_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "rw")) {
          for (@r <- @f!?("chown", 42, Nil)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

// -- Dir.openDir ------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_dir_mints_nested_dir() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("openDir", "subdir", "r")) {
            match r {
              [true, nested] => {
                // Verify the nested Dir is usable — call stat on it.
                for (@statReply <- @nested!?("stat", "child.txt")) {
                  @"out"!([r, statReply])
                }
              }
              _ => @"out"!([r, [false, "openDir failed"]])
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
    assert!(open_ok, "openDir must succeed");
    let (stat_ok, _, _, _) = extract_reply(&outer.ps[1]);
    assert!(stat_ok, "nested Dir.stat must succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_dir_rejects_rw_from_readonly() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "r", *File)) {
          for (@r <- @d!?("openDir", "subdir", "rw")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_dir_rejects_invalid_mode() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("openDir", "subdir", "w+")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

// -- Dir mutation methods (all rw-gated) ------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_file_rw_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("removeFile", "old.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_file_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "r", *File)) {
          for (@r <- @d!?("removeFile", "old.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_dir_recursive_succeeds_on_rw() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("removeDir", "olddir", true)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_dir_non_bool_recursive_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("removeDir", "olddir", "yes")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_rename_rw_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("rename", "old.txt", "new.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_rename_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "r", *File)) {
          for (@r <- @d!?("rename", "old.txt", "new.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_copy_file_returns_nbytes() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("copyFile", "src.txt", "dst.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, n, _) = extract_reply(&reply);
    assert!(ok);
    // The mock returns [true, 42] per copyLog contract.
    assert_eq!(n, Some(42));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_chmod_valid_mode_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("chmod", "config.json", "rw-r--r--")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_chmod_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "r", *File)) {
          for (@r <- @d!?("chmod", "config.json", "rw-r--r--")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_chown_rw_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("chown", "f.txt", "alice", "wheel")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_chown_empty_string_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "rw", *File)) {
          for (@r <- @d!?("chown", "f.txt", "", Nil)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}
