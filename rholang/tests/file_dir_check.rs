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
const STREAM_RHO: &str = include_str!("../../casper/src/main/resources/Stream.rho");

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
            Stream, paramsP, gatherN, foldLoop, forEachLoop,
            openFileImpl, openDirImpl, joinRel,
            parseRwxToBits, parseRwxLoop,
            fsRead, fsReadAt, fsWrite, fsSeek, fsTell, fsSize, fsFlush, fsClose,
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

          // fsReadAt — positional read (pread semantics).  Does NOT
          // touch mockFdCell's cursor; returns bytes[offset, offset+n)
          // (or fewer if past EOF).
          contract fsReadAt(@_fd, @off, @n, ret) = {{
            for (@state <<- mockFdCell) {{
              match state {{
                (bytes, _cur) => {{
                  match bytes.length() {{
                    blen => {{
                      match off >= blen {{
                        true => ret!([true, "".hexToBytes()])
                        false => {{
                          match off + n <= blen {{
                            true => ret!([true, bytes.slice(off, off + n)])
                            false => ret!([true, bytes.slice(off, blen)])
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

          // fsStat: distinguishes directories from files by rel name.
          //   - "subdir" → directory (openDir mint-time validation)
          //   - "missing.txt" → FSERR_NOTFOUND (openFile creation-mode
          //     regression: stat may fail on a nonexistent target and
          //     openFileImpl must pass through to fsOpen)
          //   - everything else → regular file
          contract fsStat(@_root, @rel, ret) = {{
            match rel {{
              "subdir"      => ret!([true, {{"kind": "dir", "size": 0}}])
              "missing.txt" => ret!([false, "FSERR_NOTFOUND", "no such file"])
              _             => ret!([true, {{"kind": "file", "size": 100}}])
            }}
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
          {stream_body}
          |
          // -- Test snippet -----------------------------------------
          {snippet}
        }}
        "#,
        file_body = lib_body(FILE_RHO),
        dir_body = lib_body(DIR_RHO),
        stream_body = lib_body(STREAM_RHO),
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "r")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@d <- Dir!?("/root", "", "r", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", *File)) {
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
            fsRead, fsReadAt, fsWrite, fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsTruncate, fsChmod, fsChown, Stream
        in {{
          contract fsRead(@_fd, @_n, ret)  = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsReadAt(@_fd, @_o, @_n, ret) = {{ ret!([true, "".hexToBytes()]) }} |
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
          // Stream stub — File.rho's bytes()/bytesAt() capture it as a
          // free var to mint stream handles.  This bespoke test doesn't
          // exercise bytes(), so a minimal identity constructor suffices.
          contract Stream(retCh, @_producer, @_builder) = {{ retCh!(Nil) }} |

{}

          |
          for (@f <- File!?(1, "/root", "test.txt", "rw")) {{
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
            for (@d <- Dir!?("/root", "", "rw", *File)) {{
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "r")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@r <- @d!?("chown", "f.txt", "", Nil)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

// ---------------------------------------------------------------------
// Phase 5 second-slice review-fix regressions
//
// Lock in:
//   B-1 openDir sandbox escape → subPath state; canonRoot immutable
//   B-2 parseRwx UTF-8 panic → byte-level parsing
//   B-3 File.chmod/chown broken empty-rel → state carries rel
//   M-1 File.chmod/chown not gated on write-mode → gated
//   M-2 openDir minted broken handles → fsStat-validated
// ---------------------------------------------------------------------

/// B-1 regression: a nested Dir's method dispatches with the COMPOSED
/// subPath, preserving the trusted original canonRoot.  This is the
/// property that closes the escape: canonRoot never absorbs caller-
/// controlled bytes, and safe_descend always walks the full joined
/// path from the trusted root — so a `..` at any level is rejected by
/// Phase 1's quarantine regardless of how deeply Dirs are nested.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_nested_dispatches_with_composed_subpath() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@openReply <- @d!?("openDir", "subdir", "rw")) {
            match openReply {
              [true, nested] => {
                for (@chmodReply <- @nested!?("chmod", "f.txt", "rw-r--r--")) {
                  for (@log <<- chmodLog) {
                    @"out"!([chmodReply, log])
                  }
                }
              }
              _ => @"out"!([openReply, []])
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
    let (chmod_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(chmod_ok, "nested Dir.chmod must succeed");
    let log_list = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("chmodLog was not a list"),
    };
    let last = log_list.ps.last().expect("chmodLog empty");
    let tup = match single_expr(last).unwrap().expr_instance {
        Some(ExprInstance::ETupleBody(t)) => t,
        _ => panic!("log entry not a tuple"),
    };
    let root = match single_expr(&tup.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    let rel = match single_expr(&tup.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert_eq!(
        root, "/root",
        "canonRoot must be the trusted original, not composed with subPath"
    );
    assert_eq!(
        rel, "subdir/f.txt",
        "rel must be joinRel(subPath, childRel)"
    );
}

/// B-2 regression: parseRwx does not panic on a 9-byte string that
/// contains a multi-byte UTF-8 codepoint.  U+1F600 is 4 bytes, plus
/// 5 ASCII bytes = 9 total; passes the length guard.  The byte-level
/// loop sees 0xF0 at position 0 which matches neither '-' (45) nor
/// 'r' (114), so it falls to FSERR_BAD_ARG cleanly — never slicing
/// mid-codepoint (which would panic and fork consensus).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_multibyte_utf8_rejects_without_panic() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        "for (@f <- File!?(1, \"/root\", \"test.txt\", \"rw\")) {\
           for (@r <- @f!?(\"chmod\", \"\u{1F600}rwxr-\")) { @\"out\"!(r) }\
         }",
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "multi-byte UTF-8 input must be rejected, not accepted");
    assert_eq!(code, "FSERR_BAD_ARG");
}

/// B-3 regression: File.chmod dispatches with the stored rel, not "".
/// Prior slice passed "" so every real chmod broke on Phase 1's
/// safe_descend, which returns QuarantineError::Empty on empty rel.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_dispatches_with_stored_rel_not_empty() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "config.json", "rw")) {
          for (@_ <- @f!?("chmod", "rw-r--r--")) {
            for (@log <<- chmodLog) { @"out"!(log) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let log_list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let last = log_list.ps.last().expect("chmodLog empty");
    let tup = match single_expr(last).unwrap().expr_instance {
        Some(ExprInstance::ETupleBody(t)) => t,
        _ => panic!(),
    };
    let rel = match single_expr(&tup.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert_eq!(
        rel, "config.json",
        "chmod must dispatch with the File's stored rel"
    );
    assert_ne!(
        rel, "",
        "chmod must NOT pass empty rel (Phase 1 rejects with QuarantineError::Empty)"
    );
}

/// B-3 regression, chown variant.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_dispatches_with_stored_rel_not_empty() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "data.bin", "rw")) {
          for (@_ <- @f!?("chown", "alice", "wheel")) {
            for (@log <<- chownLog) { @"out"!(log) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let log_list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let last = log_list.ps.last().expect("chownLog empty");
    let tup = match single_expr(last).unwrap().expr_instance {
        Some(ExprInstance::ETupleBody(t)) => t,
        _ => panic!(),
    };
    let rel = match single_expr(&tup.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert_eq!(rel, "data.bin");
    assert_ne!(rel, "");
}

/// M-1 regression: File.chmod on a read-only File returns
/// FSERR_UNSUPPORTED (spec §File > Permissions:1005).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_on_readonly_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "r")) {
          for (@r <- @f!?("chmod", "rw-r--r--")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// M-1 regression, chown variant.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_on_readonly_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "r")) {
          for (@r <- @f!?("chown", "alice", "wheel")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// M-2 regression: openDir on a path that fsStat reports as a file
/// (kind == "file") returns FSERR_BAD_ARG instead of minting a broken
/// Dir handle whose every subsequent method would fail with confusing
/// errors.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_dir_on_non_directory_returns_bad_arg() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Mock fsStat returns "file" for anything other than "subdir".
    // openDir("regular.txt", "r") hits the "not a directory" arm.
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@r <- @d!?("openDir", "regular.txt", "r")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

/// joinRel edge case: an empty subPath must produce just `rel`, never
/// `/rel`.  A leading `/` would make the path absolute and Phase 1's
/// safe_descend would reject with EscapesRoot — breaking every root-
/// Dir method call.  Verified via chmodLog on a root Dir.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn join_rel_empty_sub_path_produces_bare_rel() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@_ <- @d!?("chmod", "foo.txt", "rw-r--r--")) {
            for (@log <<- chmodLog) { @"out"!(log) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let log_list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let last = log_list.ps.last().expect("chmodLog empty");
    let tup = match single_expr(last).unwrap().expr_instance {
        Some(ExprInstance::ETupleBody(t)) => t,
        _ => panic!(),
    };
    let rel = match single_expr(&tup.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert_eq!(
        rel, "foo.txt",
        "joinRel(\"\", rel) must yield rel, not \"/rel\""
    );
    assert!(
        !rel.starts_with('/'),
        "joinRel must not yield an absolute path (Phase 1 would reject)"
    );
}

// ---------------------------------------------------------------------
// Coverage backfill
//
// Fills gaps identified during the Phase 5 slice-2 review:
//   - Closed-state coverage for every File method (previously only
//     readN was checked)
//   - Direct writeString tests (previously only exercised as a
//     delegation from writeByteArray)
//   - Bad-arg on seek offset/whence
//   - flush() happy path
//   - Dir.copyFile rw-gate (previously untested)
//   - Dir.chmod malformed-rwx path (via parseRwxToBits from Dir side)
//   - Non-string rel bad-arg on Dir.exists/removeFile/rename
//
// ---------------------------------------------------------------------

// -- Closed-state for File methods that were previously untested for it

async fn close_then_call(method_call: &str) -> Par {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(&format!(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {{
          for (@_ <- @f!?("close")) {{
            for (@r <- @f{}) {{ @"out"!(r) }}
          }}
        }}
        "#,
        method_call
    ));
    eval_and_read_out(&space, &reducer, &src).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytearray_on_closed_returns_fserr_closed() {
    let reply = close_then_call(r#"!?("writeByteArray", "hi".toUtf8Bytes())"#).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_string_on_closed_returns_fserr_closed() {
    let reply = close_then_call(r#"!?("writeString", "hi")"#).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_seek_on_closed_returns_fserr_closed() {
    let reply = close_then_call(r#"!?("seek", 0, "set")"#).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_tell_on_closed_returns_fserr_closed() {
    let reply = close_then_call(r#"!?("tell")"#).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_size_on_closed_returns_fserr_closed() {
    let reply = close_then_call(r#"!?("size")"#).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_flush_on_closed_returns_fserr_closed() {
    let reply = close_then_call(r#"!?("flush")"#).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_truncate_on_closed_returns_fserr_closed() {
    let reply = close_then_call(r#"!?("truncate", 0)"#).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_on_closed_returns_fserr_closed() {
    let reply = close_then_call(r#"!?("chmod", "rw-r--r--")"#).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_on_closed_returns_fserr_closed() {
    let reply = close_then_call(r#"!?("chown", "alice", "wheel")"#).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

// -- writeString direct coverage --------------------------------------

/// writeString round-trips a UTF-8 payload through writeByteArray.  The
/// previous suite only exercised this method transitively via a
/// writeByteArray test; this asserts writeString itself accepts a
/// String and reports the byte-count in its reply.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_string_round_trips_utf8() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@writeReply <- @f!?("writeString", "hello")) {
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
    let (write_ok, _, n, _) = extract_reply(&outer.ps[0]);
    assert!(write_ok, "writeString must succeed");
    assert_eq!(n, Some(5), "writeString must report the UTF-8 byte count");
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[2]);
    assert!(read_ok);
    assert_eq!(bytes, Some(b"hello".to_vec()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_string_non_string_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@r <- @f!?("writeString", 42)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

// -- File.seek bad-arg ------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_seek_non_int_offset_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@r <- @f!?("seek", "notint", "set")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_seek_non_string_whence_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@r <- @f!?("seek", 0, 42)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

// -- File.flush happy path --------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_flush_on_open_returns_true() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@r <- @f!?("flush")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

// -- Dir bad-arg coverage ---------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_exists_non_string_rel_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "r", *File)) {
          for (@r <- @d!?("exists", 42)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_file_non_string_rel_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@r <- @d!?("removeFile", 42)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_rename_non_string_from_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@r <- @d!?("rename", 42, "new.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_rename_non_string_to_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@r <- @d!?("rename", "old.txt", 42)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

/// The single glaring gap in the original attenuation coverage:
/// Dir.copyFile lacked a readonly-rejection test.  Every other mutation
/// method has one; copyFile did not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_copy_file_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "r", *File)) {
          for (@r <- @d!?("copyFile", "src.txt", "dst.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// Dir.chmod exercises parseRwxToBits from the Dir side (previously
/// only the File side was exercised for malformed input).  Verifies
/// the parser is reachable and rejects the same inputs consistently.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_chmod_malformed_rwx_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@r <- @d!?("chmod", "f.txt", "not-a-mode")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

// ---------------------------------------------------------------------
// Phase 5 slice 3: stream producers (bytes, bytesAt) + openFile stat-
// verify.
// ---------------------------------------------------------------------

// -- File.bytes() sequential byte producer ----------------------------

/// Priming write, then `bytes()` streams the bytes back.  Verifies:
///   - bytes() returns [true, streamHandle]
///   - next() on the stream yields single 1-byte ByteArrays
///   - the byte values match the priming write in order
///   - EOS is signaled after the last byte
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_streams_content_byte_by_byte() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Write "ab" (0x61, 0x62), seek to 0, then bytes() and drain 3 next()
    // calls: [true, 0x61-BA], [true, 0x62-BA], [false, "EOS", ...].
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@_ <- @f!?("writeByteArray", "ab".toUtf8Bytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@bytesReply <- @f!?("bytes")) {
                match bytesReply {
                  [true, stream] => {
                    for (@n1 <- @stream!?("next")) {
                      for (@n2 <- @stream!?("next")) {
                        for (@n3 <- @stream!?("next")) {
                          @"out"!([n1, n2, n3])
                        }
                      }
                    }
                  }
                  _ => @"out"!([bytesReply, Nil, Nil])
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
    let (ok1, _, _, b1) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(b1, Some(vec![0x61u8]));
    let (ok2, _, _, b2) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(b2, Some(vec![0x62u8]));
    let (ok3, code3, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok3, "third next() must signal EOS");
    assert_eq!(code3, "EOS");
}

/// bytes() over an empty file signals EOS immediately.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_empty_file_eos_immediately() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@bytesReply <- @f!?("bytes")) {
            match bytesReply {
              [true, stream] => {
                for (@r <- @stream!?("next")) { @"out"!(r) }
              }
              _ => @"out"!(bytesReply)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

/// bytes() on a closed File returns FSERR_CLOSED without touching the fd.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@_ <- @f!?("close")) {
            for (@r <- @f!?("bytes")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

/// bytes()'s chunk builder correctly concatBytes the per-element 1-byte
/// arrays: chunk(2) on ["a", "b"] yields a 2-byte ByteArray "ab".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_chunk_concatenates_via_builder() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@_ <- @f!?("writeByteArray", "ab".toUtf8Bytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@bytesReply <- @f!?("bytes")) {
                match bytesReply {
                  [true, stream] => {
                    for (@r <- @stream!?("chunk", 2)) { @"out"!(r) }
                  }
                  _ => @"out"!(bytesReply)
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
        Some(b"ab".to_vec()),
        "chunk(2) must return a 2-byte ByteArray built via concatBytes"
    );
}

// -- File.bytesAt(offset, length) positional byte producer ------------

/// bytesAt(offset=1, length=2) over "abcd" returns "bc".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_at_positional_read() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@_ <- @f!?("writeByteArray", "abcd".toUtf8Bytes())) {
            for (@bytesReply <- @f!?("bytesAt", 1, 2)) {
              match bytesReply {
                [true, stream] => {
                  for (@r <- @stream!?("chunk", 4)) { @"out"!(r) }
                }
                _ => @"out"!(bytesReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, bytes) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(bytes, Some(b"bc".to_vec()));
}

/// bytesAt with length=Nil reads to EOF.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_at_nil_length_reads_to_eof() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@_ <- @f!?("writeByteArray", "abcd".toUtf8Bytes())) {
            for (@bytesReply <- @f!?("bytesAt", 2, Nil)) {
              match bytesReply {
                [true, stream] => {
                  for (@r <- @stream!?("chunk", 10)) { @"out"!(r) }
                }
                _ => @"out"!(bytesReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, bytes) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(bytes, Some(b"cd".to_vec()));
}

/// bytesAt does NOT touch the sequential cursor.  Verified by writing
/// "abcd", calling bytesAt(0, 4) then draining, then tell() — the
/// cursor should still be at 4 (from the write), not moved by the
/// positional read.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_at_does_not_move_cursor() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@_ <- @f!?("writeByteArray", "abcd".toUtf8Bytes())) {
            for (@bytesReply <- @f!?("bytesAt", 0, 4)) {
              match bytesReply {
                [true, stream] => {
                  for (@_ <- @stream!?("chunk", 10)) {
                    for (@tellReply <- @f!?("tell")) {
                      @"out"!(tellReply)
                    }
                  }
                }
                _ => @"out"!(bytesReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, pos, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(
        pos,
        Some(4),
        "cursor must remain at the write's endpoint; bytesAt is positional"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_at_negative_offset_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@r <- @f!?("bytesAt", -1, 4)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_at_negative_length_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@r <- @f!?("bytesAt", 0, -1)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_at_non_int_offset_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@r <- @f!?("bytesAt", "zero", 4)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_at_bad_length_type_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // length must be Int OR Nil; a String is neither.
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@r <- @f!?("bytesAt", 0, "all")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_at_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@_ <- @f!?("close")) {
            for (@r <- @f!?("bytesAt", 0, 4)) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

// -- Dir.openFile stat-verify -----------------------------------------

/// Plan §294: Dir.openFile stat-verifies the target before dispatching
/// to fsOpen.  If the target is a directory (mock returns
/// {"kind": "dir"} for rel=="subdir"), openFile rejects with
/// FSERR_BAD_ARG rather than trying to fsOpen a directory.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_file_on_directory_target_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // The mock fsStat returns kind == "dir" for rel exactly "subdir".
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@r <- @d!?("openFile", "subdir", "r")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "openFile on a directory target must reject");
    assert_eq!(code, "FSERR_BAD_ARG");
}

/// stat-verify sanity: openFile on a regular file target still works
/// (regression against the stat-verify accidentally rejecting
/// legitimate files).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_file_on_regular_file_still_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@r <- @d!?("openFile", "regular.txt", "r")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "openFile on a regular file must still succeed");
}

// ---------------------------------------------------------------------
// Slice-3 review coverage: stream-refill boundary, bytesAt edge cases,
// openFile creation-mode when stat fails NOT_FOUND, chunk-builder
// edge cases.
// ---------------------------------------------------------------------

/// Reviewer gap: bytes() drains across the 4KB internal buffer
/// boundary.  Stages a 4128-byte payload (just past 4096) via
/// `mockFdCell`; producer must issue two fsRead calls (first fills to
/// 4096, second returns 32 bytes remaining, third returns empty →
/// EOS).  Verifies via round-trip equality — recovered bytes must
/// match the staged payload byte-for-byte, so any reordering,
/// duplication, or loss across the refill boundary is caught.
///
/// NOTE ON RUNTIME: bytes() vends 1 byte per next() in the MVP
/// (matches the spec's §ByteStream element-is-u8 shape), so a
/// 4128-byte drain issues ~4130 tuplespace round-trips through the
/// Stream + producer.  In the interpreter this runs in ~10 minutes
/// on a laptop — too slow for default CI.  Marked `#[ignore]`; run
/// with `cargo test -p rholang --test file_dir_check -- --ignored`
/// or in a nightly job.  A follow-up slice that vends larger chunks
/// per next() would make it fast.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn file_bytes_streams_across_refill_boundary() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        // Build a 4128-byte payload:
        //   b16  = 16 bytes ("0123456789ABCDEF")
        //   b512 = 32 × b16 = 512 bytes
        //   b4096 = 8 × b512 = 4096 bytes
        //   big   = b4096 ++ (2 × b16) = 4128 bytes
        // 4128 > 4096, so bytes() must refill mid-stream.
        match "0123456789ABCDEF".toUtf8Bytes() {
          b16 => {
            match [b16, b16, b16, b16, b16, b16, b16, b16,
                   b16, b16, b16, b16, b16, b16, b16, b16,
                   b16, b16, b16, b16, b16, b16, b16, b16,
                   b16, b16, b16, b16, b16, b16, b16, b16].concatBytes() {
              b512 => {
                match [b512, b512, b512, b512, b512, b512, b512, b512].concatBytes() {
                  b4096 => {
                    match [b4096, b16, b16].concatBytes() {
                      big => {
                        for (@_ <- mockFdCell) {
                          mockFdCell!((big, 0)) |
                          for (@f <- File!?(1, "/root", "test.txt", "rw")) {
                            for (@bytesReply <- @f!?("bytes")) {
                              match bytesReply {
                                [true, stream] => {
                                  for (@r <- @stream!?("chunk", 4128)) {
                                    match r {
                                      [true, gotBytes] => {
                                        @"out"!([true,
                                                 gotBytes.length(),
                                                 gotBytes == big])
                                      }
                                      _ => @"out"!([false, 0, false])
                                    }
                                  }
                                }
                                _ => @"out"!([false, 0, false])
                              }
                            }
                          }
                        }
                      }
                    }
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
        _ => panic!("expected list reply"),
    };
    let ok = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("head not Bool"),
    };
    assert!(ok, "chunk over refill boundary must succeed");
    let length = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GInt(n)) => n,
        _ => panic!("length not Int"),
    };
    assert_eq!(
        length, 4128,
        "must recover full 4128-byte payload across the refill boundary"
    );
    let equal_to_staged = match single_expr(&outer.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("equalToStaged not Bool"),
    };
    assert!(
        equal_to_staged,
        "recovered bytes must equal staged payload byte-for-byte (verifies \
         no reordering / duplication across the 4KB refill boundary)"
    );
}

/// Reviewer gap: bytesAt(offset, 0) is a valid no-op — analogous to
/// readN(0) — and must not read from the fd.  Producer sees `rem = 0`
/// on the first refill attempt and returns EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_at_zero_length_yields_eos_immediately() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@_ <- @f!?("writeByteArray", "abcd".toUtf8Bytes())) {
            for (@bytesReply <- @f!?("bytesAt", 0, 0)) {
              match bytesReply {
                [true, stream] => {
                  for (@r <- @stream!?("next")) { @"out"!(r) }
                }
                _ => @"out"!(bytesReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "bytesAt(offset, 0) must not yield any bytes");
    assert_eq!(code, "EOS");
}

/// Reviewer gap: bytesAt with an offset past EOF yields EOS
/// immediately.  Producer's first fsReadAt returns empty bytes,
/// which maps to EOS in the producer's `got == 0` arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_at_offset_beyond_eof_yields_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@_ <- @f!?("writeByteArray", "ab".toUtf8Bytes())) {
            for (@bytesReply <- @f!?("bytesAt", 100, 10)) {
              match bytesReply {
                [true, stream] => {
                  for (@r <- @stream!?("next")) { @"out"!(r) }
                }
                _ => @"out"!(bytesReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

/// Reviewer gap: Dir.openFile in a creation mode ("w") must proceed
/// to fsOpen even when fsStat fails FSERR_NOTFOUND — the file may
/// legitimately not exist yet.  The stat-verify's "stat failed →
/// pass through" arm.  Mock fsStat returns NOTFOUND for
/// "missing.txt"; fsOpen unconditionally returns [true, 1].  The
/// call must succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_file_creation_mode_passes_stat_notfound() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", *File)) {
          for (@r <- @d!?("openFile", "missing.txt", "w")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "openFile in creation mode must succeed even when stat says NOT_FOUND"
    );
}

/// Reviewer gap: chunk() on an already-exhausted bytes() stream
/// (empty file, then chunk instead of next) returns EOS via the
/// early-terminal check in Stream.chunk — the builder is never
/// invoked with an empty list.  This locks in that behavior.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_chunk_after_exhaustion_returns_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@bytesReply <- @f!?("bytes")) {
            match bytesReply {
              [true, stream] => {
                // Drain via next() until EOS, then chunk() must also
                // report EOS (not [true, empty-bytes]).
                for (@_first <- @stream!?("next")) {
                  for (@r <- @stream!?("chunk", 10)) { @"out"!(r) }
                }
              }
              _ => @"out"!(bytesReply)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

/// Reviewer gap: chunk-builder single-element case.  Write 1 byte,
/// chunk(1) yields a 1-byte ByteArray via concatBytes (which handles
/// the 1-element list correctly).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_bytes_chunk_single_element() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw")) {
          for (@_ <- @f!?("writeByteArray", "X".toUtf8Bytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@bytesReply <- @f!?("bytes")) {
                match bytesReply {
                  [true, stream] => {
                    for (@r <- @stream!?("chunk", 1)) { @"out"!(r) }
                  }
                  _ => @"out"!(bytesReply)
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
    assert_eq!(bytes, Some(b"X".to_vec()));
}
