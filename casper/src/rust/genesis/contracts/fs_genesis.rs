//! File I/O FIP genesis composition (slice 19).
//!
//! Assembles the File / Dir / Stream / Buffer / Stdin / Stdout / Fs
//! library agents into a single Rholang deploy that (a) binds the
//! native primitive URNs into the shared new-scope, (b) mints one Fs
//! instance with default stdio fds and an empty static bundle, and
//! (c) publishes that Fs cap via the legacy
//! `rho:registry:insertSigned:secp256k1` mechanism (same pattern as
//! Stack.rho, ListOps.rho, etc.).
//!
//! # Native URN scheme
//!
//! `rho:io:fs:native:1.0.0/*` names are private implementation detail
//! shared between `rholang::interpreter::rho_runtime` (which registers
//! them) and this module (which binds them into the FsGenesis new-
//! scope).  The FIP itself does not fix these names; treat them as
//! internal until a §Native URNs spec section pins them.
//!
//! # Publication URI
//!
//! The Fs cap is published at `rho:id:<hash-of-FS_GENERATOR_PK>` via
//! `insertSigned`, following the legacy blessed-contract pattern.  The
//! FIP's spec examples (§826, §829, §874) obtain the Fs via
//! `getFS(`rho:io:fs:1.*`)` — a Versioned Registry lookup.  Slice 19
//! does NOT publish at the versioned URI; deploys must use the derived
//! `rho:id:...` returned by `fs_genesis_uri()`.  See MVP simplification
//! #4 below.
//!
//! # MVP simplifications (documented deferrals)
//!
//! 1. **Shared-Fs model.**  A single Fs instance is published at the
//!    registry URI derived from FS_GENERATOR_PK.  All deploys look up
//!    the same handle and thus share the cache and stdio caps.  Spec
//!    §867 wants per-principal Fs instances from the powerbox — that
//!    requires runtime changes to the URN resolver (each grantee sees
//!    a distinct cap) and is deferred to a future powerbox slice.
//!
//! 2. **Empty static bundle.**  The published Fs has `bMap = {}`, so
//!    `openFile` / `openDir` return `FSERR_UNSUPPORTED` for every
//!    logical name.  Static provisioning (spec §846, config-driven
//!    bundle) is Phase 7.  Stdio methods (`stdin` / `stdout` /
//!    `stderr`) work.
//!
//! 3. **Hardwired stdio fds.**  The instance is minted with fds
//!    (0, 1, 2).  A future powerbox slice will let each principal
//!    override these (e.g. `/dev/null`-equivalent for cases 4-6 per
//!    spec §Storage).
//!
//! 4. **Versioned Registry URI not published.**  The Fs cap lives at
//!    `rho:id:<hash>` (see `fs_genesis_uri()`), not the spec's
//!    `rho:io:fs:1.0.0`.  A future slice must add an `insertVersion`
//!    call (Versioned Registry) mapping `rho:io:fs:1.0.0` to the same
//!    handle so spec-canonical `getFS(`rho:io:fs:1.*`)` lookups
//!    resolve.  Interim: deploys should substitute the `rho:id:...`
//!    form returned by `fs_genesis_uri(&FS_GENERATOR_PUB_KEY)`.
//!
//! 5. **`rho:io:fs:native:*` URN filter removed.**  The Rust runtime
//!    previously filtered these URNs from the user-lookupable urn_map
//!    so only the genesis Fs agent could bind them.  The filter was
//!    removed to let this composed deploy compile.  Consequence:
//!    ANY user deploy can now `new fsOpen(`rho:io:fs:native:1.0.0/open`)
//!    in { fsOpen!(...) }` and hit the raw syscalls directly,
//!    bypassing Fs.rho's sandbox / mode-cap / bundle checks.  This is
//!    a documented "MUST FIX BEFORE PRODUCTION" deferral: a future
//!    powerbox / blessed-deploy slice must reinstate the filter with
//!    a genesis-only exception (either by pre-binding the URNs into
//!    the fs_generator's Env before applying the filter, or by
//!    tagging deploys with signer identity and consulting a
//!    whitelist at URN-lookup time).
//!
//! 6. **`Buffer` / `Allocator` compiled but unreachable.**  The composed
//!    source splices in Buffer.rho's body (agent definitions live at
//!    module scope alongside Fs, File, Dir, etc.) but does NOT publish
//!    the `Allocator` cap — only Fs is exported via `rs!(...)`.  Storage
//!    cost is incurred (contracts persist in the tuplespace) with zero
//!    user reach.  Consequence: File.rho's Buffer-taking methods
//!    (`readInto` / `writeFrom` / `readLineInto` / `readLinesInto`) are
//!    functionally dead until PB-B-5 lands the per-principal Allocator
//!    delegation at `rho:lang:buffer:1.0.0`.  Deploys cannot obtain a
//!    Buffer without that publication.
//!
//! 7. **`ConsensusMode` always Oracular.**  Phase 1 threaded a
//!    `ConsensusMode` enum from `ProcessContext` to native handlers,
//!    but no code path currently sets it to `Consensus` — Fs.rho takes
//!    no mode arg and rho_runtime.rs hardcodes `ConsensusMode::default()`.
//!    `chown` short-circuit and `stat`/`entries` field-omission are
//!    therefore unreachable.  Real fix belongs to Phase 7 config wiring
//!    (per PB-M-12 / PB-M-4 mode routing) — a principal's mode must be
//!    derived from the config bucket (`oracle-static-*` vs
//!    `consensus-static-*`) their bundle came from.

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::utils::{new_etuple_par, new_gint_par};
use prost::Message;
use rholang::rust::interpreter::registry::registry::Registry;

use super::embedded_rho;

/// Nonce used in the FsGenesis signed-registry insertion.  MAX_LONG
/// so nobody can overwrite the entry once published.
pub const FS_NONCE: i64 = i64::MAX;

/// URN prefix shared between the runtime's `fs_native_def` registrations
/// and this module's composed FsGenesis source.  A future Phase 1 hotfix
/// bumping to `1.0.1` must edit HERE only, and both the runtime
/// (`rho_runtime.rs`) and the composed source rebuild from this constant.
pub const FS_NATIVE_URN_PREFIX: &str = "rho:io:fs:native:1.0.0/";

/// Native URN suffixes that this module binds into the FsGenesis
/// new-scope.  Combined with `FS_NATIVE_URN_PREFIX` to form the full
/// URN.  Kept as a constant so a slice-drift assertion in the test
/// suite can cross-check against the runtime's registered set
/// (`rho::interpreter::rho_runtime::fs_native_def` call sites).
///
/// Order matches the `new`-clause below (documentation aid only).
pub const FS_NATIVE_URN_SUFFIXES: &[&str] = &[
    "open",
    "close",
    "read",
    "readAt",
    "write",
    "writeAt",
    "seek",
    "tell",
    "size",
    "flush",
    "stat",
    "exists",
    "truncate",
    "chmod",
    "chown",
    "removeFile",
    "removeDir",
    "rename",
    "copyFile",
    "entries",
];

/// Extract the body between the top-level `new ... in {` and the
/// closing `}` of a `.rho` library file.
///
/// Lexically-aware scanner: skips (a) `//` line comments (to end of
/// line), (b) `/* ... */` block comments, (c) `"..."` string
/// literals with `\` escape handling, and (d) `` `...` `` URI
/// literals.  Brace-matches to find the CLOSING `}` that pairs with
/// the top-level `new ... in {` opener — this is more robust than
/// the byte-level `rfind('}')` approach, which silently truncates
/// on stray `}` inside string literals or trailing comments.
fn lib_body(src: &str) -> &str {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;

    // Scanner state.  When advancing, we always land at the next
    // "top-level" byte (i.e., not inside a comment / string / URI).
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut in_uri = false;

    // Locate the top-level `new ... in {` opener.
    let mut open_pos: Option<usize> = None;
    while i < n {
        let c = bytes[i];
        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_string {
            if c == b'\\' && i + 1 < n {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if in_uri {
            if c == b'`' {
                in_uri = false;
            }
            i += 1;
            continue;
        }
        // Top-level dispatch.
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            in_line_comment = true;
            i += 2;
            continue;
        }
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            in_block_comment = true;
            i += 2;
            continue;
        }
        if c == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c == b'`' {
            in_uri = true;
            i += 1;
            continue;
        }
        // Look for `in {` at top level.
        if c == b'i'
            && i + 3 < n
            && bytes[i + 1] == b'n'
            && bytes[i + 2] == b' '
            && bytes[i + 3] == b'{'
        {
            // Sanity: ensure `in` is a keyword not the tail of an ident.
            let prev_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            if prev_ok {
                open_pos = Some(i + 4); // one past the `{`
                break;
            }
        }
        i += 1;
    }
    let body_start = open_pos.expect("library source must contain a top-level `in {`");

    // Walk from body_start, tracking brace depth.  The FIRST `}` that
    // brings depth back to 0 is the closing `}` of the `new ... in {`.
    let mut depth: i64 = 1;
    let mut j = body_start;
    // Reset lexer state.
    in_line_comment = false;
    in_block_comment = false;
    in_string = false;
    in_uri = false;
    while j < n {
        let c = bytes[j];
        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            j += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && j + 1 < n && bytes[j + 1] == b'/' {
                in_block_comment = false;
                j += 2;
                continue;
            }
            j += 1;
            continue;
        }
        if in_string {
            if c == b'\\' && j + 1 < n {
                j += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            j += 1;
            continue;
        }
        if in_uri {
            if c == b'`' {
                in_uri = false;
            }
            j += 1;
            continue;
        }
        if c == b'/' && j + 1 < n && bytes[j + 1] == b'/' {
            in_line_comment = true;
            j += 2;
            continue;
        }
        if c == b'/' && j + 1 < n && bytes[j + 1] == b'*' {
            in_block_comment = true;
            j += 2;
            continue;
        }
        if c == b'"' {
            in_string = true;
            j += 1;
            continue;
        }
        if c == b'`' {
            in_uri = true;
            j += 1;
            continue;
        }
        if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return &src[body_start..j];
            }
        }
        j += 1;
    }
    panic!("library source must have a balanced closing `}}` for the top-level `new ... in {{`");
}

fn is_ident_char(b: u8) -> bool { matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_') }

/// Compose the full FsGenesis Rholang source.
///
/// Wraps every library body inside a shared `new` scope that also
/// binds the native URNs to the names the library bodies capture
/// lexically (`fsRead`, `fsWrite`, etc.) and the registry-insertion
/// URN needed for publication.
///
/// `pk_hex` and `sig_hex` MUST be lowercase ASCII hex strings; the
/// debug-asserts below prevent a future refactor from passing
/// untrusted bytes through the `format!` boundary.
pub fn compose_fs_genesis_source(pk_hex: &str, sig_hex: &str) -> String {
    debug_assert!(
        pk_hex.chars().all(|c| c.is_ascii_hexdigit()),
        "pk_hex must be ASCII hex"
    );
    debug_assert!(
        sig_hex.chars().all(|c| c.is_ascii_hexdigit()),
        "sig_hex must be ASCII hex"
    );

    let file_body = lib_body(embedded_rho::FILE);
    let dir_body = lib_body(embedded_rho::DIR);
    let stream_body = lib_body(embedded_rho::STREAM);
    let buffer_body = lib_body(embedded_rho::BUFFER);
    let stdin_body = lib_body(embedded_rho::STDIN);
    let stdout_body = lib_body(embedded_rho::STDOUT);
    let fs_body = lib_body(embedded_rho::FS);

    let nonce = FS_NONCE;

    format!(
        r#"
new
  File, fdP, stateP, Dir, rootP,
  Stream, paramsP, gatherN, foldLoop, forEachLoop, foldChunksLoop,
  Buffer, Allocator, Rows, metaP, chunkP, innerP, rowsMetaP,
  gatherChunks, drainChunks, allocInnersLoop, parkInnersLoop,
  clearInnersLoop, closeInnersLoop,
  Stdin, stdinFdP, stdinStateP,
  Stdout, stdoutFdP, stdoutStateP,
  Fs, fsBundleP, fsCacheP,
  fsStdinFdP, fsStdoutFdP, fsStderrFdP,
  cacheAndOpenFile, cacheAndOpenDir,
  openFileImpl, openDirImpl, joinRel,
  parseRwxToBits, parseRwxLoop,
  writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
  readLinesIntoLoop, drainToNextLF,
  codepointLen, concatStringsLoop, scanLineForLF,
  fsOpen(`rho:io:fs:native:1.0.0/open`),
  fsClose(`rho:io:fs:native:1.0.0/close`),
  fsRead(`rho:io:fs:native:1.0.0/read`),
  fsReadAt(`rho:io:fs:native:1.0.0/readAt`),
  fsWrite(`rho:io:fs:native:1.0.0/write`),
  fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
  fsSeek(`rho:io:fs:native:1.0.0/seek`),
  fsTell(`rho:io:fs:native:1.0.0/tell`),
  fsSize(`rho:io:fs:native:1.0.0/size`),
  fsFlush(`rho:io:fs:native:1.0.0/flush`),
  fsStat(`rho:io:fs:native:1.0.0/stat`),
  fsExists(`rho:io:fs:native:1.0.0/exists`),
  fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
  fsChmod(`rho:io:fs:native:1.0.0/chmod`),
  fsChown(`rho:io:fs:native:1.0.0/chown`),
  fsRemoveFile(`rho:io:fs:native:1.0.0/removeFile`),
  fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`),
  fsRename(`rho:io:fs:native:1.0.0/rename`),
  fsCopyFile(`rho:io:fs:native:1.0.0/copyFile`),
  fsEntries(`rho:io:fs:native:1.0.0/entries`),
  rs(`rho:registry:insertSigned:secp256k1`),
  uriOut
in {{
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
  // MVP: mint one shared Fs instance (stdio fds 0/1/2, empty static
  // bundle) and publish it at the registry URI derived from
  // FS_GENERATOR_PK.  Per-principal delegation via powerbox is a
  // future slice (see fs_genesis.rs docstring for deferral notes).
  for (@fs <- Fs!?(0, 1, 2, {{}})) {{
    rs!(
      "{pk_hex}".hexToBytes(),
      ({nonce}, fs),
      "{sig_hex}".hexToBytes(),
      *uriOut
    )
  }}
}}
"#
    )
}

/// Deterministically derive the secp256k1 signature the composed
/// FsGenesis source needs for the `rs!(...)` call.  Matches
/// RegistrySigGen::derive_from's `to_sign` construction and hashing.
/// Signing is RFC 6979 (deterministic k) via the k256 crate — cross-
/// process consistency required for consensus.
pub fn fs_genesis_signature_hex(sk: &PrivateKey, timestamp: i64) -> String {
    let secp256k1 = Secp256k1;
    let pk: PublicKey = secp256k1.to_public(sk);
    let to_sign: Par = new_etuple_par(vec![
        new_gint_par(timestamp, Vec::new(), false),
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(pk.bytes.to_vec())),
        }]),
        new_gint_par(FS_NONCE, Vec::new(), false),
    ]);
    let sign_bytes = Blake2b256::hash(to_sign.encode_to_vec());
    let sig = secp256k1.sign(&sign_bytes, &sk.bytes);
    hex::encode(sig)
}

/// The registry URI at which the FsGenesis deploy publishes the
/// shared Fs cap.  Deterministic function of FS_GENERATOR_PK.
pub fn fs_genesis_uri(pk: &PublicKey) -> String {
    let key_hash = Blake2b256::hash(pk.bytes.to_vec());
    Registry::build_uri(&key_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lib_body_extracts_simple_body() {
        let src = "new x in { x!(1) }";
        assert_eq!(lib_body(src), " x!(1) ");
    }

    #[test]
    fn lib_body_skips_line_comment_in_marker() {
        // The FIRST `in {` is inside a `//` comment; the SECOND is real.
        let src = "// docs: `new x in { ... }`\nnew x in { x!(2) }";
        assert_eq!(lib_body(src), " x!(2) ");
    }

    #[test]
    fn lib_body_skips_block_comment_in_marker() {
        let src = "/* new x in { hidden } */\nnew x in { x!(3) }";
        assert_eq!(lib_body(src), " x!(3) ");
    }

    #[test]
    fn lib_body_handles_brace_inside_string_literal() {
        // If we used rfind('}'), the `}` inside "hi}" would truncate the body.
        let src = r#"new x in { x!("hi}") }"#;
        assert_eq!(lib_body(src), r#" x!("hi}") "#);
    }

    #[test]
    fn lib_body_handles_escaped_quote_in_string() {
        let src = r#"new x in { x!("say \"}\"") }"#;
        assert_eq!(lib_body(src), r#" x!("say \"}\"") "#);
    }

    #[test]
    fn lib_body_handles_backtick_uri_with_brace() {
        // Backtick-quoted URIs shouldn't shift the brace-matcher.
        let src = "new x(`rho:io:fs:1.0.0/{weird}`) in { x!(4) }";
        assert_eq!(lib_body(src), " x!(4) ");
    }

    #[test]
    fn lib_body_handles_nested_braces() {
        let src = "new x in { match 1 { 1 => x!(5) } }";
        assert_eq!(lib_body(src), " match 1 { 1 => x!(5) } ");
    }

    #[test]
    fn lib_body_handles_trailing_content_past_close_brace() {
        // A hypothetical future maintainer adds a `// trailing } comment`
        // after the closing brace.  rfind('}') would break on this;
        // the depth-tracker doesn't.
        let src = "new x in { x!(6) }\n// trailing }";
        assert_eq!(lib_body(src), " x!(6) ");
    }

    #[test]
    fn lib_body_rejects_in_inside_identifier() {
        // A hypothetical library where the byte sequence `in {` appears
        // as the tail of an identifier (e.g., `sortin {`) must NOT be
        // matched.  Prev-char check enforces `in` is a keyword.
        let src = "new x in { new sortin { 1 } }";
        // With the identifier check, we still find the OUTER `in {`.
        assert_eq!(lib_body(src), " new sortin { 1 } ");
    }

    #[test]
    fn fs_native_urn_suffixes_covers_composed_source() {
        // Every URN listed in FS_NATIVE_URN_SUFFIXES must be bound in
        // the composed source.  This is a drift assertion: if someone
        // adds a suffix to the constant without updating the format!
        // template, this test fails.
        let src = compose_fs_genesis_source("00", "00");
        for suffix in FS_NATIVE_URN_SUFFIXES {
            let expected = format!("`{FS_NATIVE_URN_PREFIX}{suffix}`");
            assert!(
                src.contains(&expected),
                "composed source missing URN binding: {expected}"
            );
        }
    }
}
