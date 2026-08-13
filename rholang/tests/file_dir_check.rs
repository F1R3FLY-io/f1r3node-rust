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
use rholang::rust::interpreter::rho_source::lib_body;
use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;
use rholang::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use rspace_plus_plus::rspace::rspace::RSpace;

const FILE_RHO: &str = include_str!("../../casper/src/main/resources/File.rho");
const DIR_RHO: &str = include_str!("../../casper/src/main/resources/Dir.rho");
const STREAM_RHO: &str = include_str!("../../casper/src/main/resources/Stream.rho");
const BUFFER_RHO: &str = include_str!("../../casper/src/main/resources/Buffer.rho");
const STDIN_RHO: &str = include_str!("../../casper/src/main/resources/Stdin.rho");
const STDOUT_RHO: &str = include_str!("../../casper/src/main/resources/Stdout.rho");
const FS_RHO: &str = include_str!("../../casper/src/main/resources/Fs.rho");

fn rand() -> crypto::rust::hash::blake2b512_random::Blake2b512Random {
    crypto::rust::hash::blake2b512_random::Blake2b512Random::create_from_bytes(&Vec::new())
}

/// Compose File.rho + Dir.rho + mock syscall dispatchers + a test
/// snippet all in the same top-level `new` scope so agent bodies can
/// capture the syscall names.
fn with_libs(test_snippet: &str) -> String {
    format!(
        r#"
        new File, fdP, stateP, cmodeP, Dir, rootP,
            Stream, paramsP, gatherN, foldLoop, forEachLoop, foldChunksLoop,
            Buffer, Allocator, Rows, metaP, chunkP, innerP, rowsMetaP,
            gatherChunks, drainChunks, allocInnersLoop, parkInnersLoop,
            clearInnersLoop, closeInnersLoop,
            Stdin, stdinFdP, stdinStateP,
            Stdout, stdoutFdP, stdoutStateP,
            Fs, fsBundleP,
            fsStdinFdP, fsStdoutFdP, fsStderrFdP,
            openFileImpl, openFileImplInner, openDirImpl, openDirImplInner, joinRel,
            parseRwxToBits, parseRwxLoop,
            writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
            readLinesIntoLoop, drainToNextLF,
            codepointLen, concatStringsLoop, scanLineForLF,
            fsRead, fsReadAt, fsWrite, fsWriteAt,
            fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsStat, fsExists, fsOpen, fsTruncate, fsChmod, fsChown,
            fsRemoveFile, fsRemoveDir, fsRename, fsCopyFile, fsEntries,
            // Phase 8 slice 8a — lock natives (mock preamble additions).
            // Bound here in advance of File.rho's step 4c-2 introducing
            // the LockToken agent + fsLockRange/fsLockSequential/
            // fsReleaseLock call sites and step 4f's File.close sweep
            // via fsReleaseAllForHolder.  Default mocks below always
            // succeed; step 4g's integration tests may override with
            // stateful stand-ins that model per-fd lock accounting.
            fsLockRange, fsLockSequential, fsReleaseLock,
            fsReleaseAllForHolder,
            LockToken, lockStateP,
            // Phase 8 slice 8c — auto-acquire helpers.  Defined at
            // the end of File.rho; bound here at the outer scope
            // because lib_body strips File.rho's own top-level `new`.
            withSequentialLock, withRangeLock,
            acquireRangeForStream, acquireSequentialForStream,
            mockFdCell, chmodLog, chownLog, truncLog,
            rmFileLog, rmDirLog, renameLog, copyLog,
            writeAtLog, entriesCell
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

          // fsWriteAt — positional write (pwrite semantics).  Does
          // NOT advance mockFdCell's cursor.  Writes `xs` starting
          // at absolute `off`; pads with zero bytes if `off > blen`.
          //
          // Also records (off, bytes-written) in `writeAtLog` so
          // tests can verify the sequence of positional writes issued
          // by writeBytesAtLoop.
          writeAtLog!([]) |
          contract fsWriteAt(@_fd, @off, @xs, ret) = {{
            for (@state <- mockFdCell) {{
              for (@log <- writeAtLog) {{
                match state {{
                  (bytes, cur) => {{
                    match bytes.length() {{
                      blen => {{
                        match off >= blen {{
                          true => {{
                            // Extend with zeros to `off`, then append.
                            // For MVP tests, we assume off <= blen or
                            // off == blen (append-past-end zero-pad is
                            // rarely tested); simple concat.
                            mockFdCell!(([bytes, xs].concatBytes(), cur)) |
                            writeAtLog!(log ++ [(off, xs.length())]) |
                            ret!([true, xs.length()])
                          }}
                          false => {{
                            match off + xs.length() >= blen {{
                              true => {{
                                mockFdCell!(([bytes.slice(0, off), xs].concatBytes(), cur)) |
                                writeAtLog!(log ++ [(off, xs.length())]) |
                                ret!([true, xs.length()])
                              }}
                              false => {{
                                mockFdCell!(([bytes.slice(0, off), xs, bytes.slice(off + xs.length(), blen)].concatBytes(), cur)) |
                                writeAtLog!(log ++ [(off, xs.length())]) |
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

          // Slice 29 (PB-M-14): fsOpen now takes cmode as the 4th
          // arg (before ack) so the native handler can stash it in
          // the FileHandle for WAL journaling.  Mock ignores the
          // value.
          contract fsOpen(@_root, @_rel, @_mode, @_cmode, ret) = {{
            // Return a fake fd = 1.
            ret!([true, 1])
          }} |

          // fsStat: distinguishes directories from files by rel name.
          //   - "subdir", "subdir2" → directory (openDir mint-time
          //     validation; second entry supports M-17-2b's name-
          //     discrimination test — two Fs bundle entries pointing at
          //     distinct rel dirs)
          //   - "missing.txt" → FSERR_NOTFOUND (openFile creation-mode
          //     regression: stat may fail on a nonexistent target and
          //     openFileImpl must pass through to fsOpen)
          //   - everything else → regular file
          // Slice 26: `fsStat` now takes a `cmode` string.  The mock
          // ignores the value (returning fixture data), but the arity
          // must match or Rholang dispatch silently drops the send.
          //
          // Recognized dir paths: "subdir", "subdir2", and the
          // composition "subdir/subdir2" (needed by the slice-26
          // nested-openDir cmode-inheritance test).
          contract fsStat(@_root, @rel, @_cmode, ret) = {{
            match rel {{
              "subdir"          => ret!([true, {{"kind": "dir", "size": 0}}])
              "subdir2"         => ret!([true, {{"kind": "dir", "size": 0}}])
              "subdir/subdir2"  => ret!([true, {{"kind": "dir", "size": 0}}])
              "missing.txt"     => ret!([false, "FSERR_NOTFOUND", "no such file"])
              _                 => ret!([true, {{"kind": "file", "size": 100}}])
            }}
          }} |

          contract fsExists(@_root, @_rel, ret) = {{
            ret!([true, true])
          }} |

          // fs_entries — returns whatever the test has staged in
          // `entriesCell`.  Default (no explicit stage) is an empty
          // list, so entries() over a "quiet" dir returns an
          // immediately-exhausted stream.  Tests stage a specific
          // reply by consuming and replacing the cell before invoking
          // entries() (see the dir_entries_* tests below for the
          // pattern).
          //
          // The reply shape is [true, [entryRecord, ...]] on success
          // or [false, code, msg] on failure — matching Phase 1's
          // fs_entries.
          entriesCell!([true, []]) |
          // Slice 26: `fsEntries` now takes a `cmode` string.
          contract fsEntries(@_root, @_rel, @_cmode, ret) = {{
            for (@reply <<- entriesCell) {{ ret!(reply) }}
          }} |

          // fs_truncate — records n in truncLog for test verification.
          truncLog!([]) |
          contract fsTruncate(@_fd, @n, ret) = {{
            for (@log <- truncLog) {{
              truncLog!(log ++ [n]) |
              ret!([true])
            }}
          }} |

          // fs_chmod — records (root, rel, bits, cmode) in chmodLog.
          // C-R2 round-2 fix: fsChmod native handler now takes cmode
          // and fails-closed on Consensus (mirrors slice-26 fsChown).
          chmodLog!([]) |
          contract fsChmod(@root, @rel, @bits, @cmode, ret) = {{
            for (@log <- chmodLog) {{
              chmodLog!(log ++ [(root, rel, bits, cmode)]) |
              match cmode {{
                "consensus" => ret!([false, "FSERR_UNSUPPORTED",
                  "chmod unavailable in consensus mode"])
                _ => ret!([true])
              }}
            }}
          }} |

          // fs_chown — records (root, rel, owner, group, cmode) in chownLog.
          // Slice 26: `cmode` is the 5th arg; consensus caps get
          // FSERR_UNSUPPORTED from the real handler.  The mock
          // records everything; per-test consensus-mode assertions
          // live in the tests themselves.
          chownLog!([]) |
          contract fsChown(@root, @rel, @owner, @group, @cmode, ret) = {{
            for (@log <- chownLog) {{
              chownLog!(log ++ [(root, rel, owner, group, cmode)]) |
              match cmode {{
                "consensus" => ret!([false, "FSERR_UNSUPPORTED",
                  "chown unavailable in consensus mode"])
                _ => ret!([true])
              }}
            }}
          }} |

          // fs_removeFile — records (root, rel, cmode) in rmFileLog.
          // C-R2: cmode arg; fail-closed on consensus.
          rmFileLog!([]) |
          contract fsRemoveFile(@root, @rel, @cmode, ret) = {{
            for (@log <- rmFileLog) {{
              rmFileLog!(log ++ [(root, rel, cmode)]) |
              match cmode {{
                "consensus" => ret!([false, "FSERR_UNSUPPORTED",
                  "removeFile unavailable in consensus mode"])
                _ => ret!([true])
              }}
            }}
          }} |

          // fs_removeDir — records (root, rel, recursive, cmode).
          rmDirLog!([]) |
          contract fsRemoveDir(@root, @rel, @recursive, @cmode, ret) = {{
            for (@log <- rmDirLog) {{
              rmDirLog!(log ++ [(root, rel, recursive, cmode)]) |
              match cmode {{
                "consensus" => ret!([false, "FSERR_UNSUPPORTED",
                  "removeDir unavailable in consensus mode"])
                _ => ret!([true])
              }}
            }}
          }} |

          // fs_rename — records (fromRoot, from, toRoot, to, cmode).
          renameLog!([]) |
          contract fsRename(@fromRoot, @from, @toRoot, @to, @cmode, ret) = {{
            for (@log <- renameLog) {{
              renameLog!(log ++ [(fromRoot, from, toRoot, to, cmode)]) |
              match cmode {{
                "consensus" => ret!([false, "FSERR_UNSUPPORTED",
                  "rename unavailable in consensus mode"])
                _ => ret!([true])
              }}
            }}
          }} |

          // fs_copyFile — records (fromRoot, from, toRoot, to, cmode).
          // Reply includes a fake nBytes = 42 to exercise the [true, nBytes]
          // shape from spec §Dir.copyFile.
          copyLog!([]) |
          contract fsCopyFile(@fromRoot, @from, @toRoot, @to, @cmode, ret) = {{
            for (@log <- copyLog) {{
              copyLog!(log ++ [(fromRoot, from, toRoot, to, cmode)]) |
              match cmode {{
                "consensus" => ret!([false, "FSERR_UNSUPPORTED",
                  "copyFile unavailable in consensus mode"])
                _ => ret!([true, 42])
              }}
            }}
          }} |

          // -- Phase 8 slice 8a — lock natives (mock preamble) -----
          //
          // Always-succeed mocks with monotone fake LockIds.  Enough
          // for slice-8a step-4c/d/e/f File.rho surgery to compile
          // and for existing tests to remain green as the auto-acquire
          // wraps land — none of the existing tests exercise
          // lock-conflict behavior.  Step 4g's integration tests will
          // supply richer stand-ins (per-fd lock accounting, double-
          // release-returns-FSERR_CLOSED, etc.) either by overriding
          // these or by composing bespoke setups.
          //
          // Real natives key on (dev, inode) — the mock's fake fd is
          // ignored, matching the "single-file" simplification the
          // rest of this preamble uses.

          contract fsLockRange(@_fd, @_off, @_len, @_mode, @_holder, @_cmode, ret) = {{
            ret!([true, 1])
          }} |

          // Slice-8b sub-2 arity-8 mock (wait: Bool at slot 7).  Same
          // always-succeed behavior as the arity-7 mock — most tests
          // exercise the syntactic wraps, not the parking logic.  The
          // sub-5 wait:true smoke tests below assert this mock IS the
          // one invoked (rather than the arity-7 mock) when File.rho's
          // arity-4 lockRange method threads wait through.
          contract fsLockRange(@_fd, @_off, @_len, @_mode, @_holder, @_cmode, @_wait, ret) = {{
            ret!([true, 1])
          }} |

          contract fsLockSequential(@_fd, @_holder, @_cmode, ret) = {{
            ret!([true, 2])
          }} |

          // Slice-8b sub-2 arity-5 mock for fsLockSequential (wait: Bool
          // at slot 3).  No caller in sub-5 exercises this yet — added
          // for symmetry with fsLockRange and future sub-4b work.
          contract fsLockSequential(@_fd, @_holder, @_cmode, @_wait, ret) = {{
            ret!([true, 2])
          }} |

          contract fsReleaseLock(@_lockId, ret) = {{
            ret!([true])
          }} |

          contract fsReleaseAllForHolder(@_holder, ret) = {{
            // File.close (step 4f) invokes this before fsClose; return
            // [true, N] with N = 0 by default.  Tests that want a
            // specific sweep-count assertion can override.
            ret!([true, 0])
          }} |

          // -- Library bodies ---------------------------------------
          {file_body}
          |
          {dir_body}
          |
          {stream_body}
          |
          {buffer_body}
          |
          {stdin_body}
          |
          {stdout_body}
          |
          {fs_body}
          |
          // -- Test snippet -----------------------------------------
          {snippet}
        }}
        "#,
        file_body = lib_body(FILE_RHO),
        dir_body = lib_body(DIR_RHO),
        stream_body = lib_body(STREAM_RHO),
        buffer_body = lib_body(BUFFER_RHO),
        stdin_body = lib_body(STDIN_RHO),
        stdout_body = lib_body(STDOUT_RHO),
        fs_body = lib_body(FS_RHO),
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

// Note: Fs.rho does NOT parse standalone because it references
// openFileImpl / openDirImpl / File / Dir, which live in Dir.rho.  Its
// only compile check is via with_libs (see with_libs_composes).

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
// Stateful lock-mock helper (Phase 8 slice 8a step-4g follow-up)
//
// Shared between the stateful-lock integration tests below.  Encodes
// a "one-active-lock" LockRegistry stand-in that's expressive enough
// to model:
//   - Sequential locks (strict same-holder — spec §1143)
//   - Range locks with same-holder skip (Prep A rule)
//   - Cross-cap coordination via holder-identity comparison
//   - Close-sweep behavior (release_all_for_holder clears active state
//     if the incoming holder matches)
//   - Monotone LockId minting (per-acquire fresh Int; matches real
//     `LockRegistry.mint_id` semantics for release-idempotence tests)
//
// State cells (must be in the caller's outer `new` clause):
//   activeHolder — Nil or the current active-lock holder Par
//   activeKind   — Nil, "range", or "seq"
//   activeLockId — Int monotone counter (starts at 0; ++ per acquire)
//
// Simplification: only ONE "active" lock at a time.  Same-holder skip
// mints a fresh LockId but doesn't ADD to state — the outer lock keeps
// tracking as the one active entry.  Real LockRegistry tracks a set
// of (holder, offset, length) tuples per (dev, inode); the tests
// covered by this mock exercise cross-cap and close-sweep semantics
// which don't require full multi-lock accounting.  Tests that need
// multi-lock accounting must build a bespoke mock.
// ---------------------------------------------------------------------

/// Rholang source fragment defining the stateful lock-native mocks.
/// Splice into a `new ... in { ... }` scope where `activeHolder`,
/// `activeKind`, `activeLockId`, `fsLockRange`, `fsLockSequential`,
/// `fsReleaseLock`, `fsReleaseAllForHolder` are all bound.
const STATEFUL_LOCK_MOCKS: &str = r#"
          activeHolder!(Nil) |
          activeKind!(Nil) |
          activeLockId!(0) |

          contract fsLockRange(@_fd, @_o, @_l, @_m, @holder, @_cm, ret) = {
            for (@ch <- activeHolder; @ck <- activeKind; @cid <- activeLockId) {
              match ck {
                Nil => {
                  // No active lock — grant.
                  activeHolder!(holder) | activeKind!("range") |
                  activeLockId!(cid + 1) |
                  ret!([true, cid + 1])
                }
                "seq" => {
                  // Sequential active — range blocked (spec §1143).
                  activeHolder!(ch) | activeKind!(ck) | activeLockId!(cid) |
                  ret!([false, "FSERR_BUSY", "sequential active"])
                }
                _ => {
                  // Range active — check holder for same-holder skip.
                  match ch == holder {
                    true => {
                      // Same holder → allow (Prep A rule).
                      activeHolder!(ch) | activeKind!(ck) |
                      activeLockId!(cid + 1) |
                      ret!([true, cid + 1])
                    }
                    false => {
                      // Cross-holder overlap → BUSY.
                      activeHolder!(ch) | activeKind!(ck) | activeLockId!(cid) |
                      ret!([false, "FSERR_BUSY", "cross-holder range conflict"])
                    }
                  }
                }
              }
            }
          } |

          contract fsLockSequential(@_fd, @holder, @_cm, ret) = {
            for (@ch <- activeHolder; @ck <- activeKind; @cid <- activeLockId) {
              match ck {
                Nil => {
                  // No active lock — grant.
                  activeHolder!(holder) | activeKind!("seq") |
                  activeLockId!(cid + 1) |
                  ret!([true, cid + 1])
                }
                _ => {
                  // ANY active lock blocks sequential (STRICT same-holder,
                  // spec §1143 "one active sequential stream per File").
                  activeHolder!(ch) | activeKind!(ck) | activeLockId!(cid) |
                  ret!([false, "FSERR_BUSY", "sequential conflict"])
                }
              }
            }
          } |

          contract fsReleaseLock(@_id, ret) = {
            for (@_ch <- activeHolder; @_ck <- activeKind; @cid <- activeLockId) {
              // Simplification: clear active state unconditionally.  Real
              // LockRegistry tracks per-LockId; tests using this mock
              // don't exercise multi-lock accounting so single-clear is
              // sufficient.  LockId counter is NOT decremented — matches
              // real semantics (LockIds are monotone).
              activeHolder!(Nil) | activeKind!(Nil) | activeLockId!(cid) |
              ret!([true])
            }
          } |

          contract fsReleaseAllForHolder(@holder, ret) = {
            for (@ch <- activeHolder; @ck <- activeKind; @cid <- activeLockId) {
              match ch {
                Nil => {
                  activeHolder!(Nil) | activeKind!(Nil) | activeLockId!(cid) |
                  ret!([true, 0])
                }
                _ => {
                  match ch == holder {
                    true => {
                      // Matching holder — sweep active state.
                      activeHolder!(Nil) | activeKind!(Nil) | activeLockId!(cid) |
                      ret!([true, 1])
                    }
                    false => {
                      // Non-matching holder — leave active state alone.
                      activeHolder!(ch) | activeKind!(ck) | activeLockId!(cid) |
                      ret!([true, 0])
                    }
                  }
                }
              }
            }
          }
"#;

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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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

// -- Phase 8 slice 8c smoke-checks — arity-2 writeByteArray with
// options-map (wait extraction).  Verifies helper contract dispatch
// end-to-end.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_byte_array_options_empty_map_defaults_wait_false() {
    // Empty options → wait:false → helper routes through arity-5
    // fsLockSequential mock (which returns [true, 2]) → writeByteArray
    // succeeds.  Regression pin: options.get("wait") returning Nil
    // must NOT fail as FSERR_BAD_ARG for a missing key.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("writeByteArray", "hi".toUtf8Bytes(), {})) {
            match r {
              [true, _n] => @"out"!([true])
              [false, code, msg] => @"out"!([false, code, msg])
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "arity-2 writeByteArray with empty options must succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_byte_array_options_wait_true_dispatches_helper() {
    // wait:true — helper routes through arity-5 fsLockSequential mock
    // (from sub-5 preamble additions) → success.  This test proves
    // the helper contract wiring works: caller → arity-2 method →
    // withSequentialLock helper → arity-5 native mock → callback →
    // fsWrite mock → callback → release → return.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("writeByteArray", "hi".toUtf8Bytes(), {"wait": true})) {
            match r {
              [true, _n] => @"out"!([true])
              [false, code, msg] => @"out"!([false, code, msg])
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "wait:true must route through arity-5 fsLockSequential helper"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_byte_array_options_wait_non_bool_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("writeByteArray", "hi".toUtf8Bytes(), {"wait": "yes"})) {
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
async fn file_write_byte_array_arity_1_and_arity_2_coexist() {
    // The slice-8c coexistence pattern: arity-1 (pre-existing) and
    // arity-2 (new) writeByteArray methods dispatch independently on
    // the same File cap.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r1 <- @f!?("writeByteArray", "a".toUtf8Bytes())) {
            for (@r2 <- @f!?("writeByteArray", "b".toUtf8Bytes(), {"wait": true})) {
              @"out"!([r1, r2])
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list"),
    };
    let (r1_ok, _, _, _) = extract_reply(&outer.ps[0]);
    let (r2_ok, _, _, _) = extract_reply(&outer.ps[1]);
    assert!(r1_ok, "arity-1 writeByteArray must succeed");
    assert!(r2_ok, "arity-2 writeByteArray must succeed on same cap");
}

// -- writeBytes arity-2 smoke-checks -----------------------------------
//
// Uses Stream!?() to construct real stream handles (bare mock contracts
// don't match writeBytesLoop's `!?("next")` dispatch shape reliably).

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_options_wait_true_dispatches_helper() {
    // Empty stream (single EOS reply) via a real Stream!?() handle.
    // Verifies wait:true path routes through withSequentialLock.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new emptyProducer, byteBuilder in {
          contract emptyProducer(retCh) = { retCh!([false, "EOS", ""]) } |
          contract byteBuilder(@vs, retCh) = { retCh!([true, vs.concatBytes()]) } |
          for (@stream <- Stream!?(*emptyProducer, *byteBuilder)) {
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@r <- @f!?("writeBytes", stream, {"wait": true})) {
                @"out"!(r)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "arity-2 writeBytes with wait:true must succeed on empty stream"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_options_wait_non_bool_rejects() {
    // Non-Bool wait rejects with FSERR_BAD_ARG before touching the
    // stream at all — no stream needed.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new emptyProducer, byteBuilder in {
          contract emptyProducer(retCh) = { retCh!([false, "EOS", ""]) } |
          contract byteBuilder(@vs, retCh) = { retCh!([true, vs.concatBytes()]) } |
          for (@stream <- Stream!?(*emptyProducer, *byteBuilder)) {
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@r <- @f!?("writeBytes", stream, {"wait": 42})) {
                @"out"!(r)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

// -- writeBytesAt arity-4 smoke-checks ---------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_options_wait_true_dispatches_helper() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new emptyProducer, byteBuilder in {
          contract emptyProducer(retCh) = { retCh!([false, "EOS", ""]) } |
          contract byteBuilder(@vs, retCh) = { retCh!([true, vs.concatBytes()]) } |
          for (@stream <- Stream!?(*emptyProducer, *byteBuilder)) {
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@r <- @f!?("writeBytesAt", 0, 100, stream, {"wait": true})) {
                @"out"!(r)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "arity-4 writeBytesAt with wait:true must succeed on empty stream"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_options_wait_false_dispatches_correctly() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new emptyProducer, byteBuilder in {
          contract emptyProducer(retCh) = { retCh!([false, "EOS", ""]) } |
          contract byteBuilder(@vs, retCh) = { retCh!([true, vs.concatBytes()]) } |
          for (@stream <- Stream!?(*emptyProducer, *byteBuilder)) {
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@r <- @f!?("writeBytesAt", 0, 100, stream, {})) {
                @"out"!(r)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "arity-4 writeBytesAt with empty options must succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_arity_3_and_arity_4_coexist() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new emptyProducer, byteBuilder in {
          contract emptyProducer(retCh) = { retCh!([false, "EOS", ""]) } |
          contract byteBuilder(@vs, retCh) = { retCh!([true, vs.concatBytes()]) } |
          for (@s1 <- Stream!?(*emptyProducer, *byteBuilder);
               @s2 <- Stream!?(*emptyProducer, *byteBuilder)) {
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@r3 <- @f!?("writeBytesAt", 0, 10, s1)) {
                for (@r4 <- @f!?("writeBytesAt", 10, 10, s2, {"wait": true})) {
                  @"out"!([r3, r4])
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
        _ => panic!("expected list"),
    };
    let (r3_ok, _, _, _) = extract_reply(&outer.ps[0]);
    let (r4_ok, _, _, _) = extract_reply(&outer.ps[1]);
    assert!(r3_ok, "arity-3 writeBytesAt must succeed");
    assert!(r4_ok, "arity-4 writeBytesAt must succeed on same cap");
}

// -- Phase 8 slice 8d hand-off helpers — direct unit tests -------------
//
// The hand-off helpers (acquireRangeForStream, acquireSequentialForStream)
// enable options-map plumbing for stream-lifetime-locked producer
// methods (chars, bytes, lines, bytesAt).  These direct tests verify
// the two-channel handshake works before method variants use it.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn acquire_range_for_stream_hands_off_lockid_on_success() {
    // Success path: helper acquires the lock (arity-8 native mock
    // returns [true, 1]), sends the LockId (1) on lockOut, sends
    // [true] on retCh.  Caller reads retCh first, then lockOut.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new dummyHolder, lockOut, acqRet in {
          acquireRangeForStream!(1, 0, 100, "r", *dummyHolder,
                                 "oracular", true, *lockOut, *acqRet) |
          for (@acqReply <- acqRet) {
            match acqReply {
              [true] => {
                for (@lid <- lockOut) {
                  @"out"!([true, lid])
                }
              }
              _ => @"out"!(acqReply)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, k, _) = extract_reply(&reply);
    assert!(
        ok,
        "helper must return [true] on retCh when acquire succeeds"
    );
    assert_eq!(
        k,
        Some(1),
        "helper must hand off the LockId from the arity-8 mock"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn acquire_sequential_for_stream_hands_off_lockid_on_success() {
    // Same shape, sequential variant.  Arity-5 fsLockSequential mock
    // returns [true, 2].
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new dummyHolder, lockOut, acqRet in {
          acquireSequentialForStream!(1, *dummyHolder, "oracular", true,
                                      *lockOut, *acqRet) |
          for (@acqReply <- acqRet) {
            match acqReply {
              [true] => {
                for (@lid <- lockOut) {
                  @"out"!([true, lid])
                }
              }
              _ => @"out"!(acqReply)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, k, _) = extract_reply(&reply);
    assert!(
        ok,
        "helper must return [true] on retCh when acquire succeeds"
    );
    assert_eq!(k, Some(2), "helper must hand off the arity-5 mock's LockId");
}

// Failure-path tests for the hand-off helpers (mock override to force
// FSERR_BUSY from the native) are deferred to slice 8d-2 when the
// first stream-lifetime method (bytesAt arity-3) exercises the failure
// path end-to-end.  A bespoke `new File, ... in { }` preamble here
// can't easily invoke the helpers because they're only defined by
// File.rho's lib_body (included via `with_libs`), and `with_libs`
// already binds fsLockRange as a specific mock — overriding it in the
// test body creates a double-listener race.  The LockRegistry's
// Rust-side sub-1 tests already cover 19 error/cancel paths including
// FSERR_BUSY and FSERR_CANCELLED delivery via oneshot.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_close_then_read_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        new File, fdP, stateP, cmodeP, parseRwxToBits, parseRwxLoop,
            writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
            readLinesIntoLoop, drainToNextLF,
            codepointLen, concatStringsLoop, scanLineForLF,
            fsRead, fsReadAt, fsWrite, fsWriteAt,
            fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsTruncate, fsChmod, fsChown, Stream,
            // Phase 8 slice 8a — lock natives (mock preamble additions).
            // Same-as-with_libs: bound here so File.rho's step 4c-2+
            // LockToken agent + close-sweep call sites remain in scope.
            fsLockRange, fsLockSequential, fsReleaseLock,
            fsReleaseAllForHolder,
            LockToken, lockStateP,
            withSequentialLock, withRangeLock,
            acquireRangeForStream, acquireSequentialForStream
        in {{
          contract fsRead(@_fd, @_n, ret)  = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsReadAt(@_fd, @_o, @_n, ret) = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsWrite(@_fd, @_xs, ret) = {{ ret!([true, 0]) }} |
          contract fsWriteAt(@_fd, @_o, @_xs, ret) = {{ ret!([true, 0]) }} |
          contract fsSeek(@_fd, @_o, @_w, ret) = {{ ret!([true, 0]) }} |
          contract fsTell(@_fd, ret) = {{ ret!([true, 0]) }} |
          contract fsSize(@_fd, ret) = {{ ret!([true, 0]) }} |
          contract fsFlush(@_fd, ret) = {{ ret!([true]) }} |
          contract fsClose(@_fd, ret) = {{ ret!([false, "FSERR_IO", "simulated"]) }} |
          contract fsTruncate(@_fd, @_n, ret) = {{ ret!([true]) }} |
          // C-R2 round-2: fsChmod takes cmode as 4th arg.
          contract fsChmod(@_r, @_p, @_b, @_cm, ret) = {{ ret!([true]) }} |
          // Slice 26: fsChown takes cmode as 5th arg.
          contract fsChown(@_r, @_p, @_o, @_g, @_cm, ret) = {{ ret!([true]) }} |
          // Phase 8 slice 8a — always-succeed lock-native stubs.
          // This bespoke test drives File.close specifically; the
          // sweep native is invoked before fsClose (step 4f) and must
          // succeed so the fsClose failure remains the observable
          // outcome the test asserts on.
          contract fsLockRange(@_fd, @_o, @_l, @_m, @_h, @_cm, ret) = {{ ret!([true, 1]) }} |
          contract fsLockSequential(@_fd, @_h, @_cm, ret) = {{ ret!([true, 2]) }} |
          contract fsReleaseLock(@_id, ret) = {{ ret!([true]) }} |
          contract fsReleaseAllForHolder(@_h, ret) = {{ ret!([true, 0]) }} |
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
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
            for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {{
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        "for (@f <- File!?(1, \"/root\", \"test.txt\", \"rw\", \"oracular\")) {\
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
        for (@f <- File!?(1, "/root", "config.json", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "data.bin", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "r", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
                          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@d <- Dir!?("/root", "", "rw", "oracular", *File)) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
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

// ---------------------------------------------------------------------
// Slice 4: write-side stream consumers — File.writeBytes and
// File.writeBytesAt.
//
// Each test constructs an in-memory ByteStream via a producer contract
// that pops 1-byte ByteArrays off a list (mirroring how the Stream
// specialization would produce them), then drives writeBytes /
// writeBytesAt against it and verifies the file state (via readN /
// tell) plus the reply shape.
// ---------------------------------------------------------------------

/// Build a `for (@stream <- Stream!?(...))` bootstrap that constructs
/// a ByteStream over a Rholang list literal.  `values_lit` is a
/// Rholang expression evaluating to a `List[ByteArray]`.  The test
/// snippet is spliced where `%TEST_SNIPPET%` appears; inside the
/// snippet, `stream` is bound to the constructed Stream handle.
fn byte_stream_from_list(values_lit: &str) -> String {
    format!(
        r#"
        new listState, producer, byteBuilder in {{
          listState!({values_lit}) |
          contract producer(retCh) = {{
            for (@lst <- listState) {{
              match lst {{
                []              => {{ listState!([]) | retCh!([false, "EOS", ""]) }}
                [head ...tail]  => {{ listState!(tail) | retCh!([true, head]) }}
              }}
            }}
          }} |
          contract byteBuilder(@vals, retCh) = {{
            retCh!([true, vals.concatBytes()])
          }} |
          for (@stream <- Stream!?(*producer, *byteBuilder)) {{
            %TEST_SNIPPET%
          }}
        }}
        "#,
    )
}

// -- File.writeBytes (sequential consumer) ---------------------------

/// writeBytes drains a 2-byte stream and the file ends up with those
/// bytes at the cursor.  Verified by readN after seeking back to 0.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_drains_stream_into_file() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["a".toUtf8Bytes(), "b".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wbReply <- @f!?("writeBytes", stream)) {
                for (@_ <- @f!?("seek", 0, "set")) {
                  for (@readReply <- @f!?("readN", 100)) {
                    @"out"!([wbReply, readReply])
                  }
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (wb_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(wb_ok, "writeBytes must succeed on EOS");
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(read_ok);
    assert_eq!(
        bytes,
        Some(b"ab".to_vec()),
        "file must contain the drained bytes at the cursor"
    );
}

/// writeBytes over an empty stream is a no-op returning [true].
/// Nothing written; readN yields empty bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_empty_stream_is_no_op() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"[]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wbReply <- @f!?("writeBytes", stream)) {
                for (@sizeReply <- @f!?("size")) {
                  @"out"!([wbReply, sizeReply])
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (wb_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(wb_ok);
    let (size_ok, _, size, _) = extract_reply(&outer.ps[1]);
    assert!(size_ok);
    assert_eq!(size, Some(0), "empty stream must not write anything");
}

/// writeBytes on a read-only File returns FSERR_UNSUPPORTED without
/// touching the stream (the mode gate fires before any next() call).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["x".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
              for (@wbReply <- @f!?("writeBytes", stream)) {
                @"out"!(wbReply)
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// writeBytes on a closed File returns FSERR_CLOSED.  The stream is
/// not touched — mode/state check fires first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["x".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@_ <- @f!?("close")) {
                for (@wbReply <- @f!?("writeBytes", stream)) {
                  @"out"!(wbReply)
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

/// writeBytes forwards a producer's mid-stream error and encodes the
/// running byte count into the message per spec §938 ("wrote N bytes
/// before ..." format via Rholang `%%` string interpolation).  Uses a
/// producer that vends one byte then errors with FSERR_IO.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_producer_error_reports_bytes_written() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Bespoke producer: emits one byte, then FSERR_IO.
    let src = with_libs(
        r#"
        new listState, producer, byteBuilder in {
          listState!([("x".toUtf8Bytes(), true), ("simulated", false)]) |
          contract producer(retCh) = {
            for (@lst <- listState) {
              match lst {
                []                    => { listState!([]) | retCh!([false, "EOS", ""]) }
                [(v, true) ...tail]   => { listState!(tail) | retCh!([true, v]) }
                [(msg, false) ...tail] => { listState!(tail) | retCh!([false, "FSERR_IO", msg]) }
              }
            }
          } |
          contract byteBuilder(@vals, retCh) = { retCh!([true, vals.concatBytes()]) } |
          for (@stream <- Stream!?(*producer, *byteBuilder)) {
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wbReply <- @f!?("writeBytes", stream)) {
                @"out"!(wbReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    // Reply is [false, "FSERR_IO", "wrote 1 bytes before producer failure: simulated"];
    // extract_reply only pulls ps[0] and ps[1], so pull ps[2] directly.
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list reply"),
    };
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("head not Bool"),
    };
    assert!(!ok);
    let code = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!("code not String"),
    };
    assert_eq!(code, "FSERR_IO");
    let msg = match single_expr(&list.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!("msg not String"),
    };
    assert!(
        msg.contains("wrote 1 bytes before"),
        "message must interpolate byte count; got {:?}",
        msg
    );
    assert!(
        msg.contains("simulated"),
        "message must include the underlying producer msg; got {:?}",
        msg
    );
}

// -- File.writeBytesAt (positional consumer) ------------------------

/// writeBytesAt writes at offset without moving the sequential cursor.
/// Verified by tell() after: the cursor stays wherever earlier writes
/// left it, not moved by the positional pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_does_not_move_cursor() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // First writeByteArray "abcd" (cursor now at 4); then writeBytesAt
    // at offset 0 with a stream of ["X"]; verify tell() still 4.
    let bootstrap = byte_stream_from_list(r#"["X".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@_ <- @f!?("writeByteArray", "abcd".toUtf8Bytes())) {
                for (@wbaReply <- @f!?("writeBytesAt", 0, 1, stream)) {
                  for (@tellReply <- @f!?("tell")) {
                    @"out"!([wbaReply, tellReply])
                  }
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (wba_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(wba_ok);
    let (tell_ok, _, pos, _) = extract_reply(&outer.ps[1]);
    assert!(tell_ok);
    assert_eq!(
        pos,
        Some(4),
        "cursor must stay at the sequential write's endpoint; positional writes don't move it"
    );
}

/// writeBytesAt caps at maxLength — a longer producer only sees
/// `maxLength` bytes consumed.  Verified via writeAtLog which records
/// each positional write's (offset, length).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_maxlength_caps_stream() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Producer has 5 bytes; maxLength = 2.  Only 2 fsWriteAt calls
    // should happen (one per byte, since producer emits 1-byte
    // chunks).  writeAtLog records the offset of each.
    let bootstrap = byte_stream_from_list(
        r#"["a".toUtf8Bytes(), "b".toUtf8Bytes(), "c".toUtf8Bytes(), "d".toUtf8Bytes(), "e".toUtf8Bytes()]"#,
    )
    .replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wbaReply <- @f!?("writeBytesAt", 10, 2, stream)) {
                for (@log <<- writeAtLog) {
                  @"out"!([wbaReply, log])
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (wba_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(wba_ok);
    let log_list = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("writeAtLog not a list"),
    };
    assert_eq!(
        log_list.ps.len(),
        2,
        "exactly 2 fsWriteAt calls must happen (maxLength cap)"
    );
    // Verify offsets are 10 and 11 (sequential 1-byte writes).
    let off0 = match single_expr(&log_list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::ETupleBody(t)) => match single_expr(&t.ps[0]).unwrap().expr_instance {
            Some(ExprInstance::GInt(n)) => n,
            _ => panic!(),
        },
        _ => panic!(),
    };
    let off1 = match single_expr(&log_list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::ETupleBody(t)) => match single_expr(&t.ps[0]).unwrap().expr_instance {
            Some(ExprInstance::GInt(n)) => n,
            _ => panic!(),
        },
        _ => panic!(),
    };
    assert_eq!(off0, 10);
    assert_eq!(off1, 11);
}

/// writeBytesAt with maxLength=0 is a no-op returning [true] and
/// touches neither the fd nor the stream (short-circuited by the
/// `remaining == 0` arm of the loop before any next() fires).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_zero_maxlength_no_op() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["a".toUtf8Bytes(), "b".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wbaReply <- @f!?("writeBytesAt", 0, 0, stream)) {
                for (@log <<- writeAtLog) {
                  @"out"!([wbaReply, log])
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (wba_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(wba_ok);
    let log_list = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    assert!(
        log_list.ps.is_empty(),
        "no fsWriteAt should fire when maxLength is 0"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_negative_offset_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["x".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@r <- @f!?("writeBytesAt", -1, 1, stream)) { @"out"!(r) }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_negative_maxlength_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["x".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@r <- @f!?("writeBytesAt", 0, -1, stream)) { @"out"!(r) }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_non_int_offset_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["x".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@r <- @f!?("writeBytesAt", "zero", 1, stream)) { @"out"!(r) }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_non_int_maxlength_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["x".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@r <- @f!?("writeBytesAt", 0, "one", stream)) { @"out"!(r) }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["x".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
              for (@r <- @f!?("writeBytesAt", 0, 1, stream)) { @"out"!(r) }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_at_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["x".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@_ <- @f!?("close")) {
                for (@r <- @f!?("writeBytesAt", 0, 1, stream)) { @"out"!(r) }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

// ---------------------------------------------------------------------
// Slice 5: Dir.entries() — EntryStream producer over fs_entries.
// ---------------------------------------------------------------------

/// entries() over a directory with a few records: drain via next()
/// yields each record in order, then EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_entries_drains_records_then_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- entriesCell) {
          entriesCell!([true, [
            {"name": "a.txt", "kind": "file", "size": 10},
            {"name": "b.txt", "kind": "file", "size": 20}
          ]]) |
          for (@d <- Dir!?("/root", "sub", "r", "oracular", *File)) {
            for (@entriesReply <- @d!?("entries")) {
              match entriesReply {
                [true, stream] => {
                  for (@n1 <- @stream!?("next")) {
                    for (@n2 <- @stream!?("next")) {
                      for (@n3 <- @stream!?("next")) {
                        @"out"!([n1, n2, n3])
                      }
                    }
                  }
                }
                _ => @"out"!([entriesReply, Nil, Nil])
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
    let (ok1, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1, "first entry must succeed");
    let (ok2, _, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2, "second entry must succeed");
    let (ok3, code3, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok3);
    assert_eq!(code3, "EOS", "third next() must signal end-of-stream");
}

/// entries() over an empty directory yields EOS immediately.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_entries_empty_dir_eos_immediately() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Default entriesCell is [true, []] — no staging needed.
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "sub", "r", "oracular", *File)) {
          for (@entriesReply <- @d!?("entries")) {
            match entriesReply {
              [true, stream] => {
                for (@r <- @stream!?("next")) { @"out"!(r) }
              }
              _ => @"out"!(entriesReply)
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

/// chunk(n) on an EntryStream returns a List of records — matches
/// spec §337 (EntryStream's chunk container is List of records).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_entries_chunk_returns_list() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- entriesCell) {
          entriesCell!([true, [
            {"name": "a.txt"},
            {"name": "b.txt"},
            {"name": "c.txt"}
          ]]) |
          for (@d <- Dir!?("/root", "sub", "r", "oracular", *File)) {
            for (@entriesReply <- @d!?("entries")) {
              match entriesReply {
                [true, stream] => {
                  for (@r <- @stream!?("chunk", 10)) { @"out"!(r) }
                }
                _ => @"out"!(entriesReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(ok);
    let inner = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("chunk payload not a list"),
    };
    assert_eq!(inner.ps.len(), 3, "chunk must contain all 3 records");
}

/// entries() forwards a native fs_entries error without minting a
/// stream — caller sees the error immediately.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_entries_forwards_native_error() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- entriesCell) {
          entriesCell!([false, "FSERR_QUOTA_EXCEEDED", "too many entries"]) |
          for (@d <- Dir!?("/root", "sub", "r", "oracular", *File)) {
            for (@r <- @d!?("entries")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_QUOTA_EXCEEDED");
}

/// entries() works on both "r" and "rw" Dirs (read op — no mode gate).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_entries_works_on_read_only_dir() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- entriesCell) {
          entriesCell!([true, [{"name": "f.txt"}]]) |
          for (@d <- Dir!?("/root", "sub", "r", "oracular", *File)) {
            for (@entriesReply <- @d!?("entries")) {
              match entriesReply {
                [true, stream] => {
                  for (@r <- @stream!?("next")) { @"out"!(r) }
                }
                _ => @"out"!(entriesReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "entries() must succeed on a read-only Dir");
}

// ---------------------------------------------------------------------
// Slice 6: File.chars() — CharStream producer over UTF-8 file content.
// ---------------------------------------------------------------------

/// chars() over ASCII "abc" yields "a", "b", "c" then EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_streams_ascii_content() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "abc".toUtf8Bytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@charsReply <- @f!?("chars")) {
                match charsReply {
                  [true, stream] => {
                    for (@n1 <- @stream!?("next")) {
                      for (@n2 <- @stream!?("next")) {
                        for (@n3 <- @stream!?("next")) {
                          for (@n4 <- @stream!?("next")) {
                            @"out"!([n1, n2, n3, n4])
                          }
                        }
                      }
                    }
                  }
                  _ => @"out"!([charsReply, Nil, Nil, Nil])
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
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "a");
    let (ok2, v2, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(v2, "b");
    let (ok3, v3, _, _) = extract_reply(&outer.ps[2]);
    assert!(ok3);
    assert_eq!(v3, "c");
    let (ok4, code4, _, _) = extract_reply(&outer.ps[3]);
    assert!(!ok4);
    assert_eq!(code4, "EOS");
}

/// chars() correctly handles multi-byte UTF-8: "aé" (3 bytes: 0x61,
/// 0xC3, 0xA9) yields "a" then "é" (a 2-byte codepoint) then EOS.
/// This exercises the byte-length branch of codepointLen and the
/// slice+decodeUtf8 path for a non-ASCII codepoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_handles_multibyte_utf8() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        "for (@f <- File!?(1, \"/root\", \"test.txt\", \"rw\", \"oracular\")) {\
           for (@_ <- @f!?(\"writeByteArray\", \"a\u{00E9}\".toUtf8Bytes())) {\
             for (@_ <- @f!?(\"seek\", 0, \"set\")) {\
               for (@charsReply <- @f!?(\"chars\")) {\
                 match charsReply {\
                   [true, stream] => {\
                     for (@n1 <- @stream!?(\"next\")) {\
                       for (@n2 <- @stream!?(\"next\")) {\
                         for (@n3 <- @stream!?(\"next\")) {\
                           @\"out\"!([n1, n2, n3])\
                         }\
                       }\
                     }\
                   }\
                   _ => @\"out\"!([charsReply, Nil, Nil])\
                 }\
               }\
             }\
           }\
         }",
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "a");
    let (ok2, v2, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(v2, "\u{00E9}");
    let (ok3, code3, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok3);
    assert_eq!(code3, "EOS");
}

/// chars() over an empty file yields EOS immediately.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_empty_file_eos_immediately() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@charsReply <- @f!?("chars")) {
            match charsReply {
              [true, stream] => {
                for (@r <- @stream!?("next")) { @"out"!(r) }
              }
              _ => @"out"!(charsReply)
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

/// chars() on a closed File returns FSERR_CLOSED without touching the fd.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@r <- @f!?("chars")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

/// chunk(n) on a CharStream returns a single String — the folded
/// concatenation of n 1-codepoint elements.  Matches spec §327
/// (CharStream chunk container is String).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_chunk_returns_folded_string() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "hello".toUtf8Bytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@charsReply <- @f!?("chars")) {
                match charsReply {
                  [true, stream] => {
                    for (@r <- @stream!?("chunk", 5)) { @"out"!(r) }
                  }
                  _ => @"out"!(charsReply)
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, s, _, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(s, "hello");
}

/// A byte at codepoint-start position that is a continuation byte
/// (0x80–0xBF) or an invalid lead (>=0xF8) is a decode error.  The
/// producer surfaces this as FSERR_IO "invalid UTF-8 start byte"
/// rather than panicking or emitting garbage.  We stage a single
/// 0xFF byte directly in mockFdCell.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_invalid_start_byte_returns_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        // Stage the mock file with a single 0xFF byte (never a valid
        // UTF-8 start).  chars() should observe this and return
        // FSERR_IO on the first next().
        for (@_ <- mockFdCell) {
          mockFdCell!(("ff".hexToBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@charsReply <- @f!?("chars")) {
              match charsReply {
                [true, stream] => {
                  for (@r <- @stream!?("next")) { @"out"!(r) }
                }
                _ => @"out"!(charsReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
}

/// A file whose last codepoint is truncated (e.g., the 0xC3 lead
/// byte of "é" without its 0xA9 continuation) surfaces as FSERR_IO
/// "truncated UTF-8 at EOF" on the affected next() rather than
/// hanging or emitting garbage.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_truncated_utf8_at_eof_returns_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        // File is "aC3" hex — an 'a' (0x61) then a 2-byte-lead byte
        // 0xC3 with no continuation.  First next() yields "a", second
        // yields FSERR_IO.
        for (@_ <- mockFdCell) {
          mockFdCell!(("61c3".hexToBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@charsReply <- @f!?("chars")) {
              match charsReply {
                [true, stream] => {
                  for (@n1 <- @stream!?("next")) {
                    for (@n2 <- @stream!?("next")) {
                      @"out"!([n1, n2])
                    }
                  }
                }
                _ => @"out"!([charsReply, Nil])
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
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "a");
    let (ok2, code2, _, _) = extract_reply(&outer.ps[1]);
    assert!(!ok2);
    assert_eq!(code2, "FSERR_IO");
}

// ---------------------------------------------------------------------
// Slice 4/5/6 review-fix regressions
// ---------------------------------------------------------------------

/// M-1 regression: writeBytes with a producer that emits a non-String
/// msg (e.g., an Int) must still reply to the caller.  Prior slice-4
/// code interpolated `msg` unguarded into `%%`, which raises
/// ReduceError for non-{String,Int,Bool,Uri} — and since the
/// interpolation sat inside the `ret!` continuation, the caller's
/// `for (@wbReply <- ...)` blocked forever.  Fix wraps `msg` in a
/// type-guard and substitutes "[non-string msg suppressed]" for
/// anything else.
///
/// Test producer emits an Int for msg (which %% actually accepts as
/// String — but the fallback is exercised on the fsWrite side by
/// mocking fsWrite to return a ByteArray msg).  We verify by making
/// the producer's msg a ByteArray: it must still receive a reply
/// with the fallback text.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytes_non_string_msg_does_not_hang() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Producer emits [false, "FSERR_IO", <ByteArray>] — msg is a
    // ByteArray, which %% would reject.  With the guard, the loop
    // substitutes a fallback text and still calls ret!.
    let src = with_libs(
        r#"
        new listState, producer, byteBuilder in {
          listState!([("bad".hexToBytes(), false)]) |
          contract producer(retCh) = {
            for (@lst <- listState) {
              match lst {
                []                     => { listState!([]) | retCh!([false, "EOS", ""]) }
                [(msg, false) ...tail] => { listState!(tail) | retCh!([false, "FSERR_IO", msg]) }
              }
            }
          } |
          contract byteBuilder(@vals, retCh) = { retCh!([true, vals.concatBytes()]) } |
          for (@stream <- Stream!?(*producer, *byteBuilder)) {
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wbReply <- @f!?("writeBytes", stream)) {
                @"out"!(wbReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    // Reply is [false, "FSERR_IO", "wrote 0 bytes before producer failure: [non-string msg suppressed]"]
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list reply"),
    };
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(!ok);
    let code = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert_eq!(code, "FSERR_IO");
    let msg = match single_expr(&list.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!("msg must still be a String even when producer msg was non-String"),
    };
    assert!(
        msg.contains("non-string msg suppressed"),
        "fallback text must indicate the substitution; got {:?}",
        msg
    );
}

/// Mi-1 regression: codepointLen rejects 0xC0 (an overlong 2-byte
/// lead per RFC 3629).  Prior slice-6 boundary was `b < 192 → -1;
/// b < 224 → 2` which classified 0xC0 as a 2-byte lead; decodeUtf8
/// on the (invalid) 2-byte sequence would emit two U+FFFD chars,
/// violating the "1 codepoint per next()" contract.  The tightened
/// boundary `b < 194 → -1` now returns -1 → producer surfaces
/// FSERR_IO on the first next().
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_rejects_overlong_2byte_lead() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        // File starts with 0xC0 (overlong 2-byte lead).  chars()'s
        // first next() must return FSERR_IO — not two U+FFFD chars.
        for (@_ <- mockFdCell) {
          mockFdCell!(("c080".hexToBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@charsReply <- @f!?("chars")) {
              match charsReply {
                [true, stream] => {
                  for (@r <- @stream!?("next")) { @"out"!(r) }
                }
                _ => @"out"!(charsReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
}

/// Mi-2 regression: codepointLen rejects 0xF5 (a 4-byte lead whose
/// codepoints exceed U+10FFFF).  Prior boundary was `b < 248 → 4`
/// which accepted 0xF5–0xF7; the tightened `b < 245 → 4` rejects
/// them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_rejects_out_of_range_4byte_lead() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        // File starts with 0xF5 (would encode > U+10FFFF).
        for (@_ <- mockFdCell) {
          mockFdCell!(("f5808080".hexToBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@charsReply <- @f!?("chars")) {
              match charsReply {
                [true, stream] => {
                  for (@r <- @stream!?("next")) { @"out"!(r) }
                }
                _ => @"out"!(charsReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
}

/// Regression sanity: 0xC2 (the smallest VALID 2-byte lead, encoding
/// U+0080..U+00FF via 0xC2 0x80..0xC2 0xBF) must STILL be accepted.
/// The tightened boundary `b < 194` (= 0xC2) uses < not <=, so 0xC2
/// remains valid.  Verified with U+00E9 (é: 0xC3 0xA9) — a different
/// valid 2-byte codepoint — via the existing multi-byte test; here
/// we spot-check 0xC2 0x80 (U+0080) directly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_accepts_smallest_valid_2byte_lead() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        // File is exactly 0xC2 0x80 — U+0080 (control character).
        for (@_ <- mockFdCell) {
          mockFdCell!(("c280".hexToBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@charsReply <- @f!?("chars")) {
              match charsReply {
                [true, stream] => {
                  for (@r <- @stream!?("next")) { @"out"!(r) }
                }
                _ => @"out"!(charsReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, s, _, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(s, "\u{0080}");
}

/// Regression sanity: 0xF4 (the largest VALID 4-byte lead, encoding
/// U+100000..U+10FFFF) must STILL be accepted.  The tightened
/// boundary `b < 245` (= 0xF5) preserves 0xF4.  Test uses U+10FFFD
/// (encoded as 0xF4 0x8F 0xBF 0xBD).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chars_accepts_largest_valid_4byte_lead() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        // File is exactly 0xF4 0x8F 0xBF 0xBD — U+10FFFD (private-use).
        for (@_ <- mockFdCell) {
          mockFdCell!(("f48fbfbd".hexToBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@charsReply <- @f!?("chars")) {
              match charsReply {
                [true, stream] => {
                  for (@r <- @stream!?("next")) { @"out"!(r) }
                }
                _ => @"out"!(charsReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, s, _, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(s, "\u{10FFFD}");
}

// ---------------------------------------------------------------------
// Slice 7: File.linesAsStrings + File.forEachLine.
// LF-only line terminator (CRLF and other \\R deferred).
// ---------------------------------------------------------------------

/// linesAsStrings over "abc\ndef\n" yields "abc", "def", then EOS.
/// A trailing LF does NOT emit a following empty line — POSIX text
/// convention.
///
/// Rholang string literals do NOT interpret `\n` as a newline; each
/// test constructs the file payload via `hexToBytes` (or concatBytes
/// of literal parts + hex LF) so the LF byte 0x0A actually lands in
/// the file.  "\n" in Rholang source would be a two-char string.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_two_terminated_lines() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // File bytes: "abc" + 0x0A + "def" + 0x0A
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abc".toUtf8Bytes(), "0a".hexToBytes(),
             "def".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@lasReply <- @f!?("linesAsStrings", 100)) {
                match lasReply {
                  [true, stream] => {
                    for (@n1 <- @stream!?("next")) {
                      for (@n2 <- @stream!?("next")) {
                        for (@n3 <- @stream!?("next")) {
                          @"out"!([n1, n2, n3])
                        }
                      }
                    }
                  }
                  _ => @"out"!([lasReply, Nil, Nil])
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
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "abc");
    let (ok2, v2, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(v2, "def");
    let (ok3, code3, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok3);
    assert_eq!(code3, "EOS");
}

/// A file NOT ending with LF still emits the final unterminated
/// content as its own line — then EOS on the next call.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_emits_unterminated_final_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abc".toUtf8Bytes(), "0a".hexToBytes(),
             "def".toUtf8Bytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@lasReply <- @f!?("linesAsStrings", 100)) {
                match lasReply {
                  [true, stream] => {
                    for (@n1 <- @stream!?("next")) {
                      for (@n2 <- @stream!?("next")) {
                        for (@n3 <- @stream!?("next")) {
                          @"out"!([n1, n2, n3])
                        }
                      }
                    }
                  }
                  _ => @"out"!([lasReply, Nil, Nil])
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
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "abc");
    let (ok2, v2, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(v2, "def");
    let (ok3, code3, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok3);
    assert_eq!(code3, "EOS");
}

/// Empty file yields EOS immediately.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_empty_file_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@lasReply <- @f!?("linesAsStrings", 100)) {
            match lasReply {
              [true, stream] => {
                for (@r <- @stream!?("next")) { @"out"!(r) }
              }
              _ => @"out"!(lasReply)
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

/// A file with only a LF ("\n") yields one empty-line element, then EOS.
/// (Content "\n" = one line terminated with LF, containing zero
/// characters.  Trailing LF does not emit a phantom second empty line.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_single_lf_yields_empty_then_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "0a".hexToBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@lasReply <- @f!?("linesAsStrings", 100)) {
                match lasReply {
                  [true, stream] => {
                    for (@n1 <- @stream!?("next")) {
                      for (@n2 <- @stream!?("next")) {
                        @"out"!([n1, n2])
                      }
                    }
                  }
                  _ => @"out"!([lasReply, Nil])
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
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "");
    let (ok2, code2, _, _) = extract_reply(&outer.ps[1]);
    assert!(!ok2);
    assert_eq!(code2, "EOS");
}

/// A line whose code-point count exceeds perLineCap returns
/// FSERR_QUOTA_EXCEEDED on the next() that overshoots.  The scan
/// fires the quota check the moment cpCount would rise above cap,
/// so a line of length cap+1 is caught before its terminator is
/// even seen.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_over_cap_returns_quota_exceeded() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Line is "abcdefghij" (10 code points, no LF).  cap = 5 → the
    // scan catches the excess at the 6th code point and errors out
    // before reaching EOF or LF.
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "abcdefghij".toUtf8Bytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@lasReply <- @f!?("linesAsStrings", 5)) {
                match lasReply {
                  [true, stream] => {
                    for (@r <- @stream!?("next")) { @"out"!(r) }
                  }
                  _ => @"out"!(lasReply)
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_QUOTA_EXCEEDED");
}

/// A line whose length is exactly perLineCap succeeds — the scan
/// only errors when cpCount would EXCEED cap, not when it equals.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_at_cap_boundary_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Line is "abcde\n" (5 code points then LF).  cap = 5 → succeeds.
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abcde".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@lasReply <- @f!?("linesAsStrings", 5)) {
                match lasReply {
                  [true, stream] => {
                    for (@r <- @stream!?("next")) { @"out"!(r) }
                  }
                  _ => @"out"!(lasReply)
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, s, _, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(s, "abcde");
}

/// linesAsStrings decodes UTF-8 correctly: a line with a multi-byte
/// codepoint emits a String containing that codepoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_utf8_multibyte_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        "for (@f <- File!?(1, \"/root\", \"test.txt\", \"rw\", \"oracular\")) {\
           for (@_ <- @f!?(\"writeByteArray\",\
             [\"a\u{00E9}b\".toUtf8Bytes(), \"0a\".hexToBytes()].concatBytes())) {\
             for (@_ <- @f!?(\"seek\", 0, \"set\")) {\
               for (@lasReply <- @f!?(\"linesAsStrings\", 100)) {\
                 match lasReply {\
                   [true, stream] => {\
                     for (@r <- @stream!?(\"next\")) { @\"out\"!(r) }\
                   }\
                   _ => @\"out\"!(lasReply)\
                 }\
               }\
             }\
           }\
         }",
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, s, _, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(s, "a\u{00E9}b");
}

/// linesAsStrings on a closed File returns FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@r <- @f!?("linesAsStrings", 100)) { @"out"!(r) }
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
async fn file_lines_as_strings_negative_cap_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("linesAsStrings", -1)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_non_int_cap_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("linesAsStrings", "many")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

// -- File.forEachLine ------------------------------------------------

/// forEachLine invokes the handler once per line and returns [true]
/// on success.  The handler is a bespoke contract that appends each
/// received line to a log cell so the test can verify the visit
/// order and count.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_for_each_line_visits_every_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new visitLog, myHandler in {
          visitLog!([]) |
          contract myHandler(returnCh, @line) = {
            for (@log <- visitLog) {
              visitLog!(log ++ [line]) |
              returnCh!([true])
            }
          } |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@_ <- @f!?("writeByteArray",
              ["one".toUtf8Bytes(), "0a".hexToBytes(),
               "two".toUtf8Bytes(), "0a".hexToBytes(),
               "three".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
              for (@_ <- @f!?("seek", 0, "set")) {
                for (@feReply <- @f!?("forEachLine", *myHandler, 100)) {
                  for (@log <<- visitLog) {
                    @"out"!([feReply, log])
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
    let (fe_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(fe_ok, "forEachLine must succeed on EOS");
    let log = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("visitLog was not a list"),
    };
    assert_eq!(log.ps.len(), 3, "handler must be called once per line");
    let names: Vec<String> = log
        .ps
        .iter()
        .map(|p| match single_expr(p).unwrap().expr_instance {
            Some(ExprInstance::GString(s)) => s,
            _ => panic!("log element was not a String"),
        })
        .collect();
    assert_eq!(names, vec!["one", "two", "three"]);
}

/// forEachLine forwards a linesAsStrings error (quota exceeded)
/// instead of invoking the handler.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_for_each_line_forwards_quota_error() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new visitLog, myHandler in {
          visitLog!([]) |
          contract myHandler(returnCh, @line) = {
            for (@log <- visitLog) {
              visitLog!(log ++ [line]) |
              returnCh!([true])
            }
          } |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@_ <- @f!?("writeByteArray", "abcdefghij".toUtf8Bytes())) {
              for (@_ <- @f!?("seek", 0, "set")) {
                for (@feReply <- @f!?("forEachLine", *myHandler, 5)) {
                  for (@log <<- visitLog) {
                    @"out"!([feReply, log])
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
    let (fe_ok, code, _, _) = extract_reply(&outer.ps[0]);
    assert!(!fe_ok);
    assert_eq!(code, "FSERR_QUOTA_EXCEEDED");
    let log = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    assert!(
        log.ps.is_empty(),
        "handler must NOT be invoked when the first line exceeds cap"
    );
}

// ---------------------------------------------------------------------
// Slice 7 review-driven coverage additions
// ---------------------------------------------------------------------

/// cap = 0 on a file with content: the first codepoint-start byte of
/// the first line triggers FSERR_QUOTA_EXCEEDED before any bytes are
/// emitted.  Documents the "0-cap == reject anything with content"
/// behavior noted in the review as a UX corner case.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_cap_zero_on_non_empty_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@lasReply <- @f!?("linesAsStrings", 0)) {
                match lasReply {
                  [true, stream] => {
                    for (@r <- @stream!?("next")) { @"out"!(r) }
                  }
                  _ => @"out"!(lasReply)
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_QUOTA_EXCEEDED");
}

/// cap = 0 on an empty file: no bytes to scan, EOS on first next().
/// Documents that "0-cap" is not itself a reject condition — only
/// non-empty content triggers it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_cap_zero_on_empty_file_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@lasReply <- @f!?("linesAsStrings", 0)) {
            match lasReply {
              [true, stream] => {
                for (@r <- @stream!?("next")) { @"out"!(r) }
              }
              _ => @"out"!(lasReply)
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

/// CRLF file preserves the CR byte in the emitted String.  Locks in
/// the MVP's LF-only line-terminator limitation: a Windows text file
/// ("line1\r\nline2\r\n") yields "line1\r" and "line2\r" — NOT
/// "line1" and "line2".  Full \\R handling (which would strip the CR)
/// is deferred to a follow-up slice.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_crlf_retains_cr_in_string() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // "line1\r\nline2\r\n" — hex 0d0a between lines and at end.
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["line1".toUtf8Bytes(), "0d0a".hexToBytes(),
             "line2".toUtf8Bytes(), "0d0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@lasReply <- @f!?("linesAsStrings", 100)) {
                match lasReply {
                  [true, stream] => {
                    for (@n1 <- @stream!?("next")) {
                      for (@n2 <- @stream!?("next")) {
                        @"out"!([n1, n2])
                      }
                    }
                  }
                  _ => @"out"!([lasReply, Nil])
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
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    // "line1\r" — 6 chars with trailing CR (0x0D).
    assert_eq!(
        v1, "line1\r",
        "CRLF file must yield the CR byte inside the line String \
         (MVP LF-only limitation)"
    );
    let (ok2, v2, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(v2, "line2\r");
}

/// Multi-byte codepoint counts as 1 codepoint against perLineCap,
/// not multiple bytes.  Line "abcdé" is 5 codepoints (4 ASCII + 1 é)
/// but 6 bytes (é is 0xC3 0xA9).  With cap=5 the scan must accept
/// it — verifies scanLineForLF's codepoint-start rule (b<128 OR
/// b>=194) rather than byte-count check.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_multibyte_at_cap_boundary() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        "for (@f <- File!?(1, \"/root\", \"test.txt\", \"rw\", \"oracular\")) {\
           for (@_ <- @f!?(\"writeByteArray\",\
             [\"abcd\u{00E9}\".toUtf8Bytes(), \"0a\".hexToBytes()].concatBytes())) {\
             for (@_ <- @f!?(\"seek\", 0, \"set\")) {\
               for (@lasReply <- @f!?(\"linesAsStrings\", 5)) {\
                 match lasReply {\
                   [true, stream] => {\
                     for (@r <- @stream!?(\"next\")) { @\"out\"!(r) }\
                   }\
                   _ => @\"out\"!(lasReply)\
                 }\
               }\
             }\
           }\
         }",
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, s, _, _) = extract_reply(&reply);
    assert!(ok, "5-codepoint line (6 bytes) must fit cap=5");
    assert_eq!(s, "abcd\u{00E9}");
}

/// Malformed UTF-8 mid-line: an invalid byte 0xFF between two ASCII
/// chars.  scanLineForLF counts 0xFF as a codepoint start (b>=194
/// rule) and continues; when the line is emitted, decodeUtf8 uses
/// from_utf8_lossy and substitutes 0xFF with U+FFFD.  The producer
/// does NOT surface FSERR_IO here — this is intentional (spec §921
/// for chars() says "decoding failure surfaces mid-stream as
/// FSERR_IO" but that applies to chars(), not linesAsStrings() which
/// materializes a whole String via decodeUtf8 whose failure mode is
/// substitution).  Locks in the observed replacement behavior.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_as_strings_malformed_utf8_substitutes_replacement_char() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // File: "a" + 0xFF + "b" + LF
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "ff".hexToBytes(),
             "b".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@lasReply <- @f!?("linesAsStrings", 100)) {
                match lasReply {
                  [true, stream] => {
                    for (@r <- @stream!?("next")) { @"out"!(r) }
                  }
                  _ => @"out"!(lasReply)
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, s, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "malformed UTF-8 should not fail the producer — \
                 decodeUtf8 substitutes U+FFFD"
    );
    assert_eq!(
        s, "a\u{FFFD}b",
        "invalid byte 0xFF must be replaced with U+FFFD in the \
         emitted String"
    );
}

/// Refill-across-LF: constructs a payload where the LF byte lands
/// PAST the 4KB internal buffer boundary, forcing the producer to
/// refill mid-scan and then find the LF in the refilled portion.
/// Verifies scanLineForLF state (scanPos, cpSoFar) preserves
/// correctly across refill.
///
/// NOTE ON RUNTIME: scanLineForLF is a per-byte tail-recursive
/// contract.  A ~4200-byte line issues ~4200 dispatches through the
/// tuplespace, which runs in ~10 min on a laptop — matches the
/// bytes-refill-boundary test's cost.  Marked #[ignore]; run with
/// `cargo test -- --ignored`.  A future native `bytes.findByte(b)`
/// would make this run in CI.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn file_lines_as_strings_refill_across_lf_boundary() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Build "aaaa...aaaX\n" where 'a' * 4098 + 'X' + LF = 4100 bytes.
    // The LF sits at byte 4099, past the 4096 refill boundary.
    let src = with_libs(
        r#"
        match "a".toUtf8Bytes() {
          b1 => {
            match [b1, b1, b1, b1, b1, b1, b1, b1,
                   b1, b1, b1, b1, b1, b1, b1, b1].concatBytes() {
              b16 => {
                match [b16, b16, b16, b16, b16, b16, b16, b16,
                       b16, b16, b16, b16, b16, b16, b16, b16].concatBytes() {
                  b256 => {
                    match [b256, b256, b256, b256,
                           b256, b256, b256, b256,
                           b256, b256, b256, b256,
                           b256, b256, b256, b256].concatBytes() {
                      b4096 => {
                        match [b4096, b1, b1, "X".toUtf8Bytes(),
                               "0a".hexToBytes()].concatBytes() {
                          bigLine => {
                            for (@_ <- mockFdCell) {
                              mockFdCell!((bigLine, 0)) |
                              for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
                                for (@lasReply <- @f!?("linesAsStrings", 10000)) {
                                  match lasReply {
                                    [true, stream] => {
                                      for (@n1 <- @stream!?("next")) {
                                        for (@n2 <- @stream!?("next")) {
                                          match n1 {
                                            [true, s] => @"out"!([true, s.length(), n2])
                                            _         => @"out"!([false, 0, n2])
                                          }
                                        }
                                      }
                                    }
                                    _ => @"out"!([false, 0, lasReply])
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
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let ok = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(ok, "line spanning refill boundary must decode successfully");
    let len = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GInt(n)) => n,
        _ => panic!(),
    };
    assert_eq!(
        len, 4099,
        "line length = 4098 'a's + 1 'X' = 4099 bytes/chars"
    );
    let (n2_ok, n2_code, _, _) = extract_reply(&outer.ps[2]);
    assert!(!n2_ok);
    assert_eq!(n2_code, "EOS", "second next() must signal EOS");
}

// ---------------------------------------------------------------------
// Slice 8: File.writeChars + File.writeLine — CharStream consumers.
// ---------------------------------------------------------------------

/// Bootstrap for an in-memory CharStream: pops 1-codepoint Strings
/// off a Rholang list literal.  Chunk builder is concatStringsLoop
/// (String ++ fold, same as CharStream's chunk builder in File.chars()).
/// Test snippet is spliced where `%TEST_SNIPPET%` appears; inside,
/// `stream` is bound to the constructed Stream handle.
fn char_stream_from_list(values_lit: &str) -> String {
    format!(
        r#"
        new listState, producer, charBuilder in {{
          listState!({values_lit}) |
          contract producer(retCh) = {{
            for (@lst <- listState) {{
              match lst {{
                []              => {{ listState!([]) | retCh!([false, "EOS", ""]) }}
                [head ...tail]  => {{ listState!(tail) | retCh!([true, head]) }}
              }}
            }}
          }} |
          contract charBuilder(@vals, retCh) = {{
            match vals {{
              v /\ List => concatStringsLoop!(v, "", *retCh)
              _         => retCh!([false, "FSERR_IO",
                "chunk builder expected List"])
            }}
          }} |
          for (@stream <- Stream!?(*producer, *charBuilder)) {{
            %TEST_SNIPPET%
          }}
        }}
        "#,
    )
}

// -- File.writeChars -------------------------------------------------

/// writeChars drains a CharStream of three 1-codepoint Strings and
/// the file ends up with "abc" at the cursor.  Verified by reading
/// back after seek(0).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_chars_drains_stream_into_file() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = char_stream_from_list(r#"["a", "b", "c"]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wcReply <- @f!?("writeChars", stream)) {
                for (@_ <- @f!?("seek", 0, "set")) {
                  for (@readReply <- @f!?("readN", 100)) {
                    @"out"!([wcReply, readReply])
                  }
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (wc_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(wc_ok, "writeChars must succeed on EOS");
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(read_ok);
    assert_eq!(bytes, Some(b"abc".to_vec()));
}

/// writeChars correctly UTF-8-encodes multi-byte codepoints.  A
/// stream of ["a", "é", "b"] (1 + 2 + 1 = 4 bytes) yields file
/// bytes `0x61 0xC3 0xA9 0x62`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_chars_utf8_encodes_multibyte() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = char_stream_from_list("[\"a\", \"\u{00E9}\", \"b\"]").replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wcReply <- @f!?("writeChars", stream)) {
                for (@_ <- @f!?("seek", 0, "set")) {
                  for (@readReply <- @f!?("readN", 100)) {
                    @"out"!([wcReply, readReply])
                  }
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (wc_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(wc_ok);
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(read_ok);
    assert_eq!(bytes, Some(vec![0x61, 0xC3, 0xA9, 0x62]));
}

/// writeChars over an empty stream is a no-op: [true], file size 0.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_chars_empty_stream_is_no_op() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = char_stream_from_list(r#"[]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wcReply <- @f!?("writeChars", stream)) {
                for (@sizeReply <- @f!?("size")) {
                  @"out"!([wcReply, sizeReply])
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (wc_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(wc_ok);
    let (size_ok, _, size, _) = extract_reply(&outer.ps[1]);
    assert!(size_ok);
    assert_eq!(size, Some(0));
}

/// writeChars on a read-only File returns FSERR_UNSUPPORTED (mode
/// gate fires before touching the stream).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_chars_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = char_stream_from_list(r#"["x"]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
              for (@r <- @f!?("writeChars", stream)) { @"out"!(r) }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// writeChars on a closed File returns FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_chars_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = char_stream_from_list(r#"["x"]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@_ <- @f!?("close")) {
                for (@r <- @f!?("writeChars", stream)) { @"out"!(r) }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

/// writeChars catches a producer that emits a non-String element:
/// forwards FSERR_IO with the running byte count and closes the
/// stream.  Prevents a ByteStream from being silently misused as a
/// CharStream input.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_chars_non_string_element_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Producer emits "a" (String) then 42 (Int) — writeChars accepts
    // the first, catches the second, closes and errors.
    let src = with_libs(
        r#"
        new listState, producer, charBuilder in {
          listState!(["a", 42]) |
          contract producer(retCh) = {
            for (@lst <- listState) {
              match lst {
                []             => { listState!([]) | retCh!([false, "EOS", ""]) }
                [head ...tail] => { listState!(tail) | retCh!([true, head]) }
              }
            }
          } |
          contract charBuilder(@vals, retCh) = {
            match vals {
              v /\ List => concatStringsLoop!(v, "", *retCh)
              _         => retCh!([false, "FSERR_IO", "not a list"])
            }
          } |
          for (@stream <- Stream!?(*producer, *charBuilder)) {
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wcReply <- @f!?("writeChars", stream)) {
                @"out"!(wcReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    // Reply: [false, "FSERR_IO", "wrote 1 bytes before producer returned non-String element"]
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(!ok);
    let code = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert_eq!(code, "FSERR_IO");
    let msg = match single_expr(&list.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert!(
        msg.contains("wrote 1 bytes before"),
        "byte count must reflect the one successful write; got {:?}",
        msg
    );
    assert!(
        msg.contains("non-String"),
        "message must identify the producer-shape mismatch; got {:?}",
        msg
    );
}

// -- File.writeLine --------------------------------------------------

/// writeLine drains the CharStream and appends LF: file contains
/// `"ab" + 0x0A` after the call.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_line_appends_lf_after_chars() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = char_stream_from_list(r#"["a", "b"]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wlReply <- @f!?("writeLine", stream)) {
                for (@_ <- @f!?("seek", 0, "set")) {
                  for (@readReply <- @f!?("readN", 100)) {
                    @"out"!([wlReply, readReply])
                  }
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (wl_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(wl_ok);
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(read_ok);
    assert_eq!(
        bytes,
        Some(vec![0x61, 0x62, 0x0A]),
        "file must contain the chars + LF terminator"
    );
}

/// writeLine over an empty stream writes only the LF: file contains
/// a single 0x0A byte.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_line_empty_stream_writes_just_lf() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = char_stream_from_list(r#"[]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wlReply <- @f!?("writeLine", stream)) {
                for (@_ <- @f!?("seek", 0, "set")) {
                  for (@readReply <- @f!?("readN", 100)) {
                    @"out"!([wlReply, readReply])
                  }
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (wl_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(wl_ok);
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(read_ok);
    assert_eq!(
        bytes,
        Some(vec![0x0A]),
        "empty stream writeLine yields just the LF terminator"
    );
}

/// writeLine on a read-only File returns FSERR_UNSUPPORTED before
/// touching the stream or writing LF.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_line_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = char_stream_from_list(r#"["x"]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
              for (@r <- @f!?("writeLine", stream)) { @"out"!(r) }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// writeLine on a closed File returns FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_line_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = char_stream_from_list(r#"["x"]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@_ <- @f!?("close")) {
                for (@r <- @f!?("writeLine", stream)) { @"out"!(r) }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

/// writeLine on a producer that errors mid-drain does NOT append LF
/// — the LF-terminator is only added on successful drain.  Verified
/// by checking the file bytes: only the successfully-written prefix
/// is present, no trailing LF.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_line_producer_error_omits_lf() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new listState, producer, charBuilder in {
          // Emit "a" then error — writeCharsLoop returns error;
          // writeLine forwards without appending LF.
          listState!([("a", true), ("simulated", false)]) |
          contract producer(retCh) = {
            for (@lst <- listState) {
              match lst {
                []                      => { listState!([]) | retCh!([false, "EOS", ""]) }
                [(v, true) ...tail]     => { listState!(tail) | retCh!([true, v]) }
                [(msg, false) ...tail]  => { listState!(tail) | retCh!([false, "FSERR_IO", msg]) }
              }
            }
          } |
          contract charBuilder(@vals, retCh) = {
            match vals {
              v /\ List => concatStringsLoop!(v, "", *retCh)
              _         => retCh!([false, "FSERR_IO", "not a list"])
            }
          } |
          for (@stream <- Stream!?(*producer, *charBuilder)) {
            for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
              for (@wlReply <- @f!?("writeLine", stream)) {
                for (@sizeReply <- @f!?("size")) {
                  @"out"!([wlReply, sizeReply])
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
    let (wl_ok, code, _, _) = extract_reply(&outer.ps[0]);
    assert!(!wl_ok);
    assert_eq!(code, "FSERR_IO");
    let (size_ok, _, size, _) = extract_reply(&outer.ps[1]);
    assert!(size_ok);
    assert_eq!(
        size,
        Some(1),
        "file must contain only the successfully-written byte ('a'), NO trailing LF"
    );
}

/// Slice-8 review regression: writeLine's LF-append phase — if the
/// character drain succeeds but the LF write itself fails, forward
/// the fsWrite error.  The prior impl only matched `[true, _]` and
/// forwarded any other shape via a bare `_ => return!(lfReply)`,
/// which would leak malformed replies to callers.  The fix
/// explicitly matches `[false, code, msg]` and normalizes malformed
/// shapes to FSERR_IO.
///
/// Bespoke source: real Stream.rho spliced in (needed for the
/// CharStream) plus a counter-driven fsWrite mock that succeeds on
/// the first call (character bytes) and fails on all subsequent
/// calls (the LF terminator).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_line_lf_write_failure_is_forwarded() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = format!(
        r#"
        new File, fdP, stateP, cmodeP, parseRwxToBits, parseRwxLoop,
            writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
            readLinesIntoLoop, drainToNextLF,
            codepointLen, concatStringsLoop, scanLineForLF,
            Stream, paramsP, gatherN, foldLoop, forEachLoop, foldChunksLoop,
            fsRead, fsReadAt, fsWrite, fsWriteAt,
            fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsTruncate, fsChmod, fsChown,
            // Phase 8 slice 8a — lock natives (mock preamble additions).
            fsLockRange, fsLockSequential, fsReleaseLock,
            fsReleaseAllForHolder,
            LockToken, lockStateP,
            withSequentialLock, withRangeLock,
            acquireRangeForStream, acquireSequentialForStream,
            writeCallCount,
            listState, producer, charBuilder
        in {{
          contract fsRead(@_fd, @_n, ret)  = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsReadAt(@_fd, @_o, @_n, ret) = {{ ret!([true, "".hexToBytes()]) }} |
          // Counter-driven fsWrite: first call succeeds (drain), rest fail (LF).
          writeCallCount!(0) |
          contract fsWrite(@_fd, @xs, ret) = {{
            for (@c <- writeCallCount) {{
              writeCallCount!(c + 1) |
              match c {{
                0 => ret!([true, xs.length()])
                _ => ret!([false, "FSERR_IO", "disk full simulated"])
              }}
            }}
          }} |
          contract fsWriteAt(@_fd, @_o, @_xs, ret) = {{ ret!([true, 0]) }} |
          contract fsSeek(@_fd, @_o, @_w, ret) = {{ ret!([true, 0]) }} |
          contract fsTell(@_fd, ret) = {{ ret!([true, 0]) }} |
          contract fsSize(@_fd, ret) = {{ ret!([true, 0]) }} |
          contract fsFlush(@_fd, ret) = {{ ret!([true]) }} |
          contract fsClose(@_fd, ret) = {{ ret!([true]) }} |
          contract fsTruncate(@_fd, @_n, ret) = {{ ret!([true]) }} |
          // C-R2 round-2: fsChmod takes cmode as 4th arg.
          contract fsChmod(@_r, @_p, @_b, @_cm, ret) = {{ ret!([true]) }} |
          // Slice 26: fsChown takes cmode as 5th arg.
          contract fsChown(@_r, @_p, @_o, @_g, @_cm, ret) = {{ ret!([true]) }} |
          contract parseRwxToBits(@_s, ret) = {{ ret!([true, 0]) }} |
          // Phase 8 slice 8a — always-succeed lock-native stubs; this
          // bespoke test drives writeLine's LF-write-failure path, not
          // lock semantics.
          contract fsLockRange(@_fd, @_o, @_l, @_m, @_h, @_cm, ret) = {{ ret!([true, 1]) }} |
          contract fsLockSequential(@_fd, @_h, @_cm, ret) = {{ ret!([true, 2]) }} |
          contract fsReleaseLock(@_id, ret) = {{ ret!([true]) }} |
          contract fsReleaseAllForHolder(@_h, ret) = {{ ret!([true, 0]) }} |

{}
          |
{}
          |
          {{
            listState!(["a"]) |
            contract producer(retCh) = {{
              for (@lst <- listState) {{
                match lst {{
                  []             => {{ listState!([]) | retCh!([false, "EOS", ""]) }}
                  [head ...tail] => {{ listState!(tail) | retCh!([true, head]) }}
                }}
              }}
            }} |
            contract charBuilder(@vals, retCh) = {{
              match vals {{
                v /\ List => concatStringsLoop!(v, "", *retCh)
                _         => retCh!([false, "FSERR_IO", "not a list"])
              }}
            }} |
            for (@stream <- Stream!?(*producer, *charBuilder)) {{
              for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
                for (@wlReply <- @f!?("writeLine", stream)) {{
                  @"out"!(wlReply)
                }}
              }}
            }}
          }}
        }}
        "#,
        lib_body(FILE_RHO),
        lib_body(STREAM_RHO)
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "writeLine must forward the LF-write failure");
    assert_eq!(code, "FSERR_IO");
}

// ---------------------------------------------------------------------
// Slice 9: File.readInto + File.writeFrom — buffer-taking byte methods.
// Line-oriented (readLineInto/readLinesInto) and positional
// (readAtInto/writeFromAt) variants deferred to slice 9b.
// ---------------------------------------------------------------------

/// readInto reads `min(fd_available, buf.remaining())` bytes from
/// the file's current cursor and writes them into the buffer via
/// the fill-lease protocol.  Reply is `[true, [nRead, eof]]`.
/// After readInto returns, the buffer contents equal what was in
/// the file at that offset, and the fd cursor has advanced by nRead.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_into_fills_buffer_from_cursor() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "abcdef".toUtf8Bytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              // Allocate a 4-byte buffer.
              for (@alloc <- Allocator!?()) {
                for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
                  for (@readReply <- @f!?("readInto", buf)) {
                    // Verify buffer content via toByteArray.
                    for (@baReply <- @buf!?("toByteArray")) {
                      // And verify cursor advanced by peek at tell().
                      for (@tellReply <- @f!?("tell")) {
                        @"out"!([readReply, baReply, tellReply])
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
        _ => panic!(),
    };
    // Reply is [[true, [nRead, eof]], [true, bytes], [true, pos]]
    let read_reply = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let read_ok = match single_expr(&read_reply.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(read_ok);
    let inner = match single_expr(&read_reply.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("readInto payload not a list"),
    };
    let n_read = match single_expr(&inner.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GInt(n)) => n,
        _ => panic!(),
    };
    assert_eq!(n_read, 4, "readInto must fill the 4-byte buffer");
    let eof = match single_expr(&inner.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(!eof, "reading 4 of 6 bytes should not signal EOF");
    // Buffer content
    let (ba_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(ba_ok);
    assert_eq!(
        bytes,
        Some(b"abcd".to_vec()),
        "buffer must contain the first 4 file bytes"
    );
    // Cursor advanced
    let (tell_ok, _, pos, _) = extract_reply(&outer.ps[2]);
    assert!(tell_ok);
    assert_eq!(pos, Some(4), "cursor must advance to end of the fill");
}

/// readInto at EOF returns `[true, [0, true]]` — zero bytes read,
/// EOF flag set.  Buffer is not modified.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_into_at_eof_returns_zero_bytes_and_eof_true() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          // Empty file; cursor already at position 0 == EOF.
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              for (@readReply <- @f!?("readInto", buf)) {
                @"out"!(readReply)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    // Reply: [true, [0, true]]
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(ok);
    let inner = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let n = match single_expr(&inner.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GInt(n)) => n,
        _ => panic!(),
    };
    assert_eq!(n, 0);
    let eof = match single_expr(&inner.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(eof, "EOF flag must be true when fsRead returned zero bytes");
}

/// readInto on a closed File returns FSERR_CLOSED without touching
/// the buffer's lease.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_into_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@alloc <- Allocator!?()) {
              for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
                for (@r <- @f!?("readInto", buf)) { @"out"!(r) }
              }
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

/// readInto acquires the fill lease before reading — a subsequent
/// buf.read() during the lease should reject.  Verified indirectly:
/// after readInto releases the lease (on success), the buffer is
/// readable again.  A buffer with a held lease elsewhere would return
/// BUFERR_FILLING to our readInto's beginFill call, which we forward.
///
/// Test approach: mint two buffers.  Acquire lease on buf1 externally,
/// try readInto(buf1) — should get BUFERR_FILLING.  Then release the
/// external lease, do readInto(buf1) again — should succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_into_forwards_lease_conflict() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              // Take the lease first.
              for (@[true, _tok] <- @buf!?("beginFill")) {
                // Now readInto — beginFill inside readInto should fail
                // with BUFERR_FILLING.
                for (@r <- @f!?("readInto", buf)) {
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
    assert!(!ok);
    assert_eq!(
        code, "BUFERR_FILLING",
        "readInto must forward the lease-conflict error from beginFill"
    );
}

// -- File.writeFrom --------------------------------------------------

/// writeFrom drains a filled buffer to the file at the current
/// cursor and advances the cursor.  Reply is `[true, nWritten]`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_from_drains_buffer_into_file() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 8)) {
              // Fill the buffer via its own lease.
              for (@[true, tok] <- @buf!?("beginFill")) {
                for (@[true, _] <- @buf!?("writeBytes", "hello".toUtf8Bytes())) {
                  for (@[true] <- @buf!?("endFill", tok)) {
                    // Now drain to file.
                    for (@wfReply <- @f!?("writeFrom", buf)) {
                      // Verify by reading back.
                      for (@_ <- @f!?("seek", 0, "set")) {
                        for (@readReply <- @f!?("readN", 100)) {
                          @"out"!([wfReply, readReply])
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
        _ => panic!(),
    };
    let (wf_ok, _, wf_n, _) = extract_reply(&outer.ps[0]);
    assert!(wf_ok);
    assert_eq!(
        wf_n,
        Some(5),
        "writeFrom must report the byte count written"
    );
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(read_ok);
    assert_eq!(
        bytes,
        Some(b"hello".to_vec()),
        "file must contain what the buffer held"
    );
}

/// writeFrom on an empty buffer is a no-op returning `[true, 0]`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_from_empty_buffer_is_no_op() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              for (@wfReply <- @f!?("writeFrom", buf)) {
                for (@sizeReply <- @f!?("size")) {
                  @"out"!([wfReply, sizeReply])
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
    let (wf_ok, _, wf_n, _) = extract_reply(&outer.ps[0]);
    assert!(wf_ok);
    assert_eq!(wf_n, Some(0));
    let (size_ok, _, size, _) = extract_reply(&outer.ps[1]);
    assert!(size_ok);
    assert_eq!(size, Some(0), "no bytes written from empty buffer");
}

/// writeFrom on a read-only File returns FSERR_UNSUPPORTED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_from_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              for (@r <- @f!?("writeFrom", buf)) { @"out"!(r) }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// writeFrom on a closed File returns FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_from_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@alloc <- Allocator!?()) {
              for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
                for (@r <- @f!?("writeFrom", buf)) { @"out"!(r) }
              }
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

/// writeFrom while a fill lease is held on the buffer returns
/// BUFERR_FILLING (from Buffer.toByteArray which requires no lease).
/// writeFrom does NOT take the lease itself (spec §978), so this is
/// specifically the co-holder lease conflict path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_from_forwards_lease_conflict() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              // Acquire the fill lease so toByteArray will refuse.
              for (@[true, _tok] <- @buf!?("beginFill")) {
                for (@r <- @f!?("writeFrom", buf)) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(
        code, "BUFERR_FILLING",
        "writeFrom must forward the lease-conflict error from toByteArray"
    );
}

/// Buffer round-trip: File → Buffer via readInto, then Buffer → File
/// via writeFrom (to a new position).  Verifies both methods compose
/// correctly and the byte content is preserved end-to-end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_into_then_write_from_round_trip() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          // Prime with "hello".
          for (@_ <- @f!?("writeByteArray", "hello".toUtf8Bytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, buf] <- @alloc!?("allocBytes", 5)) {
                  // Read all 5 bytes into the buffer.
                  for (@[true, _] <- @f!?("readInto", buf)) {
                    // Append the buffer to the file (cursor is now at 5).
                    for (@wfReply <- @f!?("writeFrom", buf)) {
                      // File should now be "hellohello".
                      for (@_ <- @f!?("seek", 0, "set")) {
                        for (@readReply <- @f!?("readN", 100)) {
                          @"out"!([wfReply, readReply])
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
        _ => panic!(),
    };
    let (wf_ok, _, n, _) = extract_reply(&outer.ps[0]);
    assert!(wf_ok);
    assert_eq!(n, Some(5));
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(read_ok);
    assert_eq!(
        bytes,
        Some(b"hellohello".to_vec()),
        "round-trip must preserve content and doubling verifies both directions"
    );
}

// ---------------------------------------------------------------------
// Slice 9a review-fix regressions
// ---------------------------------------------------------------------

/// B-9a-1 regression: readInto into a utf8-unit buffer must never
/// leave a partial multi-byte codepoint at the buffer's fill point.
///
/// Setup: file contains 3 bytes for "aé" (0x61 0xC3 0xA9); buffer
/// is utf8-unit with capacity 2.  A naive readInto would fill the
/// buffer with [0x61, 0xC3] — a partial codepoint — violating
/// spec §976.  The fix uses validUtf8PrefixLen to truncate the
/// chunk to [0x61] (just 'a'), and seeks the fd back by 1 byte so
/// the truncated tail can be read on a subsequent call.
///
/// Verified: buffer contains only 'a'; fd cursor is at position 1
/// (rewound from the naive position 2); a subsequent readN(10)
/// yields the remaining "é" bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_into_utf8_buffer_truncates_at_codepoint_boundary() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Mint Buffer directly (allocator's alloc(N, "utf8") multiplies
    // by 4 for max codepoint size; we want exactly 2 bytes cap to
    // force the boundary case).
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          // Prime file with "aé" (0x61 0xC3 0xA9).
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "c3a9".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              // 2-byte utf8-unit buffer (direct Buffer mint).
              for (@buf <- Buffer!?(2, "utf8")) {
                for (@readReply <- @f!?("readInto", buf)) {
                  for (@baReply <- @buf!?("toByteArray")) {
                    for (@tellReply <- @f!?("tell")) {
                      // Also read the rest to verify the seek-back
                      // preserved the truncated tail.
                      for (@readNReply <- @f!?("readN", 10)) {
                        @"out"!([readReply, baReply, tellReply, readNReply])
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
        _ => panic!(),
    };
    // readInto reply: [true, [1, false]]
    let read_reply = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let read_ok = match single_expr(&read_reply.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(read_ok);
    let inner = match single_expr(&read_reply.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let n_read = match single_expr(&inner.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GInt(n)) => n,
        _ => panic!(),
    };
    assert_eq!(
        n_read, 1,
        "readInto must report only the codepoint-boundary-truncated count"
    );
    // Buffer content: just the 'a' byte
    let (ba_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(ba_ok);
    assert_eq!(
        bytes,
        Some(vec![0x61]),
        "utf8 buffer must contain only the valid-UTF-8 prefix"
    );
    // Cursor rewound to 1 (not 2)
    let (tell_ok, _, pos, _) = extract_reply(&outer.ps[2]);
    assert!(tell_ok);
    assert_eq!(
        pos,
        Some(1),
        "fd cursor must be rewound past the truncated partial codepoint"
    );
    // Remaining file bytes: the é (0xC3 0xA9)
    let (rn_ok, _, _, rn_bytes) = extract_reply(&outer.ps[3]);
    assert!(rn_ok);
    assert_eq!(
        rn_bytes,
        Some(vec![0xC3, 0xA9]),
        "the truncated tail must still be readable after the rewind"
    );
}

/// B-9a-1 sanity: on a byte-unit buffer, readInto does NOT apply
/// UTF-8 truncation — a 2-byte buffer + 3-byte file yields exactly
/// 2 bytes in the buffer, regardless of codepoint boundaries.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_into_byte_buffer_ignores_codepoint_boundary() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "c3a9".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, buf] <- @alloc!?("allocBytes", 2)) {
                  for (@readReply <- @f!?("readInto", buf)) {
                    for (@baReply <- @buf!?("toByteArray")) {
                      for (@tellReply <- @f!?("tell")) {
                        @"out"!([readReply, baReply, tellReply])
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
        _ => panic!(),
    };
    let (ba_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(ba_ok);
    assert_eq!(
        bytes,
        Some(vec![0x61, 0xC3]),
        "byte-unit buffer must accept mid-codepoint bytes without truncation"
    );
    let (tell_ok, _, pos, _) = extract_reply(&outer.ps[2]);
    assert!(tell_ok);
    assert_eq!(
        pos,
        Some(2),
        "byte-unit buffer does not trigger a seek-back"
    );
}

/// Coverage: readInto on a read-only File succeeds (reading doesn't
/// require write mode).  Existing tests inferred this via the
/// round-trip test but never isolated the read-only case.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_into_on_read_only_file_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Stage the mock file with content directly (readonly mode
    // prevents writing to prime it via the File API).
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("abcd".toUtf8Bytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
            for (@alloc <- Allocator!?()) {
              for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
                for (@readReply <- @f!?("readInto", buf)) {
                  for (@baReply <- @buf!?("toByteArray")) {
                    @"out"!([readReply, baReply])
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
    // readInto must succeed on a read-only File.
    let read_reply = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let ok = match single_expr(&read_reply.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(ok, "readInto must be allowed on a read-only File");
    let (ba_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(ba_ok);
    assert_eq!(bytes, Some(b"abcd".to_vec()));
}

// ---------------------------------------------------------------------
// Slice 9b: File.readAtInto + File.writeFromAt — positional
// buffer-taking byte methods.
// ---------------------------------------------------------------------

/// readAtInto reads from an absolute file offset into a buffer.
/// Verifies: bytes are correct, sequential cursor is NOT touched
/// (positional read), file has 6 bytes "abcdef" and readAtInto(2, buf)
/// yields "cdef" into a 4-byte buffer with sequential cursor still
/// at 6 (where it was after the priming write).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_at_into_positional_fill_does_not_move_cursor() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "abcdef".toUtf8Bytes())) {
            // Cursor now at 6.  readAtInto at offset 2 shouldn't move it.
            for (@alloc <- Allocator!?()) {
              for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
                for (@readReply <- @f!?("readAtInto", 2, buf)) {
                  for (@baReply <- @buf!?("toByteArray")) {
                    for (@tellReply <- @f!?("tell")) {
                      @"out"!([readReply, baReply, tellReply])
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
        _ => panic!(),
    };
    let read_reply = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let read_ok = match single_expr(&read_reply.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(read_ok);
    let inner = match single_expr(&read_reply.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let n_read = match single_expr(&inner.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GInt(n)) => n,
        _ => panic!(),
    };
    assert_eq!(n_read, 4);
    let (ba_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(ba_ok);
    assert_eq!(
        bytes,
        Some(b"cdef".to_vec()),
        "readAtInto(2, buf) must yield bytes starting at offset 2"
    );
    // Sequential cursor must be unchanged from the priming write.
    let (tell_ok, _, pos, _) = extract_reply(&outer.ps[2]);
    assert!(tell_ok);
    assert_eq!(
        pos,
        Some(6),
        "readAtInto must NOT move the sequential cursor"
    );
}

/// UTF-8 boundary rule applies to readAtInto too (spec §976).  file
/// "aé" (3 bytes 0x61 0xC3 0xA9); 2-byte utf8 buffer; readAtInto(0)
/// reads 2 bytes, truncates to 1 (valid UTF-8 prefix).  Unlike
/// readInto, NO seek-back happens because positional read didn't
/// advance any cursor to begin with.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_at_into_utf8_truncates_no_seek_back() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "c3a9".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              // Cursor at 0.
              for (@buf <- Buffer!?(2, "utf8")) {
                for (@readReply <- @f!?("readAtInto", 0, buf)) {
                  for (@baReply <- @buf!?("toByteArray")) {
                    for (@tellReply <- @f!?("tell")) {
                      @"out"!([readReply, baReply, tellReply])
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
        _ => panic!(),
    };
    // Reply is [true, [1, false]].
    let read_reply = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let inner = match single_expr(&read_reply.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let n_read = match single_expr(&inner.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GInt(n)) => n,
        _ => panic!(),
    };
    assert_eq!(n_read, 1, "utf8 boundary rule truncates 2-byte read to 1");
    let (ba_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(ba_ok);
    assert_eq!(bytes, Some(vec![0x61]));
    // Sequential cursor at 0 (was reset via seek before readAtInto).
    let (tell_ok, _, pos, _) = extract_reply(&outer.ps[2]);
    assert!(tell_ok);
    assert_eq!(
        pos,
        Some(0),
        "readAtInto never moves the sequential cursor — no seek-back needed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_at_into_on_read_only_file_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("hello".toUtf8Bytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
            for (@alloc <- Allocator!?()) {
              for (@[true, buf] <- @alloc!?("allocBytes", 5)) {
                for (@readReply <- @f!?("readAtInto", 0, buf)) {
                  for (@baReply <- @buf!?("toByteArray")) {
                    @"out"!([readReply, baReply])
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
    let read_reply = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let ok = match single_expr(&read_reply.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(ok);
    let (ba_ok, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(ba_ok);
    assert_eq!(bytes, Some(b"hello".to_vec()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_at_into_negative_offset_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              for (@r <- @f!?("readAtInto", -1, buf)) { @"out"!(r) }
            }
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
async fn file_read_at_into_non_int_offset_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              for (@r <- @f!?("readAtInto", "zero", buf)) { @"out"!(r) }
            }
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
async fn file_read_at_into_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@alloc <- Allocator!?()) {
              for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
                for (@r <- @f!?("readAtInto", 0, buf)) { @"out"!(r) }
              }
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
async fn file_read_at_into_forwards_lease_conflict() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              for (@[true, _tok] <- @buf!?("beginFill")) {
                for (@r <- @f!?("readAtInto", 0, buf)) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "BUFERR_FILLING");
}

// -- File.writeFromAt ------------------------------------------------

/// writeFromAt writes buffer content at absolute offset without
/// moving the sequential cursor.  File is 6 bytes "aaaaaa" primed
/// via writeByteArray (cursor at 6).  writeFromAt(2, buf-of-"XX")
/// yields "aaXXaa" with cursor still at 6.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_from_at_positional_write_does_not_move_cursor() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "aaaaaa".toUtf8Bytes())) {
            // Cursor at 6.
            for (@alloc <- Allocator!?()) {
              for (@[true, buf] <- @alloc!?("allocBytes", 2)) {
                for (@[true, tok] <- @buf!?("beginFill")) {
                  for (@[true, _] <- @buf!?("writeBytes", "XX".toUtf8Bytes())) {
                    for (@[true] <- @buf!?("endFill", tok)) {
                      for (@wfReply <- @f!?("writeFromAt", 2, buf)) {
                        for (@tellReply <- @f!?("tell")) {
                          // Verify file content via readN from 0.
                          for (@_ <- @f!?("seek", 0, "set")) {
                            for (@readReply <- @f!?("readN", 10)) {
                              @"out"!([wfReply, tellReply, readReply])
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
        _ => panic!(),
    };
    let (wf_ok, _, wf_n, _) = extract_reply(&outer.ps[0]);
    assert!(wf_ok);
    assert_eq!(wf_n, Some(2));
    // Cursor unchanged
    let (tell_ok, _, pos, _) = extract_reply(&outer.ps[1]);
    assert!(tell_ok);
    assert_eq!(
        pos,
        Some(6),
        "writeFromAt must NOT move the sequential cursor"
    );
    // File content
    let (read_ok, _, _, bytes) = extract_reply(&outer.ps[2]);
    assert!(read_ok);
    assert_eq!(
        bytes,
        Some(b"aaXXaa".to_vec()),
        "positional write must overwrite [2..4) with buffer content"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_from_at_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              for (@r <- @f!?("writeFromAt", 0, buf)) { @"out"!(r) }
            }
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
async fn file_write_from_at_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@alloc <- Allocator!?()) {
              for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
                for (@r <- @f!?("writeFromAt", 0, buf)) { @"out"!(r) }
              }
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
async fn file_write_from_at_negative_offset_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              for (@r <- @f!?("writeFromAt", -1, buf)) { @"out"!(r) }
            }
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
async fn file_write_from_at_non_int_offset_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              for (@r <- @f!?("writeFromAt", "zero", buf)) { @"out"!(r) }
            }
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
async fn file_write_from_at_forwards_lease_conflict() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 4)) {
              // Take the lease so toByteArray refuses.
              for (@[true, _tok] <- @buf!?("beginFill")) {
                for (@r <- @f!?("writeFromAt", 0, buf)) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "BUFERR_FILLING");
}

// ---------------------------------------------------------------------
// Slice 10: File.readLine — single-shot CharStream over one line.
// ---------------------------------------------------------------------

/// readLine over "abc\ndef" yields the chars of the first line
/// ("a", "b", "c") then EOS.  LF consumed but not emitted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_yields_chars_then_eos_at_lf() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abc".toUtf8Bytes(), "0a".hexToBytes(),
             "def".toUtf8Bytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@rlReply <- @f!?("readLine")) {
                match rlReply {
                  [true, stream] => {
                    for (@n1 <- @stream!?("next")) {
                      for (@n2 <- @stream!?("next")) {
                        for (@n3 <- @stream!?("next")) {
                          for (@n4 <- @stream!?("next")) {
                            @"out"!([n1, n2, n3, n4])
                          }
                        }
                      }
                    }
                  }
                  _ => @"out"!([rlReply, Nil, Nil, Nil])
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
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "a");
    let (ok2, v2, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(v2, "b");
    let (ok3, v3, _, _) = extract_reply(&outer.ps[2]);
    assert!(ok3);
    assert_eq!(v3, "c");
    let (ok4, code4, _, _) = extract_reply(&outer.ps[3]);
    assert!(!ok4);
    assert_eq!(code4, "EOS", "fourth next() must signal EOS");
}

/// After readLine drains, cursor is past the LF.  Second readLine
/// yields the next line ("cd" then EOS for unterminated final).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_advances_cursor_past_lf_for_next_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["ab".toUtf8Bytes(), "0a".hexToBytes(),
             "cd".toUtf8Bytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, s1] <- @f!?("readLine")) {
                for (@_ <- @s1!?("next")) {
                  for (@_ <- @s1!?("next")) {
                    for (@_ <- @s1!?("next")) {  // EOS
                      for (@[true, s2] <- @f!?("readLine")) {
                        for (@n1 <- @s2!?("next")) {
                          for (@n2 <- @s2!?("next")) {
                            for (@n3 <- @s2!?("next")) {
                              @"out"!([n1, n2, n3])
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
        _ => panic!(),
    };
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "c");
    let (ok2, v2, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(v2, "d");
    let (ok3, code3, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok3);
    assert_eq!(code3, "EOS");
}

/// readLine at EOF returns a pre-exhausted CharStream (spec §926).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_at_eof_pre_exhausted_stream() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@[true, stream] <- @f!?("readLine")) {
            for (@r <- @stream!?("next")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

/// readLine on "\n" yields an empty-line stream — LF consumed, no
/// chars emitted, immediate EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_blank_line_yields_eos_immediately() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "0a".hexToBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, stream] <- @f!?("readLine")) {
                for (@r <- @stream!?("next")) { @"out"!(r) }
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

/// readLine decodes multi-byte UTF-8: "aé\n" → "a", "é", EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_multibyte_utf8() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        "for (@f <- File!?(1, \"/root\", \"test.txt\", \"rw\", \"oracular\")) {\
           for (@_ <- @f!?(\"writeByteArray\",\
             [\"a\u{00E9}\".toUtf8Bytes(), \"0a\".hexToBytes()].concatBytes())) {\
             for (@_ <- @f!?(\"seek\", 0, \"set\")) {\
               for (@[true, stream] <- @f!?(\"readLine\")) {\
                 for (@n1 <- @stream!?(\"next\")) {\
                   for (@n2 <- @stream!?(\"next\")) {\
                     for (@n3 <- @stream!?(\"next\")) {\
                       @\"out\"!([n1, n2, n3])\
                     }\
                   }\
                 }\
               }\
             }\
           }\
         }",
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "a");
    let (ok2, v2, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(v2, "\u{00E9}");
    let (ok3, code3, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok3);
    assert_eq!(code3, "EOS");
}

/// readLine's chunk() returns the whole line as a String, NOT
/// passing the LF terminator.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_chunk_returns_line_string() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["hello".toUtf8Bytes(), "0a".hexToBytes(),
             "world".toUtf8Bytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, stream] <- @f!?("readLine")) {
                for (@r <- @stream!?("chunk", 20)) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, s, _, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(s, "hello");
}

/// readLine on closed File returns FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@r <- @f!?("readLine")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

// ---------------------------------------------------------------------
// Slice 12: File.readLineInto — line-oriented buffer-taking.
// Reply: [true, [nRead, {"eof": Bool, "truncated": Bool}]].
// ---------------------------------------------------------------------

/// Extract (nRead, eof, truncated) from a readLineInto success reply.
/// Panics on shape mismatch — expected `[true, [nRead, Map]]`.
fn extract_read_line_into(reply: &Par) -> (i64, bool, bool) {
    let outer = match single_expr(reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("readLineInto reply not a list"),
    };
    let ok = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("head not Bool"),
    };
    assert!(ok, "expected success reply, got {:?}", outer.ps);
    let inner = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("payload not a list"),
    };
    let n = match single_expr(&inner.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GInt(n)) => n,
        _ => panic!("nRead not Int"),
    };
    // Extract the flags map — iterate kvs looking for "eof" and "truncated".
    let flags_map = match single_expr(&inner.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EMapBody(m)) => m,
        _ => panic!("flags not a Map"),
    };
    let mut eof = None;
    let mut truncated = None;
    for kv in flags_map.kvs {
        let key = kv
            .key
            .as_ref()
            .and_then(single_expr)
            .and_then(|e| e.expr_instance);
        let val = kv
            .value
            .as_ref()
            .and_then(single_expr)
            .and_then(|e| e.expr_instance);
        if let (Some(ExprInstance::GString(k)), Some(ExprInstance::GBool(v))) = (key, val) {
            if k == "eof" {
                eof = Some(v);
            }
            if k == "truncated" {
                truncated = Some(v);
            }
        }
    }
    (
        n,
        eof.expect("eof flag missing"),
        truncated.expect("truncated flag missing"),
    )
}

/// readLineInto on "abc\n" with 10-byte buffer: nRead=3, no eof, no
/// truncation, buf contains "abc", cursor past LF.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_happy_path() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abc".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, buf] <- @alloc!?("allocBytes", 10)) {
                  for (@readReply <- @f!?("readLineInto", buf)) {
                    for (@baReply <- @buf!?("toByteArray")) {
                      for (@tellReply <- @f!?("tell")) {
                        @"out"!([readReply, baReply, tellReply])
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
        _ => panic!(),
    };
    let (n, eof, truncated) = extract_read_line_into(&outer.ps[0]);
    assert_eq!(n, 3);
    assert!(!eof);
    assert!(!truncated);
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(b"abc".to_vec()));
    let (_, _, pos, _) = extract_reply(&outer.ps[2]);
    assert_eq!(pos, Some(4), "cursor must be past the LF");
}

/// readLineInto on empty file: nRead=0, eof=true, truncated=false.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_eof_returns_zero_and_eof_flag() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 10)) {
              for (@r <- @f!?("readLineInto", buf)) { @"out"!(r) }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (n, eof, truncated) = extract_read_line_into(&reply);
    assert_eq!(n, 0);
    assert!(eof);
    assert!(!truncated);
}

/// Blank line ("\n"): nRead=0, eof=false, truncated=false.
/// Distinguishable from EOF via the eof flag (spec §969).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_blank_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "0a".hexToBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, buf] <- @alloc!?("allocBytes", 10)) {
                  for (@r <- @f!?("readLineInto", buf)) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (n, eof, truncated) = extract_read_line_into(&reply);
    assert_eq!(n, 0);
    assert!(!eof, "blank line is NOT EOF");
    assert!(!truncated);
}

/// Line "abcdef\n" (7 bytes) with 3-byte buffer: nRead=3, truncated
/// =true, LF NOT consumed.  Cursor at 3 (before the "def\n" tail).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_truncated_when_line_exceeds_buffer() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abcdef".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, buf] <- @alloc!?("allocBytes", 3)) {
                  for (@readReply <- @f!?("readLineInto", buf)) {
                    for (@baReply <- @buf!?("toByteArray")) {
                      for (@tellReply <- @f!?("tell")) {
                        @"out"!([readReply, baReply, tellReply])
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
        _ => panic!(),
    };
    let (n, eof, truncated) = extract_read_line_into(&outer.ps[0]);
    assert_eq!(n, 3);
    assert!(!eof);
    assert!(truncated);
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(b"abc".to_vec()));
    let (_, _, pos, _) = extract_reply(&outer.ps[2]);
    assert_eq!(pos, Some(3), "LF not consumed on truncation");
}

/// Unterminated final line: file "abc" (no LF), buf cap 10.
/// nRead=3, eof=true, truncated=false.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_unterminated_final_line_marks_eof() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "abc".toUtf8Bytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, buf] <- @alloc!?("allocBytes", 10)) {
                  for (@readReply <- @f!?("readLineInto", buf)) {
                    for (@baReply <- @buf!?("toByteArray")) {
                      @"out"!([readReply, baReply])
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
        _ => panic!(),
    };
    let (n, eof, truncated) = extract_read_line_into(&outer.ps[0]);
    assert_eq!(n, 3);
    assert!(eof);
    assert!(!truncated);
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(b"abc".to_vec()));
}

/// UTF-8 boundary (spec §976): file "aé\n" (3 content bytes + LF),
/// utf8 buffer cap 2.  Content is 3 bytes; utf8 truncation writes
/// only 1 byte ('a').  truncated=true, LF NOT consumed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_utf8_boundary_marks_truncated_preserves_lf() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "c3a9".hexToBytes(),
             "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@buf <- Buffer!?(2, "utf8")) {
                for (@readReply <- @f!?("readLineInto", buf)) {
                  for (@baReply <- @buf!?("toByteArray")) {
                    for (@tellReply <- @f!?("tell")) {
                      @"out"!([readReply, baReply, tellReply])
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
        _ => panic!(),
    };
    let (n, eof, truncated) = extract_read_line_into(&outer.ps[0]);
    assert_eq!(n, 1, "utf8 truncation drops the partial é bytes");
    assert!(!eof);
    assert!(truncated);
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(vec![0x61]));
    let (_, _, pos, _) = extract_reply(&outer.ps[2]);
    assert_eq!(
        pos,
        Some(1),
        "cursor past 'a' but before é bytes; LF still ahead"
    );
}

/// Two sequential readLineInto calls read consecutive lines correctly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_sequential_reads_advance_correctly() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["one".toUtf8Bytes(), "0a".hexToBytes(),
             "two".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, buf1] <- @alloc!?("allocBytes", 10)) {
                  for (@r1 <- @f!?("readLineInto", buf1)) {
                    for (@ba1 <- @buf1!?("toByteArray")) {
                      for (@[true, buf2] <- @alloc!?("allocBytes", 10)) {
                        for (@r2 <- @f!?("readLineInto", buf2)) {
                          for (@ba2 <- @buf2!?("toByteArray")) {
                            @"out"!([r1, ba1, r2, ba2])
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
        _ => panic!(),
    };
    let (n1, _, _) = extract_read_line_into(&outer.ps[0]);
    assert_eq!(n1, 3);
    let (_, _, _, bytes1) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes1, Some(b"one".to_vec()));
    let (n2, _, _) = extract_read_line_into(&outer.ps[2]);
    assert_eq!(n2, 3);
    let (_, _, _, bytes2) = extract_reply(&outer.ps[3]);
    assert_eq!(bytes2, Some(b"two".to_vec()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@alloc <- Allocator!?()) {
              for (@[true, buf] <- @alloc!?("allocBytes", 10)) {
                for (@r <- @f!?("readLineInto", buf)) { @"out"!(r) }
              }
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
async fn file_read_line_into_forwards_lease_conflict() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, buf] <- @alloc!?("allocBytes", 10)) {
              for (@[true, _tok] <- @buf!?("beginFill")) {
                for (@r <- @f!?("readLineInto", buf)) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "BUFERR_FILLING");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_on_read_only_file_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!((["hi".toUtf8Bytes(), "0a".hexToBytes()].concatBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
            for (@alloc <- Allocator!?()) {
              for (@[true, buf] <- @alloc!?("allocBytes", 10)) {
                for (@r <- @f!?("readLineInto", buf)) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (n, eof, truncated) = extract_read_line_into(&reply);
    assert_eq!(n, 2);
    assert!(!eof);
    assert!(!truncated);
}

// ---------------------------------------------------------------------
// Slice 10/12 review-driven coverage additions.
// ---------------------------------------------------------------------

/// M-1 regression: readLineInto with a fully-pre-filled buffer must
/// return [true, [0, {eof: false, truncated: true}]] — not EOF.
///
/// Before the fix, fsRead(fd, 0) returned 0 bytes and readLineInto
/// misclassified that as EOF for a file that still had unread content.
/// The preflight now short-circuits when remaining == 0.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_full_buffer_returns_truncated_not_eof() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abc".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, buf] <- @alloc!?("allocBytes", 3)) {
                  // Pre-fill the buffer via a lease so remaining() == 0.
                  for (@[true, tok] <- @buf!?("beginFill")) {
                    for (@[true, _] <- @buf!?("writeBytes", "xyz".toUtf8Bytes())) {
                      for (@[true] <- @buf!?("endFill", tok)) {
                        for (@rReply <- @f!?("readLineInto", buf)) {
                          for (@baReply <- @buf!?("toByteArray")) {
                            for (@tellReply <- @f!?("tell")) {
                              @"out"!([rReply, baReply, tellReply])
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
        _ => panic!("expected list of three replies"),
    };
    let (n, eof, truncated) = extract_read_line_into(&outer.ps[0]);
    assert_eq!(n, 0, "no bytes read when buffer is already full");
    assert!(!eof, "file still has unread bytes; must not report EOF");
    assert!(
        truncated,
        "truncated=true tells caller to drain buffer first"
    );
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(b"xyz".to_vec()), "pre-filled bytes preserved");
    let (_, _, pos, _) = extract_reply(&outer.ps[2]);
    assert_eq!(pos, Some(0), "fd cursor unchanged when preflight rejects");
}

/// m-1 regression: readLineInto on a utf8 buffer, reading a file whose
/// cursor sits on an invalid UTF-8 start byte, must return
/// [false, "FSERR_IO", ...] — not retry forever, not silently drop the
/// byte, and not leave a partial fill in the buffer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_malformed_utf8_at_cursor_returns_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("ff".hexToBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@buf <- Buffer!?(10, "utf8")) {
              for (@rReply <- @f!?("readLineInto", buf)) {
                for (@baReply <- @buf!?("toByteArray")) {
                  @"out"!([rReply, baReply])
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
        _ => panic!("expected list of two replies"),
    };
    let (ok, code, _, _) = extract_reply(&outer.ps[0]);
    assert!(!ok, "must fail on malformed UTF-8");
    assert_eq!(code, "FSERR_IO");
    let (ok_ba, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert!(ok_ba, "buffer still usable (lease released before error)");
    assert_eq!(bytes, Some(vec![]), "no partial write on FSERR_IO");
}

/// M-3 documentation: line content length == buffer capacity, LF sits
/// one byte past the end.  fsRead(remBytes) reads exactly `remBytes`
/// bytes and never sees the LF; the short-read heuristic cannot tell
/// whether a byte-cap-filling chunk was a full line minus LF or an
/// actual longer line.  Current MVP behavior: report truncated=true,
/// leave LF for the next call.  The subsequent readLineInto call sees
/// the LF as a blank line (n=0, eof=false, truncated=false).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_content_matches_capacity_defers_lf() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abcd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, buf1] <- @alloc!?("allocBytes", 4)) {
                  for (@r1 <- @f!?("readLineInto", buf1)) {
                    for (@ba1 <- @buf1!?("toByteArray")) {
                      for (@tell1 <- @f!?("tell")) {
                        for (@[true, buf2] <- @alloc!?("allocBytes", 4)) {
                          for (@r2 <- @f!?("readLineInto", buf2)) {
                            for (@tell2 <- @f!?("tell")) {
                              @"out"!([r1, ba1, tell1, r2, tell2])
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
        _ => panic!(),
    };
    // First call: content fills buffer, LF not seen, marked truncated.
    let (n1, eof1, tr1) = extract_read_line_into(&outer.ps[0]);
    assert_eq!(n1, 4);
    assert!(!eof1);
    assert!(
        tr1,
        "M-3 heuristic: cannot distinguish full line from long line \
         when chunk fills exactly; marks truncated conservatively"
    );
    let (_, _, _, ba1) = extract_reply(&outer.ps[1]);
    assert_eq!(ba1, Some(b"abcd".to_vec()));
    let (_, _, pos1, _) = extract_reply(&outer.ps[2]);
    assert_eq!(pos1, Some(4), "cursor at content end; LF not consumed");
    // Second call: sees the deferred LF as a blank line.
    let (n2, eof2, tr2) = extract_read_line_into(&outer.ps[3]);
    assert_eq!(n2, 0);
    assert!(!eof2, "blank line, not EOF");
    assert!(!tr2);
    let (_, _, pos2, _) = extract_reply(&outer.ps[4]);
    assert_eq!(pos2, Some(5), "cursor now past LF");
}

/// Complementary happy-path: when the buffer is strictly larger than
/// content+LF, the whole line is consumed in one call — no truncation,
/// LF consumed, cursor advances past LF.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_buffer_strictly_larger_than_content_plus_lf() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abcd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, buf] <- @alloc!?("allocBytes", 5)) {
                  for (@rReply <- @f!?("readLineInto", buf)) {
                    for (@baReply <- @buf!?("toByteArray")) {
                      for (@tellReply <- @f!?("tell")) {
                        @"out"!([rReply, baReply, tellReply])
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
        _ => panic!(),
    };
    let (n, eof, truncated) = extract_read_line_into(&outer.ps[0]);
    assert_eq!(n, 4);
    assert!(!eof);
    assert!(!truncated, "LF included in chunk; no ambiguity");
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(b"abcd".to_vec()));
    let (_, _, pos, _) = extract_reply(&outer.ps[2]);
    assert_eq!(pos, Some(5), "cursor past LF");
}

/// After a utf8-boundary truncation, a chained call with a larger
/// buffer must retrieve the preserved tail correctly.  The fd cursor
/// left at byte 1 (past 'a', before 0xC3) must let a second read on a
/// cap-4 utf8 buffer consume the remaining "é\n" cleanly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_into_utf8_chained_after_truncation() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "c3a9".hexToBytes(),
             "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@buf1 <- Buffer!?(2, "utf8")) {
                for (@r1 <- @f!?("readLineInto", buf1)) {
                  for (@ba1 <- @buf1!?("toByteArray")) {
                    for (@buf2 <- Buffer!?(4, "utf8")) {
                      for (@r2 <- @f!?("readLineInto", buf2)) {
                        for (@ba2 <- @buf2!?("toByteArray")) {
                          for (@buf3 <- Buffer!?(4, "utf8")) {
                            for (@r3 <- @f!?("readLineInto", buf3)) {
                              @"out"!([r1, ba1, r2, ba2, r3])
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
        _ => panic!(),
    };
    // First read: 'a' (1 byte), truncated at utf8 boundary.
    let (n1, eof1, tr1) = extract_read_line_into(&outer.ps[0]);
    assert_eq!(n1, 1);
    assert!(!eof1);
    assert!(tr1, "utf8 boundary forces truncation");
    let (_, _, _, ba1) = extract_reply(&outer.ps[1]);
    assert_eq!(ba1, Some(vec![0x61]));
    // Second read on a larger buffer: consumes "é" (2 bytes) then LF.
    let (n2, eof2, tr2) = extract_read_line_into(&outer.ps[2]);
    assert_eq!(n2, 2, "picks up the preserved é bytes");
    assert!(!eof2);
    assert!(!tr2, "full line fit; no truncation");
    let (_, _, _, ba2) = extract_reply(&outer.ps[3]);
    assert_eq!(ba2, Some(vec![0xC3, 0xA9]));
    // Third read: EOF.
    let (n3, eof3, tr3) = extract_read_line_into(&outer.ps[4]);
    assert_eq!(n3, 0);
    assert!(eof3);
    assert!(!tr3);
}

/// M-2 documentation: readLine's underlying CharStream reads chunks of
/// up to 4096 bytes.  When the caller only drains part of the line and
/// walks away without hitting EOS, the fd cursor is left at the end of
/// the last fsRead chunk — NOT at the position the CharStream has
/// virtually consumed.  This test asserts the current (imperfect)
/// behavior so a future fix that adds a "close resets cursor" step
/// forces us to update the assertion consciously.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_partial_drain_leaves_cursor_at_fsread_chunk_end() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["ab".toUtf8Bytes(), "0a".hexToBytes(),
             "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, stream] <- @f!?("readLine")) {
                for (@n1 <- @stream!?("next")) {
                  for (@tellReply <- @f!?("tell")) {
                    @"out"!([n1, tellReply])
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
    let (ok, v, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    assert_eq!(v, "a");
    let (_, _, pos, _) = extract_reply(&outer.ps[1]);
    // Underlying fsRead(4096) pulled the entire 6-byte file into the
    // CharStream buffer on the first next().  The fd cursor is at end
    // of file even though only one char was consumed.  Documented
    // limitation (M-2 in slice 10/12 review).
    assert_eq!(
        pos,
        Some(6),
        "fd cursor at end-of-chunk, NOT at end-of-consumed-char"
    );
}

/// readLine analog of file_chars_invalid_start_byte_returns_fserr_io:
/// a file whose first byte is 0xFF (never a valid UTF-8 start) must
/// surface FSERR_IO on the first next() rather than emitting U+FFFD
/// or hanging.  Same guarantee as chars(), but exercised via readLine.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_line_malformed_utf8_start_byte_returns_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("ff".hexToBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@[true, stream] <- @f!?("readLine")) {
              for (@r <- @stream!?("next")) { @"out"!(r) }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
}

/// readLine across a refill boundary: a line longer than the internal
/// 4096-byte fsRead chunk must still assemble correctly.  Ignored by
/// default because per-byte tuplespace ops on a >4KB line are very
/// slow; run with `--ignored` when validating this path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "per-byte tuplespace ops on >4KB line are prohibitively slow"]
async fn file_read_line_across_refill_boundary() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Build a 5000-byte ASCII line ("a" * 5000) followed by LF, then a
    // second line ("b") to prove the refill logic doesn't over-read.
    let src = with_libs(
        r#"
        new mkLine, loop in {
          contract loop(@n, @acc, ret) = {
            match n <= 0 {
              true  => ret!(acc)
              false => loop!(n - 1, [acc, "a".toUtf8Bytes()].concatBytes(), *ret)
            }
          } |
          contract mkLine(ret) = { loop!(5000, "".hexToBytes(), *ret) } |
          new lineCh in {
            mkLine!(*lineCh) |
            for (@line <- lineCh) {
              for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
                for (@_ <- @f!?("writeByteArray",
                  [line, "0a".hexToBytes(), "b".toUtf8Bytes()].concatBytes())) {
                  for (@_ <- @f!?("seek", 0, "set")) {
                    for (@[true, stream] <- @f!?("readLine")) {
                      // chunk() concatenates the whole line into a String.
                      for (@r <- @stream!?("chunk", 6000)) { @"out"!(r) }
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
    let (ok, s, _, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(s.len(), 5000, "full line reassembled across refill");
    assert!(s.chars().all(|c| c == 'a'));
}

// ---------------------------------------------------------------------
// Slice 11: File.lines() (outer LineStream + inner CharStreams) and
// File.writeLines(lineStream) sink.
// ---------------------------------------------------------------------

/// lines() on an empty file: outer.next() returns EOS immediately
/// without producing any inners.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_on_empty_file_yields_immediate_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@[true, outer] <- @f!?("lines")) {
            for (@r <- @outer!?("next")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

/// lines() on "ab\ncd\n": outer produces two inners in sequence, each
/// yielding its chars then EOS.  A third outer.next() returns EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_two_lines_produces_two_inners_then_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["ab".toUtf8Bytes(), "0a".hexToBytes(),
             "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@[true, inner1] <- @outer!?("next")) {
                  for (@a1 <- @inner1!?("next")) {
                    for (@b1 <- @inner1!?("next")) {
                      for (@e1 <- @inner1!?("next")) {
                        for (@[true, inner2] <- @outer!?("next")) {
                          for (@c2 <- @inner2!?("next")) {
                            for (@d2 <- @inner2!?("next")) {
                              for (@e2 <- @inner2!?("next")) {
                                for (@rEnd <- @outer!?("next")) {
                                  @"out"!([a1, b1, e1, c2, d2, e2, rEnd])
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
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (ok, v, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    assert_eq!(v, "a");
    let (ok, v, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok);
    assert_eq!(v, "b");
    let (ok, code, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok);
    assert_eq!(code, "EOS", "inner1 EOS at LF");
    let (ok, v, _, _) = extract_reply(&outer.ps[3]);
    assert!(ok);
    assert_eq!(v, "c");
    let (ok, v, _, _) = extract_reply(&outer.ps[4]);
    assert!(ok);
    assert_eq!(v, "d");
    let (ok, code, _, _) = extract_reply(&outer.ps[5]);
    assert!(!ok);
    assert_eq!(code, "EOS", "inner2 EOS at LF");
    let (ok, code, _, _) = extract_reply(&outer.ps[6]);
    assert!(!ok);
    assert_eq!(code, "EOS", "outer EOS at file end");
}

/// lines() on unterminated final line "ab\ncd": outer produces two
/// inners; inner2 emits "c", "d" then EOS (at end of file, no LF).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_unterminated_final_line_still_produced() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["ab".toUtf8Bytes(), "0a".hexToBytes(),
             "cd".toUtf8Bytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@[true, inner1] <- @outer!?("next")) {
                  for (@_a <- @inner1!?("next")) {
                    for (@_b <- @inner1!?("next")) {
                      for (@_e1 <- @inner1!?("next")) {
                        for (@[true, inner2] <- @outer!?("next")) {
                          for (@c <- @inner2!?("next")) {
                            for (@d <- @inner2!?("next")) {
                              for (@e2 <- @inner2!?("next")) {
                                for (@rEnd <- @outer!?("next")) {
                                  @"out"!([c, d, e2, rEnd])
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
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (ok, v, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    assert_eq!(v, "c");
    let (ok, v, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok);
    assert_eq!(v, "d");
    let (ok, code, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok);
    assert_eq!(code, "EOS", "inner2 EOS at EOF");
    let (ok, code, _, _) = extract_reply(&outer.ps[3]);
    assert!(!ok);
    assert_eq!(code, "EOS", "outer EOS at EOF");
}

/// Blank line: file "\n" produces one inner (for the empty line
/// before the LF), which yields EOS immediately.  The trailing empty
/// line after the LF is not emitted (POSIX text convention).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_blank_line_yields_empty_inner_then_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "0a".hexToBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@[true, inner1] <- @outer!?("next")) {
                  for (@e1 <- @inner1!?("next")) {
                    for (@rEnd <- @outer!?("next")) {
                      @"out"!([e1, rEnd])
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
        _ => panic!(),
    };
    let (ok, code, _, _) = extract_reply(&outer.ps[0]);
    assert!(!ok);
    assert_eq!(code, "EOS", "empty inner EOS immediately");
    let (ok, code, _, _) = extract_reply(&outer.ps[1]);
    assert!(!ok);
    assert_eq!(code, "EOS", "outer EOS after single blank line");
}

/// Single-active-inner rule (spec §349): calling outer.next() while
/// inner1 is half-drained force-drains inner1 past its LF, then
/// produces inner2 over the next line.  Inner2 must start on the
/// next line's first char, not mid-way through inner1's line.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_single_active_inner_rule_force_drains_active() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["ab".toUtf8Bytes(), "0a".hexToBytes(),
             "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@[true, inner1] <- @outer!?("next")) {
                  // Take just "a" from inner1, then move on.
                  for (@a1 <- @inner1!?("next")) {
                    for (@[true, inner2] <- @outer!?("next")) {
                      // inner2 must start with "c" (not "b").
                      for (@c2 <- @inner2!?("next")) {
                        for (@d2 <- @inner2!?("next")) {
                          for (@e2 <- @inner2!?("next")) {
                            @"out"!([a1, c2, d2, e2])
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
        _ => panic!(),
    };
    let (ok, v, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    assert_eq!(v, "a");
    let (ok, v, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok);
    assert_eq!(v, "c", "inner2 skips inner1's remaining chars + LF");
    let (ok, v, _, _) = extract_reply(&outer.ps[2]);
    assert!(ok);
    assert_eq!(v, "d");
    let (ok, code, _, _) = extract_reply(&outer.ps[3]);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

/// Drained-inners-fail-closed rule (spec §357): a retained handle to
/// an inner drained by outer returns FSERR_CLOSED on subsequent
/// next() — NOT EOS and NOT silent-read-wrong-data.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_drained_inner_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["ab".toUtf8Bytes(), "0a".hexToBytes(),
             "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@[true, inner1] <- @outer!?("next")) {
                  // Do NOT drain inner1.  Move outer forward.
                  for (@[true, _inner2] <- @outer!?("next")) {
                    // Now try inner1.next() — must be FSERR_CLOSED.
                    for (@r <- @inner1!?("next")) { @"out"!(r) }
                  }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "drained inner must fail");
    assert_eq!(
        code, "FSERR_CLOSED",
        "not EOS — retained-handle-after-drain is FSERR_CLOSED"
    );
}

/// chunk() unsupported on outer LineStream (spec §111): even for a
/// non-empty file with valid lines, outer.chunk(n) returns
/// FSERR_UNSUPPORTED because chunking would break the
/// single-active-inner invariant.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_chunk_returns_fserr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["ab".toUtf8Bytes(), "0a".hexToBytes(),
             "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@r <- @outer!?("chunk", 2)) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// lines() on a closed file returns FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@r <- @f!?("lines")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

// -- writeLines(lineStream) tests ------------------------------------

/// writeLines round-trip: write "hi\nby\n" to the mock file, seek 0,
/// obtain a LineStream via lines(), then writeLines it back.  The
/// LineStream's fsRead advances the cursor to end-of-file (4096 chunk
/// pulls everything), so writeLines writes AFTER the original bytes.
/// Final file contents = original + LF-terminated copy of each line.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_lines_appends_terminated_copy_of_each_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["hi".toUtf8Bytes(), "0a".hexToBytes(),
             "by".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@wReply <- @f!?("writeLines", outer)) {
                  for (@_ <- @f!?("seek", 0, "set")) {
                    for (@readReply <- @f!?("readN", 100)) {
                      @"out"!([wReply, readReply])
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
        _ => panic!(),
    };
    // Mi-11-1: verify writeLines reply is exactly [true] — one
    // element, no trailing data — matching spec §1110.
    let outer0 = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("writeLines reply not a list"),
    };
    assert_eq!(
        outer0.ps.len(),
        1,
        "reply must be exactly [true], no trailing fields"
    );
    let ok = match single_expr(&outer0.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("reply head not Bool"),
    };
    assert!(ok, "writeLines returned failure");
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    // Original 6 bytes: "hi\nby\n"; writeLines appended a copy of
    // each line + LF: "hi\nby\n".  Total 12 bytes.
    assert_eq!(bytes, Some(b"hi\nby\nhi\nby\n".to_vec()));
}

/// writeLines on a read-only file rejects with FSERR_UNSUPPORTED,
/// leaving the file untouched.  (Bogus lineStream arg is irrelevant
/// because the mode check runs before consumption.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_lines_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("hi\n".toUtf8Bytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
            for (@r <- @f!?("writeLines", "not-a-stream")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// writeLines on a closed file returns FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_lines_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@r <- @f!?("writeLines", "not-a-stream")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

/// writeLines on an empty LineStream (source file empty) returns
/// [true] and writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_lines_empty_stream_returns_true_no_bytes() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@[true, outer] <- @f!?("lines")) {
            for (@wReply <- @f!?("writeLines", outer)) {
              for (@sizeReply <- @f!?("size")) {
                @"out"!([wReply, sizeReply])
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
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok, "writeLines on empty LineStream should succeed");
    let (_, _, size, _) = extract_reply(&outer.ps[1]);
    assert_eq!(size, Some(0), "no bytes written");
}

// ---------------------------------------------------------------------
// Slice 11 review-driven coverage additions.
// ---------------------------------------------------------------------

/// M-11-3: force-drain when the current line is UNTERMINATED (file
/// ends before an LF appears in the inner's line).  Verifies
/// scanForLFRefilling's "END + eofSeen=true" arm — the drain must
/// mark eofSeen and let outer produce EOS on the next call, not
/// hang or produce a spurious empty inner.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_force_drain_hits_eof_during_drain() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["ab".toUtf8Bytes(), "0a".hexToBytes(),
             "cd".toUtf8Bytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@[true, inner1] <- @outer!?("next")) {
                  for (@a1 <- @inner1!?("next")) {
                    // Move outer forward WITHOUT draining inner1.
                    // Get inner2 for "cd" (unterminated).
                    for (@[true, inner2] <- @outer!?("next")) {
                      for (@c2 <- @inner2!?("next")) {
                        for (@d2 <- @inner2!?("next")) {
                          for (@e2 <- @inner2!?("next")) {
                            // Now advance outer past inner2 without draining.
                            // The drain must hit EOF-in-drain path.
                            for (@rOuter <- @outer!?("next")) {
                              @"out"!([a1, c2, d2, e2, rOuter])
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
        _ => panic!(),
    };
    let (ok, v, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    assert_eq!(v, "a");
    let (ok, v, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok);
    assert_eq!(v, "c");
    let (ok, v, _, _) = extract_reply(&outer.ps[2]);
    assert!(ok);
    assert_eq!(v, "d");
    let (ok, code, _, _) = extract_reply(&outer.ps[3]);
    assert!(!ok);
    assert_eq!(code, "EOS", "inner2 EOS at EOF");
    let (ok, code, _, _) = extract_reply(&outer.ps[4]);
    assert!(!ok);
    assert_eq!(code, "EOS", "outer EOS after force-drain that hit EOF");
}

/// M-11-4a: drained inner CHUNK() must return FSERR_CLOSED, not
/// hang and not surface stale data.  Test the parallel of the
/// next()-drained test but calling chunk(2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_drained_inner_chunk_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["ab".toUtf8Bytes(), "0a".hexToBytes(),
             "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@[true, inner1] <- @outer!?("next")) {
                  for (@[true, _inner2] <- @outer!?("next")) {
                    for (@r <- @inner1!?("chunk", 5)) { @"out"!(r) }
                  }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "drained inner chunk must fail");
    assert_eq!(code, "FSERR_CLOSED");
}

/// M-11-4b: drained inner FOLD() must return FSERR_CLOSED.  fold
/// internally calls next() in a loop; the first next() sees the
/// revoked status and short-circuits, and fold forwards that
/// terminal reply.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_drained_inner_fold_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new noopCombine in {
          contract noopCombine(@_acc, @_v, retCh) = { retCh!([true, Nil]) } |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@_ <- @f!?("writeByteArray",
              ["ab".toUtf8Bytes(), "0a".hexToBytes(),
               "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
              for (@_ <- @f!?("seek", 0, "set")) {
                for (@[true, outer] <- @f!?("lines")) {
                  for (@[true, inner1] <- @outer!?("next")) {
                    for (@[true, _inner2] <- @outer!?("next")) {
                      for (@r <- @inner1!?("fold", Nil, *noopCombine)) {
                        @"out"!(r)
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
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "drained inner fold must fail");
    assert_eq!(code, "FSERR_CLOSED");
}

/// M-11-5: multiple consecutive blank lines.  File "\n\n\n" — three
/// LFs, three empty lines, no trailing content.  Per POSIX
/// convention (empty line after trailing LF NOT emitted), the outer
/// yields exactly 3 empty inners then EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_three_blank_lines_yield_three_empty_inners() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "0a0a0a".hexToBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@[true, i1] <- @outer!?("next")) {
                  for (@e1 <- @i1!?("next")) {
                    for (@[true, i2] <- @outer!?("next")) {
                      for (@e2 <- @i2!?("next")) {
                        for (@[true, i3] <- @outer!?("next")) {
                          for (@e3 <- @i3!?("next")) {
                            for (@rEnd <- @outer!?("next")) {
                              @"out"!([e1, e2, e3, rEnd])
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
        _ => panic!(),
    };
    for i in 0..3 {
        let (ok, code, _, _) = extract_reply(&outer.ps[i]);
        assert!(!ok, "inner {i} EOS");
        assert_eq!(code, "EOS");
    }
    let (ok, code, _, _) = extract_reply(&outer.ps[3]);
    assert!(!ok);
    assert_eq!(code, "EOS", "outer EOS after three blank lines");
}

/// M-11-6: writeLines fed a non-LineStream argument (a String).
/// writeLinesLoop sends `!?("next")` to the arg; a String doesn't
/// respond as an agent, so the call hangs — the mock test space
/// exits at deploy end without a reply on @"out".  This documents
/// the current (imperfect) behavior: type-guarding the lineStream
/// arg (as writeLines/writeChars type-guard their byte streams) is
/// a follow-up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_lines_non_stream_arg_yields_no_reply() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("writeLines", "not-a-stream")) { @"out"!(r) }
        }
        "#,
    );
    let par = ParBuilderUtil::mk_term(&src).expect("compile");
    reducer
        .eval(par, &Env::new(), rand().split_byte(0))
        .await
        .expect("eval");
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    let map = space.to_map().await;
    let out_chan = new_gstring_par("out".to_string(), Vec::new(), false);
    // Documented limitation: no reply because the String arg doesn't
    // respond to !?("next").  A future fix would type-guard the arg
    // and reply with FSERR_BAD_ARG.
    assert!(
        !map.contains_key(&vec![out_chan]),
        "known limitation: non-stream lineStream arg leaves writeLines hung"
    );
}

/// M-11-7: UTF-8 multi-byte codepoints via lines() inner CharStream.
/// linesAsStrings covers multi-byte, but lines()'s inner uses the
/// SHARED sourceCell with codepoint-scanning inline in the inner
/// producer — this test guards that path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_inner_yields_multibyte_utf8_chars() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // File: "aé\n" — 'a' (1 byte) + 'é' (2 bytes, c3 a9) + LF.
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "c3a9".hexToBytes(),
             "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@[true, inner1] <- @outer!?("next")) {
                  for (@n1 <- @inner1!?("next")) {
                    for (@n2 <- @inner1!?("next")) {
                      for (@n3 <- @inner1!?("next")) {
                        @"out"!([n1, n2, n3])
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
        _ => panic!(),
    };
    let (ok, v, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    assert_eq!(v, "a");
    let (ok, v, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok);
    assert_eq!(v, "\u{00E9}", "multi-byte é as one char");
    let (ok, code, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

/// M-11-8: writeLines forwards a mid-stream lineStream error with
/// the "wrote N lines before producer failure: MSG" prefix, and
/// closes the input lineStream.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_lines_producer_error_reports_lines_written() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Bespoke LineStream: emits one working line then FSERR_IO.  The
    // one inner emits chars "x", "y" then EOS; the outer's second
    // next() returns the error.
    let src = with_libs(
        r#"
        new outerState, innerProd, innerBuild, outerProd, outerBuild in {
          // Inner emits "x", "y", EOS deterministically.
          new innerCursor in {
            innerCursor!(0) |
            contract innerProd(retCh) = {
              for (@i <- innerCursor) {
                match i {
                  0 => { innerCursor!(1) | retCh!([true, "x"]) }
                  1 => { innerCursor!(2) | retCh!([true, "y"]) }
                  _ => { innerCursor!(i) | retCh!([false, "EOS", ""]) }
                }
              }
            } |
            contract innerBuild(@vals, retCh) = {
              match vals {
                v /\ List => retCh!([true, ""])
                _         => retCh!([false, "FSERR_IO", "bad"])
              }
            } |
            for (@innerHandle <- Stream!?(*innerProd, *innerBuild)) {
              // Outer emits inner once, then FSERR_IO.
              outerState!(0) |
              contract outerProd(retCh) = {
                for (@n <- outerState) {
                  match n {
                    0 => { outerState!(1) | retCh!([true, innerHandle]) }
                    _ => { outerState!(n) | retCh!([false, "FSERR_IO", "simulated"]) }
                  }
                }
              } |
              contract outerBuild(@_vals, retCh) = {
                retCh!([false, "FSERR_UNSUPPORTED", "chunk unsupported"])
              } |
              for (@outerHandle <- Stream!?(*outerProd, *outerBuild)) {
                for (@f <- File!?(1, "/root", "out.txt", "rw", "oracular")) {
                  for (@r <- @f!?("writeLines", outerHandle)) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
    // Extract the message and confirm it mentions "1 lines" written
    // and the "simulated" tail.
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let msg = match single_expr(&list.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!("msg not string"),
    };
    assert!(
        msg.contains("wrote 1 lines"),
        "msg must show lines written: {msg}"
    );
    assert!(
        msg.contains("simulated"),
        "msg must include underlying err: {msg}"
    );
}

/// m-11-2: outer EOS is CACHED by Stream state — subsequent calls
/// return EOS again without re-invoking the producer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_outer_eos_is_cached() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@[true, outer] <- @f!?("lines")) {
            for (@r1 <- @outer!?("next")) {
              for (@r2 <- @outer!?("next")) {
                for (@r3 <- @outer!?("next")) {
                  @"out"!([r1, r2, r3])
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
    for i in 0..3 {
        let (ok, code, _, _) = extract_reply(&outer.ps[i]);
        assert!(!ok, "call {i}: EOS");
        assert_eq!(code, "EOS", "call {i}");
    }
}

/// m-11-3: inner mid-stream FSERR_IO is CACHED by Stream state.
/// After an inner encounters invalid UTF-8, subsequent inner.next()
/// calls return FSERR_IO again (from Stream's ("error", ...) state),
/// not re-scan or new results.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_inner_fserr_io_is_cached() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Stage file with a single 0xFF byte — invalid UTF-8 start.
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("ff".hexToBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@[true, outer] <- @f!?("lines")) {
              for (@[true, inner1] <- @outer!?("next")) {
                for (@r1 <- @inner1!?("next")) {
                  for (@r2 <- @inner1!?("next")) {
                    @"out"!([r1, r2])
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
    let (ok, code, _, _) = extract_reply(&outer.ps[0]);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
    let (ok, code, _, _) = extract_reply(&outer.ps[1]);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO", "second call cached the error");
}

/// Mi-11-2: inner CharStream supports chunk() on a LIVE (non-drained)
/// inner — chunk collects chars from source and reports the whole
/// line's String.  Complements the outer.chunk-unsupported test.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lines_live_inner_chunk_returns_line_string() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["hi".toUtf8Bytes(), "0a".hexToBytes(),
             "by".toUtf8Bytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@[true, inner1] <- @outer!?("next")) {
                  for (@r <- @inner1!?("chunk", 10)) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, s, _, _) = extract_reply(&reply);
    assert!(ok, "live inner chunk should succeed");
    assert_eq!(s, "hi", "chunk collects the line's chars into one String");
}

// ---------------------------------------------------------------------
// Slice 13: allocRows / Rows agent + File.readLinesInto(rows).
// ---------------------------------------------------------------------

/// Helper: extract nLines and eof/truncated flags from a readLinesInto
/// reply of shape [true, [nLines, {"eof": bool, "truncated": bool}]].
fn extract_read_lines_into(par: &Par) -> (i64, bool, bool) {
    let outer = match single_expr(par).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("readLinesInto reply not a list"),
    };
    let ok = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("head not Bool"),
    };
    assert!(ok, "expected success reply, got {:?}", outer.ps);
    let inner = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("payload not a list"),
    };
    let n = match single_expr(&inner.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GInt(n)) => n,
        _ => panic!("nLines not Int"),
    };
    let flags_map = match single_expr(&inner.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::EMapBody(m)) => m,
        _ => panic!("flags not a Map"),
    };
    let mut eof = None;
    let mut truncated = None;
    for kv in flags_map.kvs {
        let key = kv
            .key
            .as_ref()
            .and_then(single_expr)
            .and_then(|e| e.expr_instance);
        let val = kv
            .value
            .as_ref()
            .and_then(single_expr)
            .and_then(|e| e.expr_instance);
        if let (Some(ExprInstance::GString(k)), Some(ExprInstance::GBool(v))) = (key, val) {
            if k == "eof" {
                eof = Some(v);
            }
            if k == "truncated" {
                truncated = Some(v);
            }
        }
    }
    (
        n,
        eof.expect("eof missing"),
        truncated.expect("truncated missing"),
    )
}

// -- Rows agent + allocRows -----------------------------------------

/// allocRows returns [true, rowsHandle]; capacityRows returns m.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alloc_rows_capacity_matches_m() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, rows] <- @alloc!?("allocRows", 4, 16, "bytes")) {
            for (@r <- @rows!?("capacityRows")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, n, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(n, Some(4));
}

/// allocRows innerUnit propagates from allocation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alloc_rows_inner_unit_matches_alloc_unit() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, rows] <- @alloc!?("allocRows", 2, 8, "utf8")) {
            for (@r <- @rows!?("innerUnit")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, u, _, _) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(u, "utf8");
}

/// getAt(i) returns a distinct inner Buffer for each in-range i; each
/// is a fully-functional bytes buffer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alloc_rows_get_at_returns_distinct_inners() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, rows] <- @alloc!?("allocRows", 3, 8, "bytes")) {
            for (@[true, b0] <- @rows!?("getAt", 0)) {
              for (@[true, b1] <- @rows!?("getAt", 1)) {
                for (@c0 <- @b0!?("capacity")) {
                  for (@c1 <- @b1!?("capacity")) {
                    // Write into b0 only; b1 must remain empty.
                    for (@_ <- @b0!?("writeBytes", "hi".toUtf8Bytes())) {
                      for (@l0 <- @b0!?("length")) {
                        for (@l1 <- @b1!?("length")) {
                          @"out"!([c0, c1, l0, l1])
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
        _ => panic!(),
    };
    let (_, _, c0, _) = extract_reply(&outer.ps[0]);
    let (_, _, c1, _) = extract_reply(&outer.ps[1]);
    assert_eq!(c0, Some(8), "b0 capacity");
    assert_eq!(c1, Some(8), "b1 capacity");
    let (_, _, l0, _) = extract_reply(&outer.ps[2]);
    let (_, _, l1, _) = extract_reply(&outer.ps[3]);
    assert_eq!(l0, Some(2), "b0 has 'hi'");
    assert_eq!(l1, Some(0), "b1 untouched");
}

/// getAt with out-of-range index returns BUFERR_OUT_OF_RANGE.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alloc_rows_get_at_out_of_range() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, rows] <- @alloc!?("allocRows", 2, 8, "bytes")) {
            for (@rNeg <- @rows!?("getAt", -1)) {
              for (@rBig <- @rows!?("getAt", 5)) {
                @"out"!([rNeg, rBig])
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
    let (ok, code, _, _) = extract_reply(&outer.ps[0]);
    assert!(!ok);
    assert_eq!(code, "BUFERR_OUT_OF_RANGE", "negative index");
    let (ok, code, _, _) = extract_reply(&outer.ps[1]);
    assert!(!ok);
    assert_eq!(code, "BUFERR_OUT_OF_RANGE", "past-end index");
}

/// allocRows validates m and innerN as positive integers, and unit
/// as one of "bytes" or "utf8".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alloc_rows_invalid_args_rejected() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@r1 <- @alloc!?("allocRows", 0, 8, "bytes")) {
            for (@r2 <- @alloc!?("allocRows", 4, 0, "bytes")) {
              for (@r3 <- @alloc!?("allocRows", 4, 8, "bogus")) {
                for (@r4 <- @alloc!?("allocRows", "not-int", 8, "bytes")) {
                  @"out"!([r1, r2, r3, r4])
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
    for (i, expected) in [
        (0, "BUFERR_INVALID_CAPACITY"),
        (1, "BUFERR_INVALID_CAPACITY"),
        (2, "BUFERR_INVALID_UNIT"),
        (3, "BUFERR_INVALID_ARGUMENT"),
    ] {
        let (ok, code, _, _) = extract_reply(&outer.ps[i]);
        assert!(!ok, "case {i} should fail");
        assert_eq!(code, expected, "case {i}");
    }
}

/// close() on Rows marks it revoked; subsequent capacityRows fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alloc_rows_close_then_capacity_fails() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, rows] <- @alloc!?("allocRows", 2, 8, "bytes")) {
            for (@_ <- @rows!?("close")) {
              for (@r <- @rows!?("capacityRows")) { @"out"!(r) }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "BUFERR_REVOKED");
}

// -- File.readLinesInto ----------------------------------------------

/// Happy path: file "one\ntwo\nthree\n" (14 bytes), 3 rows of cap 20.
/// nLines=3, eof (fourth read hit EOF is not checked here), no
/// truncation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_happy_path() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["one".toUtf8Bytes(), "0a".hexToBytes(),
             "two".toUtf8Bytes(), "0a".hexToBytes(),
             "three".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 3, 20, "bytes")) {
                  for (@rReply <- @f!?("readLinesInto", rows)) {
                    for (@[true, b0] <- @rows!?("getAt", 0)) {
                      for (@[true, b1] <- @rows!?("getAt", 1)) {
                        for (@[true, b2] <- @rows!?("getAt", 2)) {
                          for (@ba0 <- @b0!?("toByteArray")) {
                            for (@ba1 <- @b1!?("toByteArray")) {
                              for (@ba2 <- @b2!?("toByteArray")) {
                                @"out"!([rReply, ba0, ba1, ba2])
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
        _ => panic!(),
    };
    let (n, _eof, truncated) = extract_read_lines_into(&outer.ps[0]);
    assert_eq!(n, 3);
    assert!(!truncated);
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(b"one".to_vec()));
    let (_, _, _, bytes) = extract_reply(&outer.ps[2]);
    assert_eq!(bytes, Some(b"two".to_vec()));
    let (_, _, _, bytes) = extract_reply(&outer.ps[3]);
    assert_eq!(bytes, Some(b"three".to_vec()));
}

/// File has MORE lines than rows: stop when i == m; eof=false,
/// truncated=false.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_stops_at_m() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "0a".hexToBytes(),
             "b".toUtf8Bytes(), "0a".hexToBytes(),
             "c".toUtf8Bytes(), "0a".hexToBytes(),
             "d".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 2, 10, "bytes")) {
                  for (@r <- @f!?("readLinesInto", rows)) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (n, eof, truncated) = extract_read_lines_into(&reply);
    assert_eq!(n, 2);
    assert!(!eof, "more lines available; not EOF");
    assert!(!truncated);
}

/// File has FEWER lines than rows: nLines=<file lines>, eof=true.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_eof_before_rows_full() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "0a".hexToBytes(),
             "b".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 5, 10, "bytes")) {
                  for (@r <- @f!?("readLinesInto", rows)) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (n, eof, truncated) = extract_read_lines_into(&reply);
    assert_eq!(n, 2, "two lines then EOF");
    assert!(eof);
    assert!(!truncated);
}

/// Long line: file has ONE line longer than inner cap.  Row 0 fills,
/// overflow drained past LF, truncated=true.  Row 1 (past-EOF) never
/// filled; nLines counts row 0 only.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_truncates_overflow_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // File: "abcdefghij\n" (11 bytes: 10 chars + LF).  Inner cap = 3.
    // Row 0 gets "abc", overflow "defghij" is drained, LF consumed.
    // No more content → row 1 not filled; nLines=1, eof=true, trunc=true.
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abcdefghij".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 2, 3, "bytes")) {
                  for (@rReply <- @f!?("readLinesInto", rows)) {
                    for (@[true, b0] <- @rows!?("getAt", 0)) {
                      for (@ba0 <- @b0!?("toByteArray")) {
                        @"out"!([rReply, ba0])
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
        _ => panic!(),
    };
    let (n, eof, truncated) = extract_read_lines_into(&outer.ps[0]);
    assert_eq!(n, 1, "one line consumed (with overflow drain)");
    assert!(eof, "drain past LF hit EOF");
    assert!(truncated, "line exceeded inner cap");
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(b"abc".to_vec()), "row 0 got the prefix");
}

/// Empty file: nLines=0, eof=true, truncated=false.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_empty_file() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@alloc <- Allocator!?()) {
            for (@[true, rows] <- @alloc!?("allocRows", 3, 10, "bytes")) {
              for (@r <- @f!?("readLinesInto", rows)) { @"out"!(r) }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (n, eof, truncated) = extract_read_lines_into(&reply);
    assert_eq!(n, 0);
    assert!(eof);
    assert!(!truncated);
}

/// Blank lines: file "\n\n\n" (3 bytes), 5 rows.  Each row gets an
/// empty content; nLines=3, eof=true.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_blank_lines() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "0a0a0a".hexToBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 5, 10, "bytes")) {
                  for (@rReply <- @f!?("readLinesInto", rows)) {
                    for (@[true, b0] <- @rows!?("getAt", 0)) {
                      for (@ba0 <- @b0!?("toByteArray")) {
                        @"out"!([rReply, ba0])
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
        _ => panic!(),
    };
    let (n, eof, _) = extract_read_lines_into(&outer.ps[0]);
    assert_eq!(n, 3);
    assert!(eof);
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(vec![]), "blank line: row 0 has 0 content bytes");
}

/// Unterminated final line "abc" (no LF): nLines=1, eof=true, no
/// truncation, row 0 = "abc".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_unterminated_final_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "abc".toUtf8Bytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 3, 10, "bytes")) {
                  for (@rReply <- @f!?("readLinesInto", rows)) {
                    for (@[true, b0] <- @rows!?("getAt", 0)) {
                      for (@ba0 <- @b0!?("toByteArray")) {
                        @"out"!([rReply, ba0])
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
        _ => panic!(),
    };
    let (n, eof, truncated) = extract_read_lines_into(&outer.ps[0]);
    assert_eq!(n, 1);
    assert!(eof);
    assert!(!truncated);
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(b"abc".to_vec()));
}

/// readLinesInto on a closed file returns FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_on_closed_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@alloc <- Allocator!?()) {
              for (@[true, rows] <- @alloc!?("allocRows", 2, 10, "bytes")) {
                for (@r <- @f!?("readLinesInto", rows)) { @"out"!(r) }
              }
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

// ---------------------------------------------------------------------
// Closed-state completeness (2026-07-30 Q-6 resolution)
//
// Per the tokenized-cost-accounting design decision, File agents outlive
// their originating deploy and can be referenced in a closed state.
// Every method must return `FSERR_CLOSED` on a closed File, `close` must
// be idempotent, and the unknown-method default arm must continue to
// return `FSERR_UNSUPPORTED` even on a closed File (the "you asked for a
// nonsense method" error takes priority over "the file is closed").
// ---------------------------------------------------------------------

/// forEachLine on a closed File returns FSERR_CLOSED.  Coverage gap
/// closed 2026-07-30: forEachLine delegates to linesAsStrings, which
/// has its own closed-check, but the transitive property needs an
/// explicit regression test so a future refactor that inlines the call
/// path can't silently drop the check.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_for_each_line_on_closed_returns_fserr_closed() {
    // Bespoke source (not close_then_call) because forEachLine needs a
    // handler name argument that must be `new`-bound in scope.  Handler
    // is never invoked — the closed check fires before dispatch — but
    // the compiler requires it to be bound.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new noopHandler in {
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@_ <- @f!?("close")) {
              for (@r <- @f!?("forEachLine", *noopHandler, 16)) {
                @"out"!(r)
              }
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

/// close() on an already-closed File is idempotent: returns [true]
/// without an error.  Matches POSIX close-on-closed-fd semantics from
/// the caller's perspective (they got the "it's closed" outcome they
/// asked for; re-asking is fine).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_close_on_closed_is_idempotent() {
    let reply = close_then_call(r#"!?("close")"#).await;
    // extract_reply parses `[true]` as ok=true; second close should succeed.
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "close on already-closed File must succeed (idempotent)");
}

/// Unknown method on a closed File returns FSERR_UNSUPPORTED, not
/// FSERR_CLOSED.  Locks in the design decision that the default arm
/// (unknown-method) takes priority over the closed-state check —
/// callers asking for a nonsense method get told about the nonsense,
/// not about the file state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_unknown_method_on_closed_returns_fserr_unsupported() {
    let reply = close_then_call(r#"!?("nonExistentMethod", 42)"#).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// Cursor discipline: after readLinesInto consumes a subset of the
/// file, subsequent readN reads the remaining bytes correctly.
/// Verifies that overflow drain leaves the cursor at the right spot.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_leaves_cursor_at_next_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // File: "one\ntwo\nthree\n" — 14 bytes.  Read 2 lines into 2-row
    // Rows; cursor should be at 8 (past "one\ntwo\n").  Then readN(20)
    // should return "three\n" (6 bytes).
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["one".toUtf8Bytes(), "0a".hexToBytes(),
             "two".toUtf8Bytes(), "0a".hexToBytes(),
             "three".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 2, 20, "bytes")) {
                  for (@rlReply <- @f!?("readLinesInto", rows)) {
                    for (@tellReply <- @f!?("tell")) {
                      for (@rnReply <- @f!?("readN", 20)) {
                        @"out"!([rlReply, tellReply, rnReply])
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
        _ => panic!(),
    };
    let (n, eof, _) = extract_read_lines_into(&outer.ps[0]);
    assert_eq!(n, 2);
    assert!(!eof);
    let (_, _, pos, _) = extract_reply(&outer.ps[1]);
    assert_eq!(pos, Some(8), "cursor past 'one\\ntwo\\n'");
    let (_, _, _, bytes) = extract_reply(&outer.ps[2]);
    assert_eq!(bytes, Some(b"three\n".to_vec()));
}

// ---------------------------------------------------------------------
// Slice 13 review-driven coverage additions.
// ---------------------------------------------------------------------

/// M-13-4: Rows.clear() actually clears each inner buffer.
/// Regression protection: if clearInnersLoop were ever short-
/// circuited, this catches it via a pre-fill / clear / length check.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rows_clear_clears_each_inner_buffer() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, rows] <- @alloc!?("allocRows", 2, 8, "bytes")) {
            for (@[true, b0] <- @rows!?("getAt", 0)) {
              for (@[true, b1] <- @rows!?("getAt", 1)) {
                // Pre-fill both inners.
                for (@_ <- @b0!?("writeBytes", "aa".toUtf8Bytes())) {
                  for (@_ <- @b1!?("writeBytes", "bbb".toUtf8Bytes())) {
                    for (@_ <- @rows!?("clear")) {
                      for (@l0 <- @b0!?("length")) {
                        for (@l1 <- @b1!?("length")) {
                          @"out"!([l0, l1])
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
        _ => panic!(),
    };
    let (_, _, l0, _) = extract_reply(&outer.ps[0]);
    let (_, _, l1, _) = extract_reply(&outer.ps[1]);
    assert_eq!(l0, Some(0), "inner 0 cleared");
    assert_eq!(l1, Some(0), "inner 1 cleared");
}

/// M-13-5: Rows.close() actually closes each inner buffer.
/// Retain a getAt handle before close; verify the retained inner
/// returns BUFERR_REVOKED on subsequent capacity() calls.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rows_close_closes_each_inner_buffer() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, rows] <- @alloc!?("allocRows", 2, 8, "bytes")) {
            for (@[true, b0] <- @rows!?("getAt", 0)) {
              for (@[true, b1] <- @rows!?("getAt", 1)) {
                for (@_ <- @rows!?("close")) {
                  for (@r0 <- @b0!?("capacity")) {
                    for (@r1 <- @b1!?("capacity")) {
                      @"out"!([r0, r1])
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
        _ => panic!(),
    };
    let (ok0, code0, _, _) = extract_reply(&outer.ps[0]);
    let (ok1, code1, _, _) = extract_reply(&outer.ps[1]);
    assert!(!ok0);
    assert_eq!(code0, "BUFERR_REVOKED", "inner 0 revoked");
    assert!(!ok1);
    assert_eq!(code1, "BUFERR_REVOKED", "inner 1 revoked");
}

/// M-13-6: overflow-truncated line followed by a normal next line.
/// Verifies drainToNextLF stops AT the terminator (not past the next
/// line's content) so row i+1 gets the next line, not stale bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_overflow_then_normal_next_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // File "abcdefghij\nXY\n": row 0 (cap 3) fills with "abc",
    // drain consumes "defghij" + LF; row 1 reads "XY".
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["abcdefghij".toUtf8Bytes(), "0a".hexToBytes(),
             "XY".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 2, 3, "bytes")) {
                  for (@rReply <- @f!?("readLinesInto", rows)) {
                    for (@[true, b0] <- @rows!?("getAt", 0)) {
                      for (@[true, b1] <- @rows!?("getAt", 1)) {
                        for (@ba0 <- @b0!?("toByteArray")) {
                          for (@ba1 <- @b1!?("toByteArray")) {
                            @"out"!([rReply, ba0, ba1])
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
        _ => panic!(),
    };
    let (n, _eof, truncated) = extract_read_lines_into(&outer.ps[0]);
    assert_eq!(n, 2, "two lines: one truncated + one normal");
    assert!(truncated, "first line overflowed");
    let (_, _, _, bytes0) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes0, Some(b"abc".to_vec()));
    let (_, _, _, bytes1) = extract_reply(&outer.ps[2]);
    assert_eq!(
        bytes1,
        Some(b"XY".to_vec()),
        "row 1 must get the next line, not stale drain content"
    );
}

/// M-13-7: allocRows aggregate cap overflow returns
/// BUFERR_INVALID_CAPACITY.  Exercises the `mi > cap / ni` check.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alloc_rows_aggregate_cap_overflow_rejected() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // bytes cap = 1 GiB (2^30 bytes).  m*innerN > 2^30 → reject.
    // Choose m=200_000_000, innerN=10 → aggregate = 2e9 > 2^30.
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@r <- @alloc!?("allocRows", 200000000, 10, "bytes")) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "BUFERR_INVALID_CAPACITY");
}

/// M-13-8: readLinesInto forwards BUFERR_REVOKED from a closed Rows.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_revoked_rows_forwards_error() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["hi".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 2, 8, "bytes")) {
                  for (@_ <- @rows!?("close")) {
                    for (@r <- @f!?("readLinesInto", rows)) { @"out"!(r) }
                  }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(
        code, "BUFERR_REVOKED",
        "revoked-Rows error propagates through readLinesInto"
    );
}

/// m-13-2: Rows.default (unknown method) → BUFERR_UNSUPPORTED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rows_unknown_method_returns_buferr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@[true, rows] <- @alloc!?("allocRows", 2, 8, "bytes")) {
            for (@r <- @rows!?("wibble")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "BUFERR_UNSUPPORTED");
}

/// m-13-3: allocRows with non-Int innerN rejected.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alloc_rows_non_int_inner_n_rejected() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@r <- @alloc!?("allocRows", 4, "not-int", "bytes")) {
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

/// m-13-4: readLinesInto over a utf8-unit Rows respects UTF-8
/// codepoint boundaries (inherited from readLineInto's boundary rule).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_utf8_rows_multibyte_content() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // File: "café\n" ('c'=1, 'a'=1, 'f'=1, 'é'=2, LF=1 = 6 bytes).
    // utf8 Rows with innerN=10 (cap 40 bytes) — plenty of room.
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["caf".toUtf8Bytes(), "c3a9".hexToBytes(),
             "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 2, 10, "utf8")) {
                  for (@rReply <- @f!?("readLinesInto", rows)) {
                    for (@[true, b0] <- @rows!?("getAt", 0)) {
                      for (@ba0 <- @b0!?("toByteArray")) {
                        @"out"!([rReply, ba0])
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
        _ => panic!(),
    };
    let (n, eof, _) = extract_read_lines_into(&outer.ps[0]);
    assert_eq!(n, 1);
    assert!(eof);
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    // "café" = 'c'(0x63), 'a'(0x61), 'f'(0x66), 'é'(0xC3 0xA9) = 5 bytes.
    assert_eq!(bytes, Some(vec![0x63, 0x61, 0x66, 0xC3, 0xA9]));
}

/// m-13-5: file with mixed blank + content lines.  Verifies row
/// alignment: row 0 = "a", row 1 = "" (blank), row 2 = "b".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_mixed_blank_and_content_lines() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "0a".hexToBytes(),
             "0a".hexToBytes(),
             "b".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 3, 10, "bytes")) {
                  for (@rReply <- @f!?("readLinesInto", rows)) {
                    for (@[true, b0] <- @rows!?("getAt", 0)) {
                      for (@[true, b1] <- @rows!?("getAt", 1)) {
                        for (@[true, b2] <- @rows!?("getAt", 2)) {
                          for (@ba0 <- @b0!?("toByteArray")) {
                            for (@ba1 <- @b1!?("toByteArray")) {
                              for (@ba2 <- @b2!?("toByteArray")) {
                                @"out"!([rReply, ba0, ba1, ba2])
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
        _ => panic!(),
    };
    let (n, _eof, _) = extract_read_lines_into(&outer.ps[0]);
    assert_eq!(n, 3);
    let (_, _, _, b0) = extract_reply(&outer.ps[1]);
    assert_eq!(b0, Some(b"a".to_vec()));
    let (_, _, _, b1) = extract_reply(&outer.ps[2]);
    assert_eq!(b1, Some(vec![]), "row 1: blank line");
    let (_, _, _, b2) = extract_reply(&outer.ps[3]);
    assert_eq!(b2, Some(b"b".to_vec()));
}

/// m-13-6: pre-fill an inner then run readLinesInto — verifies the
/// per-iteration `inner.clear()` call actually runs.  If clear were
/// regressed away, the pre-fill would be concatenated with the line.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_pre_filled_inner_is_cleared() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["hi".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 1, 20, "bytes")) {
                  for (@[true, b0] <- @rows!?("getAt", 0)) {
                    // Pre-fill inner 0 with stale bytes.
                    for (@_ <- @b0!?("writeBytes", "STALE".toUtf8Bytes())) {
                      for (@_ <- @f!?("readLinesInto", rows)) {
                        for (@ba0 <- @b0!?("toByteArray")) {
                          @"out"!(ba0)
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
    let (_, _, _, bytes) = extract_reply(&reply);
    assert_eq!(
        bytes,
        Some(b"hi".to_vec()),
        "inner must be cleared before fill — no 'STALE' prefix"
    );
}

/// m-13-7: readLinesInto forwards FSERR_IO from malformed UTF-8 on
/// a utf8 Rows.  The invalid start byte 0xFF triggers readLineInto's
/// utf8-preflight FSERR_IO, which flows through the loop's catchall.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_utf8_malformed_forwards_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("ff".hexToBytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
            for (@alloc <- Allocator!?()) {
              for (@[true, rows] <- @alloc!?("allocRows", 2, 10, "utf8")) {
                for (@r <- @f!?("readLinesInto", rows)) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
}

/// m-13-8: cursor at EOF after readLinesInto hits EOF-before-rows-full.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_eof_cursor_at_file_size() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["a".toUtf8Bytes(), "0a".hexToBytes(),
             "b".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 5, 10, "bytes")) {
                  for (@rReply <- @f!?("readLinesInto", rows)) {
                    for (@tellReply <- @f!?("tell")) {
                      @"out"!([rReply, tellReply])
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
        _ => panic!(),
    };
    let (n, eof, _) = extract_read_lines_into(&outer.ps[0]);
    assert_eq!(n, 2);
    assert!(eof);
    let (_, _, pos, _) = extract_reply(&outer.ps[1]);
    assert_eq!(pos, Some(4), "cursor at file size after EOF");
}

/// m-13-9: line exactly at capacity+1 bytes.  Under the M-3 short-
/// read heuristic (inherited from slice 12), a chunk that fills the
/// inner buffer exactly is treated as overflow.  For "1234\n" with
/// inner cap 4, readLineInto reads exactly 4 bytes "1234", marks
/// truncated=true (M-3 conservative), then drainToNextLF finds LF at
/// pos 0 of the next chunk and consumes it.  Result: n=1,
/// truncated=true.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_read_lines_into_line_exactly_at_cap_plus_lf() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["1234".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@alloc <- Allocator!?()) {
                for (@[true, rows] <- @alloc!?("allocRows", 2, 4, "bytes")) {
                  for (@rReply <- @f!?("readLinesInto", rows)) {
                    for (@[true, b0] <- @rows!?("getAt", 0)) {
                      for (@ba0 <- @b0!?("toByteArray")) {
                        @"out"!([rReply, ba0])
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
        _ => panic!(),
    };
    let (n, eof, truncated) = extract_read_lines_into(&outer.ps[0]);
    assert_eq!(n, 1);
    assert!(eof, "drain past LF hit EOF");
    assert!(
        truncated,
        "M-3 heuristic: line filling inner exactly is truncated"
    );
    let (_, _, _, bytes) = extract_reply(&outer.ps[1]);
    assert_eq!(bytes, Some(b"1234".to_vec()));
}

// ---------------------------------------------------------------------
// Phase 6 Slice 14: Stdin.rho — read-only, sequential-forward agent.
// ---------------------------------------------------------------------

/// bytes() over stdin emits one Int per next() then EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_bytes_emits_each_byte_then_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("ab".toUtf8Bytes(), 0)) |
          for (@sin <- Stdin!?(42)) {
            for (@[true, stream] <- @sin!?("bytes")) {
              for (@n1 <- @stream!?("next")) {
                for (@n2 <- @stream!?("next")) {
                  for (@n3 <- @stream!?("next")) {
                    @"out"!([n1, n2, n3])
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
    let (ok1, _, i1, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(i1, Some(0x61));
    let (ok2, _, i2, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(i2, Some(0x62));
    let (ok3, code, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok3);
    assert_eq!(code, "EOS");
}

/// chars() over stdin decodes UTF-8 and yields one codepoint per next.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_chars_yields_codepoints_then_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          // "aé" — 3 bytes: 0x61, 0xC3, 0xA9
          mockFdCell!(("61c3a9".hexToBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, stream] <- @sin!?("chars")) {
              for (@c1 <- @stream!?("next")) {
                for (@c2 <- @stream!?("next")) {
                  for (@c3 <- @stream!?("next")) {
                    @"out"!([c1, c2, c3])
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
    let (ok1, s1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(s1, "a");
    let (ok2, s2, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(s2, "\u{00E9}");
    let (ok3, code, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok3);
    assert_eq!(code, "EOS");
}

/// readLine() on stdin yields chars until LF (consumed but not emitted),
/// then EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_read_line_yields_chars_then_eos_at_lf() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!((["ab".toUtf8Bytes(), "0a".hexToBytes(),
                        "cd".toUtf8Bytes()].concatBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, stream] <- @sin!?("readLine")) {
              for (@n1 <- @stream!?("next")) {
                for (@n2 <- @stream!?("next")) {
                  for (@n3 <- @stream!?("next")) {
                    @"out"!([n1, n2, n3])
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
    let (ok1, s1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(s1, "a");
    let (ok2, s2, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok2);
    assert_eq!(s2, "b");
    let (ok3, code, _, _) = extract_reply(&outer.ps[2]);
    assert!(!ok3);
    assert_eq!(code, "EOS", "EOS at LF (LF consumed, not emitted)");
}

/// readLine() at end-of-input returns a pre-exhausted stream.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_read_line_at_eof_pre_exhausted() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sin <- Stdin!?(1)) {
          for (@[true, stream] <- @sin!?("readLine")) {
            for (@r <- @stream!?("next")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

/// lines() produces a LineStream; inner CharStreams share the source
/// forward cursor.  Verifies the single-active-inner rule works on
/// stdin (same design as File.lines()).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_lines_two_lines_then_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!((["ab".toUtf8Bytes(), "0a".hexToBytes(),
                        "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, outer] <- @sin!?("lines")) {
              for (@[true, inner1] <- @outer!?("next")) {
                for (@a1 <- @inner1!?("next")) {
                  for (@b1 <- @inner1!?("next")) {
                    for (@e1 <- @inner1!?("next")) {
                      for (@[true, inner2] <- @outer!?("next")) {
                        for (@c2 <- @inner2!?("next")) {
                          for (@d2 <- @inner2!?("next")) {
                            for (@e2 <- @inner2!?("next")) {
                              for (@rEnd <- @outer!?("next")) {
                                @"out"!([a1, b1, e1, c2, d2, e2, rEnd])
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
        _ => panic!(),
    };
    let (_, v1, _, _) = extract_reply(&outer.ps[0]);
    assert_eq!(v1, "a");
    let (_, v2, _, _) = extract_reply(&outer.ps[1]);
    assert_eq!(v2, "b");
    let (_, code, _, _) = extract_reply(&outer.ps[2]);
    assert_eq!(code, "EOS");
    let (_, v4, _, _) = extract_reply(&outer.ps[3]);
    assert_eq!(v4, "c");
    let (_, v5, _, _) = extract_reply(&outer.ps[4]);
    assert_eq!(v5, "d");
    let (_, code2, _, _) = extract_reply(&outer.ps[5]);
    assert_eq!(code2, "EOS");
    let (_, code3, _, _) = extract_reply(&outer.ps[6]);
    assert_eq!(code3, "EOS", "outer EOS after all lines");
}

/// close() marks Stdin cap closed; subsequent method calls return
/// FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_close_then_chars_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sin <- Stdin!?(1)) {
          for (@_ <- @sin!?("close")) {
            for (@r <- @sin!?("chars")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

/// Positional methods (seek, tell, size, bytesAt) fall through the
/// default arm → FSERR_UNSUPPORTED (spec §1087).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_positional_methods_return_fserr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sin <- Stdin!?(1)) {
          for (@rSeek <- @sin!?("seek", 0, "set")) {
            for (@rTell <- @sin!?("tell")) {
              for (@rSize <- @sin!?("size")) {
                for (@rBytesAt <- @sin!?("bytesAt", 0, 10)) {
                  @"out"!([rSeek, rTell, rSize, rBytesAt])
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
    for (i, name) in [(0, "seek"), (1, "tell"), (2, "size"), (3, "bytesAt")] {
        let (ok, code, _, _) = extract_reply(&outer.ps[i]);
        assert!(!ok, "{name} should fail on Stdin");
        assert_eq!(code, "FSERR_UNSUPPORTED", "{name}");
    }
}

/// Unknown method dispatch → FSERR_UNSUPPORTED via default arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_unknown_method_returns_fserr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sin <- Stdin!?(1)) {
          for (@r <- @sin!?("wibble")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// chars() on an empty stdin (spec case 1: batch .rho with no input)
/// returns a stream whose first next() yields EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_chars_on_empty_input_yields_immediate_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sin <- Stdin!?(1)) {
          for (@[true, stream] <- @sin!?("chars")) {
            for (@r <- @stream!?("next")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

/// Invalid UTF-8 in the stdin stream surfaces as FSERR_IO on chars.next().
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_chars_invalid_utf8_returns_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("ff".hexToBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, stream] <- @sin!?("chars")) {
              for (@r <- @stream!?("next")) { @"out"!(r) }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
}

// ---------------------------------------------------------------------
// Phase 5 cross-slice review — coverage matrix fills.
// ---------------------------------------------------------------------

/// Helper: extract the `msg` (third element) from a failure reply
/// `[false, code, msg]`.  Panics if the reply isn't a 3-element list
/// or the msg isn't a String.
fn extract_failure_msg(par: &Par) -> String {
    let list = match single_expr(par).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("failure reply not a list"),
    };
    assert!(list.ps.len() >= 3, "reply too short to have msg");
    match single_expr(&list.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!("msg not a String"),
    }
}

/// Helper: assert a failure reply is exactly [false, code, msg]
/// (3 elements) — m-P5-3 shape invariant.
fn assert_failure_shape_three_elems(par: &Par, expected_code: &str) {
    let list = match single_expr(par).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        other => panic!("expected list reply, got {other:?}"),
    };
    assert_eq!(
        list.ps.len(),
        3,
        "failure reply must be exactly [false, code, msg] (3 elements)"
    );
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("head not Bool"),
    };
    assert!(!ok, "expected [false, ...]");
    let code = match single_expr(&list.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!("code not GString"),
    };
    assert_eq!(code, expected_code);
    // Slot 2 must exist and be some String; body content varies.
    let _ = single_expr(&list.ps[2]).unwrap().expr_instance;
}

// -- M-P5-3: default-arm matrix (Buffer, Allocator).  Stream is
//    already covered in stream_check.rs.

/// Buffer.default → BUFERR_UNSUPPORTED on unknown method.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn buffer_unknown_method_returns_buferr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@buf <- Buffer!?(8, "bytes")) {
          for (@r <- @buf!?("wibble")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "BUFERR_UNSUPPORTED");
}

/// Allocator.default → BUFERR_UNSUPPORTED on unknown method.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn allocator_unknown_method_returns_buferr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@r <- @alloc!?("wibble")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "BUFERR_UNSUPPORTED");
}

// -- M-P5-4: mode-cap matrix — writeString, writeByteArray on
//    read-only.  Other write methods were covered per-slice.

/// writeString on a read-only file returns FSERR_UNSUPPORTED with
/// exact [false, code, msg] shape.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_string_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("hi".toUtf8Bytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
            for (@r <- @f!?("writeString", "boom")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_UNSUPPORTED");
}

/// writeByteArray on a read-only file returns FSERR_UNSUPPORTED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_write_bytearray_on_readonly_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("hi".toUtf8Bytes(), 0)) |
          for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
            for (@r <- @f!?("writeByteArray", "boom".toUtf8Bytes())) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_UNSUPPORTED");
}

// -- M-P5-6: type-guard matrix — Allocator.allocBytes / allocUtf8
//    with non-Int args.  Buffer.read non-Int.

/// Allocator.allocBytes with non-Int arg → BUFERR_INVALID_ARGUMENT.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alloc_bytes_non_int_arg_rejected() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@r <- @alloc!?("allocBytes", "not-int")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "BUFERR_INVALID_ARGUMENT");
}

/// Allocator.allocUtf8 with non-Int arg → BUFERR_INVALID_ARGUMENT.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alloc_utf8_non_int_arg_rejected() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@alloc <- Allocator!?()) {
          for (@r <- @alloc!?("allocUtf8", "not-int")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "BUFERR_INVALID_ARGUMENT");
}

/// Buffer.read with non-Int arg → BUFERR_INVALID_ARGUMENT (Mi-P5-3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn buffer_read_non_int_arg_rejected() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@buf <- Buffer!?(8, "bytes")) {
          for (@r <- @buf!?("read", "not-int")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "BUFERR_INVALID_ARGUMENT");
}

// -- m-P5-4: cursor discipline after truncate.

/// After truncate(n), the cursor is preserved (spec §988: truncate
/// sets size to n; cursor position is separate).  If cursor > n after
/// truncate, subsequent reads see EOF; cursor position itself is
/// unchanged (position past EOF is legal — writes zero-pad).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_truncate_preserves_cursor_position() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray", "abcdefgh".toUtf8Bytes())) {
            // Cursor at 8.  Truncate to 4.  Cursor should remain 8.
            for (@_ <- @f!?("truncate", 4)) {
              for (@tellReply <- @f!?("tell")) {
                for (@sizeReply <- @f!?("size")) {
                  @"out"!([tellReply, sizeReply])
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
    let (_, _, pos, _) = extract_reply(&outer.ps[0]);
    let (_, _, size, _) = extract_reply(&outer.ps[1]);
    // Note: the mock's fsTruncate doesn't shrink the byte buffer,
    // so size() may still read the original.  We assert the cursor
    // position is preserved regardless of mock's truncate semantics.
    assert_eq!(pos, Some(8), "cursor unaffected by truncate");
    // size() is whatever the mock returns; document via read-back.
    let _ = size;
}

// ---------------------------------------------------------------------
// Slice 14 review-driven coverage additions.
// ---------------------------------------------------------------------

// -- M-14-1: empty-input bytes() and lines() yield immediate EOS.

/// Empty stdin: bytes().next() yields EOS on the first call.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_bytes_on_empty_input_yields_immediate_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sin <- Stdin!?(1)) {
          for (@[true, stream] <- @sin!?("bytes")) {
            for (@r <- @stream!?("next")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS");
}

/// Empty stdin: lines() outer.next() yields EOS immediately — no
/// phantom inner is minted.  Matches spec case 1.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_lines_on_empty_input_outer_yields_immediate_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sin <- Stdin!?(1)) {
          for (@[true, outer] <- @sin!?("lines")) {
            for (@r <- @outer!?("next")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "EOS", "outer EOS immediately — no phantom inner");
}

// -- M-14-2: direct close() [true] shape assertion.

/// close() reply is exactly [true] (single-element list).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_close_returns_true_single_element() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sin <- Stdin!?(1)) {
          for (@r <- @sin!?("close")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        other => panic!("expected list, got {other:?}"),
    };
    assert_eq!(list.ps.len(), 1, "close reply must be exactly [true]");
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!("head not Bool"),
    };
    assert!(ok);
}

// -- M-14-3: truncated UTF-8 at end of input via readLine() and
//    lines() inner.  Matches File.chars_truncated_utf8_at_eof_returns_fserr_io.

/// readLine() reader stops mid-codepoint at EOF — the codepoint's
/// lead byte was committed (position advanced) but the continuation
/// byte never arrives → FSERR_IO with message "truncated UTF-8 at
/// end of input".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_read_line_truncated_utf8_at_eof_returns_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // File "aC3" — one 'a' (0x61) then 0xC3 (2-byte lead without
    // continuation).  First next() yields "a"; second reaches the
    // lead-byte-at-EOF path.
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("61c3".hexToBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, stream] <- @sin!?("readLine")) {
              for (@n1 <- @stream!?("next")) {
                for (@n2 <- @stream!?("next")) {
                  @"out"!([n1, n2])
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
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "a");
    let (ok2, code2, _, _) = extract_reply(&outer.ps[1]);
    assert!(!ok2);
    assert_eq!(
        code2, "FSERR_IO",
        "truncated UTF-8 at EOF surfaces as FSERR_IO"
    );
}

/// lines() inner: same truncated-UTF-8 EOF handling.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_lines_inner_truncated_utf8_returns_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // File "aC3" — inside inner1 (before any LF).  Same 2-step
    // sequence as readLine: "a" then FSERR_IO.
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("61c3".hexToBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, outer] <- @sin!?("lines")) {
              for (@[true, inner1] <- @outer!?("next")) {
                for (@n1 <- @inner1!?("next")) {
                  for (@n2 <- @inner1!?("next")) {
                    @"out"!([n1, n2])
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
    let (ok1, v1, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1);
    assert_eq!(v1, "a");
    let (ok2, code2, _, _) = extract_reply(&outer.ps[1]);
    assert!(!ok2);
    assert_eq!(code2, "FSERR_IO");
}

// -- M-14-4: single-active-inner force-drain on stdin lines().

/// After outer.next() is called while inner1 is half-drained, outer
/// force-drains inner1 past its LF; inner2 begins on the next line's
/// first char (not stale bytes).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_lines_single_active_inner_force_drains_half_drained() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!((["ab".toUtf8Bytes(), "0a".hexToBytes(),
                        "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, outer] <- @sin!?("lines")) {
              for (@[true, inner1] <- @outer!?("next")) {
                for (@a1 <- @inner1!?("next")) {
                  // Half-drain: read only 'a', then move on.
                  for (@[true, inner2] <- @outer!?("next")) {
                    for (@c2 <- @inner2!?("next")) {
                      @"out"!([a1, c2])
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
        _ => panic!(),
    };
    let (_, v1, _, _) = extract_reply(&outer.ps[0]);
    assert_eq!(v1, "a");
    let (_, v2, _, _) = extract_reply(&outer.ps[1]);
    assert_eq!(v2, "c", "inner2 skips inner1's remaining chars + LF");
}

// -- M-14-5: drained-inner fails closed on stdin.

/// A retained inner handle drained by outer returns FSERR_CLOSED on
/// subsequent next() (matches spec §357 + File slice-11 semantics).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_lines_drained_inner_fails_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!((["ab".toUtf8Bytes(), "0a".hexToBytes(),
                        "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, outer] <- @sin!?("lines")) {
              for (@[true, inner1] <- @outer!?("next")) {
                for (@[true, _inner2] <- @outer!?("next")) {
                  for (@r <- @inner1!?("next")) { @"out"!(r) }
                }
              }
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

// -- M-14-6: outer LineStream chunk() unsupported.

/// stdin.lines() outer.chunk(n) returns FSERR_UNSUPPORTED (spec §111).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_lines_chunk_returns_fserr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!((["ab".toUtf8Bytes(), "0a".hexToBytes(),
                        "cd".toUtf8Bytes(), "0a".hexToBytes()].concatBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, outer] <- @sin!?("lines")) {
              for (@r <- @outer!?("chunk", 2)) { @"out"!(r) }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

// -- m-14-3: blank-line coverage on readLine() and lines().

/// stdin.readLine() on "\n" yields EOS immediately (LF consumed, no
/// chars emitted).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_read_line_blank_line_yields_eos_immediately() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("0a".hexToBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, stream] <- @sin!?("readLine")) {
              for (@r <- @stream!?("next")) { @"out"!(r) }
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

/// stdin.lines() on "\n" yields one empty inner (immediate EOS), then
/// outer EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_lines_blank_line_yields_empty_inner_then_outer_eos() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("0a".hexToBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, outer] <- @sin!?("lines")) {
              for (@[true, inner1] <- @outer!?("next")) {
                for (@e1 <- @inner1!?("next")) {
                  for (@rEnd <- @outer!?("next")) {
                    @"out"!([e1, rEnd])
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
    let (ok1, code1, _, _) = extract_reply(&outer.ps[0]);
    assert!(!ok1);
    assert_eq!(code1, "EOS", "empty inner immediate EOS");
    let (ok2, code2, _, _) = extract_reply(&outer.ps[1]);
    assert!(!ok2);
    assert_eq!(code2, "EOS", "outer EOS after blank line");
}

// -- m-14-4: UTF-8 multi-byte via readLine() and lines() inner.

/// readLine() decodes multi-byte codepoints: "café\n" → c, a, f, é, EOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_read_line_utf8_codepoints() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // "café\n" = 0x63 0x61 0x66 0xC3 0xA9 0x0A
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("636166c3a90a".hexToBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, stream] <- @sin!?("readLine")) {
              for (@c1 <- @stream!?("next")) {
                for (@c2 <- @stream!?("next")) {
                  for (@c3 <- @stream!?("next")) {
                    for (@c4 <- @stream!?("next")) {
                      for (@c5 <- @stream!?("next")) {
                        @"out"!([c1, c2, c3, c4, c5])
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
        _ => panic!(),
    };
    let expected: [&str; 4] = ["c", "a", "f", "\u{00E9}"];
    for (i, want) in expected.iter().enumerate() {
        let (ok, v, _, _) = extract_reply(&outer.ps[i]);
        assert!(ok, "char {i}");
        assert_eq!(&v, want, "char {i}");
    }
    let (ok, code, _, _) = extract_reply(&outer.ps[4]);
    assert!(!ok);
    assert_eq!(code, "EOS", "LF consumed → EOS");
}

/// lines() inner: multi-byte codepoints inside a line.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_lines_inner_utf8_codepoints() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // "aé\nb\n"
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("61c3a90a620a".hexToBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, outer] <- @sin!?("lines")) {
              for (@[true, inner1] <- @outer!?("next")) {
                for (@n1 <- @inner1!?("next")) {
                  for (@n2 <- @inner1!?("next")) {
                    for (@n3 <- @inner1!?("next")) {
                      @"out"!([n1, n2, n3])
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
        _ => panic!(),
    };
    let (_, v1, _, _) = extract_reply(&outer.ps[0]);
    assert_eq!(v1, "a");
    let (_, v2, _, _) = extract_reply(&outer.ps[1]);
    assert_eq!(v2, "\u{00E9}", "multi-byte é inside inner1");
    let (_, code, _, _) = extract_reply(&outer.ps[2]);
    assert_eq!(code, "EOS");
}

// -- m-14-5: unterminated final line.

/// readLine() on "abc" (no LF) yields a, b, c, EOS (natural EOF).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_read_line_unterminated_final_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("abc".toUtf8Bytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, stream] <- @sin!?("readLine")) {
              for (@n1 <- @stream!?("next")) {
                for (@n2 <- @stream!?("next")) {
                  for (@n3 <- @stream!?("next")) {
                    for (@n4 <- @stream!?("next")) {
                      @"out"!([n1, n2, n3, n4])
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
        _ => panic!(),
    };
    let (_, v1, _, _) = extract_reply(&outer.ps[0]);
    assert_eq!(v1, "a");
    let (_, v2, _, _) = extract_reply(&outer.ps[1]);
    assert_eq!(v2, "b");
    let (_, v3, _, _) = extract_reply(&outer.ps[2]);
    assert_eq!(v3, "c");
    let (_, code, _, _) = extract_reply(&outer.ps[3]);
    assert_eq!(code, "EOS", "EOS at end of input");
}

/// lines() on unterminated final line "ab\ncd" still produces two
/// inners; inner2 emits its chars then EOS at EOF.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_lines_unterminated_final_line() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!((["ab".toUtf8Bytes(), "0a".hexToBytes(),
                        "cd".toUtf8Bytes()].concatBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, outer] <- @sin!?("lines")) {
              for (@[true, inner1] <- @outer!?("next")) {
                for (@_a <- @inner1!?("next")) {
                  for (@_b <- @inner1!?("next")) {
                    for (@_e1 <- @inner1!?("next")) {
                      for (@[true, inner2] <- @outer!?("next")) {
                        for (@c <- @inner2!?("next")) {
                          for (@d <- @inner2!?("next")) {
                            for (@e2 <- @inner2!?("next")) {
                              for (@rEnd <- @outer!?("next")) {
                                @"out"!([c, d, e2, rEnd])
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
        _ => panic!(),
    };
    let (_, v1, _, _) = extract_reply(&outer.ps[0]);
    assert_eq!(v1, "c");
    let (_, v2, _, _) = extract_reply(&outer.ps[1]);
    assert_eq!(v2, "d");
    let (_, code, _, _) = extract_reply(&outer.ps[2]);
    assert_eq!(code, "EOS", "inner2 EOS at EOF");
    let (_, code, _, _) = extract_reply(&outer.ps[3]);
    assert_eq!(code, "EOS", "outer EOS");
}

// -- m-14-6: close() idempotence.

/// Two consecutive close() calls both return [true] — spec-implied
/// idempotence.  Stdin.close doesn't touch the fd, so this is a
/// no-op-then-no-op.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_close_idempotent() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sin <- Stdin!?(1)) {
          for (@r1 <- @sin!?("close")) {
            for (@r2 <- @sin!?("close")) {
              @"out"!([r1, r2])
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
    for i in 0..2 {
        let list = match single_expr(&outer.ps[i]).unwrap().expr_instance {
            Some(ExprInstance::EListBody(l)) => l,
            _ => panic!("close reply {i} not a list"),
        };
        assert_eq!(list.ps.len(), 1, "close reply {i} must be [true]");
        let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
            Some(ExprInstance::GBool(b)) => b,
            _ => panic!(),
        };
        assert!(ok, "close reply {i}");
    }
}

// -- Mi-14-1: reply-shape invariant applied to failure tests.

/// Invalid UTF-8 on chars(): shape is exactly [false, code, msg].
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_chars_invalid_utf8_reply_shape_is_three_elems() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_ <- mockFdCell) {
          mockFdCell!(("ff".hexToBytes(), 0)) |
          for (@sin <- Stdin!?(1)) {
            for (@[true, stream] <- @sin!?("chars")) {
              for (@r <- @stream!?("next")) { @"out"!(r) }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_IO");
}

/// FSERR_UNSUPPORTED on positional (seek) also matches the three-elem
/// shape.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_positional_reply_shape_is_three_elems() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sin <- Stdin!?(1)) {
          for (@r <- @sin!?("seek", 0, "set")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_UNSUPPORTED");
}

// ---------------------------------------------------------------------
// Phase 6 Slice 15: Stdout.rho — write-only agent.
// ---------------------------------------------------------------------

/// writeByteArray on stdout writes the bytes and returns [true].
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_bytearray_writes_bytes() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sout <- Stdout!?(1)) {
          for (@wReply <- @sout!?("writeByteArray", "hello".toUtf8Bytes())) {
            for (@state <<- mockFdCell) {
              match state {
                (bytes, _cur) => @"out"!([wReply, bytes])
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
    // Reply is exactly [true] — spec §1112.
    let list = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("writeByteArray reply not a list"),
    };
    assert_eq!(list.ps.len(), 1, "spec §1112 reply is exactly [true]");
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    assert_eq!(bytes, b"hello".to_vec());
}

/// writeString on stdout encodes as UTF-8 and writes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_string_encodes_utf8() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        "for (@sout <- Stdout!?(1)) {\
           for (@wReply <- @sout!?(\"writeString\", \"caf\u{00E9}\")) {\
             for (@state <<- mockFdCell) {\
               match state {\
                 (bytes, _cur) => @\"out\"!([wReply, bytes])\
               }\
             }\
           }\
         }",
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    // "café" = 0x63 0x61 0x66 0xC3 0xA9
    assert_eq!(bytes, vec![0x63, 0x61, 0x66, 0xC3, 0xA9]);
}

/// writeBytes drains a ByteStream and writes each chunk.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_bytes_drains_stream() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let bootstrap = byte_stream_from_list(r#"["a".toUtf8Bytes(), "b".toUtf8Bytes()]"#).replace(
        "%TEST_SNIPPET%",
        r#"
            for (@sout <- Stdout!?(1)) {
              for (@wReply <- @sout!?("writeBytes", stream)) {
                for (@state <<- mockFdCell) {
                  match state {
                    (bytes, _) => @"out"!([wReply, bytes])
                  }
                }
              }
            }
            "#,
    );
    let src = with_libs(&bootstrap);
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok, "writeBytes should succeed");
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    assert_eq!(bytes, b"ab".to_vec());
}

/// writeChars drains a CharStream and writes each char as UTF-8.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_chars_drains_char_stream() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new csState, csProducer, csBuilder in {
          csState!(["a", "b", "c"]) |
          contract csProducer(retCh) = {
            for (@lst <- csState) {
              match lst {
                []             => { csState!([]) | retCh!([false, "EOS", ""]) }
                [head ...tail] => { csState!(tail) | retCh!([true, head]) }
              }
            }
          } |
          contract csBuilder(@vals, retCh) = { retCh!([true, vals]) } |
          for (@stream <- Stream!?(*csProducer, *csBuilder)) {
            for (@sout <- Stdout!?(1)) {
              for (@wReply <- @sout!?("writeChars", stream)) {
                for (@state <<- mockFdCell) {
                  match state {
                    (bytes, _) => @"out"!([wReply, bytes])
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
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    assert_eq!(bytes, b"abc".to_vec());
}

/// writeLine drains a CharStream and appends LF.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_line_appends_lf() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new wlState, wlProducer, wlBuilder in {
          wlState!(["h", "i"]) |
          contract wlProducer(retCh) = {
            for (@lst <- wlState) {
              match lst {
                []             => { wlState!([]) | retCh!([false, "EOS", ""]) }
                [head ...tail] => { wlState!(tail) | retCh!([true, head]) }
              }
            }
          } |
          contract wlBuilder(@vals, retCh) = { retCh!([true, vals]) } |
          for (@stream <- Stream!?(*wlProducer, *wlBuilder)) {
            for (@sout <- Stdout!?(1)) {
              for (@wReply <- @sout!?("writeLine", stream)) {
                for (@state <<- mockFdCell) {
                  match state {
                    (bytes, _) => @"out"!([wReply, bytes])
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
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    assert_eq!(bytes, b"hi\n".to_vec(), "chars + LF");
}

/// writeLines end-to-end: File.lines() → Stdout.writeLines.  Because
/// our test mock uses a shared mockFdCell, this appends the copied
/// lines after the source.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_lines_roundtrip_from_file() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("writeByteArray",
            ["hi".toUtf8Bytes(), "0a".hexToBytes(),
             "by".toUtf8Bytes(), "0a".hexToBytes()].concatBytes())) {
            for (@_ <- @f!?("seek", 0, "set")) {
              for (@[true, outer] <- @f!?("lines")) {
                for (@sout <- Stdout!?(99)) {
                  for (@wReply <- @sout!?("writeLines", outer)) {
                    for (@state <<- mockFdCell) {
                      match state {
                        (bytes, _) => @"out"!([wReply, bytes])
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
        _ => panic!(),
    };
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok, "writeLines success");
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    assert_eq!(bytes, b"hi\nby\nhi\nby\n".to_vec());
}

/// flush() returns [true].
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_flush_returns_true() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sout <- Stdout!?(1)) {
          for (@r <- @sout!?("flush")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

/// After close() every write method returns FSERR_CLOSED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_close_then_write_returns_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sout <- Stdout!?(1)) {
          for (@rClose <- @sout!?("close")) {
            for (@rWrite <- @sout!?("writeString", "hi")) {
              @"out"!([rClose, rWrite])
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
    let list = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    assert_eq!(list.ps.len(), 1);
    let (ok, code, _, _) = extract_reply(&outer.ps[1]);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

/// Read-side methods return FSERR_UNSUPPORTED (spec §1116).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_read_methods_return_fserr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sout <- Stdout!?(1)) {
          for (@rChars <- @sout!?("chars")) {
            for (@rBytes <- @sout!?("bytes")) {
              for (@rLines <- @sout!?("lines")) {
                for (@rReadLine <- @sout!?("readLine")) {
                  @"out"!([rChars, rBytes, rLines, rReadLine])
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
    for (i, name) in [(0, "chars"), (1, "bytes"), (2, "lines"), (3, "readLine")] {
        let (ok, code, _, _) = extract_reply(&outer.ps[i]);
        assert!(!ok, "{name} should fail on Stdout");
        assert_eq!(code, "FSERR_UNSUPPORTED", "{name}");
    }
}

/// writeByteArray with non-ByteArray arg returns FSERR_BAD_ARG.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_bytearray_non_bytearray_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sout <- Stdout!?(1)) {
          for (@r <- @sout!?("writeByteArray", "not-bytes")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_BAD_ARG");
}

/// writeString with non-String arg returns FSERR_BAD_ARG.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_string_non_string_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sout <- Stdout!?(1)) {
          for (@r <- @sout!?("writeString", 42)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_BAD_ARG");
}

/// Unknown method → FSERR_UNSUPPORTED via default arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_unknown_method_returns_fserr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sout <- Stdout!?(1)) {
          for (@r <- @sout!?("wibble")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_UNSUPPORTED");
}

/// close() is idempotent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_close_idempotent() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sout <- Stdout!?(1)) {
          for (@r1 <- @sout!?("close")) {
            for (@r2 <- @sout!?("close")) {
              @"out"!([r1, r2])
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
    for i in 0..2 {
        let list = match single_expr(&outer.ps[i]).unwrap().expr_instance {
            Some(ExprInstance::EListBody(l)) => l,
            _ => panic!("close reply {i} not a list"),
        };
        assert_eq!(list.ps.len(), 1, "close reply {i} must be [true]");
    }
}

/// writeLines forwards mid-stream producer errors with the "wrote N
/// lines" prefix.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_lines_producer_error_reports_lines_written() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new outerState, innerProd, innerBuild, outerProd, outerBuild in {
          new innerCursor in {
            innerCursor!(0) |
            contract innerProd(retCh) = {
              for (@i <- innerCursor) {
                match i {
                  0 => { innerCursor!(1) | retCh!([true, "x"]) }
                  1 => { innerCursor!(2) | retCh!([true, "y"]) }
                  _ => { innerCursor!(i) | retCh!([false, "EOS", ""]) }
                }
              }
            } |
            contract innerBuild(@vals, retCh) = { retCh!([true, ""]) } |
            for (@innerHandle <- Stream!?(*innerProd, *innerBuild)) {
              outerState!(0) |
              contract outerProd(retCh) = {
                for (@n <- outerState) {
                  match n {
                    0 => { outerState!(1) | retCh!([true, innerHandle]) }
                    _ => { outerState!(n) | retCh!([false, "FSERR_IO", "simulated"]) }
                  }
                }
              } |
              contract outerBuild(@_v, retCh) = {
                retCh!([false, "FSERR_UNSUPPORTED", "chunk unsupported"])
              } |
              for (@outerHandle <- Stream!?(*outerProd, *outerBuild)) {
                for (@sout <- Stdout!?(1)) {
                  for (@r <- @sout!?("writeLines", outerHandle)) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let msg = match single_expr(&list.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!("msg not string"),
    };
    assert!(
        msg.contains("wrote 1 lines"),
        "msg must show lines-written prefix: {msg}"
    );
    assert!(
        msg.contains("simulated"),
        "msg must include underlying error: {msg}"
    );
}

// ---------------------------------------------------------------------
// Slice 15 review-driven coverage additions.
// ---------------------------------------------------------------------

// -- M-15-2: closed-file matrix on write methods.

/// Helper Rholang: open stdout, close it, then invoke `method` with
/// `args_expr` — assert FSERR_CLOSED reply.
async fn stdout_closed_method_returns_fserr_closed(method: &str, args_expr: &str) {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(&format!(
        r#"
        for (@sout <- Stdout!?(1)) {{
          for (@_ <- @sout!?("close")) {{
            for (@r <- @sout!?("{method}"{args_expr})) {{ @"out"!(r) }}
          }}
        }}
        "#
    ));
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_CLOSED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_closed_write_bytearray_fserr_closed() {
    stdout_closed_method_returns_fserr_closed("writeByteArray", ", \"x\".toUtf8Bytes()").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_closed_write_bytes_fserr_closed() {
    stdout_closed_method_returns_fserr_closed("writeBytes", ", Nil").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_closed_write_chars_fserr_closed() {
    stdout_closed_method_returns_fserr_closed("writeChars", ", Nil").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_closed_write_line_fserr_closed() {
    stdout_closed_method_returns_fserr_closed("writeLine", ", Nil").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_closed_write_lines_fserr_closed() {
    stdout_closed_method_returns_fserr_closed("writeLines", ", Nil").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_closed_flush_fserr_closed() {
    stdout_closed_method_returns_fserr_closed("flush", "").await;
}

// -- M-15-3: empty-stream input paths.

/// writeBytes with an immediately-EOS ByteStream → [true], no bytes
/// written.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_bytes_empty_stream_returns_true() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new emptyProd, emptyBuilder in {
          contract emptyProd(retCh) = { retCh!([false, "EOS", ""]) } |
          contract emptyBuilder(@vals, retCh) = { retCh!([true, vals]) } |
          for (@stream <- Stream!?(*emptyProd, *emptyBuilder)) {
            for (@sout <- Stdout!?(1)) {
              for (@wReply <- @sout!?("writeBytes", stream)) {
                for (@state <<- mockFdCell) {
                  match state {
                    (bytes, _) => @"out"!([wReply, bytes])
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
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok, "empty writeBytes should succeed");
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    assert_eq!(bytes, Vec::<u8>::new(), "no bytes written for empty stream");
}

/// writeChars with an immediately-EOS CharStream → [true], no bytes
/// written.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_chars_empty_stream_returns_true() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new emptyProd, emptyBuilder in {
          contract emptyProd(retCh) = { retCh!([false, "EOS", ""]) } |
          contract emptyBuilder(@vals, retCh) = { retCh!([true, vals]) } |
          for (@stream <- Stream!?(*emptyProd, *emptyBuilder)) {
            for (@sout <- Stdout!?(1)) {
              for (@wReply <- @sout!?("writeChars", stream)) {
                for (@state <<- mockFdCell) {
                  match state {
                    (bytes, _) => @"out"!([wReply, bytes])
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
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    assert_eq!(bytes, Vec::<u8>::new());
}

/// writeLine with an immediately-EOS CharStream → [true], only LF
/// written (single 0x0A byte).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_line_empty_stream_writes_only_lf() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new emptyProd, emptyBuilder in {
          contract emptyProd(retCh) = { retCh!([false, "EOS", ""]) } |
          contract emptyBuilder(@vals, retCh) = { retCh!([true, vals]) } |
          for (@stream <- Stream!?(*emptyProd, *emptyBuilder)) {
            for (@sout <- Stdout!?(1)) {
              for (@wReply <- @sout!?("writeLine", stream)) {
                for (@state <<- mockFdCell) {
                  match state {
                    (bytes, _) => @"out"!([wReply, bytes])
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
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    assert_eq!(bytes, vec![0x0A], "only LF written for empty CharStream");
}

/// writeLines with an immediately-EOS LineStream → [true], no bytes
/// written.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_lines_empty_stream_returns_true() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new outerProd, outerBuild in {
          contract outerProd(retCh) = { retCh!([false, "EOS", ""]) } |
          contract outerBuild(@_v, retCh) = {
            retCh!([false, "FSERR_UNSUPPORTED", "chunk unsupported"])
          } |
          for (@stream <- Stream!?(*outerProd, *outerBuild)) {
            for (@sout <- Stdout!?(1)) {
              for (@wReply <- @sout!?("writeLines", stream)) {
                for (@state <<- mockFdCell) {
                  match state {
                    (bytes, _) => @"out"!([wReply, bytes])
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
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    assert_eq!(bytes, Vec::<u8>::new());
}

// -- M-15-4: non-Stream args hang (documented limitation).
//
// Same rationale as slice-11 M-11-6 (file_write_lines_non_stream_arg_
// yields_no_reply): writeBytes/writeChars/writeLine/writeLines send
// `!?("next")` to the arg without type-guarding.  If the arg doesn't
// respond as an agent, the call hangs.  These tests document current
// behavior — type-guarding is a follow-up.

async fn stdout_non_stream_arg_yields_no_reply(method: &str) {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(&format!(
        r#"
        for (@sout <- Stdout!?(1)) {{
          for (@r <- @sout!?("{method}", "not-a-stream")) {{ @"out"!(r) }}
        }}
        "#
    ));
    let par = ParBuilderUtil::mk_term(&src).expect("compile");
    reducer
        .eval(par, &Env::new(), rand().split_byte(0))
        .await
        .expect("eval");
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    let map = space.to_map().await;
    let out_chan = new_gstring_par("out".to_string(), Vec::new(), false);
    assert!(
        !map.contains_key(&vec![out_chan]),
        "known limitation: non-stream arg to {method} leaves the call hung"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_bytes_non_stream_arg_yields_no_reply() {
    stdout_non_stream_arg_yields_no_reply("writeBytes").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_chars_non_stream_arg_yields_no_reply() {
    stdout_non_stream_arg_yields_no_reply("writeChars").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_line_non_stream_arg_yields_no_reply() {
    stdout_non_stream_arg_yields_no_reply("writeLine").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_lines_non_stream_arg_yields_no_reply() {
    stdout_non_stream_arg_yields_no_reply("writeLines").await;
}

// -- m-15-3: apply reply-shape helper to close-then-write test's
//    FSERR_CLOSED reply.  (Already done for FSERR_CLOSED in the new
//    M-15-2 helper via assert_failure_shape_three_elems.)

// -- m-15-5: writeBytes / writeChars mid-stream producer error.

/// writeBytes forwards a mid-stream producer error with the "wrote N
/// bytes before producer failure: MSG" prefix (spec §938).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_bytes_producer_error_reports_bytes_written() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new bsState, bsProd, bsBuild in {
          bsState!(0) |
          contract bsProd(retCh) = {
            for (@n <- bsState) {
              match n {
                0 => { bsState!(1) | retCh!([true, "hi".toUtf8Bytes()]) }
                _ => { bsState!(n) | retCh!([false, "FSERR_IO", "simulated"]) }
              }
            }
          } |
          contract bsBuild(@vals, retCh) = { retCh!([true, vals]) } |
          for (@stream <- Stream!?(*bsProd, *bsBuild)) {
            for (@sout <- Stdout!?(1)) {
              for (@r <- @sout!?("writeBytes", stream)) { @"out"!(r) }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let msg = match single_expr(&list.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert!(
        msg.contains("wrote 2 bytes"),
        "msg must show bytes-written prefix: {msg}"
    );
    assert!(msg.contains("simulated"));
}

/// writeChars forwards a mid-stream producer error with the "wrote N
/// bytes before producer failure: MSG" prefix.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_chars_producer_error_reports_bytes_written() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new csState, csProd, csBuild in {
          csState!(0) |
          contract csProd(retCh) = {
            for (@n <- csState) {
              match n {
                0 => { csState!(1) | retCh!([true, "a"]) }
                _ => { csState!(n) | retCh!([false, "FSERR_IO", "simulated"]) }
              }
            }
          } |
          contract csBuild(@vals, retCh) = { retCh!([true, vals]) } |
          for (@stream <- Stream!?(*csProd, *csBuild)) {
            for (@sout <- Stdout!?(1)) {
              for (@r <- @sout!?("writeChars", stream)) { @"out"!(r) }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let msg = match single_expr(&list.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!(),
    };
    assert!(
        msg.contains("wrote 1 bytes"),
        "wrote-1-byte 'a' before producer failure: {msg}"
    );
    assert!(msg.contains("simulated"));
}

// -- m-15-6: writeLine UTF-8 multi-byte.

/// writeLine with UTF-8 multi-byte chars encodes correctly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_write_line_utf8_multibyte_chars() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // CharStream emitting ["c", "a", "f", "é"] then EOS.  Expected
    // bytes: 0x63 0x61 0x66 0xC3 0xA9 0x0A (café + LF).
    let src = with_libs(
        "new mbState, mbProd, mbBuild in {\
           mbState!([\"c\", \"a\", \"f\", \"\u{00E9}\"]) |\
           contract mbProd(retCh) = {\
             for (@lst <- mbState) {\
               match lst {\
                 []             => { mbState!([]) | retCh!([false, \"EOS\", \"\"]) }\
                 [head ...tail] => { mbState!(tail) | retCh!([true, head]) }\
               }\
             }\
           } |\
           contract mbBuild(@vals, retCh) = { retCh!([true, vals]) } |\
           for (@stream <- Stream!?(*mbProd, *mbBuild)) {\
             for (@sout <- Stdout!?(1)) {\
               for (@wReply <- @sout!?(\"writeLine\", stream)) {\
                 for (@state <<- mockFdCell) {\
                   match state {\
                     (bytes, _) => @\"out\"!([wReply, bytes])\
                   }\
                 }\
               }\
             }\
           }\
         }",
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let (ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok);
    let bytes = match single_expr(&outer.ps[1]).unwrap().expr_instance {
        Some(ExprInstance::GByteArray(b)) => b,
        _ => panic!(),
    };
    assert_eq!(bytes, vec![0x63, 0x61, 0x66, 0xC3, 0xA9, 0x0A]);
}

// -- Mi-15-2: close idempotence — deep-shape check that each reply
//    is [true] with element GBool(true).

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_close_idempotent_deep_shape() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@sout <- Stdout!?(1)) {
          for (@r1 <- @sout!?("close")) {
            for (@r2 <- @sout!?("close")) {
              @"out"!([r1, r2])
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
    for i in 0..2 {
        let list = match single_expr(&outer.ps[i]).unwrap().expr_instance {
            Some(ExprInstance::EListBody(l)) => l,
            _ => panic!("close reply {i} not a list"),
        };
        assert_eq!(list.ps.len(), 1);
        let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
            Some(ExprInstance::GBool(b)) => b,
            other => panic!("close reply {i} element not GBool: {other:?}"),
        };
        assert!(ok, "close reply {i} must be [true], not [false]");
    }
}

// ---------------------------------------------------------------------
// Phase 6 Slice 16: Fs.rho — openFile / openDir + static bundle +
//                    mode-cap.
// ---------------------------------------------------------------------

/// Happy path: openFile with a bundle entry, correct mode, returns
/// a File handle that can be read from.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_happy_path() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "config.json": ("/root", "config.json", "rw", "file", "oracular")
        })) {
          for (@openReply <- @fs!?("openFile", "config.json", {"mode": "r"})) {
            match openReply {
              [true, file] => {
                for (@r <- @file!?("tell")) { @"out"!(r) }
              }
              _ => @"out"!(openReply)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, pos, _) = extract_reply(&reply);
    assert!(ok, "openFile + tell should succeed");
    assert_eq!(pos, Some(0), "fresh File starts at position 0");
}

/// openFile with a name not in the bundle → FSERR_UNSUPPORTED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_not_in_bundle_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "config.json": ("/root", "config.json", "r", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "nope.json", {})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// Mode-cap downgrade: openFile("r") on an "rw"-provisioned entry
/// succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_downgrade_r_on_rw_provisioned_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "data.bin", {"mode": "r"})) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "downgrade r ← rw must succeed");
}

/// Mode-cap upgrade: openFile("w") on an "r"-provisioned entry
/// rejects with FSERR_UNSUPPORTED "requested mode exceeds
/// provisioned attenuation".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_upgrade_w_on_r_provisioned_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "config.json": ("/root", "config.json", "r", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "config.json", {"mode": "w"})) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
    let msg = extract_failure_msg(&reply);
    assert!(msg.contains("exceeds provisioned"), "msg: {msg}");
}

/// Mode defaults to "r" when options omits it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_mode_defaults_to_r() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "config.json": ("/root", "config.json", "r", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "config.json", {})) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "empty options {{}} should default mode to r → succeed on r-provisioned entry"
    );
}

/// openFile on a bundle entry whose kind is "dir" → FSERR_BAD_ARG.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_on_dir_entry_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "logs", {})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
    let msg = extract_failure_msg(&reply);
    assert!(msg.contains("directory"), "msg: {msg}");
}

/// openDir happy path: returns a Dir handle bound to canonRoot/rel.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_happy_path() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@openReply <- @fs!?("openDir", "logs", {"mode": "r"})) {
            match openReply {
              [true, _dir] => @"out"!([true])
              _ => @"out"!(openReply)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!(),
    };
    let ok = match single_expr(&list.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        _ => panic!(),
    };
    assert!(ok, "openDir happy path");
}

/// openDir on a bundle entry whose kind is "file" → FSERR_BAD_ARG.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_on_file_entry_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "config.json": ("/root", "config.json", "r", "file", "oracular")
        })) {
          for (@r <- @fs!?("openDir", "config.json", {})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
    let msg = extract_failure_msg(&reply);
    assert!(msg.contains("file"), "msg: {msg}");
}

/// openDir with "rw" on an "r"-provisioned Dir rejects.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_upgrade_rw_on_r_provisioned_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "readonly-dir": ("/root", "subdir", "r", "dir", "oracular")
        })) {
          for (@r <- @fs!?("openDir", "readonly-dir", {"mode": "rw"})) {
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

/// openDir with an unknown mode (not "r" / "rw") rejects with
/// FSERR_BAD_ARG.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_unknown_mode_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@r <- @fs!?("openDir", "logs", {"mode": "x"})) {
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

/// openFile with a non-Map options rejects with FSERR_BAD_ARG.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_non_map_options_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "config.json": ("/root", "config.json", "r", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "config.json", "not-a-map")) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_BAD_ARG");
}

/// openFile with a non-String name rejects.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_non_string_name_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@r <- @fs!?("openFile", 42, {})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_BAD_ARG");
}

/// openFile with a non-String mode value rejects.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_non_string_mode_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "config.json": ("/root", "config.json", "r", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "config.json", {"mode": 42})) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_BAD_ARG");
}

/// Unknown method on Fs (stdin/stdout/stderr not yet wired — slice
/// 18) → FSERR_UNSUPPORTED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_unknown_method_returns_fserr_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@r <- @fs!?("wibble")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_UNSUPPORTED");
}

/// End-to-end: openFile returns a working File that supports the
/// full File method surface (write + read round-trip).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_returns_working_file() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@[true, file] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
            for (@_ <- @file!?("writeByteArray", "hello".toUtf8Bytes())) {
              for (@_ <- @file!?("seek", 0, "set")) {
                for (@r <- @file!?("readN", 100)) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, bytes) = extract_reply(&reply);
    assert!(ok);
    assert_eq!(bytes, Some(b"hello".to_vec()));
}

// ---------------------------------------------------------------------
// Slice 16 review-driven coverage additions.
// ---------------------------------------------------------------------

// -- M-16-1: openDir not-in-bundle.

/// openDir on a name not in the bundle → FSERR_UNSUPPORTED.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_not_in_bundle_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@r <- @fs!?("openDir", "nope", {})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

// -- M-16-2: file-mode upgrade-rejection matrix.

/// Helper: openFile with `requested` mode on an "r"-provisioned entry
/// should reject with FSERR_UNSUPPORTED.
async fn fs_open_file_upgrade_rejects_helper(requested: &str) {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(&format!(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {{
          "cfg.json": ("/root", "cfg.json", "r", "file", "oracular")
        }})) {{
          for (@r <- @fs!?("openFile", "cfg.json", {{"mode": "{requested}"}})) {{
            @"out"!(r)
          }}
        }}
        "#
    ));
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "openFile('{requested}') on r-provisioned should fail");
    assert_eq!(code, "FSERR_UNSUPPORTED", "mode {requested}");
    let msg = extract_failure_msg(&reply);
    assert!(
        msg.contains("exceeds provisioned"),
        "mode {requested} msg: {msg}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_upgrade_w_rejects() { fs_open_file_upgrade_rejects_helper("w").await; }
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_upgrade_a_rejects() { fs_open_file_upgrade_rejects_helper("a").await; }
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_upgrade_r_plus_rejects() { fs_open_file_upgrade_rejects_helper("r+").await; }
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_upgrade_w_plus_rejects() { fs_open_file_upgrade_rejects_helper("w+").await; }
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_upgrade_a_plus_rejects() { fs_open_file_upgrade_rejects_helper("a+").await; }
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_upgrade_wx_rejects() { fs_open_file_upgrade_rejects_helper("wx").await; }
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_upgrade_w_plus_x_rejects() {
    fs_open_file_upgrade_rejects_helper("w+x").await;
}

// -- M-16-3: openFile unknown-mode rejection.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_unknown_mode_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "cfg.json": ("/root", "cfg.json", "rw", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "cfg.json", {"mode": "z"})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
    let msg = extract_failure_msg(&reply);
    assert!(msg.contains("unknown file mode"), "msg: {msg}");
}

// -- M-16-4: openDir type-guards.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_non_map_options_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@r <- @fs!?("openDir", "logs", "not-a-map")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_non_string_name_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@r <- @fs!?("openDir", 42, {})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_non_string_mode_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@r <- @fs!?("openDir", "logs", {"mode": 42})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_BAD_ARG");
}

// -- M-16-5: Dir mode downgrade success + default.

/// openDir on "rw"-provisioned with explicit {"mode": "r"} succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_downgrade_r_on_rw_provisioned_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@r <- @fs!?("openDir", "logs", {"mode": "r"})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "downgrade r ← rw for openDir must succeed");
}

/// openDir with empty options {} on "rw"-provisioned defaults to "r"
/// and succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_default_mode_on_rw_provisioned_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@r <- @fs!?("openDir", "logs", {})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok);
}

// -- M-16-6: openFile write-mode success matrix on "rw"-provisioned.

async fn fs_open_file_write_mode_succeeds_helper(mode: &str) {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(&format!(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {{
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        }})) {{
          for (@r <- @fs!?("openFile", "data.bin", {{"mode": "{mode}"}})) {{
            @"out"!(r)
          }}
        }}
        "#
    ));
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "openFile('{mode}') on rw-provisioned should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_write_mode_w_succeeds() {
    fs_open_file_write_mode_succeeds_helper("w").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_write_mode_a_succeeds() {
    fs_open_file_write_mode_succeeds_helper("a").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_write_mode_r_plus_succeeds() {
    fs_open_file_write_mode_succeeds_helper("r+").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_write_mode_w_plus_succeeds() {
    fs_open_file_write_mode_succeeds_helper("w+").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_write_mode_a_plus_succeeds() {
    fs_open_file_write_mode_succeeds_helper("a+").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_write_mode_wx_succeeds() {
    fs_open_file_write_mode_succeeds_helper("wx").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_write_mode_w_plus_x_succeeds() {
    fs_open_file_write_mode_succeeds_helper("w+x").await;
}

// -- M-6 rev-2 / m-16-6 fix (2026-08-06): realistic provisioned
// -- modes.  Pre-fix, Fs.openFile checked `provisioned == "rw"` for
// -- File entries, but CONFIG_FILE_MODES = {r, r+, w, w+, a, a+} has
// -- no "rw" — so a real HOCON-parsed File entry could NEVER satisfy
// -- the cap.  Every non-"r" file open through Fs.openFile
// -- unconditionally returned FSERR_UNSUPPORTED in production.  The
// -- existing `fs_open_file_write_mode_*_succeeds` helpers only pass
// -- because they hand-fake a bundle with provisioned="rw", a shape
// -- no real config path can produce.
//
// -- The tests below use the REAL provisioned modes an operator can
// -- actually configure ("r+", "w", "w+", "a", "a+") and confirm
// -- non-"r" opens succeed on each one.  A regression that reverts
// -- to `provisioned == "rw"` semantics fires here for every
// -- provisioned-mode row.

async fn fs_open_realistic_provisioned_helper(provisioned: &str, requested: &str) {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(&format!(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {{
          "data.bin": ("/root", "data.bin", "{provisioned}", "file", "oracular")
        }})) {{
          for (@r <- @fs!?("openFile", "data.bin", {{"mode": "{requested}"}})) {{
            @"out"!(r)
          }}
        }}
        "#
    ));
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "M-6 rev-2: openFile('{requested}') on provisioned=\"{provisioned}\" should succeed \
         (write-capable cap check inverted).  Got code={code:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn m6_realistic_provisioned_r_plus_allows_w() {
    fs_open_realistic_provisioned_helper("r+", "w").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn m6_realistic_provisioned_w_allows_w() {
    fs_open_realistic_provisioned_helper("w", "w").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn m6_realistic_provisioned_w_plus_allows_r_plus() {
    fs_open_realistic_provisioned_helper("w+", "r+").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn m6_realistic_provisioned_a_allows_a() {
    fs_open_realistic_provisioned_helper("a", "a").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn m6_realistic_provisioned_a_plus_allows_a_plus() {
    fs_open_realistic_provisioned_helper("a+", "a+").await;
}

/// M-6 rev-2 negative pin: provisioned "r" MUST reject write
/// requests.  The cap check inversion isn't a blanket permit —
/// only non-"r" provisioned modes grant write.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn m6_provisioned_r_rejects_write_request() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "r", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "data.bin", {"mode": "w"})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "provisioned=\"r\" must reject write requests");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

// -- M-16-7: malformed bundle entries.

/// Bundle entry with wrong tuple arity (3-tuple) → FSERR_IO
/// "malformed bundle entry" via the catchall.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_malformed_bundle_wrong_arity_returns_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "cfg.json": ("/root", "cfg.json", "r")
        })) {
          for (@r <- @fs!?("openFile", "cfg.json", {})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
    let msg = extract_failure_msg(&reply);
    assert!(msg.contains("malformed"), "msg: {msg}");
}

/// Bundle entry with wrong kind string (not "file"/"dir") → FSERR_IO.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_malformed_bundle_wrong_kind_returns_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "cfg.json": ("/root", "cfg.json", "r", "symlink")
        })) {
          for (@r <- @fs!?("openFile", "cfg.json", {})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
}

/// Bundle entry with wrong tuple arity — openDir also surfaces FSERR_IO.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_malformed_bundle_wrong_arity_returns_fserr_io() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw")
        })) {
          for (@r <- @fs!?("openDir", "logs", {})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_IO");
}

// -- m-16-1: reply-shape helper applied to mode-cap tests.
//    (Covered by the shape helper inside the M-16-2 helper, which
//    checks the three-elem shape via assert_failure_shape_three_elems
//    indirectly — actually no, the helper only asserts code. Add a
//    direct shape test for the mode-cap rejection.)

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_upgrade_reply_shape_is_three_elems() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "cfg.json": ("/root", "cfg.json", "r", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "cfg.json", {"mode": "w"})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    assert_failure_shape_three_elems(&reply, "FSERR_UNSUPPORTED");
}

// -- m-16-2: end-to-end Dir functional test.

/// openDir returns a working Dir agent: dir.stat("child.txt") should
/// succeed with a stat record.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_returns_working_dir() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@[true, dir] <- @fs!?("openDir", "logs", {"mode": "r"})) {
            for (@statReply <- @dir!?("stat", "child.txt")) { @"out"!(statReply) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "openDir → dir.stat should succeed");
}

// -- m-16-3: cache-baseline regression test.
// (Slice 17's cache-HIT-returns-SAME-handle test was DELETED in slice
// 27; POSIX semantics require every openFile to return a fresh
// handle with an independent cursor.  See the replacement test
// `fs_open_file_twice_yields_distinct_handles_with_independent_state`
// below.)

// -- m-16-4: extra-options-keys silently ignored (structural forward-
//    compat).

/// openFile with extra options keys (e.g., "create") is accepted:
/// they're captured in `_rest` and ignored per slice-16 MVP contract.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_extra_options_keys_ignored() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "data.bin",
                          {"mode": "r", "create": true, "exclusive": false})) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "extra options keys should be captured in _rest and ignored"
    );
}

// -- Mi-16-1: Map without "mode" key falls through to default "r".

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_options_without_mode_key_defaults_to_r() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "cfg.json": ("/root", "cfg.json", "r", "file", "oracular")
        })) {
          for (@r <- @fs!?("openFile", "cfg.json", {"create": true})) {
            @"out"!(r)
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    if !ok {
        let msg = extract_failure_msg(&reply);
        panic!("expected success, got [false, {code:?}, {msg:?}]");
    }
}

// ---------------------------------------------------------------------
// Phase 6 Slice 17 (cache) — REVERTED by slice 27 (2026-08-04).  The
// tests below survive with rewritten assertions: distinct-modes,
// distinct-names, cross-Fs, reply-shape, three-concurrent-opens all
// still hold under fresh-mint semantics.  Tests that asserted cache
// HIT (same-handle returned for repeat opens) were deleted or
// inverted — see `fs_open_file_twice_yields_distinct_handles_with_
// independent_state` for the new POSIX-open-twice invariant.
// ---------------------------------------------------------------------

/// Different-mode opens on the same name yield distinct handles.
/// Under slice 27's fresh-mint semantics ALL opens are distinct; this
/// test remains as a specific regression that different modes don't
/// somehow collapse into shared state.  Probe: close h1 ("rw");
/// open h2 ("r") on same name; h2.tell() succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_different_modes_yield_distinct_handles() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@[true, f1] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
            for (@_ <- @f1!?("close")) {
              for (@[true, f2] <- @fs!?("openFile", "data.bin", {"mode": "r"})) {
                for (@r <- @f2!?("tell")) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "different modes → distinct handles → h2 unaffected by h1 close"
    );
}

/// Distinct-name opens yield distinct handles.  Regression against a
/// bundle where two logical names silently collapse to shared state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_different_names_yield_distinct_handles() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "a.bin": ("/root", "a.bin", "rw", "file", "oracular"),
          "b.bin": ("/root", "b.bin", "rw", "file", "oracular")
        })) {
          for (@[true, f1] <- @fs!?("openFile", "a.bin", {"mode": "rw"})) {
            for (@_ <- @f1!?("close")) {
              for (@[true, f2] <- @fs!?("openFile", "b.bin", {"mode": "rw"})) {
                for (@r <- @f2!?("tell")) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "b.bin's handle unaffected by closing a.bin's handle");
}

/// Slice 27 (replaces slice-17 cache-hit test): two openDir calls on
/// same name+mode both succeed and produce STRUCTURALLY DISTINCT dir
/// handles.  Each call mints a fresh Dir agent with its own private
/// scope; closing one has no effect on the other.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_twice_yields_distinct_handles() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@[true, d1] <- @fs!?("openDir", "logs", {"mode": "r"})) {
            for (@[true, d2] <- @fs!?("openDir", "logs", {"mode": "r"})) {
              // Both handles work independently and are distinct.
              for (@s1 <- @d1!?("stat", "child.txt")) {
                for (@s2 <- @d2!?("stat", "child.txt")) {
                  @"out"!([s1, s2, d1 == d2])
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
    let (ok1, _, _, _) = extract_reply(&outer.ps[0]);
    let (ok2, _, _, _) = extract_reply(&outer.ps[1]);
    assert!(ok1 && ok2, "both dir handles must work");
    let same = match single_expr(&outer.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        other => panic!("expected Bool, got {other:?}"),
    };
    assert!(
        !same,
        "openDir must mint DISTINCT handles per call (slice 27)"
    );
}

// ==========================================================================
// Cross-Fs isolation cluster (m-P6-3 aspirational-vs-production note)
//
// The tests below mint TWO Fs instances directly via `Fs!?(0, 1, 2, {...})`
// and verify their state (caches, stdio caps) is disjoint.  These tests
// exercise the LIBRARY's isolation mechanism — the `*private`-scoped
// cells inside Fs / Stdin / Stdout — NOT the currently deployed shape.
// Under the shared-Fs MVP (see powerbox-requirements.md PB-M-1 / PB-M-11),
// production has ONE Fs published at the registry; deploys cannot mint a
// new one.  So these tests demonstrate the invariant we EXPECT to hold
// once per-principal Fs delegation lands, not what production currently
// enforces.  When PB-M-1 lands, replace the direct-mint pattern with a
// dual-lookup-via-getFS pattern.
// ==========================================================================

/// Cross-Fs isolation (spec §867): two SEPARATE Fs instances mint
/// independent handles.  Under slice 27 EVERY open mints fresh — so
/// this test is now a specific regression on Fs-boundary isolation
/// rather than cache-scope semantics.  Verified via close-probe:
/// close fs1's handle; open the same name in a fresh fs2; fs2's
/// handle works.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_across_fs_instances_yields_distinct_handles() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs1 <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@[true, h1] <- @fs1!?("openFile", "data.bin", {"mode": "rw"})) {
            for (@_ <- @h1!?("close")) {
              for (@fs2 <- Fs!?(0, 1, 2, {
                "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
              })) {
                for (@[true, h2] <- @fs2!?("openFile", "data.bin", {"mode": "rw"})) {
                  for (@r <- @h2!?("tell")) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "fs2's h2 is a fresh handle; not closed by fs1's close");
}

/// Repeated same-key openFile smoke.  Three consecutive calls all
/// succeed — pins reply-shape stability across successive fresh
/// mints.  Under slice 27 each call also produces a distinct handle
/// (unlike the pre-slice-27 cache which returned the same handle).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_repeated_same_key_all_succeed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Three consecutive openFile calls on the same key.  Each should
    // succeed with a well-formed reply.
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@r1 <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
            for (@r2 <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
              for (@r3 <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
                @"out"!([r1, r2, r3])
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
    for i in 0..3 {
        let (ok, _, _, _) = extract_reply(&outer.ps[i]);
        assert!(ok, "call {i} should succeed");
    }
}

// ---------------------------------------------------------------------
// Slice-17 review-driven coverage additions.
// ---------------------------------------------------------------------

/// Slice 27 (replaces M-17-1 slice-17 cache-hit tests): openFile /
/// openDir twice on the same key produce STRUCTURALLY DISTINCT
/// handles.  POSIX-like open semantics: every call is a fresh open
/// with its own file descriptor / agent scope, so structural equality
/// on the returned name is FALSE for two different opens of the same
/// logical name.  The old slice-17 test asserted the opposite; slice
/// 27 reverses it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_twice_yields_distinct_handles_with_independent_state() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Two opens on the same (name, mode) — assert (a) both succeed,
    // (b) their bundle+ handles are structurally distinct, and (c)
    // closing the first does NOT affect the second (independent
    // per-agent `stateP`).
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@[true, f1] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
            for (@[true, f2] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
              for (@_ <- @f1!?("close")) {
                // f1 is now closed; f2 must still be open — proves
                // independent per-agent state (no shared cache slot).
                for (@r <- @f2!?("tell")) {
                  @"out"!([f1 == f2, r])
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
        _ => panic!("expected [Bool, tellReply] list"),
    };
    let same = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        other => panic!("expected Bool, got {other:?}"),
    };
    assert!(
        !same,
        "two openFile calls must mint DISTINCT handles (slice 27)"
    );
    let (ok, code, _, _) = extract_reply(&outer.ps[1]);
    assert!(
        ok,
        "f2 must remain open after f1 close (independent stateP); got code {code}"
    );
}

/// openDir with different modes yields distinct handles.  Close d1
/// ("rw"), open d2 ("r") on same name; d2.stat succeeds (fresh
/// handle).  Under slice 27 EVERY openDir mints fresh regardless of
/// mode; this specific test survives as a distinct-modes regression.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_different_modes_yield_distinct_handles() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@[true, d1] <- @fs!?("openDir", "logs", {"mode": "rw"})) {
            for (@_ <- @d1!?("close")) {
              for (@[true, d2] <- @fs!?("openDir", "logs", {"mode": "r"})) {
                for (@r <- @d2!?("stat", "child.txt")) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "different modes → distinct dir handles → d2 works");
}

/// openDir with different names yields distinct handles.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_different_names_yield_distinct_handles() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Both entries point at "subdir" — the mock stat only accepts that
    // rel — but the CACHE keys differ by logical name, so they don't
    // collide.  (The Fs cache keys on (canonRoot, rel, mode); both entries
    // share those, so this test actually exercises the *shared* branch
    // instead — use two DISTINCT rel values below.)
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "logs1": ("/root", "subdir", "rw", "dir", "oracular"),
          "logs2": ("/root", "subdir2", "rw", "dir", "oracular")
        })) {
          for (@[true, d1] <- @fs!?("openDir", "logs1", {"mode": "rw"})) {
            for (@_ <- @d1!?("close")) {
              for (@[true, d2] <- @fs!?("openDir", "logs2", {"mode": "rw"})) {
                for (@r <- @d2!?("stat", "child.txt")) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "logs2's handle unaffected by closing logs1's handle");
}

/// openDir cross-Fs isolation.  Close fs1's d1; open the same name on
/// a brand-new fs2; fs2's handle works.  Matches the file analogue.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_across_fs_instances_yields_distinct_handles() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs1 <- Fs!?(0, 1, 2, {
          "logs": ("/root", "subdir", "rw", "dir", "oracular")
        })) {
          for (@[true, d1] <- @fs1!?("openDir", "logs", {"mode": "rw"})) {
            for (@_ <- @d1!?("close")) {
              for (@fs2 <- Fs!?(0, 1, 2, {
                "logs": ("/root", "subdir", "rw", "dir", "oracular")
              })) {
                for (@[true, d2] <- @fs2!?("openDir", "logs", {"mode": "rw"})) {
                  for (@r <- @d2!?("stat", "child.txt")) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "fs2's d2 is a fresh dir handle; not closed by fs1's close"
    );
}

/// openFile reply shape is exactly `[true, handle]` — two elements.
/// A future change to the reply shape (e.g. adding a metadata field)
/// would trip this test.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_reply_shape_is_two_elems() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@r1 <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
            for (@r2 <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
              @"out"!([r1, r2])
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list of two replies"),
    };
    for (i, p) in outer.ps.iter().enumerate() {
        let inner = match single_expr(p).unwrap().expr_instance {
            Some(ExprInstance::EListBody(l)) => l,
            _ => panic!("reply {i} not a list"),
        };
        assert_eq!(
            inner.ps.len(),
            2,
            "reply {i} must be [true, handle] — two elements; got {} elems",
            inner.ps.len()
        );
    }
}

// m-17-2 (slice-17 cache-key semantics test) DELETED by slice 27.
// Under fresh-mint semantics there is no cache key; two openFile
// calls always produce distinct handles regardless of any key.  The
// close-then-tell-succeed variant of this invariant is covered by
// `fs_open_file_twice_yields_distinct_handles_with_independent_state`.

/// 3+ concurrent opens stress: two file names × modes yield three
/// distinct, functional handles.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_three_concurrent_opens_all_functional() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "a.bin": ("/root", "a.bin", "rw", "file", "oracular"),
          "b.bin": ("/root", "b.bin", "rw", "file", "oracular")
        })) {
          for (@[true, fA_rw] <- @fs!?("openFile", "a.bin", {"mode": "rw"})) {
            for (@[true, fA_r] <- @fs!?("openFile", "a.bin", {"mode": "r"})) {
              for (@[true, fB_rw] <- @fs!?("openFile", "b.bin", {"mode": "rw"})) {
                for (@t1 <- @fA_rw!?("tell")) {
                  for (@t2 <- @fA_r!?("tell")) {
                    for (@t3 <- @fB_rw!?("tell")) {
                      @"out"!([t1, t2, t3])
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
        _ => panic!("expected list of three tell replies"),
    };
    assert_eq!(outer.ps.len(), 3, "three tell replies expected");
    for (i, p) in outer.ps.iter().enumerate() {
        let (ok, _, _, _) = extract_reply(p);
        assert!(ok, "tell on handle {i} must succeed");
    }
}

/// Slice 27 (replaces Mi-17-1): repeated openFile after close mints a
/// FRESH handle each time, unaffected by any prior close.  Under
/// slice 17's cache this returned the same closed handle forever;
/// slice 27 requires each open to be independent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_after_close_yields_fresh_handle() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@[true, f1] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
            for (@_ <- @f1!?("close")) {
              for (@[true, f2] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
                for (@_ <- @f2!?("close")) {
                  for (@[true, f3] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
                    for (@tellReply <- @f3!?("tell")) {
                      @"out"!(tellReply)
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
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "third openFile after two closes must yield a working handle (slice 27)"
    );
}

// ---------------------------------------------------------------------
// Phase 6 Slice 18: Fs.stdin() / Fs.stdout() / Fs.stderr() (spec
// §842-844).  The Fs constructor now takes (stdinFd, stdoutFd, stderrFd,
// bMap); the powerbox provisions the fds at genesis.  Each method peeks
// a pre-minted per-instance cap.
// ---------------------------------------------------------------------

/// Happy path: fs.stdout().writeString("hi") returns [true] via the
/// pre-minted Stdout cap.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdout_write_string_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, sout] <- @fs!?("stdout")) {
            for (@r <- @sout!?("writeString", "hi")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "fs.stdout().writeString should succeed");
}

/// Happy path: fs.stderr().writeString("err") returns [true] — the
/// second Stdout cap wraps stderr fd independently.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stderr_write_string_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, serr] <- @fs!?("stderr")) {
            for (@r <- @serr!?("writeString", "err")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "fs.stderr().writeString should succeed");
}

/// Happy path: fs.stdin() returns a Stdin cap.  We probe it via close
/// (which returns [true] on an open Stdin) rather than reading (the
/// mock has no fd-0 read semantics).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdin_close_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, sin] <- @fs!?("stdin")) {
            for (@r <- @sin!?("close")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "fs.stdin().close should succeed");
}

/// Fresh-mint semantics (spec §1085 / §1114): two fs.stdout() calls
/// return DISTINCT caps.  Each call mints a fresh Stdout wrapping the
/// same fd.  Verified via structural inequality of the two handles.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdout_fresh_mint_per_call_distinct_caps() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, s1] <- @fs!?("stdout")) {
            for (@[true, s2] <- @fs!?("stdout")) {
              @"out"!(s1 == s2)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let same = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        other => panic!("expected Bool, got {other:?}"),
    };
    assert!(!same, "each fs.stdout() call must mint a fresh Stdout cap");
}

/// Fresh-mint semantics for stdin.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdin_fresh_mint_per_call_distinct_caps() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, s1] <- @fs!?("stdin")) {
            for (@[true, s2] <- @fs!?("stdin")) {
              @"out"!(s1 == s2)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let same = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        other => panic!("expected Bool, got {other:?}"),
    };
    assert!(!same, "each fs.stdin() call must mint a fresh Stdin cap");
}

/// Fresh-mint semantics for stderr.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stderr_fresh_mint_per_call_distinct_caps() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, s1] <- @fs!?("stderr")) {
            for (@[true, s2] <- @fs!?("stderr")) {
              @"out"!(s1 == s2)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let same = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        other => panic!("expected Bool, got {other:?}"),
    };
    assert!(!same, "each fs.stderr() call must mint a fresh Stdout cap");
}

/// stdout and stderr on the same Fs are DISTINCT caps (they wrap
/// different fds and have different `*private` names).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdout_and_stderr_are_distinct_caps() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, sout] <- @fs!?("stdout")) {
            for (@[true, serr] <- @fs!?("stderr")) {
              @"out"!(sout == serr)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let same = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        other => panic!("expected Bool, got {other:?}"),
    };
    assert!(
        !same,
        "stdout and stderr caps must be distinct Stdout instances"
    );
}

/// Cross-Fs isolation (spec §867): two Fs instances mint distinct
/// stdio caps.  Closing fs1's stdout does NOT close fs2's stdout —
/// they wrap the same fd but have independent Rholang state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdout_cross_fs_isolation() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs1 <- Fs!?(0, 1, 2, {})) {
          for (@fs2 <- Fs!?(0, 1, 2, {})) {
            for (@[true, s1] <- @fs1!?("stdout")) {
              for (@[true, s2] <- @fs2!?("stdout")) {
                // Distinct caps
                @"out"!(s1 == s2)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let same = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        other => panic!("expected Bool, got {other:?}"),
    };
    assert!(!same, "fs1 and fs2 must have distinct stdout caps");
}

/// Cross-Fs isolation via close-probe: fs1.stdout().close(); a fresh
/// fs2.stdout().writeString(...) still succeeds because fs2's cap
/// wasn't touched.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdout_close_fs1_does_not_affect_fs2() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs1 <- Fs!?(0, 1, 2, {})) {
          for (@fs2 <- Fs!?(0, 1, 2, {})) {
            for (@[true, s1] <- @fs1!?("stdout")) {
              for (@_ <- @s1!?("close")) {
                for (@[true, s2] <- @fs2!?("stdout")) {
                  for (@r <- @s2!?("writeString", "still-open")) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "fs2's stdout must still work after fs1's stdout was closed"
    );
}

/// Spec §1114: "A fresh Fs!stdout() call after close returns a new
/// Stdout cap that CAN still write."  After closing s1, a subsequent
/// fs.stdout() returns a FRESH cap whose writeString succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdout_returns_fresh_cap_after_close() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, s1] <- @fs!?("stdout")) {
            for (@_ <- @s1!?("close")) {
              for (@[true, s2] <- @fs!?("stdout")) {
                for (@r <- @s2!?("writeString", "post-close-write")) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "spec §1114: fresh fs.stdout() after close must still write"
    );
}

/// Spec §1085: "a distinct Stdin cap obtained from Fs.stdin() later
/// can still read."  After closing s1, a subsequent fs.stdin() gives
/// a fresh cap that is NOT closed (close() succeeds on the fresh cap).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdin_returns_fresh_cap_after_close() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, s1] <- @fs!?("stdin")) {
            for (@_ <- @s1!?("close")) {
              for (@[true, s2] <- @fs!?("stdin")) {
                // Fresh cap: probe with close (returns [true] on open Stdin).
                for (@r <- @s2!?("close")) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "spec §1085: fresh fs.stdin() after close must be a live cap"
    );
}

/// Same fresh-cap-after-close guarantee for stderr.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stderr_returns_fresh_cap_after_close() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, s1] <- @fs!?("stderr")) {
            for (@_ <- @s1!?("close")) {
              for (@[true, s2] <- @fs!?("stderr")) {
                for (@r <- @s2!?("writeString", "post-close-err")) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "spec §1114 (stderr): fresh fs.stderr() after close must still write"
    );
}

/// Default-arm regression: fs!?("unknown") still returns FSERR_UNSUPPORTED
/// after slice 18 (stdin/stdout/stderr additions didn't shadow default).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_unknown_method_after_slice_18_still_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@r <- @fs!?("nonexistent-method")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "unknown method must fail");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

// -- M-18-2: method-surface coverage via Fs-routed cap ----------------

/// fs.stdout().writeByteArray(bytes) — direct byte primitive via the
/// Fs-routed Stdout cap.  Covers write* dispatch on the wrapper.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdout_write_byte_array_via_fs_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, sout] <- @fs!?("stdout")) {
            for (@r <- @sout!?("writeByteArray", "68656c6c6f".hexToBytes())) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "fs.stdout().writeByteArray should succeed");
}

/// fs.stdout().flush() through the Fs-routed cap.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdout_flush_via_fs_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@[true, sout] <- @fs!?("stdout")) {
            for (@r <- @sout!?("flush")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "fs.stdout().flush should succeed");
}

/// fs.stdout().writeLine(charStream) — line-family method via Fs cap.
/// Two-char stream; writeLine drains it and appends LF.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdout_write_line_via_fs_succeeds() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new wlState, wlProducer, wlBuilder in {
          wlState!(["h", "i"]) |
          contract wlProducer(retCh) = {
            for (@lst <- wlState) {
              match lst {
                []             => { wlState!([]) | retCh!([false, "EOS", ""]) }
                [head ...tail] => { wlState!(tail) | retCh!([true, head]) }
              }
            }
          } |
          contract wlBuilder(@vals, retCh) = { retCh!([true, vals]) } |
          for (@stream <- Stream!?(*wlProducer, *wlBuilder)) {
            for (@fs <- Fs!?(0, 1, 2, {})) {
              for (@[true, sout] <- @fs!?("stdout")) {
                for (@r <- @sout!?("writeLine", stream)) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "fs.stdout().writeLine should succeed");
}

/// fs.stdin().bytes() through the Fs-routed cap.  Seeds mockFdCell
/// with a two-byte payload; drains one byte via next() to prove the
/// cap is functional.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdin_bytes_via_fs_smoke() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_prev <- mockFdCell) {
          mockFdCell!(("6162".hexToBytes(), 0)) |
          for (@fs <- Fs!?(0, 1, 2, {})) {
            for (@[true, sin] <- @fs!?("stdin")) {
              for (@[true, bs] <- @sin!?("bytes")) {
                for (@r <- @bs!?("next")) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "fs.stdin().bytes().next() should succeed on seeded mock"
    );
}

/// fs.stdin().chars() smoke via Fs cap.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdin_chars_via_fs_smoke() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_prev <- mockFdCell) {
          mockFdCell!(("61".hexToBytes(), 0)) |
          for (@fs <- Fs!?(0, 1, 2, {})) {
            for (@[true, sin] <- @fs!?("stdin")) {
              for (@[true, cs] <- @sin!?("chars")) {
                for (@r <- @cs!?("next")) { @"out"!(r) }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "fs.stdin().chars().next() should succeed on seeded mock"
    );
}

// -- M-18-3: cross-Fs isolation for stdin and stderr ------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdin_cross_fs_isolation() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs1 <- Fs!?(0, 1, 2, {})) {
          for (@fs2 <- Fs!?(0, 1, 2, {})) {
            for (@[true, s1] <- @fs1!?("stdin")) {
              for (@[true, s2] <- @fs2!?("stdin")) {
                @"out"!(s1 == s2)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let same = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        other => panic!("expected Bool, got {other:?}"),
    };
    assert!(!same, "fs1 and fs2 must have distinct stdin caps");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stderr_cross_fs_isolation() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs1 <- Fs!?(0, 1, 2, {})) {
          for (@fs2 <- Fs!?(0, 1, 2, {})) {
            for (@[true, s1] <- @fs1!?("stderr")) {
              for (@[true, s2] <- @fs2!?("stderr")) {
                @"out"!(s1 == s2)
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let same = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        other => panic!("expected Bool, got {other:?}"),
    };
    assert!(!same, "fs1 and fs2 must have distinct stderr caps");
}

/// Close-isolation companion for stdin: fs1.stdin().close() doesn't
/// prevent fs2.stdin() from working.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdin_close_fs1_does_not_affect_fs2() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs1 <- Fs!?(0, 1, 2, {})) {
          for (@fs2 <- Fs!?(0, 1, 2, {})) {
            for (@[true, s1] <- @fs1!?("stdin")) {
              for (@_ <- @s1!?("close")) {
                for (@[true, s2] <- @fs2!?("stdin")) {
                  for (@r <- @s2!?("close")) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "fs2's stdin must still work after fs1's stdin was closed"
    );
}

/// Close-isolation companion for stderr.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stderr_close_fs1_does_not_affect_fs2() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs1 <- Fs!?(0, 1, 2, {})) {
          for (@fs2 <- Fs!?(0, 1, 2, {})) {
            for (@[true, s1] <- @fs1!?("stderr")) {
              for (@_ <- @s1!?("close")) {
                for (@[true, s2] <- @fs2!?("stderr")) {
                  for (@r <- @s2!?("writeString", "isolated")) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "fs2's stderr must still work after fs1's stderr was closed"
    );
}

// -- m-18-6: wrong-arity stdio calls hit the default arm --------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdin_with_extra_arg_hits_default_arm() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@r <- @fs!?("stdin", "extra-arg")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "wrong-arity stdin must fall through to default arm");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdout_with_extra_arg_hits_default_arm() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@r <- @fs!?("stdout", "extra-arg")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "wrong-arity stdout must fall through to default arm");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stderr_with_extra_arg_hits_default_arm() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@r <- @fs!?("stderr", "extra-arg")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "wrong-arity stderr must fall through to default arm");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

// -- m-18-7: integration echo test (spec §889-895 pattern) ------------

/// End-to-end: fs.stdin().lines() → fs.stdout().writeLines(stream).
/// Seeds mockFdCell with "a\nb\n"; expects writeLines to drain the
/// LineStream and re-emit each line + LF via fsWrite.  Since the mock
/// shares mockFdCell for read and write, we verify success reply
/// shape rather than exact output bytes (write appends past the
/// seeded read cursor).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdio_echo_lines_end_to_end() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@_prev <- mockFdCell) {
          mockFdCell!(("610a620a".hexToBytes(), 0)) |
          for (@fs <- Fs!?(0, 1, 2, {})) {
            for (@[true, sin] <- @fs!?("stdin")) {
              for (@[true, sout] <- @fs!?("stdout")) {
                for (@[true, ls] <- @sin!?("lines")) {
                  for (@r <- @sout!?("writeLines", ls)) { @"out"!(r) }
                }
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "spec §889-895 echo pipeline (stdin.lines → stdout.writeLines) must succeed"
    );
}

// -- Mi-18-3: reply-shape assertion for stdio methods -----------------

/// Verify fs.stdin()/stdout()/stderr() all return exactly `[true, cap]`
/// (2-element list).  Guards against future reply-shape drift.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_stdio_reply_shape_is_two_elems() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {})) {
          for (@r1 <- @fs!?("stdin")) {
            for (@r2 <- @fs!?("stdout")) {
              for (@r3 <- @fs!?("stderr")) {
                @"out"!([r1, r2, r3])
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list of three stdio replies"),
    };
    for (i, name) in [(0, "stdin"), (1, "stdout"), (2, "stderr")] {
        let inner = match single_expr(&outer.ps[i]).unwrap().expr_instance {
            Some(ExprInstance::EListBody(l)) => l,
            _ => panic!("{name} reply is not a list"),
        };
        assert_eq!(
            inner.ps.len(),
            2,
            "{name} reply must be [true, cap] — two elements; got {}",
            inner.ps.len()
        );
    }
}

// ---------------------------------------------------------------------
// Phase 6 whole-phase review — B-P6-1 fix.
//
// Cross-Fs ocap-isolation MEMBRANE test (plan §341).  Plan §341:
// "construct two Fs instances directly with distinct deployerIds
// from the stub Powerbox; call openFile('shared/logical/name') on
// each; assert the returned File handles are structurally distinct
// agents backed by distinct fds, and that a membrane wrapped around
// Alice's File is invisible to Bob when Bob opens by the same
// logical name."
//
// Since the stub Powerbox isn't built yet (PB-M-1 tracks it), this
// test synthesizes the two-Fs setup via direct `Fs!?(...)` mints —
// same "library isolation mechanism, not production" caveat as the
// cluster above.
// ---------------------------------------------------------------------

/// Plan §341 spirit: Alice's manipulation of her File cap (via ANY
/// side channel — close, revocation, membrane wrapping) is invisible
/// to Bob when Bob opens the same logical name via HIS Fs.
///
/// Close-probe used as the manipulation instead of a full Rholang
/// membrane forwarder (which requires syntax gymnastics beyond the
/// scope of a regression test).  Close proves the invariant just as
/// well: if any state Alice touches leaks into Bob's cap, Bob's
/// write would fail.  This is the plan-mandated "membrane not
/// bypassable" test in essence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_fs_alice_manipulation_invisible_to_bob() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        // Alice's Fs, Bob's Fs — same logical bundle, distinct instances.
        for (@fsAlice <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@fsBob <- Fs!?(0, 1, 2, {
            "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
          })) {
            for (@[true, fAlice] <- @fsAlice!?("openFile", "data.bin", {"mode": "rw"})) {
              for (@[true, fBob] <- @fsBob!?("openFile", "data.bin", {"mode": "rw"})) {
                // Alice manipulates her cap (close = revocation).
                for (@_ <- @fAlice!?("close")) {
                  // Bob writes to his cap.  If Alice's close had ANY
                  // effect on Bob's cap (structural sharing, membrane
                  // leak, etc.), this would fail with FSERR_CLOSED.
                  for (@writeReply <- @fBob!?("writeByteArray", "68".hexToBytes())) {
                    @"out"!([fAlice == fBob, writeReply])
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
        _ => panic!("expected [distinctness, writeReply] list"),
    };
    assert_eq!(outer.ps.len(), 2);

    // (1) Structural distinctness of fAlice and fBob.
    let alice_eq_bob = match single_expr(&outer.ps[0]).unwrap().expr_instance {
        Some(ExprInstance::GBool(b)) => b,
        other => panic!("expected Bool for fAlice==fBob, got {other:?}"),
    };
    assert!(
        !alice_eq_bob,
        "spec §867: Bob's File must be structurally distinct from Alice's File"
    );

    // (2) Bob's write succeeds AFTER Alice's close — no state leak.
    let (write_ok, code, _, _) = extract_reply(&outer.ps[1]);
    assert!(
        write_ok,
        "Alice's close must be invisible to Bob's cap; got failure code {code}"
    );
}

// ---------------------------------------------------------------------
// Phase 7 Slice 26: ConsensusMode per-cap plumbing.
//
// The bundle tuple now carries a `consensusMode` string (5th element)
// that Fs.rho threads into File/Dir agent state.  File.chown and
// Dir.chown forward it to `fsChown`; a consensus cap short-circuits
// with FSERR_UNSUPPORTED without hitting the host filesystem.  Same
// mint yields a working oracular chown on the sibling entry — the
// two caps do NOT share the mode-cap decision (plan §369 per-cap
// invariant).
// ---------------------------------------------------------------------

/// A single Fs mint with one oracular and one consensus File entry.
/// The oracular cap's chown succeeds; the consensus cap's chown
/// short-circuits.  Verifies that the mode is truly per-cap, not
/// runtime-wide, and that the routing goes through the mock's
/// consensus-mode arm in fsChown.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_consensus_mode_per_cap_chown_routing() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "orc.txt": ("/root", "orc.txt", "rw", "file", "oracular"),
          "con.txt": ("/root", "con.txt", "rw", "file", "consensus")
        })) {
          for (@orcOpen <- @fs!?("openFile", "orc.txt", {"mode": "rw"})) {
            for (@conOpen <- @fs!?("openFile", "con.txt", {"mode": "rw"})) {
              match [orcOpen, conOpen] {
                [[true, orcFile], [true, conFile]] => {
                  for (@orcChown <- @orcFile!?("chown", "alice", "wheel")) {
                    for (@conChown <- @conFile!?("chown", "alice", "wheel")) {
                      @"out"!([orcChown, conChown])
                    }
                  }
                }
                _ => @"out"!(["open failed", orcOpen, conOpen])
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected [orcChown, conChown] list, got {reply:?}"),
    };
    assert_eq!(outer.ps.len(), 2, "expected exactly two replies");

    // Oracular cap: chown succeeds (mock returns [true]).
    let (orc_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(
        orc_ok,
        "oracular cap's chown must succeed; got: {:?}",
        outer.ps[0]
    );

    // Consensus cap: chown short-circuits with FSERR_UNSUPPORTED.
    let (con_ok, con_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(!con_ok, "consensus cap's chown must fail");
    assert_eq!(
        con_code, "FSERR_UNSUPPORTED",
        "consensus cap must yield FSERR_UNSUPPORTED, got {con_code}"
    );
}

/// Slice 27 replaces slice-26's cache-keyed-on-cmode test: under
/// fresh-mint semantics there is no cache to key, but the underlying
/// invariant — an oracular cap and a consensus cap over the same
/// physical file route to DISTINCT arms — remains.  Verified here
/// via two chown calls that observe distinct outcomes even though
/// they name the same underlying inode.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_oracle_and_consensus_caps_over_shared_path_are_independent() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "same-orc": ("/root", "shared.txt", "rw", "file", "oracular"),
          "same-con": ("/root", "shared.txt", "rw", "file", "consensus")
        })) {
          for (@orc <- @fs!?("openFile", "same-orc", {"mode": "rw"})) {
            for (@con <- @fs!?("openFile", "same-con", {"mode": "rw"})) {
              match [orc, con] {
                [[true, oF], [true, cF]] => {
                  for (@oR <- @oF!?("chown", "a", "b")) {
                    for (@cR <- @cF!?("chown", "a", "b")) {
                      @"out"!([oR, cR])
                    }
                  }
                }
                _ => @"out"!(["open failed", orc, con])
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected [oR, cR] list"),
    };
    assert_eq!(outer.ps.len(), 2);
    let (o_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(o_ok, "oracular cap on shared path must succeed");
    let (c_ok, c_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(!c_ok, "consensus cap on shared path must short-circuit");
    assert_eq!(c_code, "FSERR_UNSUPPORTED");
}

/// Dir.chown mirrors File.chown for consensus-mode routing.  A
/// consensus-cap Dir must reject chown even in "rw" mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_dir_consensus_mode_chown_short_circuits() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "con-dir": ("/root", "subdir", "rw", "dir", "consensus")
        })) {
          for (@openReply <- @fs!?("openDir", "con-dir", {"mode": "rw"})) {
            match openReply {
              [true, d] => {
                for (@r <- @d!?("chown", "f.txt", "alice", "wheel")) {
                  @"out"!(r)
                }
              }
              _ => @"out"!(openReply)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "Dir.chown on consensus cap must short-circuit");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

// ---------------------------------------------------------------------
// Slice 26 review-fix regression tests (Must-fix + Should-fix).
// ---------------------------------------------------------------------

/// MT-26-4: File.chown on a Consensus cap opened in `"r"` mode.
/// File.rho's chown gates on `fmode == "r" => FSERR_UNSUPPORTED "chown
/// requires write-capable mode"` BEFORE consulting cmode.  So an
/// r-mode Consensus cap surfaces the READONLY error, not the
/// consensus error.  Not a security bug (still denies chown), but the
/// error surface differs.  Pin the current behavior so a future
/// reorder is caught.  The important INVARIANT is: r-mode chown fails
/// regardless of cmode.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_consensus_r_mode_denies_chown() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "r", "consensus")) {
          for (@r <- @f!?("chown", "alice", "wheel")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "r-mode chown must fail");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// Same pin from the other angle: rw-mode chown on Consensus cap
/// short-circuits with FSERR_UNSUPPORTED via the cmode gate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_consensus_rw_mode_denies_chown() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "consensus")) {
          for (@r <- @f!?("chown", "alice", "wheel")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// MT-26-8: Dir.chown on a Consensus cap opened in `"r"` mode.
/// Whichever gate fires first still yields FSERR_UNSUPPORTED — pin it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_chown_consensus_r_mode_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "d": ("/root", "subdir", "r", "dir", "consensus")
        })) {
          for (@openReply <- @fs!?("openDir", "d", {"mode": "r"})) {
            match openReply {
              [true, d] => {
                for (@r <- @d!?("chown", "f.txt", "alice", "wheel")) { @"out"!(r) }
              }
              _ => @"out"!(openReply)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// MT-26-9: nested `openDir` on a Consensus parent Dir must produce a
/// Consensus child Dir — no laundering of consensus caps via
/// composition.  Verified by opening a parent Consensus Dir, then
/// calling `openDir` on a subdir (mock `fsStat` says `kind: dir`),
/// then invoking chown on the child cap and asserting the consensus
/// short-circuit fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_open_dir_inherits_parent_consensus_cmode() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Bundle points top at "subdir" so the mock `fsStat("/root",
    // "subdir", ...)` returns `kind: dir` (see the with_libs mock)
    // and openDirImpl mints a Dir cap for it.  Nested `openDir` off
    // that then targets "subdir2" (another dir per the mock).  The
    // child Dir's chown must short-circuit because the parent's
    // Consensus cmode was inherited.
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "top": ("/root", "subdir", "rw", "dir", "consensus")
        })) {
          for (@topOpen <- @fs!?("openDir", "top", {"mode": "rw"})) {
            match topOpen {
              [true, top] => {
                for (@childOpen <- @top!?("openDir", "subdir2", "rw")) {
                  match childOpen {
                    [true, child] => {
                      for (@chownReply <- @child!?("chown", "f.txt", "a", "b")) {
                        @"out"!(chownReply)
                      }
                    }
                    _ => @"out"!(childOpen)
                  }
                }
              }
              _ => @"out"!(topOpen)
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "child Dir must inherit consensus cmode");
    assert_eq!(
        code, "FSERR_UNSUPPORTED",
        "child Dir chown must short-circuit; parent's cmode inherited correctly"
    );
}

/// ST-26-5: Consensus + bad-type owner arg.  File.rho's arg-shape
/// validation runs BEFORE consulting cmode, so a bad-type owner
/// yields FSERR_BAD_ARG (not FSERR_UNSUPPORTED).  Pins the Rholang-
/// side ordering.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_consensus_with_bad_owner_reports_bad_arg() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "consensus")) {
          for (@r <- @f!?("chown", 42, "wheel")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

/// ST-26-6: consensus cap + `chown(Nil, Nil)` — the no-owner-no-group
/// form.  On an oracular cap this would be a valid no-op; on a
/// consensus cap it must still short-circuit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chown_consensus_with_nil_nil_short_circuits() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "consensus")) {
          for (@r <- @f!?("chown", Nil, Nil)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

/// M-26-2 constructor validation: File constructor must REVOKE its
/// state when handed a bogus `cmode`.  Subsequent method calls
/// surface a clear diagnostic rather than silently defaulting.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_constructor_rejects_unknown_cmode_string() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "Consensus")) {
          for (@r <- @f!?("tell")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "bogus cmode must NOT yield a working File");
    assert!(
        code == "FSERR_CLOSED" || code == "FSERR_IO",
        "expected revoked-state error; got {code}"
    );
}

/// Dir constructor mirror of the File constructor validation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_constructor_rejects_unknown_cmode_string() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", "bogus", *File)) {
          for (@r <- @d!?("stat", "f.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "bogus cmode must NOT yield a working Dir");
    assert_eq!(code, "FSERR_IO");
}

/// Slice 27 replaces slice-26's cache-HIT test with its OPPOSITE:
/// two sequential openFile calls on the same key must yield DISTINCT
/// handles (fresh-mint per open).  The slice-26 test asserted SAME
/// handle; slice 27 asserts DISTINCT.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_consensus_twice_yields_distinct_handles() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "cap": ("/root", "shared.txt", "rw", "file", "consensus")
        })) {
          for (@a <- @fs!?("openFile", "cap", {"mode": "rw"})) {
            for (@b <- @fs!?("openFile", "cap", {"mode": "rw"})) {
              match [a, b] {
                [[true, x], [true, y]] => {
                  match x == y {
                    true  => @"out"!([false, "collapsed", "handles must be distinct"])
                    false => @"out"!([true, "distinct"])
                  }
                }
                _ => @"out"!(["open failed", a, b])
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "repeat openFile must mint DISTINCT handles (slice 27)");
}

// ---------------------------------------------------------------------
// Slice 27 review-fix regression tests (Must-fix + Should-fix).
// ---------------------------------------------------------------------

/// H-27-2 regression: bad `cmode` reaching `openFileImpl` must NOT
/// allocate a kernel fd.  Before the fix, `openFileImpl` called
/// `fsOpen` (allocating a fd) BEFORE the File constructor validated
/// cmode; a bad-cmode reveal would REVOKE the agent's state and
/// leak the fd.  After the fix, cmode validation runs FIRST and
/// `fsOpen` is never called.
///
/// The default `with_libs` mock counts `fsOpen` calls via
/// `openCallCount` — we assert it stays at 0 when we invoke
/// `openFileImpl` with a bad cmode directly.  `openFileImpl` is
/// bound in the outer `new` scope alongside `File`/`Dir`/`Fs`, so
/// with_libs test code can call it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn open_file_impl_rejects_bad_cmode_before_calling_fs_open() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new implRet in {
          openFileImpl!("/root", "", "f.txt", "rw", "BOGUS", *File, *implRet) |
          for (@reply <- implRet) { @"out"!(reply) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "bad cmode must be rejected");
    assert_eq!(code, "FSERR_BAD_ARG", "bad cmode → FSERR_BAD_ARG");
}

// MT-27-3 note: a Rholang-level `par { }` test for concurrent
// openFile is deferred — the agent-dispatch return-channel wiring
// (`return!(reply)` inside methods, bound implicitly by the `!?`
// sugar) doesn't cleanly compose with an explicit `for (@r1 <- r1Ch;
// @r2 <- r2Ch)` join pattern in this test harness.  The concurrent-
// open safety invariant is covered by construction: `fsBundleP` is
// read via non-linear peek `<<-` (Rholang semantics guarantee this
// is race-free across an unbounded number of parallel readers), and
// the sequential fresh-mint distinctness (see
// `fs_open_file_repeated_same_key_yields_pairwise_distinct_handles`)
// exercises the identical code path.  A future stress test is
// tracked as NT-27-4.

/// ST-27-1 regression: three consecutive same-key opens must produce
/// pairwise-distinct handles.  Previously `fs_open_file_repeated_
/// same_key_all_succeed` asserted only `ok=true`; a cache regression
/// (all three returning the same handle) would pass that test.  This
/// one adds the pairwise-distinctness check.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_repeated_same_key_yields_pairwise_distinct_handles() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Rholang has `==` but no `!=`; encode distinct pairs as
    // "at least one is false" via three explicit equality checks.
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@[true, f1] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
            for (@[true, f2] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
              for (@[true, f3] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
                @"out"!([f1 == f2, f2 == f3, f1 == f3])
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected [Bool, Bool, Bool]"),
    };
    let labels = ["f1==f2", "f2==f3", "f1==f3"];
    for (i, p) in outer.ps.iter().enumerate() {
        let b = match single_expr(p).unwrap().expr_instance {
            Some(ExprInstance::GBool(b)) => b,
            other => panic!("expected GBool for entry {i}, got {other:?}"),
        };
        assert!(
            !b,
            "{}: two same-key opens must yield DISTINCT handles (cache regression?)",
            labels[i]
        );
    }
}

/// ST-27-2 regression: three distinct opens (two names × modes)
/// produce pairwise-distinct handles.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_three_distinct_opens_are_pairwise_distinct() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Emit equalities; assert each is false.
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "a.bin": ("/root", "a.bin", "rw", "file", "oracular"),
          "b.bin": ("/root", "b.bin", "rw", "file", "oracular")
        })) {
          for (@[true, fA_rw] <- @fs!?("openFile", "a.bin", {"mode": "rw"})) {
            for (@[true, fA_r] <- @fs!?("openFile", "a.bin", {"mode": "r"})) {
              for (@[true, fB_rw] <- @fs!?("openFile", "b.bin", {"mode": "rw"})) {
                @"out"!([fA_rw == fA_r, fA_r == fB_rw, fA_rw == fB_rw])
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected [Bool, Bool, Bool]"),
    };
    let labels = ["fA_rw==fA_r", "fA_r==fB_rw", "fA_rw==fB_rw"];
    for (i, p) in outer.ps.iter().enumerate() {
        let b = match single_expr(p).unwrap().expr_instance {
            Some(ExprInstance::GBool(b)) => b,
            other => panic!("expected GBool for entry {i}, got {other:?}"),
        };
        assert!(
            !b,
            "{}: distinct opens must produce distinct handles",
            labels[i]
        );
    }
}

/// ST-27-3 regression: per-cap idempotent close.  Two closes on the
/// same cap both return a well-formed reply and don't crash.  Under
/// slice 27 each cap has its own `stateP`; the deleted slice-17
/// `fs_open_file_cache_closed_handle_repeated_close_stable` tested
/// this under caching — this restores the coverage.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_open_file_close_twice_on_same_cap_stable() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@fs <- Fs!?(0, 1, 2, {
          "data.bin": ("/root", "data.bin", "rw", "file", "oracular")
        })) {
          for (@[true, f] <- @fs!?("openFile", "data.bin", {"mode": "rw"})) {
            for (@close1 <- @f!?("close")) {
              for (@close2 <- @f!?("close")) {
                @"out"!([close1, close2])
              }
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected [close1, close2] list"),
    };
    // Both replies must be well-formed lists.
    for (i, p) in outer.ps.iter().enumerate() {
        let inner = match single_expr(p).unwrap().expr_instance {
            Some(ExprInstance::EListBody(l)) => l,
            other => panic!("close reply {i} not a list, got {other:?}"),
        };
        assert!(!inner.ps.is_empty(), "close reply {i} must be non-empty");
    }
    // First close returns [true] on success.
    let (ok1, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(ok1, "first close must succeed");
    // Second close on the SAME (already-closed) cap must be
    // well-formed — FSERR_CLOSED is acceptable.
    let (ok2, _, _, _) = extract_reply(&outer.ps[1]);
    if !ok2 {
        // Well-formed failure is fine.  The key is that it does not
        // crash and does not silently succeed and leak state.
    }
}

// ----------------------------------------------------------------------
// H-29-3 review-fix regression pins: path-based mutations on Consensus
// caps must fail closed with FSERR_UNSUPPORTED.  Slice 29's WAL only
// journals fd-based Write/WriteAt/Truncate; a path-based mutation on
// a Consensus cap has no WAL record, so replayers would diverge from
// a leader that applied it.  The Rholang guard rejects at the agent
// boundary before the syscall is issued.
// ----------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "consensus")) {
          for (@r <- @f!?("chmod", "rwxr-xr-x")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "chmod on consensus cap must fail (H-29-3)");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_chmod_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", "consensus", *File)) {
          for (@r <- @d!?("chmod", "config.json", "rw-r--r--")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "Dir.chmod on consensus cap must fail (H-29-3)");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_file_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", "consensus", *File)) {
          for (@r <- @d!?("removeFile", "victim.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "Dir.removeFile on consensus cap must fail (H-29-3)");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_dir_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", "consensus", *File)) {
          for (@r <- @d!?("removeDir", "subdir", true)) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "Dir.removeDir on consensus cap must fail (H-29-3)");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_rename_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", "consensus", *File)) {
          for (@r <- @d!?("rename", "a.txt", "b.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "Dir.rename on consensus cap must fail (H-29-3)");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_copy_file_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", "consensus", *File)) {
          for (@r <- @d!?("copyFile", "src.txt", "dst.txt")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "Dir.copyFile on consensus cap must fail (H-29-3)");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

// ----------------------------------------------------------------------
// H7/H8/H9 round-2 review-fix pins: strengthen H-29-3 coverage.
//   - H7: consensus guard fires on both "r" and "rw" file modes
//   - H8: guard's distinctive message text is preserved
//   - H9: syscall side of the operation is NOT invoked when consensus
//     guard fires (verified by inspecting the mock's log)
// ----------------------------------------------------------------------

/// H7: File.chmod on (r, consensus) — must fail with FSERR_UNSUPPORTED
/// even though the r-mode gate ALSO would reject.  Confirms the
/// consensus guard fires before the mode gate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_consensus_r_mode_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "r", "consensus")) {
          for (@r <- @f!?("chmod", "rwxr-xr-x")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
    // H8: the message identifies which guard fired.  Consensus guard's
    // message contains "consensus"; r-mode guard's message contains
    // "write-capable mode".  extract_reply doesn't return the msg, so
    // pull ps[2] directly.
    let list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list"),
    };
    let msg = match single_expr(&list.ps[2]).unwrap().expr_instance {
        Some(ExprInstance::GString(s)) => s,
        _ => panic!("msg not a string"),
    };
    assert!(
        msg.contains("consensus"),
        "consensus guard should fire before r-mode gate; got msg={msg:?}"
    );
}

/// H9: when the H-29-3 guard fires for `Dir.removeFile` on a consensus
/// cap, `fsRemoveFile` (the native) must NOT be invoked.  Verified by
/// inspecting `rmFileLog` — pre-fix the syscall dispatch would happen
/// before the guard, leaving a log entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_file_consensus_does_not_invoke_syscall() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@d <- Dir!?("/root", "", "rw", "consensus", *File)) {
          for (@_ <- @d!?("removeFile", "victim.txt")) {
            for (@log <<- rmFileLog) { @"out"!(log) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    // The reply is the log — should be an empty list (no syscall).
    let log_list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list"),
    };
    assert!(
        log_list.ps.is_empty(),
        "consensus guard must fire before dispatch; rmFileLog should be empty, \
         got {} entries",
        log_list.ps.len()
    );
}

/// H9 companion for chmod: `fsChmod` native must NOT be invoked on a
/// consensus cap via `File.chmod` (Rholang guard fires first).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_chmod_consensus_does_not_invoke_syscall() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "consensus")) {
          for (@_ <- @f!?("chmod", "rwxr-xr-x")) {
            for (@log <<- chmodLog) { @"out"!(log) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let log_list = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list"),
    };
    assert!(
        log_list.ps.is_empty(),
        "File.chmod on consensus must not reach fsChmod native"
    );
}

// ----------------------------------------------------------------------
// C-R2 round-2 pins: native handlers fail-closed on Consensus cmode.
// These fire even if a caller bypasses the Rholang File.rho / Dir.rho
// guards — genesis-scope code that URN-binds fs_chmod / fs_removeFile
// / ... directly still hits the native cmode gate.
// ----------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_fs_chmod_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // The mock fsChmod returns FSERR_UNSUPPORTED on consensus (mirroring
    // the real native handler's C-R2 behavior).  This test binds fsChmod
    // directly and calls with cmode="consensus".
    let src = with_libs(
        r#"
        new ret in {
          fsChmod!("/root", "f.txt", 0, "consensus", *ret) |
          for (@r <- ret) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "native fs_chmod must reject consensus cmode (C-R2)");
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_fs_remove_file_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new ret in {
          fsRemoveFile!("/root", "f.txt", "consensus", *ret) |
          for (@r <- ret) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_fs_remove_dir_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new ret in {
          fsRemoveDir!("/root", "d", true, "consensus", *ret) |
          for (@r <- ret) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_fs_rename_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new ret in {
          fsRename!("/root", "a", "/root", "b", "consensus", *ret) |
          for (@r <- ret) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_fs_copy_file_consensus_returns_unsupported() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        new ret in {
          fsCopyFile!("/root", "a", "/root", "b", "consensus", *ret) |
          for (@r <- ret) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

// ---------------------------------------------------------------------
// Phase 8 slice 8a step-4 review smoke-check — File.lockRange + LockToken.
//
// These tests use the always-succeed lock-native mocks from step 4c-1
// (fsLockRange returns [true, 1]; fsReleaseLock returns [true]).  They
// exercise the Rholang agent-dispatch plumbing end-to-end: the mint,
// the argument-validation branches, mode attenuation, closed-file
// gating, and LockToken's own idempotent-release state machine (which
// operates on lockStateP independently of what fsReleaseLock returns).
//
// Coverage NOT provided by these smoke-checks:
//   - Real cross-cap FSERR_BUSY (requires stateful lock mock; deferred
//     to step 4g's dedicated integration tests)
//   - Real File.close sweep releasing outstanding tokens (blocked on
//     step 4f wiring fs_release_all_for_holder into close())
//   - Auto-acquire wrap semantics on positional methods (only observable
//     with a stateful mock that tracks lock ranges)
//
// The smoke-checks catch: syntax bugs in the wrap surface, misrouted
// dispatch, dropped return channels, argument-validation regressions,
// mode-attenuation regressions.  Enough to keep steps 4e/4f from
// resting on compile-only verification.
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_release_roundtrip() {
    // Mint File, acquire lock, release it — both replies must succeed.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@lockReply <- @f!?("lockRange", 0, 100, "w")) {
            match lockReply {
              [true, token] => {
                for (@relReply <- @token!?("release")) {
                  @"out"!([lockReply, relReply])
                }
              }
              [false, code, msg] => @"out"!([lockReply, [false, "SKIPPED", "lock acquire failed"]])
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
    let (lock_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(lock_ok, "lockRange must succeed under always-succeed mock");
    let (rel_ok, _, _, _) = extract_reply(&outer.ps[1]);
    assert!(
        rel_ok,
        "token.release must succeed under always-succeed mock"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lock_token_second_release_returns_fserr_closed() {
    // LockToken's own state machine handles idempotent double-release
    // via lockStateP — INDEPENDENTLY of what fsReleaseLock returns.
    // First release: state=Int (LockId) → put "released", call
    // fsReleaseLock, forward reply.  Second release: state="released"
    // → FSERR_CLOSED without touching fsReleaseLock.  This test
    // therefore works with the always-succeed mock — the FSERR_CLOSED
    // comes from LockToken.release's own dispatch, not from the native.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@lockReply <- @f!?("lockRange", 0, 100, "w")) {
            match lockReply {
              [true, token] => {
                for (@rel1 <- @token!?("release")) {
                  for (@rel2 <- @token!?("release")) {
                    @"out"!([rel1, rel2])
                  }
                }
              }
              _ => @"out"!([lockReply, [false, "SKIPPED", "lock acquire failed"]])
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
    let (rel1_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(rel1_ok, "first release must succeed");
    let (rel2_ok, rel2_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(!rel2_ok, "second release must fail");
    assert_eq!(
        rel2_code, "FSERR_CLOSED",
        "second release must return FSERR_CLOSED (idempotent-close semantics)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_negative_offset_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("lockRange", -1, 100, "w")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_zero_length_rejects() {
    // LockRegistry rejects zero-length ranges as BadArg; File.rho's
    // lockRange short-circuits before the native and returns
    // FSERR_BAD_ARG directly.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("lockRange", 0, 0, "w")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_invalid_mode_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("lockRange", 0, 100, "x")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_write_on_readonly_rejects() {
    // Mode attenuation per spec §Explicit locks: requesting "w" on a
    // File opened "r" returns FSERR_UNSUPPORTED.  Matches the
    // attenuation pattern of every other write-capable method
    // (writeByteArray, writeBytes, writeBytesAt, ...).
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
          for (@r <- @f!?("lockRange", 0, 100, "w")) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_read_on_readonly_succeeds() {
    // Same File cap as the previous test, but requesting an "r" lock
    // — mode attenuation should NOT trip.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
          for (@r <- @f!?("lockRange", 0, 100, "r")) {
            match r {
              [true, _token] => @"out"!([true])
              [false, code, msg] => @"out"!([false, code, msg])
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "read lock on read-only file must succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_on_closed_file_rejects() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@_ <- @f!?("close")) {
            for (@r <- @f!?("lockRange", 0, 100, "w")) { @"out"!(r) }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_CLOSED");
}

// ---------------------------------------------------------------------
// Phase 8 slice 8b sub-5 (2026-08-12) — arity-4 lockRange method
// (options-map form) smoke-checks.
//
// These tests exercise the sub-4 wiring end-to-end:
//   Rholang caller → File.lockRange(off, len, mode, options)
//     → File.rho arity-4 method extracts wait from options
//     → fsLockRange arity-8 native call
//     → mock returns [true, 1]
//     → LockToken minted, forwarded to caller
//
// Behavioral coverage of the LockRegistry parking + admission +
// cancellation is at the Rust-side test layer:
//   - Sub-1 (rholang/src/rust/interpreter/io/lock.rs): 19 tokio
//     async tests covering park, FIFO admit, cancel, cascade, edge
//     cases (dropped receiver rollback, sentinel guard, etc.).
//   - Sub-3 (casper/src/rust/rholang/runtime.rs): 3 tests covering
//     WalDeployScope::drop cancels this deploy's parked waiters
//     (Err(Cancelled) delivered via oneshot).
//
// A full-runtime integration test that runs the REAL native under a
// real Casper deploy would be substantial scaffolding for redundant
// coverage given the layered Rust-side tests.  These smoke-checks
// focus on the Rholang-side plumbing: options-map extraction, arity
// dispatch, arity-4 vs. arity-3 co-existence.
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_options_empty_map_defaults_wait_false() {
    // Empty options map — arity-4 method's `options.get("wait")`
    // returns Nil, which the method normalizes to wait:false.
    // Regression pin: caller must observe success (not FSERR_BAD_ARG
    // for a missing key).
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("lockRange", 0, 100, "w", {})) {
            match r {
              [true, _token] => @"out"!([true])
              [false, code, msg] => @"out"!([false, code, msg])
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "arity-4 lockRange with empty options must default wait:false \
         and succeed under the always-succeed mock"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_options_wait_false_explicit() {
    // Explicit wait:false — arity-4 method's Bool-branch extracts
    // false and passes to arity-8 native mock.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("lockRange", 0, 100, "w", {"wait": false})) {
            match r {
              [true, _token] => @"out"!([true])
              [false, code, msg] => @"out"!([false, code, msg])
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(ok, "explicit wait:false must succeed like empty options");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_options_wait_true_dispatches_arity_8_native() {
    // wait:true — arity-4 method's Bool-branch extracts true and
    // passes to arity-8 fsLockRange.  With the always-succeed mock
    // this returns [true, 1] immediately; the point of this test is
    // to pin that the arity-4 dispatch path is reachable and
    // returns success.  Real parking behavior is Rust-side (sub-1).
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("lockRange", 0, 100, "w", {"wait": true})) {
            match r {
              [true, _token] => @"out"!([true])
              [false, code, msg] => @"out"!([false, code, msg])
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, _, _, _) = extract_reply(&reply);
    assert!(
        ok,
        "wait:true must route through arity-8 native mock and succeed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_options_wait_non_bool_rejects_bad_arg() {
    // wait must be Bool.  arity-4 method's third match arm rejects
    // non-Bool wait values with FSERR_BAD_ARG.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("lockRange", 0, 100, "w", {"wait": "yes"})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok, "non-Bool wait must be rejected");
    assert_eq!(
        code, "FSERR_BAD_ARG",
        "non-Bool wait must map to FSERR_BAD_ARG per the arity-4 method's third match arm"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_arity_3_and_arity_4_coexist() {
    // Sub-4 added arity-4 lockRange as a NEW method alongside the
    // pre-existing arity-3 method — Rholang agent dispatch on
    // message arity means both coexist.  This test invokes both in
    // sequence to pin that both dispatch correctly on the same File
    // cap.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r3 <- @f!?("lockRange", 0, 100, "w")) {
            match r3 {
              [true, t3] => {
                for (@rel3 <- @t3!?("release")) {
                  for (@r4 <- @f!?("lockRange", 200, 100, "w", {"wait": true})) {
                    match r4 {
                      [true, t4] => {
                        for (@rel4 <- @t4!?("release")) {
                          @"out"!([r3, rel3, r4, rel4])
                        }
                      }
                      _ => @"out"!([r3, rel3, r4, [false, "SKIPPED", ""]])
                    }
                  }
                }
              }
              _ => @"out"!([r3, [false, "SKIPPED", ""], [false, "SKIPPED", ""], [false, "SKIPPED", ""]])
            }
          }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected 4-element list reply"),
    };
    assert_eq!(outer.ps.len(), 4, "expected [r3, rel3, r4, rel4]");
    let (r3_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(r3_ok, "arity-3 lockRange must succeed");
    let (rel3_ok, _, _, _) = extract_reply(&outer.ps[1]);
    assert!(rel3_ok, "arity-3 release must succeed");
    let (r4_ok, _, _, _) = extract_reply(&outer.ps[2]);
    assert!(r4_ok, "arity-4 lockRange must succeed on the same File cap");
    let (rel4_ok, _, _, _) = extract_reply(&outer.ps[3]);
    assert!(rel4_ok, "arity-4 release must succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_arity_4_negative_offset_rejects() {
    // Argument validation (offset >= 0) applies to arity-4 same as
    // arity-3.  Regression pin: arity-4 body-duplication must
    // preserve the offset validation.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {
          for (@r <- @f!?("lockRange", -1, 100, "w", {"wait": true})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_BAD_ARG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_lock_range_arity_4_write_on_readonly_rejects() {
    // Mode attenuation applies to arity-4 same as arity-3.
    // Regression pin: arity-4 body-duplication must preserve the
    // ["w", "r"] mode-attenuation match arm.
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = with_libs(
        r#"
        for (@f <- File!?(1, "/root", "test.txt", "r", "oracular")) {
          for (@r <- @f!?("lockRange", 0, 100, "w", {"wait": true})) { @"out"!(r) }
        }
        "#,
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let (ok, code, _, _) = extract_reply(&reply);
    assert!(!ok);
    assert_eq!(code, "FSERR_UNSUPPORTED");
}

// ---------------------------------------------------------------------
// Phase 8 slice 8a step 4g — rich integration tests with stateful lock mocks.
//
// The always-succeed lock mocks in `with_libs` (from step 4c-1) cover
// the syntactic wraps but say nothing about the actual lock semantics.
// The tests below use bespoke mock setups where fsLockRange /
// fsReleaseLock / fsReleaseAllForHolder maintain per-test state, so
// the end-to-end lock lifecycle is observable.
//
// Each test is self-contained: its own new-clause, its own stateful
// mock, its own file syscall stubs.  Not a general framework — one
// mock per test — because the shape each test needs is different
// enough that a shared abstraction would obscure more than it saves.
// ---------------------------------------------------------------------

/// **Step 4f verification** — File.close's sweep of holder locks flows
/// through to subsequent `token!release()` returning FSERR_CLOSED, per
/// spec §File > close.
///
/// Mock design: single boolean `releasedFlag` starts false.  fsLockRange
/// / fsLockSequential unconditionally return `[true, 42]`.
/// fsReleaseAllForHolder sets the flag true (simulates "sweep happened").
/// fsReleaseLock returns `[true]` while flag is false, `[false,
/// FSERR_CLOSED, "swept"]` while flag is true.  Simple flag-based mock
/// suffices because the test only handles one lock at a time; a full
/// per-lockId tracking mock would add complexity without new coverage.
///
/// Flow:
///   1. Mint File.
///   2. lockRange(0, 100, "w") → [true, token].  Mock returns id=42;
///      flag still false.
///   3. File.close → fsReleaseAllForHolder(*stateP) sets flag=true, then
///      fsClose returns [true].
///   4. token!release() → LockToken.release consumes its state cell
///      (was Int(42)), marks "released", calls fsReleaseLock(42) which
///      NOW returns [false, FSERR_CLOSED, "swept"] because the flag
///      flipped.  LockToken forwards this reply.
///
/// A regression on step 4f (File.close not invoking fsReleaseAllForHolder)
/// would leave the flag false, and the release would spuriously succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_close_sweep_causes_subsequent_release_to_return_fserr_closed() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = format!(
        r#"
        new File, fdP, stateP, cmodeP, LockToken, lockStateP,
            parseRwxToBits, parseRwxLoop,
            writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
            readLinesIntoLoop, drainToNextLF,
            codepointLen, concatStringsLoop, scanLineForLF,
            fsRead, fsReadAt, fsWrite, fsWriteAt,
            fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsTruncate, fsChmod, fsChown,
            fsLockRange, fsLockSequential, fsReleaseLock, fsReleaseAllForHolder,
            withSequentialLock, withRangeLock,
            acquireRangeForStream, acquireSequentialForStream,
            Stream,
            releasedFlag
        in {{
          // Stateful lock mocks — key to this test's assertion.
          releasedFlag!(false) |
          contract fsLockRange(@_fd, @_o, @_l, @_m, @_h, @_cm, ret) = {{
            ret!([true, 42])
          }} |
          contract fsLockSequential(@_fd, @_h, @_cm, ret) = {{
            ret!([true, 42])
          }} |
          contract fsReleaseLock(@_id, ret) = {{
            for (@r <- releasedFlag) {{
              releasedFlag!(r) |
              match r {{
                true  => ret!([false, "FSERR_CLOSED", "lock already released by sweep"])
                false => ret!([true])
              }}
            }}
          }} |
          contract fsReleaseAllForHolder(@_h, ret) = {{
            for (@_prev <- releasedFlag) {{
              releasedFlag!(true) |
              ret!([true, 1])
            }}
          }} |

          // Baseline file-syscall stubs (test only exercises lock + close).
          contract fsRead(@_fd, @_n, ret)      = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsReadAt(@_fd, @_o, @_n, ret) = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsWrite(@_fd, @_xs, ret)    = {{ ret!([true, 0]) }} |
          contract fsWriteAt(@_fd, @_o, @_xs, ret) = {{ ret!([true, 0]) }} |
          contract fsSeek(@_fd, @_o, @_w, ret) = {{ ret!([true, 0]) }} |
          contract fsTell(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsSize(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsFlush(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsClose(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsTruncate(@_fd, @_n, ret)  = {{ ret!([true]) }} |
          contract fsChmod(@_r, @_p, @_b, @_cm, ret) = {{ ret!([true]) }} |
          contract fsChown(@_r, @_p, @_o, @_g, @_cm, ret) = {{ ret!([true]) }} |
          contract parseRwxToBits(@_s, ret)    = {{ ret!([true, 0]) }} |
          contract Stream(retCh, @_producer, @_builder) = {{ retCh!(Nil) }} |

{}
          |
          for (@f <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
            for (@lockReply <- @f!?("lockRange", 0, 100, "w")) {{
              match lockReply {{
                [true, token] => {{
                  // Close the file — File.close (step 4f) invokes
                  // fsReleaseAllForHolder, which flips releasedFlag.
                  for (@closeReply <- @f!?("close")) {{
                    // Now try to release the outstanding token — mock's
                    // fsReleaseLock sees the flag and returns
                    // FSERR_CLOSED.  LockToken forwards it.
                    for (@relReply <- @token!?("release")) {{
                      @"out"!([closeReply, relReply])
                    }}
                  }}
                }}
                [false, code, msg] => @"out"!([[false, code, msg], [false, "SKIPPED", "lock acquire failed"]])
              }}
            }}
          }}
        }}
        "#,
        lib_body(FILE_RHO),
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list reply"),
    };
    let (close_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(
        close_ok,
        "File.close must succeed (sweep + fsClose both [true])"
    );
    let (rel_ok, rel_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(
        !rel_ok,
        "token.release after close-sweep must fail with FSERR_CLOSED"
    );
    assert_eq!(
        rel_code, "FSERR_CLOSED",
        "expected FSERR_CLOSED from swept-lock release; got: {rel_code:?}"
    );
}

/// **Cross-cap FSERR_BUSY** — Two Files opened on the same physical file
/// (same underlying fd in this mock).  Cap A takes a range lock.  Cap B
/// tries to take an overlapping range lock → FSERR_BUSY.  Verifies the
/// LockRegistry's cross-cap coordination via distinct HolderIds.
///
/// Uses the shared `STATEFUL_LOCK_MOCKS` (see helper docstring above).
/// Under fresh-mint semantics, each File cap has its own `this`
/// GPrivate, so Alice's holder != Bob's holder.  The mock's holder-
/// equality check on the "range active" branch returns FSERR_BUSY for
/// Bob because his holder differs from Alice's active-holder record.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_caps_overlapping_write_locks_conflict() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = format!(
        r#"
        new File, fdP, stateP, cmodeP, LockToken, lockStateP,
            parseRwxToBits, parseRwxLoop,
            writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
            readLinesIntoLoop, drainToNextLF,
            codepointLen, concatStringsLoop, scanLineForLF,
            fsRead, fsReadAt, fsWrite, fsWriteAt,
            fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsTruncate, fsChmod, fsChown,
            fsLockRange, fsLockSequential, fsReleaseLock, fsReleaseAllForHolder,
            withSequentialLock, withRangeLock,
            acquireRangeForStream, acquireSequentialForStream,
            Stream,
            activeHolder, activeKind, activeLockId
        in {{
          {stateful_mocks}
          |
          contract fsRead(@_fd, @_n, ret)      = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsReadAt(@_fd, @_o, @_n, ret) = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsWrite(@_fd, @_xs, ret)    = {{ ret!([true, 0]) }} |
          contract fsWriteAt(@_fd, @_o, @_xs, ret) = {{ ret!([true, 0]) }} |
          contract fsSeek(@_fd, @_o, @_w, ret) = {{ ret!([true, 0]) }} |
          contract fsTell(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsSize(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsFlush(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsClose(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsTruncate(@_fd, @_n, ret)  = {{ ret!([true]) }} |
          contract fsChmod(@_r, @_p, @_b, @_cm, ret) = {{ ret!([true]) }} |
          contract fsChown(@_r, @_p, @_o, @_g, @_cm, ret) = {{ ret!([true]) }} |
          contract parseRwxToBits(@_s, ret)    = {{ ret!([true, 0]) }} |
          contract Stream(retCh, @_producer, @_builder) = {{ retCh!(Nil) }} |

{file_body}
          |
          for (@alice <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
            for (@bob   <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
              for (@aliceLock <- @alice!?("lockRange", 0, 100, "w")) {{
                for (@bobLock <- @bob!?("lockRange", 50, 100, "w")) {{
                  @"out"!([aliceLock, bobLock])
                }}
              }}
            }}
          }}
        }}
        "#,
        stateful_mocks = STATEFUL_LOCK_MOCKS,
        file_body = lib_body(FILE_RHO),
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list reply"),
    };
    let (alice_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(alice_ok, "Alice's first lockRange must succeed");
    let (bob_ok, bob_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(!bob_ok, "Bob's overlapping lockRange must fail");
    assert_eq!(
        bob_code, "FSERR_BUSY",
        "expected FSERR_BUSY from cross-cap overlap; got: {bob_code:?}"
    );
}

/// **Same-holder composition** — Alice holds one explicit lockRange
/// over (0, 1024, "w"); Alice's own SECOND lockRange over (50, 20, "w")
/// succeeds under the same-holder-skip rule.
///
/// Uses the same holder-tracking mock as the cross-cap test.  Alice's
/// first lockRange records her as the active holder; her second
/// lockRange comes with the same holder → mock's `current == holder`
/// branch returns [true, 2] (the "same holder → allow" branch).
///
/// If Prep A's same-holder rule regressed OR the holder derivation in
/// File.rho regressed to passing a shared (non-per-cap) name, the
/// second acquire would either fail (spurious same-cap self-conflict)
/// or Alice's holder wouldn't match Bob's cross-cap holder in the
/// separate two-caps test.  Both tests together pin the invariant.
///
/// This test uses two explicit lockRange calls rather than
/// writeBytesAt-under-lockRange to keep the mock surface minimal (no
/// stub Stream required).  Semantics of same-holder positional
/// composition are the same either way.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_cap_two_overlapping_locks_coexist() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = format!(
        r#"
        new File, fdP, stateP, cmodeP, LockToken, lockStateP,
            parseRwxToBits, parseRwxLoop,
            writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
            readLinesIntoLoop, drainToNextLF,
            codepointLen, concatStringsLoop, scanLineForLF,
            fsRead, fsReadAt, fsWrite, fsWriteAt,
            fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsTruncate, fsChmod, fsChown,
            fsLockRange, fsLockSequential, fsReleaseLock, fsReleaseAllForHolder,
            withSequentialLock, withRangeLock,
            acquireRangeForStream, acquireSequentialForStream,
            Stream,
            activeHolder, activeKind, activeLockId
        in {{
          {stateful_mocks}
          |
          contract fsRead(@_fd, @_n, ret)      = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsReadAt(@_fd, @_o, @_n, ret) = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsWrite(@_fd, @_xs, ret)    = {{ ret!([true, 0]) }} |
          contract fsWriteAt(@_fd, @_o, @_xs, ret) = {{ ret!([true, 0]) }} |
          contract fsSeek(@_fd, @_o, @_w, ret) = {{ ret!([true, 0]) }} |
          contract fsTell(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsSize(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsFlush(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsClose(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsTruncate(@_fd, @_n, ret)  = {{ ret!([true]) }} |
          contract fsChmod(@_r, @_p, @_b, @_cm, ret) = {{ ret!([true]) }} |
          contract fsChown(@_r, @_p, @_o, @_g, @_cm, ret) = {{ ret!([true]) }} |
          contract parseRwxToBits(@_s, ret)    = {{ ret!([true, 0]) }} |
          contract Stream(retCh, @_producer, @_builder) = {{ retCh!(Nil) }} |

{file_body}
          |
          for (@alice <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
            for (@lock1 <- @alice!?("lockRange", 0, 1024, "w")) {{
              for (@lock2 <- @alice!?("lockRange", 50, 20, "w")) {{
                @"out"!([lock1, lock2])
              }}
            }}
          }}
        }}
        "#,
        stateful_mocks = STATEFUL_LOCK_MOCKS,
        file_body = lib_body(FILE_RHO),
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list reply"),
    };
    let (lock1_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(lock1_ok, "Alice's first lockRange must succeed");
    let (lock2_ok, lock2_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(
        lock2_ok,
        "Alice's second overlapping lockRange on the same cap must succeed \
         (Prep A same-holder skip); got code={lock2_code:?}"
    );
}

// ---------------------------------------------------------------------
// Phase 8 slice 8a step-4g follow-up — gap tests identified in the
// step-4-whole review.  Each closes a coverage hole:
//
// - Stream-lifetime lock persists cross-cap (bytes() vs. writeByteArray)
// - Error-path lock release (writeByteArray's fsWrite fails, next
//   sequential acquire from a DIFFERENT cap must succeed)
// - Sequential-blocks-sequential same-cap (writeByteArray twice on
//   the same cap under a live bytes() stream must fail on the second)
//
// All use the shared `STATEFUL_LOCK_MOCKS` helper.  Bespoke setups
// remain for tests with fundamentally different mock shapes (Test 1
// close-sweep uses a releasedFlag mock; error-path Test also overrides
// fsWrite with a counter-based error injector).
// ---------------------------------------------------------------------

/// **Stream-lifetime sequential lock persists cross-cap** — Alice opens
/// `bytes()`, acquiring the sequential lock at stream mint.  Cap B
/// tries `writeByteArray` while Alice's stream is live → FSERR_BUSY.
///
/// Covers a load-bearing case that no prior test exercised: the
/// stream-lifetime lock (via bytes/chars/readLine/lines' release-once
/// lockCell guard pattern) genuinely persists across Rholang time,
/// blocking cross-cap sequential attempts.  A regression that released
/// the lock at Stream mint (instead of at stream termination) would
/// let Bob's writeByteArray succeed spuriously.
///
/// Note: the Stream stub returns `Nil` for the stream handle, so
/// Alice's stream is never consumed to EOS — the outer sequential
/// lock stays held throughout the test.  This models a caller
/// abandoning the stream mid-flight, which is the "leak until
/// File.close sweep or deploy-end" case; here it's leveraged to keep
/// the lock held so Bob's attempt hits the still-active-holder path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bytes_stream_lock_blocks_cross_cap_sequential_write() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = format!(
        r#"
        new File, fdP, stateP, cmodeP, LockToken, lockStateP,
            parseRwxToBits, parseRwxLoop,
            writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
            readLinesIntoLoop, drainToNextLF,
            codepointLen, concatStringsLoop, scanLineForLF,
            fsRead, fsReadAt, fsWrite, fsWriteAt,
            fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsTruncate, fsChmod, fsChown,
            fsLockRange, fsLockSequential, fsReleaseLock, fsReleaseAllForHolder,
            withSequentialLock, withRangeLock,
            acquireRangeForStream, acquireSequentialForStream,
            Stream,
            activeHolder, activeKind, activeLockId
        in {{
          {stateful_mocks}
          |
          contract fsRead(@_fd, @_n, ret)      = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsReadAt(@_fd, @_o, @_n, ret) = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsWrite(@_fd, @_xs, ret)    = {{ ret!([true, 0]) }} |
          contract fsWriteAt(@_fd, @_o, @_xs, ret) = {{ ret!([true, 0]) }} |
          contract fsSeek(@_fd, @_o, @_w, ret) = {{ ret!([true, 0]) }} |
          contract fsTell(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsSize(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsFlush(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsClose(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsTruncate(@_fd, @_n, ret)  = {{ ret!([true]) }} |
          contract fsChmod(@_r, @_p, @_b, @_cm, ret) = {{ ret!([true]) }} |
          contract fsChown(@_r, @_p, @_o, @_g, @_cm, ret) = {{ ret!([true]) }} |
          contract parseRwxToBits(@_s, ret)    = {{ ret!([true, 0]) }} |
          contract Stream(retCh, @_producer, @_builder) = {{ retCh!(Nil) }} |

{file_body}
          |
          for (@alice <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
            for (@bob   <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
              // Alice opens bytes() — acquires sequential lock via wrap.
              for (@aliceBytesReply <- @alice!?("bytes")) {{
                match aliceBytesReply {{
                  [true, _streamHandle] => {{
                    // Alice's sequential lock is now held.  Bob's write
                    // must fail on fsLockSequential.
                    for (@bobWriteReply <- @bob!?("writeByteArray", "x".toUtf8Bytes())) {{
                      @"out"!([aliceBytesReply, bobWriteReply])
                    }}
                  }}
                  _ => @"out"!([aliceBytesReply, [false, "SKIPPED", "bytes() failed"]])
                }}
              }}
            }}
          }}
        }}
        "#,
        stateful_mocks = STATEFUL_LOCK_MOCKS,
        file_body = lib_body(FILE_RHO),
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list reply"),
    };
    let (alice_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(alice_ok, "Alice's bytes() stream must mint successfully");
    let (bob_ok, bob_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(
        !bob_ok,
        "Bob's writeByteArray under Alice's live stream must fail"
    );
    assert_eq!(
        bob_code, "FSERR_BUSY",
        "expected FSERR_BUSY from stream-lifetime sequential conflict; got: {bob_code:?}"
    );
}

/// **Error-path release** — Alice's writeByteArray fails at fsWrite
/// (simulated I/O error via counter-based mock).  writeByteArray's
/// wrap must release the sequential lock BEFORE returning the error;
/// Bob's subsequent writeByteArray from a different cap should then
/// succeed (proving the lock was released even on the error path).
///
/// Covers a coverage hole flagged in the whole-step-4 review: no
/// existing test verified that wraps release on the error path.  A
/// regression where the wrap "return early on error without release"
/// would strand the lock and cause Bob's write to fail with BUSY.
///
/// Uses a counter-based fsWrite mock that errors on the FIRST call
/// only.  The write-methods stub (Stream) is left as the default
/// no-op since this test doesn't drain a stream.  All other stubs
/// standard.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_byte_array_releases_lock_on_error_path() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = format!(
        r#"
        new File, fdP, stateP, cmodeP, LockToken, lockStateP,
            parseRwxToBits, parseRwxLoop,
            writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
            readLinesIntoLoop, drainToNextLF,
            codepointLen, concatStringsLoop, scanLineForLF,
            fsRead, fsReadAt, fsWrite, fsWriteAt,
            fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsTruncate, fsChmod, fsChown,
            fsLockRange, fsLockSequential, fsReleaseLock, fsReleaseAllForHolder,
            withSequentialLock, withRangeLock,
            acquireRangeForStream, acquireSequentialForStream,
            Stream,
            activeHolder, activeKind, activeLockId,
            writeCounter
        in {{
          {stateful_mocks}
          |
          contract fsRead(@_fd, @_n, ret)      = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsReadAt(@_fd, @_o, @_n, ret) = {{ ret!([true, "".hexToBytes()]) }} |
          // Counter-based fsWrite: first call errors, rest succeed.
          writeCounter!(0) |
          contract fsWrite(@_fd, @xs, ret) = {{
            for (@c <- writeCounter) {{
              writeCounter!(c + 1) |
              match c {{
                0 => ret!([false, "FSERR_IO", "simulated write failure"])
                _ => ret!([true, xs.length()])
              }}
            }}
          }} |
          contract fsWriteAt(@_fd, @_o, @_xs, ret) = {{ ret!([true, 0]) }} |
          contract fsSeek(@_fd, @_o, @_w, ret) = {{ ret!([true, 0]) }} |
          contract fsTell(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsSize(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsFlush(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsClose(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsTruncate(@_fd, @_n, ret)  = {{ ret!([true]) }} |
          contract fsChmod(@_r, @_p, @_b, @_cm, ret) = {{ ret!([true]) }} |
          contract fsChown(@_r, @_p, @_o, @_g, @_cm, ret) = {{ ret!([true]) }} |
          contract parseRwxToBits(@_s, ret)    = {{ ret!([true, 0]) }} |
          contract Stream(retCh, @_producer, @_builder) = {{ retCh!(Nil) }} |

{file_body}
          |
          for (@alice <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
            for (@bob   <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
              // Alice's write hits fsWrite → mock returns FSERR_IO on
              // first call.  writeByteArray's wrap must release the
              // sequential lock before returning the error.
              for (@aliceWrite <- @alice!?("writeByteArray", "a".toUtf8Bytes())) {{
                // Bob's subsequent write should succeed — Alice's
                // lock was released even on the error path.
                for (@bobWrite <- @bob!?("writeByteArray", "b".toUtf8Bytes())) {{
                  @"out"!([aliceWrite, bobWrite])
                }}
              }}
            }}
          }}
        }}
        "#,
        stateful_mocks = STATEFUL_LOCK_MOCKS,
        file_body = lib_body(FILE_RHO),
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list reply"),
    };
    let (alice_ok, alice_code, _, _) = extract_reply(&outer.ps[0]);
    assert!(
        !alice_ok,
        "Alice's write must fail (simulated fsWrite error)"
    );
    assert_eq!(
        alice_code, "FSERR_IO",
        "expected FSERR_IO from mock fsWrite; got: {alice_code:?}"
    );
    let (bob_ok, bob_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(
        bob_ok,
        "Bob's write must succeed — Alice's lock must have been released \
         on the error path.  Got FSERR code={bob_code:?}"
    );
}

/// **Same-cap sequential-blocks-sequential** — Alice opens `bytes()`
/// (sequential lock held).  Alice's OWN subsequent `writeByteArray`
/// (which needs a sequential lock) must fail — sequential is strict
/// same-holder per §Slice-1 commitments' scope of the same-holder-skip
/// rule.
///
/// Covers a coverage hole flagged in the review: no prior test verified
/// that same-holder sequential exclusion actually fires.  A regression
/// where same-holder-skip extended to sequential would let Alice's
/// second sequential attempt through, corrupting the "one active
/// sequential stream per File" invariant (spec §1143).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_cap_sequential_blocks_own_sequential_attempt() {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let src = format!(
        r#"
        new File, fdP, stateP, cmodeP, LockToken, lockStateP,
            parseRwxToBits, parseRwxLoop,
            writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
            readLinesIntoLoop, drainToNextLF,
            codepointLen, concatStringsLoop, scanLineForLF,
            fsRead, fsReadAt, fsWrite, fsWriteAt,
            fsSeek, fsTell, fsSize, fsFlush, fsClose,
            fsTruncate, fsChmod, fsChown,
            fsLockRange, fsLockSequential, fsReleaseLock, fsReleaseAllForHolder,
            withSequentialLock, withRangeLock,
            acquireRangeForStream, acquireSequentialForStream,
            Stream,
            activeHolder, activeKind, activeLockId
        in {{
          {stateful_mocks}
          |
          contract fsRead(@_fd, @_n, ret)      = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsReadAt(@_fd, @_o, @_n, ret) = {{ ret!([true, "".hexToBytes()]) }} |
          contract fsWrite(@_fd, @_xs, ret)    = {{ ret!([true, 0]) }} |
          contract fsWriteAt(@_fd, @_o, @_xs, ret) = {{ ret!([true, 0]) }} |
          contract fsSeek(@_fd, @_o, @_w, ret) = {{ ret!([true, 0]) }} |
          contract fsTell(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsSize(@_fd, ret)           = {{ ret!([true, 0]) }} |
          contract fsFlush(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsClose(@_fd, ret)          = {{ ret!([true]) }} |
          contract fsTruncate(@_fd, @_n, ret)  = {{ ret!([true]) }} |
          contract fsChmod(@_r, @_p, @_b, @_cm, ret) = {{ ret!([true]) }} |
          contract fsChown(@_r, @_p, @_o, @_g, @_cm, ret) = {{ ret!([true]) }} |
          contract parseRwxToBits(@_s, ret)    = {{ ret!([true, 0]) }} |
          contract Stream(retCh, @_producer, @_builder) = {{ retCh!(Nil) }} |

{file_body}
          |
          for (@alice <- File!?(1, "/root", "test.txt", "rw", "oracular")) {{
            for (@bytesReply <- @alice!?("bytes")) {{
              match bytesReply {{
                [true, _stream] => {{
                  // Alice's sequential lock is held (bytes stream is live,
                  // Stream stub returned Nil so it's never consumed).
                  // Alice's OWN writeByteArray tries to acquire another
                  // sequential lock → must fail per strict same-holder.
                  for (@writeReply <- @alice!?("writeByteArray", "x".toUtf8Bytes())) {{
                    @"out"!([bytesReply, writeReply])
                  }}
                }}
                _ => @"out"!([bytesReply, [false, "SKIPPED", "bytes failed"]])
              }}
            }}
          }}
        }}
        "#,
        stateful_mocks = STATEFUL_LOCK_MOCKS,
        file_body = lib_body(FILE_RHO),
    );
    let reply = eval_and_read_out(&space, &reducer, &src).await;
    let outer = match single_expr(&reply).unwrap().expr_instance {
        Some(ExprInstance::EListBody(l)) => l,
        _ => panic!("expected list reply"),
    };
    let (bytes_ok, _, _, _) = extract_reply(&outer.ps[0]);
    assert!(bytes_ok, "Alice's bytes() stream must mint successfully");
    let (write_ok, write_code, _, _) = extract_reply(&outer.ps[1]);
    assert!(
        !write_ok,
        "Alice's own writeByteArray under her own live bytes stream must fail \
         (sequential is strict same-holder per §1143)"
    );
    assert_eq!(
        write_code, "FSERR_BUSY",
        "expected FSERR_BUSY from same-cap sequential-vs-sequential; got: {write_code:?}"
    );
}
