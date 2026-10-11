// Consensus-mode filesystem WAL snapshot — encoding + decoding
// primitives (slices 1 + 2 of the `snapshot` submodule tree).
//
// Slice 1 (PR #504) provided the canonical byte-encoding of
// `Vec<WalEntry>` plus its Blake2b256 root hash and the
// [`SnapshotBlob`] convenience combiner (encoded bytes + root +
// Merkle root over 4 MiB chunks).
//
// Slice 2 (PR #505) provided the symmetric decoder
// [`decode_wal_slice`] + [`SnapshotError`] + supporting
// primitives.  Together, encoder + decoder are the round-trip
// substrate that joiners use to apply a fetched snapshot to a
// fresh tree.
//
// Slice 3 (PR #506) added the on-disk read/write path:
// [`snapshot_path`] (content-addressed filename derivation),
// [`write_snapshot`] (atomic tmp+rename + fsync), and
// [`read_snapshot_bytes`] (hash + version verification on
// load).  Plus [`referenced_payload_hashes`] as a small helper
// that later slices thread through the sidecar path.
//
// Slice 4 (PR #507) added the payload-hash sidecar:
// [`hashes_sidecar_path`], [`read_hashes_sidecar`], and
// [`scan_retained_payload_hashes`].  The sidecar write is
// threaded into [`write_snapshot`] as a best-effort tail.
//
// Slice 5 (PR #508) added [`sweep_stale_tmp_files`] — a
// best-effort maintenance helper that removes leaked
// `.wal.tmp` / `.hashes.tmp` files.
//
// Slice 6 (PR #509) added the manifest wire format:
// [`MANIFEST_FORMAT_VERSION`], [`MANIFEST_FILENAME`],
// [`ManifestEntry`] (struct + `data`/`empty` constructors), and
// [`ManifestEntry::to_line`] / [`ManifestEntry::from_line`].
//
// Slice 7 (PR #510) added the manifest persistence layer:
// [`append_manifest_entry`] (O_APPEND + create-if-missing,
// line-atomic on common Linux filesystems) and
// [`read_manifest`] (parse every line, missing file → empty
// Vec, malformed line → [`SnapshotError::MalformedManifest`]).
//
// Slice 8 (PR #511) added [`prune_snapshot_dir`] — mtime-based
// retention keeping the N newest `.wal` snapshots and their
// paired `.hashes` sidecars (manifest-independent for
// correctness under lost manifest entries).
//
// Slice 9 (PR #514) added H-4 signing on `ManifestEntry`:
// [`ManifestEntry::sign_bytes`], [`ManifestEntry::signed`],
// [`ManifestEntry::verify_with_pubkey`], plus
// [`SnapshotError::UnsignedManifestEntry`] +
// [`SnapshotError::ManifestSignatureInvalid`].
//
// Slice 10 (this PR, CAPSTONE) adds [`SnapshotWriter`] — the
// cadence-driven orchestrator that ties every prior slice
// together.  Per-block entry point [`SnapshotWriter::maybe_write`]
// decides cadence hits, calls [`write_snapshot`] for data
// slices, calls [`ManifestEntry::signed`] + [`append_manifest_entry`]
// for both data and empty-sentinel manifest entries, and
// kicks [`prune_snapshot_dir`] for retention — all with
// best-effort posture so a single downstream failure does
// not block subsequent block processing.
//
// # Snapshot semantics — log-structured
//
// A snapshot in this module IS a canonical byte-encoding of
// `Vec<WalEntry>` — a WAL slice, not a materialized filesystem-
// state image.  Joining validators (later slice) replay the
// concatenation of all snapshots from genesis forward against an
// empty base image.  This commits the system to **log-structured
// checkpointing**: prevention of unbounded WAL replay for a late
// joiner depends on the operator's cadence being short enough that
// cumulative WAL slice size stays under the replay budget.
//
// The alternative — materialized fs state snapshots — was
// considered and deferred; the log-structured shape:
//   * keeps a single content-addressed encoding for both per-block
//     WAL entries (on-chain root commitment) and on-disk snapshots
//     — no separate serialization schema;
//   * requires no fs-state serializer (which for consensus-static
//     buckets would need to snapshot every file's contents);
//   * lets the joining protocol be a plain byte-stream fetch.
//
// # Canonical encoding
//
// The encoding is prefix-length + big-endian; no protobuf, no
// serde.  This keeps the byte layout independent of any code-
// generator's choices and makes the resulting root hash a stable
// consensus commitment.  A hard-fork of the encoding format is a
// hard-fork of the WAL root hash.
//
// The WAL slice header is:
//   version: u8 (= SNAPSHOT_FORMAT_VERSION)
//   entries: u32-be count + [entry bytes] × count
//
// The leading version byte makes reads self-describing: the
// (slice-2) decoder rejects unknown-version blobs cleanly instead
// of mis-decoding — critical for a network mid-fork.
//
// Per-entry layout:
//   op_tag                              (u8)
//   path                                (len-prefixed OS bytes)
//   extra_path presence-tag             (u8: 0=None, 1=Some)
//     [+ len-prefixed OS bytes if 1]
//   offset presence-tag                 (u8: 0=None, 1=Some)
//     [+ u64-be if 1]
//   length presence-tag                 (u8: 0=None, 1=Some)
//     [+ u64-be if 1]
//   payload_ref variant-tag             (u8: 0=None, 1=Hash, 2=DeployRef)
//     [+ 32 bytes if 1]
//     [+ 32 bytes + u32-be + u32-be if 2]
//   mode_bits presence-tag              (u8: 0=None, 1=Some)
//     [+ u32-be if 1]
//   owner presence-tag                  (u8: 0=None, 1=Some)
//     [+ len-prefixed UTF-8 bytes if 1]
//   group presence-tag                  (u8: 0=None, 1=Some)
//     [+ len-prefixed UTF-8 bytes if 1]
//   outcome variant-tag                 (u8: 0=Success, 1=Failure+u32-be)
//
// Reordering ANY field, renumbering ANY tag, or changing the
// leading version byte's meaning is a hard fork of the WAL root
// and requires bumping `SNAPSHOT_FORMAT_VERSION`.

use crypto::rust::hash::blake2b256::Blake2b256;

use super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome, MAX_WAL_ENTRIES};

/// Wire-format version of the WAL slice encoding.  Prepended as a
/// single `u8` at the start of every encoded WAL slice; the
/// (slice-2) decoder rejects mismatched-version blobs cleanly
/// instead of mis-decoding.
///
/// # CONSENSUS-OBSERVABLE
///
/// The encoded WAL slice's Blake2b256 root is the on-chain
/// commitment consumed by `WalSnapshotWrite` (yet-to-land); a
/// validator running a different version byte would produce
/// different root bytes for identical WAL contents and silently
/// fork.  Registered into `CONSENSUS_FOLD` at order 15
/// (`u8_raw` — the first use of that encoding shape); the
/// fingerprint fold advertises the constant to peering handshakes
/// so a mismatched validator fails the `network_id` check at boot.
///
/// # Bump discipline
///
/// A change here IS a hard fork of the WAL root.  Bumping this
/// value:
///   1. Rolls the fingerprint golden hex.
///   2. Requires a coordinated fleet-wide upgrade.
///   3. May require the slice-2 decoder to keep the previous
///      version as a fallback for reading legacy snapshots
///      (design decision at bump time).
///
/// Version history (from fileio, for reference — this triage
/// branch's version starts at the current tail):
///   1: initial encoding (pre-outcome field)
///   2: added `outcome` field (H-6 fix)
///   3: added `WalOp::Stat` / `Entries` / `Size` at tags 12/13/14
///   4: added `WalOp::EntriesStreamNext` at tag 15
///   5: (skipped in fileio; contiguity guard here)
///   6: added `WalOp::Exists` at tag 16 (post-Consensus-ban-lift)
///   7: added `WalOp::BulkApply` at tag 17 (bulk import commit)
pub const SNAPSHOT_FORMAT_VERSION: u8 = 7;

crate::register_consensus_constant!(order = 15, name = SNAPSHOT_FORMAT_VERSION, u8_raw);

/// Wire-format version of the on-disk `manifest.jsonl` line
/// format.  Distinct from [`SNAPSHOT_FORMAT_VERSION`] because
/// the two are independent wire surfaces: the `.wal` file bytes
/// and the manifest text lines evolve on separate cadences.
///
/// A WAL encoding change (added field, new op tag) does NOT
/// invalidate existing manifest lines; a manifest schema change
/// (added key, renamed field) does NOT invalidate existing
/// `.wal` snapshots.  Coupling the two under a single version
/// would force every WAL bump to invalidate manifest lines
/// (H-4 signatures unnecessarily) and vice versa.
///
/// # Wire-format contract
///
/// - The version is embedded as `"v":<n>` in the JSON line so
///   readers can multi-decode across versions cleanly.  A future
///   reader that understands versions 1..=N routes each line
///   through the version-appropriate decoder.
/// - Bumping this value is a coordinated upgrade — producers
///   and consumers on the network MUST upgrade before the
///   change activates, or joiners will refuse post-upgrade
///   manifest lines.
/// - The version is separate from consensus fold registration
///   because the manifest is a LOCAL-STORAGE artifact — a
///   joiner never trusts a manifest line without verifying the
///   referenced snapshot bytes directly against the on-chain
///   `WalSnapshotWrite` root.  Manifest divergence between
///   validators does not fork the state; it degrades peer
///   discovery.
///
/// # Version history
///
/// - `1`: initial layout.  Fields: `v`, `block_number`, `root`
///   (hex string or null), `entries`, `ts_ms`, `sig` (optional
///   hex string, populated by a later slice).
pub const MANIFEST_FORMAT_VERSION: u8 = 1;

/// Filename of the manifest inside the snapshot directory.
///
/// A single manifest per snapshot dir; appended to (`O_APPEND`)
/// by the writer machinery in a later slice.
pub const MANIFEST_FILENAME: &str = "manifest.jsonl";

/// Encoded WAL slice + its two consensus-observable hashes.
///
/// Returned from [`snapshot_blob`] as the canonical bundle
/// consumers hand to disk writers / peer-fetch responders /
/// on-chain commitment sites.  The three fields serve distinct
/// purposes:
///
///   - `bytes`: the canonical encoding (`encode_wal_slice`
///     output).  Byte-identical across validators for identical
///     input.
///   - `root`: `Blake2b256(bytes)` — the whole-snapshot content
///     hash.  This is the identifier a joining validator supplies
///     to `get_snapshot_by_root` (yet-to-land wire opcode) and
///     what the on-chain `WalSnapshotWrite` finalization effect
///     commits.
///   - `merkle_root`: Merkle root over per-4-MiB-chunk hashes (via
///     `super::snapshot_chunk`).  Anchors chunk-fetch verification
///     — a joining validator downloading a snapshot in chunks
///     verifies each 4 MiB piece against this root before applying.
///     Empty input yields the empty-snapshot sentinel `[0u8; 32]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotBlob {
    pub bytes: Vec<u8>,
    pub root: [u8; 32],
    pub merkle_root: [u8; 32],
}

/// Encode a WAL slice to canonical bytes.  Deterministic across
/// validators for identical `entries` input.
///
/// Layout: `[SNAPSHOT_FORMAT_VERSION: u8][count: u32-be][entry × count]`.
/// The leading version byte lets the slice-2 decoder reject
/// unknown-version blobs cleanly instead of mis-decoding.
///
/// Per-entry layout is documented in the module-level docstring.
pub fn encode_wal_slice(entries: &[WalEntry]) -> Vec<u8> {
    // Cap at u32::MAX entries.  In practice `MAX_WAL_ENTRIES` =
    // 65_536 per runtime keeps this well below the cap; the
    // `.expect` fires only if a caller assembles a slice past
    // that (a code bug).
    let count: u32 = entries
        .len()
        .try_into()
        .expect("WAL slice exceeds u32::MAX entries — impossible under MAX_WAL_ENTRIES cap");
    let mut buf = Vec::with_capacity(1 + 4 + entries.len() * 96);
    buf.push(SNAPSHOT_FORMAT_VERSION);
    buf.extend_from_slice(&count.to_be_bytes());
    for e in entries {
        encode_entry(e, &mut buf);
    }
    buf
}

/// Content-addressed hash of the canonical encoding.
/// `Blake2b256(encode_wal_slice(entries))`.
pub fn compute_wal_root(entries: &[WalEntry]) -> [u8; 32] {
    let bytes = encode_wal_slice(entries);
    hash_of(&bytes)
}

/// Encode + hash + Merkle-root together.  Returns the canonical
/// bundle used by writers / responders / on-chain commitment.
///
/// The Merkle root is computed via `super::snapshot_chunk::
/// snapshot_merkle_root` over 4 MiB chunk hashes.  Empty input
/// yields the `[0u8; 32]` sentinel (chunker's empty-snapshot
/// contract).
pub fn snapshot_blob(entries: &[WalEntry]) -> SnapshotBlob {
    use super::snapshot_chunk::{chunk_snapshot, snapshot_merkle_root};
    let bytes = encode_wal_slice(entries);
    let root = hash_of(&bytes);
    let chunk_hashes: Vec<[u8; 32]> = chunk_snapshot(&bytes).into_iter().map(|c| c.hash).collect();
    let merkle_root = snapshot_merkle_root(&chunk_hashes);
    SnapshotBlob {
        bytes,
        root,
        merkle_root,
    }
}

fn hash_of(bytes: &[u8]) -> [u8; 32] {
    let h = Blake2b256::hash(bytes.to_vec());
    assert_eq!(h.len(), 32, "Blake2b256 must produce 32-byte digest");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

fn encode_entry(e: &WalEntry, buf: &mut Vec<u8>) {
    buf.push(op_tag(e.op));
    encode_str_bytes(path_bytes::to_bytes(&e.path), buf);
    match &e.extra_path {
        None => buf.push(0),
        Some(p) => {
            buf.push(1);
            encode_str_bytes(path_bytes::to_bytes(p), buf);
        }
    }
    encode_opt_u64(e.offset, buf);
    encode_opt_u64(e.length, buf);
    encode_payload_ref(&e.payload_ref, buf);
    encode_opt_u32(e.mode_bits, buf);
    encode_opt_str(e.owner.as_deref(), buf);
    encode_opt_str(e.group.as_deref(), buf);
    encode_outcome(e.outcome, buf);
}

/// Explicit numeric wire tags for each `WalOp` — separate from
/// `WalOp`'s `#[repr(u8)]` discriminants (which are `0..=15`).
/// The two sequences are historically offset (wire tags start at
/// 1; discriminants at 0) because the wire tag was defined first.
/// A test in the pin block below asserts the mapping is bijective
/// and covers every variant; renumbering ANY tag is a hard fork
/// of the WAL root — the docstring on each match arm names the
/// slice that introduced it.
fn op_tag(op: WalOp) -> u8 {
    match op {
        WalOp::Write => 1,
        WalOp::WriteAt => 2,
        WalOp::Truncate => 3,
        WalOp::Chmod => 4,
        WalOp::Chown => 5,
        WalOp::RemoveFile => 6,
        WalOp::RemoveDir => 7,
        WalOp::Rename => 8,
        WalOp::CopyFile => 9,
        WalOp::Read => 10,
        WalOp::ReadAt => 11,
        // Consensus-mode state-read journaling: Stat/Entries/Size
        // at tags 12/13/14 (version 3 in fileio's history).
        WalOp::Stat => 12,
        WalOp::Entries => 13,
        WalOp::Size => 14,
        // EntriesStreamNext at tag 15 (version 4).
        WalOp::EntriesStreamNext => 15,
        // Exists at tag 16 (version 6, post-Consensus-ban-lift).
        WalOp::Exists => 16,
        WalOp::BulkApply => 17,
    }
}

fn encode_outcome(o: WalOutcome, buf: &mut Vec<u8>) {
    // Tag 0 = Success (no payload); tag 1 = Failure + u32-be code.
    // Renumbering here is a hard fork of the WAL root.
    match o {
        WalOutcome::Success => buf.push(0),
        WalOutcome::Failure { code } => {
            buf.push(1);
            buf.extend_from_slice(&code.to_be_bytes());
        }
    }
}

fn encode_str_bytes(s: &[u8], buf: &mut Vec<u8>) {
    let n: u32 = s
        .len()
        .try_into()
        .expect("string exceeds u32::MAX bytes — impossible for a filesystem path");
    buf.extend_from_slice(&n.to_be_bytes());
    buf.extend_from_slice(s);
}

fn encode_opt_str(s: Option<&str>, buf: &mut Vec<u8>) {
    match s {
        None => buf.push(0),
        Some(s) => {
            buf.push(1);
            encode_str_bytes(s.as_bytes(), buf);
        }
    }
}

fn encode_opt_u64(v: Option<u64>, buf: &mut Vec<u8>) {
    match v {
        None => buf.push(0),
        Some(n) => {
            buf.push(1);
            buf.extend_from_slice(&n.to_be_bytes());
        }
    }
}

fn encode_opt_u32(v: Option<u32>, buf: &mut Vec<u8>) {
    match v {
        None => buf.push(0),
        Some(n) => {
            buf.push(1);
            buf.extend_from_slice(&n.to_be_bytes());
        }
    }
}

fn encode_payload_ref(r: &Option<PayloadRef>, buf: &mut Vec<u8>) {
    match r {
        None => buf.push(0),
        Some(PayloadRef::Hash(h)) => {
            buf.push(1);
            buf.extend_from_slice(h);
        }
        Some(PayloadRef::DeployRef {
            block_hash,
            deploy_index,
            arg_index,
        }) => {
            buf.push(2);
            buf.extend_from_slice(block_hash);
            buf.extend_from_slice(&deploy_index.to_be_bytes());
            buf.extend_from_slice(&arg_index.to_be_bytes());
        }
    }
}

/// Cross-platform path ↔ bytes conversion helpers used by both
/// the encoder (`encode_entry` calls `to_bytes`) and the decoder
/// (`decode_entry` calls `from_bytes`).
///
/// # Discipline
///
/// On unix, uses the raw `OsStr` byte representation — path bytes
/// are opaque and can contain non-UTF-8.  On non-unix (nominally
/// covered but the io tree is unix-only in production via
/// `path/mod.rs`'s `compile_error!`), falls through to lossy
/// UTF-8 handling — kept for editor-tooling / rust-analyzer
/// buildability on macOS-hosted development against a linux
/// target set, not for production.
///
/// The unix `from_bytes` + `to_bytes` are inverses: the round-
/// trip is byte-preserving.  Downstream `decode_wal_slice`
/// tests exercise this via the diverse-fixture round-trip.
mod path_bytes {
    use std::path::PathBuf;

    #[cfg(unix)]
    pub fn to_bytes(p: &PathBuf) -> &[u8] {
        use std::os::unix::ffi::OsStrExt;
        p.as_os_str().as_bytes()
    }

    #[cfg(not(unix))]
    pub fn to_bytes(p: &PathBuf) -> &[u8] { p.to_str().unwrap_or("").as_bytes() }

    #[cfg(unix)]
    pub fn from_bytes(bytes: &[u8]) -> PathBuf {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(OsStr::from_bytes(bytes))
    }

    #[cfg(not(unix))]
    pub fn from_bytes(bytes: &[u8]) -> PathBuf {
        PathBuf::from(std::str::from_utf8(bytes).unwrap_or(""))
    }
}

// ===========================================================
// Decoder (slice 2)
// ===========================================================

/// Failures surfaced by the decoder + downstream on-disk reader
/// (slice 3+).
#[derive(Debug)]
pub enum SnapshotError {
    /// Wrapping I/O error from the on-disk reader (slice 3+
    /// consumers).  This variant is exposed here so
    /// `decode_wal_slice` callers who ALSO read from disk can
    /// use a single error type across the boundary.
    Io(std::io::Error),
    /// Downloaded snapshot's Blake2b256 root doesn't match the
    /// expected value from the on-chain commitment.  A joiner
    /// surfacing this error treats the assembled snapshot as
    /// byzantine and re-fetches from a different peer set.
    RootMismatch { expected: [u8; 32], got: [u8; 32] },
    /// The on-disk (or fetched) blob starts with a version byte
    /// this validator does not recognize.  A joiner running an
    /// older binary can see a snapshot produced by a newer, hard-
    /// forked network — surface a clean error rather than mis-
    /// decoding.
    UnsupportedVersion { got: u8, supported: u8 },
    /// Blob is too short to satisfy the declared entry count / a
    /// field's declared length.  `got` is the observed byte
    /// count; `need` is the minimum required.
    Truncated { got: usize, need: usize },
    /// `decode_wal_slice` encountered a byte pattern that doesn't
    /// match any valid encoding.  Includes an unknown op tag, an
    /// unknown `PayloadRef` variant tag, an unknown outcome tag,
    /// an unknown `Option` presence tag, or a non-UTF-8 owner /
    /// group string.  A joiner surfacing this error should treat
    /// the assembled snapshot as byzantine and re-fetch from a
    /// different peer set.
    MalformedBlob { offset: usize, message: String },
    /// [`read_manifest`] encountered a malformed line at
    /// (1-based) `line`.  Distinct from [`Io`] so the
    /// join-protocol layer can pattern-match manifest schema
    /// issues (retry with a different peer's manifest) apart
    /// from real I/O failures (disk problem, escalate).  `cause`
    /// carries the underlying [`ManifestEntry::from_line`] error
    /// text.
    MalformedManifest { line: usize, cause: String },
    /// [`ManifestEntry::verify_with_pubkey`] was called on an
    /// entry whose `sig` field is `None`.  Join clients MUST
    /// reject unsigned entries in production (unless an explicit
    /// "trust local disk" override is set) — an unsigned entry
    /// is indistinguishable from one with a stripped signature.
    UnsignedManifestEntry,
    /// Signature verification failed: the `sig` field is present
    /// but does not match the public key over the entry's
    /// canonical [`ManifestEntry::sign_bytes`].  Causes:
    /// tampering (sig forged for different field values),
    /// wrong public key for this entry's writer, or a signature
    /// over a different canonical encoding (version drift).
    /// Distinct from [`UnsignedManifestEntry`] so the join
    /// protocol can diagnose "unsigned by writer" vs. "signed
    /// but doesn't verify."
    ManifestSignatureInvalid,
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Io(e) => write!(f, "snapshot I/O error: {e}"),
            SnapshotError::RootMismatch { expected, got } => write!(
                f,
                "snapshot root mismatch: expected {}, got {}",
                hex_short(expected),
                hex_short(got)
            ),
            SnapshotError::UnsupportedVersion { got, supported } => write!(
                f,
                "snapshot format version {got} not supported by this validator \
                 (understands version {supported}); a coordinated upgrade may be needed"
            ),
            SnapshotError::Truncated { got, need } => write!(
                f,
                "snapshot blob truncated: {got} bytes, need at least {need}"
            ),
            SnapshotError::MalformedBlob { offset, message } => {
                write!(f, "snapshot blob malformed at offset {offset}: {message}")
            }
            SnapshotError::MalformedManifest { line, cause } => {
                write!(f, "manifest line {line}: {cause}")
            }
            SnapshotError::UnsignedManifestEntry => {
                write!(f, "manifest entry is unsigned (no sig field)")
            }
            SnapshotError::ManifestSignatureInvalid => {
                write!(f, "manifest entry signature verification failed")
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<std::io::Error> for SnapshotError {
    fn from(e: std::io::Error) -> Self { SnapshotError::Io(e) }
}

// Compile-time witness that `SnapshotError: Send + Sync`.
// Hoisted to module scope so every `cargo build` catches a
// regression, not only `cargo test`.  Required for propagation
// across `tokio::spawn_blocking` boundaries in the (yet-to-land)
// on-disk snapshot reader.  Same pattern as `_WAL_IS_SEND_SYNC`
// (wal.rs) and `_FILE_HANDLE_TABLE_IS_SEND_SYNC` (handle_table.rs).
const _SNAPSHOT_ERROR_IS_SEND_SYNC: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SnapshotError>();
};

/// Render the first 8 bytes of a hash as lowercase hex — the
/// short form used in operator-facing `RootMismatch` messages
/// (16 chars balances readability with disambiguation).
fn hex_short(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(16);
    for b in &bytes[..8] {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Decode canonical WAL bytes back to a `Vec<WalEntry>`.
/// Symmetric inverse of [`encode_wal_slice`]; used by joiners
/// applying a snapshot to a fresh tree (see
/// [`super::wal_applier`]).
///
/// # Version handling
///
/// Only accepts `SNAPSHOT_FORMAT_VERSION`.  A newer version byte
/// returns [`SnapshotError::UnsupportedVersion`] — this triage
/// branch does NOT carry any legacy-version fallback; the
/// production port will decide per-bump whether to keep prior
/// versions as read-only decoders.
///
/// # Errors
///
/// - [`SnapshotError::Truncated`] — insufficient bytes to satisfy
///   the declared entry count or a field's declared length.
/// - [`SnapshotError::UnsupportedVersion`] — leading version byte
///   doesn't match `SNAPSHOT_FORMAT_VERSION`.
/// - [`SnapshotError::MalformedBlob`] — an op tag, `PayloadRef`
///   variant tag, presence tag, or outcome tag outside the
///   allowed set, OR a non-UTF-8 owner / group string.
pub fn decode_wal_slice(bytes: &[u8]) -> Result<Vec<WalEntry>, SnapshotError> {
    if bytes.is_empty() {
        return Err(SnapshotError::Truncated { got: 0, need: 1 });
    }
    let version = bytes[0];
    if version != SNAPSHOT_FORMAT_VERSION {
        return Err(SnapshotError::UnsupportedVersion {
            got: version,
            supported: SNAPSHOT_FORMAT_VERSION,
        });
    }
    if bytes.len() < 5 {
        return Err(SnapshotError::Truncated {
            got: bytes.len(),
            need: 5,
        });
    }
    let count = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
    let mut cursor = 5usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let entry = decode_entry(bytes, &mut cursor)?;
        entries.push(entry);
    }
    Ok(entries)
}

fn decode_entry(bytes: &[u8], cursor: &mut usize) -> Result<WalEntry, SnapshotError> {
    let op = decode_op_tag(bytes, cursor)?;
    let path_bytes = decode_str_bytes(bytes, cursor)?;
    let path = path_bytes::from_bytes(path_bytes);
    let extra_path = match decode_u8(bytes, cursor)? {
        0 => None,
        1 => Some(path_bytes::from_bytes(decode_str_bytes(bytes, cursor)?)),
        tag => {
            return Err(SnapshotError::MalformedBlob {
                offset: *cursor - 1,
                message: format!("extra_path presence tag must be 0 or 1, got {tag}"),
            })
        }
    };
    let offset = decode_opt_u64(bytes, cursor)?;
    let length = decode_opt_u64(bytes, cursor)?;
    let payload_ref = decode_payload_ref(bytes, cursor)?;
    let mode_bits = decode_opt_u32(bytes, cursor)?;
    let owner = decode_opt_str(bytes, cursor)?;
    let group = decode_opt_str(bytes, cursor)?;
    let outcome = decode_outcome(bytes, cursor)?;
    Ok(WalEntry {
        op,
        path,
        extra_path,
        offset,
        length,
        payload_ref,
        mode_bits,
        owner,
        group,
        outcome,
    })
}

fn decode_op_tag(bytes: &[u8], cursor: &mut usize) -> Result<WalOp, SnapshotError> {
    let tag = decode_u8(bytes, cursor)?;
    match tag {
        1 => Ok(WalOp::Write),
        2 => Ok(WalOp::WriteAt),
        3 => Ok(WalOp::Truncate),
        4 => Ok(WalOp::Chmod),
        5 => Ok(WalOp::Chown),
        6 => Ok(WalOp::RemoveFile),
        7 => Ok(WalOp::RemoveDir),
        8 => Ok(WalOp::Rename),
        9 => Ok(WalOp::CopyFile),
        10 => Ok(WalOp::Read),
        11 => Ok(WalOp::ReadAt),
        12 => Ok(WalOp::Stat),
        13 => Ok(WalOp::Entries),
        14 => Ok(WalOp::Size),
        15 => Ok(WalOp::EntriesStreamNext),
        16 => Ok(WalOp::Exists),
        17 => Ok(WalOp::BulkApply),
        _ => Err(SnapshotError::MalformedBlob {
            offset: *cursor - 1,
            message: format!("unknown op tag {tag}"),
        }),
    }
}

fn decode_str_bytes<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], SnapshotError> {
    let n = decode_u32(bytes, cursor)? as usize;
    let end = cursor
        .checked_add(n)
        .ok_or_else(|| SnapshotError::MalformedBlob {
            offset: *cursor,
            message: "string length overflow".into(),
        })?;
    if end > bytes.len() {
        return Err(SnapshotError::Truncated {
            got: bytes.len(),
            need: end,
        });
    }
    let slice = &bytes[*cursor..end];
    *cursor = end;
    Ok(slice)
}

fn decode_opt_str(bytes: &[u8], cursor: &mut usize) -> Result<Option<String>, SnapshotError> {
    match decode_u8(bytes, cursor)? {
        0 => Ok(None),
        1 => {
            let s_bytes = decode_str_bytes(bytes, cursor)?;
            let s = std::str::from_utf8(s_bytes)
                .map_err(|_| SnapshotError::MalformedBlob {
                    offset: *cursor - s_bytes.len(),
                    message: "opt-str field is not valid UTF-8".into(),
                })?
                .to_string();
            Ok(Some(s))
        }
        tag => Err(SnapshotError::MalformedBlob {
            offset: *cursor - 1,
            message: format!("opt-str presence tag must be 0 or 1, got {tag}"),
        }),
    }
}

fn decode_opt_u64(bytes: &[u8], cursor: &mut usize) -> Result<Option<u64>, SnapshotError> {
    match decode_u8(bytes, cursor)? {
        0 => Ok(None),
        1 => Ok(Some(decode_u64(bytes, cursor)?)),
        tag => Err(SnapshotError::MalformedBlob {
            offset: *cursor - 1,
            message: format!("opt-u64 presence tag must be 0 or 1, got {tag}"),
        }),
    }
}

fn decode_opt_u32(bytes: &[u8], cursor: &mut usize) -> Result<Option<u32>, SnapshotError> {
    match decode_u8(bytes, cursor)? {
        0 => Ok(None),
        1 => Ok(Some(decode_u32(bytes, cursor)?)),
        tag => Err(SnapshotError::MalformedBlob {
            offset: *cursor - 1,
            message: format!("opt-u32 presence tag must be 0 or 1, got {tag}"),
        }),
    }
}

fn decode_payload_ref(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<Option<PayloadRef>, SnapshotError> {
    match decode_u8(bytes, cursor)? {
        0 => Ok(None),
        1 => {
            let h = decode_fixed_32(bytes, cursor)?;
            Ok(Some(PayloadRef::Hash(h)))
        }
        2 => {
            let block_hash = decode_fixed_32(bytes, cursor)?;
            let deploy_index = decode_u32(bytes, cursor)?;
            let arg_index = decode_u32(bytes, cursor)?;
            Ok(Some(PayloadRef::DeployRef {
                block_hash,
                deploy_index,
                arg_index,
            }))
        }
        tag => Err(SnapshotError::MalformedBlob {
            offset: *cursor - 1,
            message: format!("payload_ref variant tag must be 0/1/2, got {tag}"),
        }),
    }
}

fn decode_outcome(bytes: &[u8], cursor: &mut usize) -> Result<WalOutcome, SnapshotError> {
    match decode_u8(bytes, cursor)? {
        0 => Ok(WalOutcome::Success),
        1 => {
            let code = decode_u32(bytes, cursor)?;
            Ok(WalOutcome::Failure { code })
        }
        tag => Err(SnapshotError::MalformedBlob {
            offset: *cursor - 1,
            message: format!("outcome tag must be 0 or 1, got {tag}"),
        }),
    }
}

fn decode_u8(bytes: &[u8], cursor: &mut usize) -> Result<u8, SnapshotError> {
    if *cursor >= bytes.len() {
        return Err(SnapshotError::Truncated {
            got: bytes.len(),
            need: *cursor + 1,
        });
    }
    let v = bytes[*cursor];
    *cursor += 1;
    Ok(v)
}

fn decode_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, SnapshotError> {
    let end = *cursor + 4;
    if end > bytes.len() {
        return Err(SnapshotError::Truncated {
            got: bytes.len(),
            need: end,
        });
    }
    let v = u32::from_be_bytes([
        bytes[*cursor],
        bytes[*cursor + 1],
        bytes[*cursor + 2],
        bytes[*cursor + 3],
    ]);
    *cursor = end;
    Ok(v)
}

fn decode_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, SnapshotError> {
    let end = *cursor + 8;
    if end > bytes.len() {
        return Err(SnapshotError::Truncated {
            got: bytes.len(),
            need: end,
        });
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[*cursor..end]);
    *cursor = end;
    Ok(u64::from_be_bytes(buf))
}

fn decode_fixed_32(bytes: &[u8], cursor: &mut usize) -> Result<[u8; 32], SnapshotError> {
    let end = *cursor + 32;
    if end > bytes.len() {
        return Err(SnapshotError::Truncated {
            got: bytes.len(),
            need: end,
        });
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes[*cursor..end]);
    *cursor = end;
    Ok(out)
}

// ===========================================================
// Disk I/O (slice 3) — content-addressed read + write
// ===========================================================

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-process monotonic counter used in tmp filenames.  Belt-and-
/// suspenders alongside the nanosecond stamp: an NTP backward jump
/// could yield the same `now_nanos` for two back-to-back writes;
/// the counter guarantees distinct tmp paths regardless of clock.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Atomic file write with tmp+rename+fsync + parent-dir fsync
/// discipline.  Shared by [`write_snapshot`] (`.wal`) and
/// [`write_hashes_sidecar`] (`.hashes`).
///
/// # Discipline
///
///   1. `create_dir_all` on the parent (recover from missing dir).
///   2. Write to a per-process, per-invocation tmp path
///      (`{stem}.{pid}-{nanos}-{counter}{tmp_suffix}`).
///   3. `fsync` the tmp file (durability before rename — a crash
///      between write and rename otherwise leaves a
///      page-cache-only file that reads as truncated).
///   4. `rename` to `final_path` (POSIX-atomic on the same
///      filesystem).
///   5. Open the parent dir + `fsync` (POSIX: rename's atomicity
///      does NOT imply metadata durability).
///
/// Dir-fsync failures log at `debug` rather than surfacing —
/// tmpfs / some network filesystems reject dir fsync with a
/// legitimate errno; treating those as errors would spam warnings.
///
/// `fallback_stem` is used when `final_path.file_stem()` is not
/// valid UTF-8 (unreachable for the hex-derived paths this module
/// generates, but keeps the helper total).
fn atomic_write_file(
    final_path: &Path,
    tmp_suffix: &str,
    fallback_stem: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    use std::io::Write as _;

    if let Some(parent) = final_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!(
        "{}.{}-{}-{}{}",
        final_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(fallback_stem),
        std::process::id(),
        now_nanos,
        counter,
        tmp_suffix
    );
    let tmp_path = final_path.with_file_name(tmp_name);

    // Write + fsync tmp file.  `create_new(true)` — a collision
    // on the per-process/per-nanos/per-counter tmp path indicates
    // a serious clock or PID malfunction; surface it rather than
    // overwriting.
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o644);
        }
        let mut file = opts.open(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    std::fs::rename(&tmp_path, final_path)?;

    if let Some(parent) = final_path.parent() {
        match std::fs::File::open(parent) {
            Ok(dir_file) => {
                if let Err(e) = dir_file.sync_all() {
                    tracing::debug!(
                        target: "f1r3fly.fs_wal.atomic_write",
                        parent = %parent.display(),
                        error = %e,
                        "dir fsync after rename failed (fs may not support dir fsync)"
                    );
                }
            }
            Err(e) => {
                tracing::debug!(
                    target: "f1r3fly.fs_wal.atomic_write",
                    parent = %parent.display(),
                    error = %e,
                    "opening parent dir for fsync failed"
                );
            }
        }
    }

    Ok(())
}

/// Content-addressed on-disk snapshot path.
///
/// Layout: `{snapshot_dir}/{root_hex}.wal`.  The filename IS the
/// content hash, so a joining validator can request a snapshot
/// by root and verify byte-for-byte after fetch.  Full 64-char
/// (256-bit) hex — no truncation, so accidental collision is
/// impossible under Blake2b256's preimage resistance.
pub fn snapshot_path(snapshot_dir: &Path, root: &[u8; 32]) -> PathBuf {
    let mut hex = String::with_capacity(64);
    for b in root {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    snapshot_dir.join(format!("{hex}.wal"))
}

/// Write a snapshot to `snapshot_dir` under its content-addressed
/// filename.  Returns `(path, root, merkle_root)` where `root` is
/// the atomic Blake2b256 of the whole blob (drives the filename)
/// and `merkle_root` is the Phase 7b-1 Merkle root over 4 MiB
/// chunk hashes (used by joiners to verify chunks fetched via
/// the yet-to-land wire opcode).
///
/// # Atomic tmp+rename
///
/// A crash mid-write could leave a partial file at the final path
/// that the read-time root check would reject as
/// [`SnapshotError::RootMismatch`] — but that's a noisy
/// false-alarm.  Instead, the write goes through the shared
/// [`atomic_write_file`] helper (tmp write + fsync + rename +
/// parent-dir fsync).  Tmp suffix: `.wal.tmp`.
///
/// # Idempotent
///
/// Writing the same content twice produces the same final path
/// (content-addressed).  Two concurrent writes with distinct
/// tmp files still race on the final rename, but since the
/// content is byte-identical the outcome is one of two
/// identical files — observationally indistinguishable.
///
/// # Explicit mode 0o644 on unix
///
/// Snapshot files get an explicit `0o644` mode so the leader's
/// umask does NOT leak to any joiner reading over shared
/// storage.  Content is deterministic across validators; the
/// file metadata should be too.
///
/// # Payload-hash sidecar (best-effort tail)
///
/// After the snapshot blob is durably renamed into place, this
/// function computes the set of payload hashes referenced by
/// `entries` (via [`referenced_payload_hashes`]) and writes a
/// colocated `.hashes` sidecar (via [`write_hashes_sidecar`]).
/// The sidecar exists so retention passes can union the payload
/// hashes referenced across every retained snapshot without
/// having to decode the full WAL bytes each pass.
///
/// Sidecar failures are logged at `warn` but do NOT fail the
/// snapshot write: the snapshot bytes are already durable, so
/// the worst case is a missing sidecar → the corresponding
/// payload hashes go un-counted on the next retention pass →
/// safe over-eager delete of the payload store entries a fresh
/// joiner could have fetched from another peer anyway.
pub fn write_snapshot(
    snapshot_dir: &Path,
    entries: &[WalEntry],
) -> Result<(PathBuf, [u8; 32], [u8; 32]), SnapshotError> {
    let blob = snapshot_blob(entries);
    let final_path = snapshot_path(snapshot_dir, &blob.root);

    atomic_write_file(&final_path, ".wal.tmp", "snapshot", &blob.bytes)?;

    // Payload-hash sidecar — best-effort.  The snapshot bytes are
    // already durable at this point; a sidecar failure logs at
    // warn but does not fail the snapshot write.  See the
    // "Payload-hash sidecar" section of this function's docstring.
    let referenced = referenced_payload_hashes(entries);
    let sidecar_path = hashes_sidecar_path(snapshot_dir, &blob.root);
    if let Err(e) = write_hashes_sidecar(&sidecar_path, &referenced) {
        tracing::warn!(
            target: "f1r3fly.fs_wal.payload_store",
            path = %sidecar_path.display(),
            error = %e,
            "hashes sidecar write failed; payload retention will \
             miss this snapshot's referenced hashes on the next pass"
        );
    }

    Ok((final_path, blob.root, blob.merkle_root))
}

/// Read + verify a snapshot from `snapshot_dir` by its content-
/// addressed root.  Returns the raw bytes (still encoded — call
/// [`decode_wal_slice`] to get `Vec<WalEntry>`).
///
/// # Two-layer verification
///
/// 1. **Hash check** — recompute `Blake2b256(bytes)` and
///    compare against the requested `root`.  Mismatch returns
///    [`SnapshotError::RootMismatch`].
/// 2. **Version check** — leading byte matches
///    `SNAPSHOT_FORMAT_VERSION`.  Mismatch returns
///    [`SnapshotError::UnsupportedVersion`].
///
/// # Why hash-first ordering (diagnostic clarity, not security)
///
/// Both orderings are equally secure — an attacker needs
/// `hash(bytes) == root` either way, and a version-first check
/// would actually add a constraint on the attacker rather than
/// weakening one.  The choice is about which error surfaces
/// FIRST for the common byzantine-peer scenario and what
/// diagnostic action that implies:
///
///   - **Hash-first (this ordering)**: byzantine peer serves
///     corrupt bytes → `RootMismatch` → operator knows to
///     re-fetch from a different peer set.
///   - **Version-first (rejected)**: byzantine peer serves
///     corrupt bytes with a happen-to-match version byte →
///     `RootMismatch`; with a wrong version byte →
///     `UnsupportedVersion` — misleading, since it makes a
///     byzantine-peer situation look like a fleet-upgrade
///     issue.
///
/// The operator's remediation differs by variant:
/// `RootMismatch` → "re-fetch from another peer";
/// `UnsupportedVersion` → "the fleet needs a coordinated
/// upgrade."  Serving the wrong diagnostic for a
/// byzantine-peer scenario would send the operator down the
/// wrong triage path.
pub fn read_snapshot_bytes(snapshot_dir: &Path, root: &[u8; 32]) -> Result<Vec<u8>, SnapshotError> {
    let path = snapshot_path(snapshot_dir, root);
    let bytes = std::fs::read(&path)?;
    let got = hash_of(&bytes);
    if got != *root {
        return Err(SnapshotError::RootMismatch {
            expected: *root,
            got,
        });
    }
    // Version check post-hash (see docstring).
    let Some(&version) = bytes.first() else {
        return Err(SnapshotError::Truncated { got: 0, need: 1 });
    };
    if version != SNAPSHOT_FORMAT_VERSION {
        return Err(SnapshotError::UnsupportedVersion {
            got: version,
            supported: SNAPSHOT_FORMAT_VERSION,
        });
    }
    Ok(bytes)
}

/// Extract the set of unique payload hashes referenced by a WAL
/// slice.  Skips entries whose `payload_ref` is `None` or
/// `DeployRef` — only the `Hash` variant references bytes that
/// live in the payload store.  Deduplicates via `HashSet`.
///
/// Threaded through [`write_snapshot`] (see [`write_hashes_sidecar`])
/// so the sidecar records which payloads a given snapshot
/// references, letting payload-store retention union across all
/// retained snapshots without decoding the full WAL bytes each
/// pass.
pub fn referenced_payload_hashes(entries: &[WalEntry]) -> std::collections::HashSet<[u8; 32]> {
    let mut set = std::collections::HashSet::new();
    for e in entries {
        if let Some(PayloadRef::Hash(h)) = e.payload_ref {
            set.insert(h);
        }
    }
    set
}

// ===========================================================
// Payload-hash sidecar (slice 4)
// ===========================================================

/// Colocated sidecar path for the snapshot at content-address
/// `root`.  Layout: `{snapshot_dir}/{root_hex}.hashes`.
///
/// Colocation with the snapshot is deliberate: it lets a future
/// pruning pass pair snapshot + sidecar with a simple stem match
/// (`prune_snapshot_dir` removes the paired sidecar when it
/// removes the snapshot; see the pruning slice).
pub fn hashes_sidecar_path(snapshot_dir: &Path, root: &[u8; 32]) -> PathBuf {
    let mut hex = String::with_capacity(64);
    for b in root {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    snapshot_dir.join(format!("{hex}.hashes"))
}

/// Sidecar wire format: `[u32-be count][32-byte hash × count]`.
///
/// Writes atomically via the shared [`atomic_write_file`] helper
/// (same tmp+rename+fsync+dir-fsync discipline as
/// [`write_snapshot`]) so a mid-write crash leaves either no
/// sidecar or a fully-durable one — never partial.
///
/// # Deterministic layout (sorted)
///
/// Hashes are sorted before serialization so two independent
/// invocations that produce equivalent [`HashSet`]s produce
/// byte-identical sidecar files.  This aids diff-review and
/// keeps the sidecar itself content-addressable if we ever need
/// that property.  Set-iteration order is not guaranteed by
/// [`HashSet`], so relying on it would produce non-deterministic
/// on-disk bytes across runs.
fn write_hashes_sidecar(
    sidecar_path: &Path,
    hashes: &std::collections::HashSet<[u8; 32]>,
) -> std::io::Result<()> {
    let count: u32 = hashes.len().try_into().unwrap_or(u32::MAX);
    let mut buf = Vec::with_capacity(4 + hashes.len() * 32);
    buf.extend_from_slice(&count.to_be_bytes());
    let mut sorted: Vec<[u8; 32]> = hashes.iter().copied().collect();
    sorted.sort();
    for h in sorted {
        buf.extend_from_slice(&h);
    }

    atomic_write_file(sidecar_path, ".hashes.tmp", "sidecar", &buf)
}

/// Read a hashes sidecar back into a [`HashSet`].
///
/// # Corrupt-sidecar posture: return empty, log at debug
///
/// A short header (< 4 bytes), a body whose length disagrees
/// with the header count, or a header count exceeding
/// [`MAX_WAL_ENTRIES`] (defensive OOM cap: a snapshot IS a WAL
/// slice, so its referenced-payload cardinality is bounded by
/// the same per-slice cap) returns an empty set + a debug log
/// line.  The sidecar is best-effort, and a corrupt sidecar just
/// means the corresponding snapshot's payloads go un-counted in
/// the retained set on the next pass (over-eager delete, safe).
/// A hard error would force retention to skip the whole pass on
/// a single corrupt file.
///
/// A missing sidecar file surfaces as [`std::io::ErrorKind::NotFound`]
/// through the returned [`std::io::Result`] — callers can
/// distinguish "corrupt, ignore" (Ok(empty)) from "missing,
/// definitely didn't exist" (Err(NotFound)).
pub fn read_hashes_sidecar(
    sidecar_path: &Path,
) -> std::io::Result<std::collections::HashSet<[u8; 32]>> {
    let bytes = std::fs::read(sidecar_path)?;
    let mut set = std::collections::HashSet::new();
    if bytes.len() < 4 {
        tracing::debug!(
            target: "f1r3fly.fs_wal.payload_store",
            path = %sidecar_path.display(),
            len = bytes.len(),
            "hashes sidecar too short for u32-be count header"
        );
        return Ok(set);
    }
    let count = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    // Defensive OOM cap: a corrupt file whose count byte is
    // absurdly large (e.g., 0xFFFFFFFF) would otherwise force us
    // to allocate a multi-GiB HashSet.  A snapshot IS a WAL
    // slice, so its referenced-payload count is bounded above by
    // MAX_WAL_ENTRIES; any file claiming more is definitionally
    // corrupt.
    if count > MAX_WAL_ENTRIES {
        tracing::debug!(
            target: "f1r3fly.fs_wal.payload_store",
            path = %sidecar_path.display(),
            count,
            max = MAX_WAL_ENTRIES,
            "hashes sidecar header count exceeds MAX_WAL_ENTRIES cap; treating as corrupt"
        );
        return Ok(set);
    }
    let expected = 4usize.saturating_add(count.saturating_mul(32));
    if bytes.len() != expected {
        tracing::debug!(
            target: "f1r3fly.fs_wal.payload_store",
            path = %sidecar_path.display(),
            got = bytes.len(),
            expected,
            "hashes sidecar body length disagrees with header count"
        );
        return Ok(set);
    }
    for i in 0..count {
        let start = 4 + i * 32;
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&bytes[start..start + 32]);
        set.insert(buf);
    }
    Ok(set)
}

/// Union the payload hashes referenced by every retained snapshot
/// in `snapshot_dir`, by reading each `.hashes` sidecar.
///
/// Callers pass the returned set to the (yet-to-land) payload-
/// store prune step to delete any non-referenced entries — the
/// union across retained snapshots IS the "still-needed" set.
///
/// # Missing / corrupt handling
///
/// - Missing directory → returns an empty set (not an error),
///   so a fresh install with no snapshots yet doesn't fail
///   retention startup.
/// - Non-`.hashes` entries (`.wal` snapshots themselves,
///   `.wal.tmp` stale writes, unrelated files) → skipped.
/// - Symlinks → skipped (matches the future `prune_snapshot_dir`
///   posture — never chase links from a validator's snapshot
///   directory).
/// - Corrupt sidecar (short header, length mismatch) → skipped
///   silently via [`read_hashes_sidecar`]'s corrupt-returns-empty
///   posture, so one bad sidecar can't take down the whole
///   retention pass.
/// - Individual `read_hashes_sidecar` I/O failures → logged at
///   debug and skipped (the individual file might be
///   mid-rename); the union proceeds.
pub fn scan_retained_payload_hashes(
    snapshot_dir: &Path,
) -> std::io::Result<std::collections::HashSet<[u8; 32]>> {
    let mut union = std::collections::HashSet::new();
    let read_dir = match std::fs::read_dir(snapshot_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(union),
        Err(e) => return Err(e),
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("hashes") {
            continue;
        }
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_symlink() {
            continue;
        }
        match read_hashes_sidecar(&path) {
            Ok(set) => union.extend(set),
            Err(e) => {
                tracing::debug!(
                    target: "f1r3fly.fs_wal.payload_store",
                    path = %path.display(),
                    error = %e,
                    "hashes sidecar read failed; skipping"
                );
            }
        }
    }
    Ok(union)
}

// ===========================================================
// Stale-tmp-file sweep (slice 5)
// ===========================================================

/// Tmp-file suffixes produced by [`atomic_write_file`].  Adding a
/// third writer means adding its suffix here so
/// [`sweep_stale_tmp_files`] cleans up its stale tmp files too.
const TMP_SUFFIXES: &[&str] = &[".wal.tmp", ".hashes.tmp"];

/// Sweep stale tmp files from `snapshot_dir` — files matching a
/// suffix in [`TMP_SUFFIXES`] whose mtime is older than
/// `older_than_secs`.
///
/// # Why this is needed
///
/// [`atomic_write_file`] writes to a tmp path then `rename`s
/// atomically into place.  On a crash BETWEEN the `sync_all` and
/// the `rename` (a small window, but non-zero), the tmp file is
/// durable but the final file was never created; the tmp file
/// lives on disk forever unless something sweeps it.  Neither
/// [`prune_snapshot_dir`] (yet to land — filters on the final
/// `.wal` extension) nor [`scan_retained_payload_hashes`]
/// (filters on `.hashes`) ever GC these leaked tmp files, so
/// they accumulate proportional to crash frequency.
///
/// # Mtime-gated for concurrent safety
///
/// `older_than_secs` filters by mtime — files younger than the
/// cutoff are preserved so this can run concurrently with an
/// in-progress `atomic_write_file` without racing the tmp write
/// itself.  Typical operator cadence: called periodically with
/// a generous threshold (e.g., 1 hour), or on startup after a
/// crash-recovery scan.
///
/// # Failure posture
///
/// Individual `remove_file` failures log at `debug` and do NOT
/// propagate — a permission problem on one leaked file must not
/// abort the sweep of the rest.  The initial `read_dir` failure
/// DOES propagate (caller usually wants to know the whole sweep
/// couldn't start).
///
/// Returns the number of files successfully removed.
pub fn sweep_stale_tmp_files(snapshot_dir: &Path, older_than_secs: u64) -> std::io::Result<usize> {
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(older_than_secs))
        .unwrap_or(std::time::UNIX_EPOCH);
    let mut removed = 0;
    for entry in std::fs::read_dir(snapshot_dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        // Match by full-name suffix (not extension, which is only
        // the segment after the final dot — `foo.wal.tmp` has
        // extension `tmp`, but we want to match the compound
        // `.wal.tmp` to avoid deleting an operator's stray `.tmp`
        // file that we don't own).
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if !TMP_SUFFIXES.iter().any(|s| name.ends_with(s)) {
            continue;
        }
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);
        if mtime >= cutoff {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) => tracing::debug!(
                target: "f1r3fly.fs_wal.atomic_write",
                path = %path.display(),
                error = %e,
                "sweep_stale_tmp_files: remove failed; continuing"
            ),
        }
    }
    Ok(removed)
}

// ===========================================================
// Directory pruning (slice 8) — retention by mtime
// ===========================================================

/// Prune old `.wal` snapshots (and their paired `.hashes`
/// sidecars) from `snapshot_dir`, keeping the `keep_last_n`
/// newest by mtime.  Returns the number of `.wal` files
/// successfully removed.  Sidecar removals are silently
/// side-effected (they contribute to the retention union but
/// not to the return count).
///
/// # Conservative posture — manifest-independent
///
/// This function does NOT consult `<snapshot_dir>/manifest.jsonl`
/// when deciding what to keep.  Retention is purely mtime-
/// based over the actual `.wal` files present on disk.  The
/// rationale ties back to [`append_manifest_entry`]'s no-fsync
/// posture: a crash between a snapshot write and its manifest
/// append could leave a durable `.wal` file whose manifest
/// entry was lost, and a manifest-driven pruner would then
/// over-eagerly delete it.  By using on-disk mtime as the
/// authority, a lost manifest entry cannot orphan its `.wal`
/// file.
///
/// If a future design ever requires manifest-authoritative
/// pruning (e.g., to bound the manifest against on-disk state),
/// [`append_manifest_entry`] must first gain fsync semantics
/// AND the retention protocol must handle the "snapshot on
/// disk but not in any peer's manifest" fetch fallback.
///
/// # Symlink skip — operator hygiene defense
///
/// The scan uses `entry.file_type()` (which under the hood is
/// `lstat`, not `stat`) to detect and skip symlink `.wal`
/// entries; mtimes are read via `symlink_metadata` (also
/// lstat-based) as TOCTOU hardening — a replace-with-symlink
/// race between the file_type check and the metadata read
/// would still read the link's own metadata, not the target's.
/// Rationale: an operator whose snapshot dir contains an
/// attacker-planted `evil.wal -> /etc/passwd` symlink would
/// otherwise let the symlink freshly-touch itself past the
/// mtime cutoff on every scan (and worse, `remove_file` on the
/// symlink would just unlink the symlink — safe, but noisy).
/// Skip cleanly.  The snapshot directory is expected to be
/// exclusively owned by the validator; symlinks are not a
/// supported shape.
///
/// # Sidecar pairing — only on successful `.wal` removal
///
/// Each SUCCESSFULLY removed `.wal` triggers a best-effort
/// remove of the colocated `.hashes` sidecar
/// (`snapshot_path` and `hashes_sidecar_path` share the same
/// stem, so `path.with_extension("hashes")` matches).  ENOENT
/// on the sidecar is silently OK — pre-sidecar snapshots
/// (before PR #507) don't have one, and a mid-flight crash
/// between snapshot write and sidecar write can leave the
/// same shape.  Other sidecar errors log at `warn` but do NOT
/// fail the prune (the `.wal` is already gone; a leaked
/// sidecar just over-counts hashes in the retention union —
/// safe direction).
///
/// # The orphan-`.wal` invariant
///
/// Sidecar removal is deliberately NOT attempted when `.wal`
/// removal fails.  Removing the sidecar for a still-live
/// orphan `.wal` would under-count its payload hashes in the
/// next [`scan_retained_payload_hashes`] pass → the payload
/// store would delete those referenced bytes → the snapshot
/// would become unreadable.  This is the same "orphan
/// payload" hazard the manifest-independent pruning design
/// was built to avoid, surfacing at the `.wal`-removal-failure
/// path.  Pinned by
/// `prune_wal_removal_failure_preserves_paired_sidecar`.
///
/// # Failure posture
///
/// Individual `remove_file` failures on the `.wal` log at
/// `warn` and do NOT propagate — a permission problem on one
/// snapshot must not abort the prune of the rest.  The initial
/// `read_dir` failure DOES propagate (same posture as
/// [`sweep_stale_tmp_files`]).
pub fn prune_snapshot_dir(snapshot_dir: &Path, keep_last_n: usize) -> std::io::Result<usize> {
    let mut wal_files: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(snapshot_dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("wal") {
                return None;
            }
            let file_type = entry.file_type().ok()?;
            if file_type.is_symlink() {
                tracing::debug!(
                    target: "f1r3fly.fs_wal.snapshot",
                    path = %path.display(),
                    "prune_snapshot_dir: skipping symlink .wal entry \
                     (snapshot dir should be exclusively owned)"
                );
                return None;
            }
            std::fs::symlink_metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|mtime| (path, mtime))
        })
        .collect();
    // Newest-first; keep the first `keep_last_n`.  Secondary
    // key on filename (content-hash hex) breaks ties
    // deterministically when two snapshots share an mtime — on
    // filesystems with 1-second mtime resolution, back-to-back
    // writes within the same second would otherwise have
    // read_dir-order-dependent (OS/filesystem-dependent)
    // survivors.
    wal_files.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.file_name().cmp(&b.0.file_name()))
    });

    let mut removed = 0;
    for (path, _) in wal_files.into_iter().skip(keep_last_n) {
        match std::fs::remove_file(&path) {
            Ok(()) => {
                removed += 1;
                // Pair-remove the sidecar ONLY when the `.wal`
                // removal succeeded.  Removing the sidecar for a
                // still-live orphan `.wal` would under-count that
                // snapshot's payload hashes in the next
                // `scan_retained_payload_hashes` pass → the
                // payload store would delete the referenced
                // bytes → the snapshot would become unreadable.
                // See the "Sidecar pairing" docstring section.
                let sidecar_path = path.with_extension("hashes");
                match std::fs::remove_file(&sidecar_path) {
                    Ok(()) => {}
                    Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => tracing::warn!(
                        target: "f1r3fly.fs_wal.payload_store",
                        path = %sidecar_path.display(),
                        error = %e,
                        "prune_snapshot_dir: failed to remove hashes sidecar; \
                         the sidecar will leak but retention correctness is preserved"
                    ),
                }
            }
            Err(e) => tracing::warn!(
                target: "f1r3fly.fs_wal.snapshot",
                path = %path.display(),
                error = %e,
                "prune_snapshot_dir: failed to remove old snapshot; \
                 leaving paired sidecar in place so retention keeps counting its payloads"
            ),
        }
    }
    Ok(removed)
}

// ===========================================================
// Manifest wire format (slice 6) — ManifestEntry + to/from line
// ===========================================================

/// One line in the manifest.  Serialized to a compact JSON
/// object with fixed field ordering by [`ManifestEntry::to_line`]
/// and parsed by the strict-schema [`ManifestEntry::from_line`].
///
/// A manifest advertises "which snapshots exist at which block
/// heights" so joiners can discover fetchable snapshots by
/// scanning peers' manifests instead of guessing content-
/// addressed hashes.  A joiner still verifies each fetched
/// snapshot's bytes against the on-chain `WalSnapshotWrite`
/// root before applying — the manifest is discovery, not
/// authority.
///
/// # Field ordering (wire-format contract)
///
/// The serialization is `{v, block_number, root, entries,
/// ts_ms, [sig]}`.  Adding a field is a coordinated upgrade
/// (bump [`MANIFEST_FORMAT_VERSION`]).  Removing or renaming a
/// field is a hard fork of the manifest wire format.
///
/// # `sig` field
///
/// `sig` is an optional [`Vec<u8>`] carrying a secp256k1
/// signature over the canonicalized non-`sig` fields (H-4).
/// This slice ships the wire-format layer only — the
/// `sign_bytes` / `signed` / `verify_with_pubkey` methods land
/// in a follow-up.  Callers wanting to write an unsigned
/// manifest line pass `sig = None`; the emitted line omits the
/// `sig` field entirely.  On parse, absence of `sig` yields
/// `None`; presence yields `Some(bytes)`.  The join-protocol
/// layer (yet-to-land) MUST reject `None` in production unless
/// an explicit "trust local disk" override is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Block number the snapshot corresponds to.  Under
    /// last-finalized-block cadence (yet-to-land), the
    /// finalized-block height at which this snapshot was written.
    pub block_number: i64,
    /// `Some(root)` for a data snapshot; `None` for the
    /// empty-slice sentinel.
    pub root: Option<[u8; 32]>,
    /// Number of `WalEntry` records the snapshot encodes.  Zero
    /// for the empty sentinel; strictly positive for data.
    pub entries: u64,
    /// Wall-clock write timestamp, ms since UNIX_EPOCH.
    /// Best-effort — a validator whose clock is skewed still
    /// produces a valid manifest, but comparisons across peers
    /// are noisy.
    pub ts_ms: i64,
    /// H-4 optional secp256k1 signature (see struct-level
    /// docstring).  `None` = unsigned; `Some(bytes)` = signed.
    /// This slice ships wire-format only; the actual signing
    /// methods land later.
    pub sig: Option<Vec<u8>>,
}

impl ManifestEntry {
    /// Constructor for a data-snapshot manifest entry.  `ts_ms`
    /// is populated from [`now_ms`] so callers get a wall-clock
    /// stamp without threading a clock parameter.
    pub fn data(block_number: i64, root: [u8; 32], entries: usize) -> Self {
        Self {
            block_number,
            root: Some(root),
            entries: entries as u64,
            ts_ms: now_ms(),
            sig: None,
        }
    }

    /// Constructor for an empty-slice sentinel entry.  Some
    /// blocks produce no WAL entries (no fs syscalls); the
    /// manifest still records them so a joiner can distinguish
    /// "we produced no snapshot at height N" from "we never
    /// finalized height N."
    pub fn empty(block_number: i64) -> Self {
        Self {
            block_number,
            root: None,
            entries: 0,
            ts_ms: now_ms(),
            sig: None,
        }
    }

    /// Serialize to a single JSON line (no trailing newline).
    /// Fixed field order + minimal whitespace so peers parsing
    /// with a hand-rolled reader don't have to canonicalize.
    ///
    /// Field order: `v`, `block_number`, `root`, `entries`,
    /// `ts_ms`, [`sig`].  `sig` is omitted entirely when `None`.
    pub fn to_line(&self) -> String {
        let root_field = match &self.root {
            Some(r) => format!("\"{}\"", hex_encode(r)),
            None => "null".to_string(),
        };
        match &self.sig {
            Some(s) => format!(
                "{{\"v\":{},\"block_number\":{},\"root\":{},\"entries\":{},\"ts_ms\":{},\"sig\":\"{}\"}}",
                MANIFEST_FORMAT_VERSION,
                self.block_number,
                root_field,
                self.entries,
                self.ts_ms,
                hex_encode(s),
            ),
            None => format!(
                "{{\"v\":{},\"block_number\":{},\"root\":{},\"entries\":{},\"ts_ms\":{}}}",
                MANIFEST_FORMAT_VERSION, self.block_number, root_field, self.entries, self.ts_ms,
            ),
        }
    }

    /// Parse a single manifest line.  Rejects any input that
    /// doesn't match the strict schema — a corrupted line
    /// surfaces here rather than mid-catchup.
    ///
    /// # Version handling
    ///
    /// The `"v"` field is mandatory.  A missing `v` is either a
    /// pre-versioned line or a corrupted one; both are treated
    /// as untrusted and rejected so a silent decode of
    /// "someone's future schema as v1" can never happen.  A
    /// `v` value not matching [`MANIFEST_FORMAT_VERSION`] is
    /// rejected with a coordinated-upgrade message.
    ///
    /// # Parser posture
    ///
    /// Hand-rolled — deliberately not `serde` — to keep the
    /// line format independent of any Rust crate's
    /// deserializer behavior and to make the wire format
    /// reproducible in other languages.  Errors are returned as
    /// [`String`] here (the persistence layer, in a later
    /// slice, wraps these into `SnapshotError::Io(InvalidData)`
    /// with the line number).
    pub fn from_line(line: &str) -> Result<Self, String> {
        let trimmed = line.trim();
        let inner = trimmed
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .ok_or_else(|| format!("manifest line missing braces: {line:?}"))?;
        let mut saw_v = false;
        let mut block_number: Option<i64> = None;
        let mut root: Option<Option<[u8; 32]>> = None;
        let mut entries: Option<u64> = None;
        let mut ts_ms: Option<i64> = None;
        let mut sig: Option<Vec<u8>> = None;
        for part in split_top_level_commas(inner) {
            let (key, value) = part
                .split_once(':')
                .ok_or_else(|| format!("manifest kv missing `:` in {part:?}"))?;
            let key = key.trim().trim_matches('"');
            let value = value.trim();
            match key {
                "v" => {
                    let v: u8 = value.parse().map_err(|e| format!("v parse: {e}"))?;
                    if v != MANIFEST_FORMAT_VERSION {
                        return Err(format!(
                            "unsupported manifest version {v} (this validator understands \
                             version {MANIFEST_FORMAT_VERSION}); a coordinated upgrade may \
                             be needed"
                        ));
                    }
                    saw_v = true;
                }
                "block_number" => {
                    block_number = Some(
                        value
                            .parse()
                            .map_err(|e| format!("block_number parse: {e}"))?,
                    );
                }
                "root" => {
                    if value == "null" {
                        root = Some(None);
                    } else {
                        let hex = value.trim_matches('"');
                        if hex.len() != 64 {
                            return Err(format!("root hex must be 64 chars; got {}", hex.len()));
                        }
                        let bytes = hex_decode_32(hex)?;
                        root = Some(Some(bytes));
                    }
                }
                "entries" => {
                    entries = Some(value.parse().map_err(|e| format!("entries parse: {e}"))?);
                }
                "ts_ms" => {
                    ts_ms = Some(value.parse().map_err(|e| format!("ts_ms parse: {e}"))?);
                }
                "sig" => {
                    let hex = value.trim_matches('"');
                    // secp256k1 sigs are DER-ish, variable length (~70-72
                    // bytes typical).  Accept any even-length hex that
                    // decodes cleanly; length validation lives in
                    // `verify_with_pubkey`.
                    if hex.len() % 2 != 0 {
                        return Err(format!("sig hex length must be even; got {}", hex.len()));
                    }
                    let mut bytes = Vec::with_capacity(hex.len() / 2);
                    for i in (0..hex.len()).step_by(2) {
                        let byte = u8::from_str_radix(&hex[i..i + 2], 16)
                            .map_err(|e| format!("sig hex byte {i}: {e}"))?;
                        bytes.push(byte);
                    }
                    sig = Some(bytes);
                }
                other => {
                    return Err(format!("unknown manifest key `{other}`"));
                }
            }
        }
        if !saw_v {
            return Err(format!(
                "missing `v` field (mandatory); expected v = {MANIFEST_FORMAT_VERSION}"
            ));
        }
        Ok(Self {
            block_number: block_number.ok_or("missing block_number")?,
            root: root.ok_or("missing root")?,
            entries: entries.ok_or("missing entries")?,
            ts_ms: ts_ms.ok_or("missing ts_ms")?,
            sig,
        })
    }

    // -------------------------------------------------------
    // H-4 signing (slice 9)
    // -------------------------------------------------------

    /// Canonical byte-encoding of the four non-`sig` fields,
    /// Blake2b256-hashed → the 32-byte message a secp256k1
    /// signature covers.
    ///
    /// # Layout (binary, big-endian)
    ///
    ///   `MANIFEST_FORMAT_VERSION (u8)`
    /// | `block_number (i64 BE)`
    /// | `root presence tag (u8: 0=None, 1=Some)` [+ 32 bytes if 1]
    /// | `entries (u64 BE)`
    /// | `ts_ms (i64 BE)`
    ///
    /// Then `Blake2b256(buf)`.
    ///
    /// # Why binary (not JSON) canonical encoding
    ///
    /// The signed message is deliberately NOT the JSON output
    /// of [`ManifestEntry::to_line`].  JSON is for interop and
    /// operator inspection; binary canonical encoding is for
    /// signature stability.  A future `to_line` tweak (e.g.,
    /// Unicode escape handling, number formatting) would
    /// invalidate every existing signature if signing covered
    /// the JSON text.  Binary covers only the semantic field
    /// values, so a signature survives any wire-format
    /// refactor as long as `MANIFEST_FORMAT_VERSION` doesn't
    /// change.
    ///
    /// # Why include `MANIFEST_FORMAT_VERSION` in the message
    ///
    /// Ties the signature to the specific manifest format.  If
    /// the format ever bumps to v2 with a new field, a v1 sig
    /// cannot be mis-interpreted as a valid v2 sig (the
    /// prepended version byte would differ, Blake2b256 output
    /// would differ, verify would fail).
    pub fn sign_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + 8 + 1 + 32 + 8 + 8);
        buf.push(MANIFEST_FORMAT_VERSION);
        buf.extend_from_slice(&self.block_number.to_be_bytes());
        match &self.root {
            Some(r) => {
                buf.push(1);
                buf.extend_from_slice(r);
            }
            None => buf.push(0),
        }
        buf.extend_from_slice(&self.entries.to_be_bytes());
        buf.extend_from_slice(&self.ts_ms.to_be_bytes());
        Blake2b256::hash(buf)
    }

    /// Return a copy of `self` with the `sig` field populated
    /// by signing [`sign_bytes`] with `sk_bytes` (a 32-byte
    /// secp256k1 secret key).  The caller is responsible for
    /// invoking this before writing to the manifest;
    /// [`SnapshotWriter`] wraps the two-step (sign + append).
    ///
    /// # Signature determinism depends on the Secp256k1 strategy
    ///
    /// This crate's [`crypto::rust::signatures::secp256k1::Secp256k1`]
    /// uses `sign_prehash` (RFC 6979 deterministic nonces), so
    /// two signings of the same entry under the same key
    /// produce byte-identical `sig` bytes — pinned by
    /// `manifest_signed_is_deterministic_rfc6979`.  Do NOT rely
    /// on this cross-strategy: a future swap to a non-RFC-6979
    /// backend (e.g., randomized ECDSA) would make the `sig`
    /// bytes non-deterministic across invocations.  Both bytes
    /// would still verify under the same public key — the
    /// signature's semantic contract is "verifies under this
    /// public key over this message," not "produces identical
    /// bytes."  Consumers checking sig-equality (e.g.,
    /// detecting duplicate manifest entries) MUST use the
    /// semantic fields, not `sig` bytes.
    pub fn signed(mut self, sk_bytes: &[u8]) -> Self {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        let msg = self.sign_bytes();
        let sig = Secp256k1.sign(&msg, sk_bytes);
        self.sig = Some(sig);
        self
    }

    /// Verify the entry's signature against a public key.
    ///
    /// Returns [`SnapshotError::UnsignedManifestEntry`] if
    /// `sig` is `None`; returns
    /// [`SnapshotError::ManifestSignatureInvalid`] if the
    /// signature doesn't verify.  Join-protocol MUST call this
    /// on every manifest line before treating `root` as
    /// authoritative.  The two-variant split lets the join
    /// protocol distinguish "writer never signed" (operator
    /// misconfig or local-disk-tooling edit) from "signature
    /// present but doesn't verify" (tampering, wrong pubkey,
    /// or canonical-encoding drift).
    pub fn verify_with_pubkey(&self, pk_bytes: &[u8]) -> Result<(), SnapshotError> {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        let sig = self
            .sig
            .as_deref()
            .ok_or(SnapshotError::UnsignedManifestEntry)?;
        let msg = self.sign_bytes();
        if !Secp256k1.verify(&msg, sig, pk_bytes) {
            return Err(SnapshotError::ManifestSignatureInvalid);
        }
        Ok(())
    }
}

// ===========================================================
// Manifest persistence (slice 7) — append + read
// ===========================================================

/// Append a manifest entry to `<snapshot_dir>/manifest.jsonl`.
/// Creates the file with `0o644` on first append.
///
/// # O_APPEND semantics
///
/// Uses O_APPEND.  On common Linux filesystems (ext4, xfs), a
/// single `write_all` call for a payload <= PIPE_BUF (4 KiB on
/// Linux, 512 bytes on some BSDs) is atomic against concurrent
/// writers — no torn lines.  A single manifest line fits in
/// ~250 bytes worst case (v + block_number + 64-char hex root +
/// entries + ts_ms + optional 144-char hex sig).
///
/// POSIX itself only guarantees this atomicity for pipes;
/// regular-file behavior is filesystem-specific.  NFSv3 in
/// particular has weaker semantics.  Under the intended
/// single-writer SnapshotWriter cadence the concurrent case is
/// degenerate anyway; the O_APPEND posture is defense-in-depth
/// for operator side-band tooling on common local filesystems.
///
/// # Explicit 0o644 on unix
///
/// Matches [`atomic_write_file`]'s discipline — the leader's
/// umask does not leak to any joiner reading the manifest over
/// shared storage.  Mode applies only on first-create; a
/// subsequent append to an existing differently-moded file
/// does NOT rectify the mode.
///
/// # No fsync — manifest is discovery, not authoritative
///
/// This function does NOT `fsync` the manifest file or the
/// containing directory.  Deliberate: the manifest is a
/// discovery artifact, not consensus authority.  The
/// referenced snapshot bytes themselves are already durable
/// (via [`atomic_write_file`]'s tmp+fsync+rename+dir-fsync
/// discipline).  A crash between this append and the next
/// natural sync loses the manifest entry from local durable
/// storage; the consequences are bounded:
///
///   - **Discovery**: a joiner querying this peer's manifest
///     won't see the lost entry; they can rediscover the
///     snapshot via another peer's manifest, direct-by-root
///     fetch, or subsequent re-appends by the writer.
///   - **Retention**: [`prune_snapshot_dir`] (yet to land)
///     will follow a "keep on-disk `.wal` if referenced by
///     manifest OR present on disk within retention window"
///     posture, so a lost manifest entry does not orphan its
///     `.wal` file to over-eager deletion.
///
/// If a future pruning design instead treats the manifest as
/// authoritative (delete `.wal` files not in manifest), this
/// no-fsync posture becomes unsafe and MUST be revisited.
pub fn append_manifest_entry(
    snapshot_dir: &Path,
    entry: ManifestEntry,
) -> Result<(), SnapshotError> {
    use std::io::Write;

    let path = snapshot_dir.join(MANIFEST_FILENAME);
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o644);
    }
    let mut file = opts.open(&path)?;
    let mut line = entry.to_line();
    line.push('\n');
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// Read every manifest entry from `<snapshot_dir>/manifest.jsonl`
/// in file order.  Blank lines skipped.
///
/// # Missing file → empty Vec
///
/// A fresh install has no manifest yet; retention and discovery
/// treat "no manifest" as "no snapshots advertised."  Returning
/// an error instead would force every caller to special-case
/// [`ErrorKind::NotFound`] as an empty result.
///
/// # Malformed line → error with line number
///
/// A corrupt line halts parsing at that line and surfaces
/// [`SnapshotError::MalformedManifest`] with the 1-based line
/// number so an operator inspecting the manifest can jump
/// straight to the offending line.  The distinct variant (vs.
/// wrapping the string in `Io(InvalidData)`) lets the
/// join-protocol layer pattern-match manifest schema issues
/// (retry with a different peer's manifest) apart from real
/// I/O failures (disk problem, escalate).  Join clients should
/// NOT treat entries before the malformed line as a "best-
/// effort prefix" — that opens a "who saw what prefix"
/// divergence hazard between joiners at different manifest read
/// points; retry with a fixed manifest instead.
pub fn read_manifest(snapshot_dir: &Path) -> Result<Vec<ManifestEntry>, SnapshotError> {
    let path = snapshot_dir.join(MANIFEST_FILENAME);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(SnapshotError::Io(e)),
    };
    let mut out = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry = ManifestEntry::from_line(line)
            .map_err(|cause| SnapshotError::MalformedManifest { line: i + 1, cause })?;
        out.push(entry);
    }
    Ok(out)
}

/// Wall-clock milliseconds since UNIX_EPOCH.  Saturates to
/// [`i64::MAX`] if the clock is set far in the future
/// (astronomically improbable but keeps the return total).
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Lowercase-hex encoding.  Hand-rolled to match the rest of
/// this module's hex conventions (no `hex` crate dependency
/// coupling for a couple call sites).
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Decode a 64-char lowercase-hex string to a `[u8; 32]`.
/// Returns a diagnostic error on wrong length or non-hex
/// digits.
fn hex_decode_32(hex: &str) -> Result<[u8; 32], String> {
    if hex.len() != 64 {
        return Err(format!("hex must be 64 chars; got {}", hex.len()));
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("hex byte {i}: {e}"))?;
    }
    Ok(out)
}

/// Split a top-level JSON-object body on commas that are NOT
/// inside a quoted string.  The manifest line format has no
/// nested objects or arrays, so this is sufficient — a
/// full JSON parser would be overkill.  Handles escaped quotes
/// (`\"`) inside strings.
fn split_top_level_commas(inner: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut escape = false;
    for c in inner.chars() {
        if escape {
            cur.push(c);
            escape = false;
            continue;
        }
        if in_str {
            if c == '\\' {
                escape = true;
                cur.push(c);
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            cur.push(c);
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                cur.push(c);
            }
            ',' => {
                parts.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    parts
}

// ===========================================================
// Cadence-driven orchestrator (slice 10) — SnapshotWriter
// ===========================================================

/// Per-block snapshot orchestrator.  Called once per block from
/// the consensus runtime; decides whether this block's height
/// hits the cadence and, if so, persists the snapshot bytes,
/// signs + appends a manifest entry, and prunes old snapshots.
///
/// # Lifecycle
///
/// Instantiated at boot by a (yet-to-land) configuration layer
/// that reads operator settings (snapshot dir, cadence, retain,
/// optional validator signer key).  Held by the runtime and
/// invoked via [`SnapshotWriter::maybe_write`] on every block.
///
/// # Fields and their defaults
///
/// - [`dir`] (`PathBuf`): where snapshots and the manifest live.
///   Expected to be absolute + symlink-resolved by the config
///   layer; this type doesn't re-validate.
/// - [`cadence`] (`u64`): block interval between snapshots.
///   Validated `>= 1` at config load; `0` would divide-by-zero
///   this type's `is_multiple_of` check.
/// - [`retain`] (`usize`): how many snapshots to keep after each
///   write.  Default heuristic `max(2, cadence * 2)` ships as a
///   placeholder — a floor of 2 guarantees joining validators
///   can always fetch at least prior + current.  Operators
///   with concrete join-SLA targets should set this explicitly.
/// - [`signer_sk`] (`Option<Vec<u8>>`): optional secp256k1
///   secret key for signing manifest entries at write time.
///   `None` ships unsigned manifest lines (for tests or
///   observer nodes without a validator identity).
/// - [`payload_dir`] (`Option<PathBuf>`): optional on-disk
///   payload store directory.  Forward-declared for a
///   yet-to-land casper-integration slice that will prune the
///   payload store alongside the snapshot dir via
///   [`scan_retained_payload_hashes`]; this slice ships the
///   field but does not read it.
///
/// # Secret-key material and `Clone`
///
/// `#[derive(Clone)]` duplicates `signer_sk`'s bytes in memory
/// without zeroize-on-drop.  Fine for the single-instance-per-
/// node setup where the writer is constructed once at boot and
/// held by the runtime; a hot cloning path would want
/// `secrecy::Secret` wrapping or HSM-backed signing.  Prefer
/// moving the writer rather than cloning when both ergonomics
/// allow.
#[derive(Debug, Clone)]
pub struct SnapshotWriter {
    pub dir: PathBuf,
    pub cadence: u64,
    pub retain: usize,
    pub signer_sk: Option<Vec<u8>>,
    pub payload_dir: Option<PathBuf>,
}

impl SnapshotWriter {
    /// Construct a writer applying the documented placeholder
    /// retention heuristic `retain = max(2, cadence * 2)`.  The
    /// floor of 2 guarantees joining validators can always
    /// fetch at least prior + current.  Operators with concrete
    /// join-SLA targets should construct the struct directly
    /// with an explicit `retain` value instead.
    ///
    /// # Panics
    ///
    /// Panics if `cadence == 0` — would both divide-by-zero the
    /// `is_multiple_of` check in [`maybe_write`] and trigger
    /// `max(2, 0)` = 2 instead of the intended heuristic.  The
    /// config layer is responsible for rejecting `cadence = 0`
    /// at load time; this constructor catches it immediately
    /// instead of deferring the panic to first invocation.
    pub fn with_default_retain(
        dir: PathBuf,
        cadence: u64,
        signer_sk: Option<Vec<u8>>,
        payload_dir: Option<PathBuf>,
    ) -> Self {
        assert!(cadence >= 1, "cadence MUST be >= 1 (got 0)");
        let retain = std::cmp::max(2, (cadence as usize).saturating_mul(2));
        Self {
            dir,
            cadence,
            retain,
            signer_sk,
            payload_dir,
        }
    }

    /// Private helper: return a signed copy of `entry` if
    /// `signer_sk` is set, else the entry unchanged.  Deduplicates
    /// the sentinel + data branches of [`maybe_write`].
    fn maybe_sign(&self, entry: ManifestEntry) -> ManifestEntry {
        match &self.signer_sk {
            Some(sk) => entry.signed(sk),
            None => entry,
        }
    }

    /// Try to persist a snapshot for `block_number` given the
    /// block's consensus WAL contribution.
    ///
    /// Returns:
    ///   - `Ok(None)` on cadence miss OR empty-sentinel append
    ///     (no `.wal` file written in either case).
    ///   - `Ok(Some((root, merkle_root)))` on a successful
    ///     snapshot persist.  `root` is the atomic Blake2b256 of
    ///     the whole blob (drives the on-disk filename); the
    ///     `merkle_root` is the Phase 7b-1 Merkle root over 4 MiB
    ///     chunk hashes (used by joiners to verify chunks fetched
    ///     via the yet-to-land chunk-fetch opcode).
    ///
    /// Callers CANNOT distinguish "cadence miss" from "empty-
    /// sentinel append" from the return value — both are
    /// `Ok(None)`.  Join clients learn about empty-sentinel
    /// slices by reading the manifest.
    ///
    /// # Config invariants (debug-asserted)
    ///
    /// `cadence >= 1` (required by `is_multiple_of`) and
    /// `retain >= 1` (required for retention to leave at least
    /// one survivor) are debug-asserted at the method entry.
    /// Release builds trust the config layer to enforce these;
    /// in debug / test builds a misconfigured caller surfaces
    /// here instead of silently producing weird behavior.
    ///
    /// # Cadence math
    ///
    /// Writes on blocks where `block_number % cadence == 0`.
    /// Block 0 (genesis) is a cadence hit (`0 % N == 0`) — cheap
    /// and useful for joining validators as an early-warning
    /// content hash.  Negative `block_number` returns `Ok(None)`
    /// (treated as "no block yet").
    ///
    /// # Empty-entries case — the sentinel
    ///
    /// A cadence hit with no WAL entries writes NO `.wal` file
    /// but DOES append an empty-sentinel manifest line
    /// ([`ManifestEntry::empty`]).  Pre-sentinel semantics were
    /// a silent skip, indistinguishable from a cadence miss to
    /// a joining validator; the sentinel lets joiners verify
    /// they have not missed a snapshot boundary.  The sentinel
    /// is signed if `signer_sk` is set — H-4 authenticity
    /// extends to empty slices.
    ///
    /// # Best-effort tail: manifest append + prune
    ///
    /// The snapshot `.wal` write is the authoritative durable
    /// act.  Both manifest-append and prune failures log at
    /// `warn` but do NOT propagate — joiners can reconstruct
    /// the manifest from a directory scan (see PR #511's
    /// "conservative posture"), and retention is bounded by
    /// future successful prune passes anyway.  Logging at warn
    /// (vs. silent discard) makes persistent failures
    /// operator-observable.
    pub fn maybe_write(
        &self,
        block_number: i64,
        entries: &[WalEntry],
    ) -> Result<Option<([u8; 32], [u8; 32])>, SnapshotError> {
        debug_assert!(
            self.cadence >= 1,
            "SnapshotWriter.cadence MUST be >= 1 (config-layer invariant)"
        );
        debug_assert!(
            self.retain >= 1,
            "SnapshotWriter.retain MUST be >= 1 (config-layer invariant)"
        );

        if block_number < 0 {
            return Ok(None);
        }
        let bn = block_number as u64;
        if !bn.is_multiple_of(self.cadence) {
            return Ok(None);
        }

        if entries.is_empty() {
            let sentinel = self.maybe_sign(ManifestEntry::empty(block_number));
            if let Err(e) = append_manifest_entry(&self.dir, sentinel) {
                tracing::warn!(
                    target: "f1r3fly.fs_wal.snapshot.manifest",
                    block_number,
                    error = %e,
                    "manifest append (empty sentinel) failed; join-protocol \
                     enumeration will need directory-scan fallback"
                );
            }
            return Ok(None);
        }

        let (_, root, merkle_root) = write_snapshot(&self.dir, entries)?;
        tracing::info!(
            target: "f1r3fly.fs_wal.snapshot",
            block_number,
            root_short = %hex_short(&root),
            merkle_root_short = %hex_short(&merkle_root),
            n_entries = entries.len(),
            "snapshot persisted"
        );

        let data_entry = self.maybe_sign(ManifestEntry::data(block_number, root, entries.len()));
        if let Err(e) = append_manifest_entry(&self.dir, data_entry) {
            tracing::warn!(
                target: "f1r3fly.fs_wal.snapshot.manifest",
                block_number,
                error = %e,
                "manifest append failed; snapshot persisted but join-protocol \
                 enumeration will need directory-scan fallback"
            );
        }

        if let Err(e) = prune_snapshot_dir(&self.dir, self.retain) {
            tracing::warn!(
                target: "f1r3fly.fs_wal.snapshot",
                block_number,
                error = %e,
                "prune failed; retention will catch up on next successful pass"
            );
        }

        Ok(Some((root, merkle_root)))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Hard-fork surface pin.  A change here rolls every WAL root
    /// on the network and requires a coordinated fleet upgrade.
    /// The fingerprint fold catches drift at peering; this test
    /// catches it before the binary even ships.
    #[test]
    fn snapshot_format_version_pinned_at_7() {
        assert_eq!(SNAPSHOT_FORMAT_VERSION, 7);
    }

    /// Empty WAL slice encodes to `[6, 0, 0, 0, 0]` — version byte
    /// + u32-be zero count.  Pins the header layout at the simplest
    /// possible fixture.
    #[test]
    fn encode_empty_wal_slice_layout() {
        let bytes = encode_wal_slice(&[]);
        assert_eq!(bytes, vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 0]);
    }

    /// Exact wire-tag values for every `WalOp` variant.  Any tag
    /// renumber is a hard fork of the WAL root — pin here so the
    /// change surfaces in code review before the fingerprint fold
    /// catches it at peering.
    #[test]
    fn op_tag_values_are_pinned() {
        for (op, expected) in [
            (WalOp::Write, 1u8),
            (WalOp::WriteAt, 2),
            (WalOp::Truncate, 3),
            (WalOp::Chmod, 4),
            (WalOp::Chown, 5),
            (WalOp::RemoveFile, 6),
            (WalOp::RemoveDir, 7),
            (WalOp::Rename, 8),
            (WalOp::CopyFile, 9),
            (WalOp::Read, 10),
            (WalOp::ReadAt, 11),
            (WalOp::Stat, 12),
            (WalOp::Entries, 13),
            (WalOp::Size, 14),
            (WalOp::EntriesStreamNext, 15),
            (WalOp::Exists, 16),
            (WalOp::BulkApply, 17),
        ] {
            assert_eq!(op_tag(op), expected, "op_tag({op:?}) drifted");
        }
    }

    /// The 16 op_tag values MUST be bijective.  Pin that no two
    /// variants map to the same wire tag (a duplicate would produce
    /// ambiguous decodes in slice 2).
    #[test]
    fn op_tag_values_are_pairwise_distinct() {
        let all = [
            WalOp::Write,
            WalOp::WriteAt,
            WalOp::Truncate,
            WalOp::Chmod,
            WalOp::Chown,
            WalOp::RemoveFile,
            WalOp::RemoveDir,
            WalOp::Rename,
            WalOp::CopyFile,
            WalOp::Read,
            WalOp::ReadAt,
            WalOp::Stat,
            WalOp::Entries,
            WalOp::Size,
            WalOp::EntriesStreamNext,
            WalOp::Exists,
            WalOp::BulkApply,
        ];
        let tags: Vec<u8> = all.iter().map(|&o| op_tag(o)).collect();
        let mut sorted = tags.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), tags.len(), "op_tag collisions: {tags:?}");
    }

    fn mk_write_entry(len: u64, ack_payload: &[u8]) -> WalEntry {
        WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/@bundle/target"),
            extra_path: None,
            offset: None,
            length: Some(len),
            payload_ref: Some(PayloadRef::hash(ack_payload)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    /// Same input → same bytes.  Deterministic across validators.
    #[test]
    fn encode_wal_slice_is_deterministic() {
        let entries = vec![mk_write_entry(5, b"hello"), mk_write_entry(3, b"bye")];
        let a = encode_wal_slice(&entries);
        let b = encode_wal_slice(&entries);
        assert_eq!(a, b);
    }

    #[test]
    fn compute_wal_root_matches_hash_of_encoded_bytes() {
        let entries = vec![mk_write_entry(5, b"hello")];
        let root = compute_wal_root(&entries);
        let expected = hash_of(&encode_wal_slice(&entries));
        assert_eq!(root, expected);
    }

    #[test]
    fn compute_wal_root_differs_on_content_change() {
        let a = compute_wal_root(&[mk_write_entry(5, b"hello")]);
        let b = compute_wal_root(&[mk_write_entry(5, b"world")]);
        assert_ne!(a, b);
    }

    #[test]
    fn snapshot_blob_bundles_bytes_root_and_merkle_root() {
        let entries = vec![mk_write_entry(5, b"hello")];
        let blob = snapshot_blob(&entries);
        assert_eq!(blob.bytes, encode_wal_slice(&entries));
        assert_eq!(blob.root, compute_wal_root(&entries));
        // The Merkle root over per-chunk hashes agrees with a
        // direct call through the chunker.
        use super::super::snapshot_chunk::{chunk_snapshot, snapshot_merkle_root};
        let expected_merkle = snapshot_merkle_root(
            &chunk_snapshot(&blob.bytes)
                .iter()
                .map(|c| c.hash)
                .collect::<Vec<_>>(),
        );
        assert_eq!(blob.merkle_root, expected_merkle);
    }

    /// Empty input → the empty-snapshot sentinel Merkle root
    /// (`[0u8; 32]`) from the chunker's contract.  Pins the
    /// zero-entries edge case for both header and merkle_root.
    #[test]
    fn snapshot_blob_empty_input_uses_empty_snapshot_sentinel_for_merkle_root() {
        let blob = snapshot_blob(&[]);
        assert_eq!(blob.bytes, vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 0]);
        // `bytes` are non-empty (5-byte header), so `root` is a
        // real Blake2b256 hash — NOT the sentinel.
        assert_ne!(blob.root, [0u8; 32]);
        // But the chunker's Merkle root over an EMPTY chunk list
        // (5 bytes < 4 MiB → 1 chunk, so this is really the
        // 1-chunk case; sentinel would only appear for 0 chunks
        // which requires empty bytes — impossible here since the
        // 5-byte header is always present).
        //
        // What we can pin instead: for 1 chunk, the Merkle root
        // equals the chunk's own hash.
        use super::super::snapshot_chunk::chunk_snapshot;
        let chunks = chunk_snapshot(&blob.bytes);
        assert_eq!(chunks.len(), 1);
        assert_eq!(blob.merkle_root, chunks[0].hash);
    }

    // --- Per-sub-tag byte pins ---------------------------------------
    //
    // These pin the individual variant-tag bytes for `WalOutcome`,
    // `PayloadRef`, and every `Option<T>` presence field.  The
    // golden-hex WAL root below catches all of these too, but its
    // diagnostic message reads "WAL root drifted" — these tighter
    // pins name the specific tag that regressed.  A future
    // contributor who swaps (say) `WalOutcome::Success` and
    // `Failure` tags gets a "Success tag drifted from 0" error
    // instead of a whole-blob hash mismatch.

    /// Extract the last byte of a fresh encoding of a single
    /// `WalEntry` — used by the per-sub-tag pins below to isolate
    /// the outcome-tag byte at the entry's tail.
    fn last_byte(entry: WalEntry) -> u8 {
        let mut buf = Vec::new();
        encode_entry(&entry, &mut buf);
        *buf.last().expect("entry encodes to at least one byte")
    }

    #[test]
    fn wal_outcome_wire_tags_pinned() {
        // Outcome is the tail byte of an entry with all leading
        // Option fields = None (each contributes a 0-presence byte
        // that isn't the last).
        let base = mk_write_entry_no_payload();
        assert_eq!(
            last_byte(WalEntry {
                outcome: WalOutcome::Success,
                ..base.clone()
            }),
            0,
            "WalOutcome::Success tag drifted from 0"
        );
        // For Failure the trailing bytes are [1, code_u32_be...],
        // so the last byte is the low byte of `code` — not the tag.
        // Encode explicitly + assert the tag byte (position -5).
        let mut buf = Vec::new();
        encode_entry(
            &WalEntry {
                outcome: WalOutcome::Failure { code: 0 },
                ..base
            },
            &mut buf,
        );
        assert_eq!(
            buf[buf.len() - 5],
            1,
            "WalOutcome::Failure tag drifted from 1"
        );
    }

    fn mk_write_entry_no_payload() -> WalEntry {
        WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/@bundle/t"),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    #[test]
    fn payload_ref_wire_tags_pinned() {
        // Isolate the payload_ref tag by encoding just it into a
        // scratch buffer.
        let mut buf = Vec::new();
        encode_payload_ref(&None, &mut buf);
        assert_eq!(buf, vec![0u8], "PayloadRef::None tag drifted from 0");

        buf.clear();
        encode_payload_ref(&Some(PayloadRef::Hash([0xAAu8; 32])), &mut buf);
        assert_eq!(buf[0], 1, "PayloadRef::Hash tag drifted from 1");
        assert_eq!(buf.len(), 1 + 32);

        buf.clear();
        encode_payload_ref(
            &Some(PayloadRef::DeployRef {
                block_hash: [0xBBu8; 32],
                deploy_index: 0,
                arg_index: 0,
            }),
            &mut buf,
        );
        assert_eq!(buf[0], 2, "PayloadRef::DeployRef tag drifted from 2");
        assert_eq!(buf.len(), 1 + 32 + 4 + 4);
    }

    #[test]
    fn option_presence_tags_pinned() {
        // Every `encode_opt_*` helper uses the SAME 0=None /
        // 1=Some convention — swapping would flip decoding on
        // the paired field.  Pin each shape.
        let mut buf = Vec::new();
        encode_opt_u64(None, &mut buf);
        assert_eq!(buf, vec![0u8], "encode_opt_u64(None) tag drifted from 0");
        buf.clear();
        encode_opt_u64(Some(0), &mut buf);
        assert_eq!(buf[0], 1, "encode_opt_u64(Some) tag drifted from 1");

        buf.clear();
        encode_opt_u32(None, &mut buf);
        assert_eq!(buf, vec![0u8], "encode_opt_u32(None) tag drifted from 0");
        buf.clear();
        encode_opt_u32(Some(0), &mut buf);
        assert_eq!(buf[0], 1, "encode_opt_u32(Some) tag drifted from 1");

        buf.clear();
        encode_opt_str(None, &mut buf);
        assert_eq!(buf, vec![0u8], "encode_opt_str(None) tag drifted from 0");
        buf.clear();
        encode_opt_str(Some(""), &mut buf);
        assert_eq!(buf[0], 1, "encode_opt_str(Some) tag drifted from 1");
    }

    /// Golden-hex pin: a specific 2-entry WAL slice hashes to a
    /// specific 32-byte root.  Any encoding drift (tag renumber,
    /// field reorder, presence-tag flip, version byte change)
    /// flips this hash — visible in CI before deployment.
    ///
    /// This is the WAL-side analog of `verify.rs`'s
    /// `cons3_stable_hash_pinned_for_known_pars` and
    /// `snapshot_chunk.rs`'s `merkle_tree_shape_pinned_by_golden_hex`
    /// — each layer's canonical hash pinned at a well-known input
    /// so a serialization change is caught at the boundary that
    /// introduces the drift.
    #[test]
    fn wal_root_pinned_for_known_entries_golden_hex() {
        let entries = vec![
            WalEntry {
                op: WalOp::Write,
                path: PathBuf::from("/@bundle/f1"),
                extra_path: None,
                offset: None,
                length: Some(5),
                payload_ref: Some(PayloadRef::hash(b"hello")),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            WalEntry {
                op: WalOp::Truncate,
                path: PathBuf::from("/@bundle/f2"),
                extra_path: None,
                offset: Some(1024),
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Failure { code: 42 },
            },
        ];
        let root = compute_wal_root(&entries);
        let hex = root.iter().fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        });
        // Regenerate on intentional encoding-format hard fork via
        //   cargo test -p rholang --lib -- \
        //     wal_root_pinned_for_known_entries_golden_hex --nocapture
        // and update; anchor the roll to a coordinated fleet upgrade.
        const EXPECTED: &str = "4671dbc803e460d40c2cdbdbffc70d886d3aaac4cb3a22aae13522939f63d108";
        assert_eq!(
            hex, EXPECTED,
            "WAL root drifted for the known 2-entry fixture — encoding \
             format changed?  If intentional, roll `SNAPSHOT_FORMAT_VERSION` \
             and update this pin."
        );
    }

    // --- Slice 2: decoder + SnapshotError --------------------------

    /// Test fixture — a rich set of `WalEntry` variants that
    /// exercises every optional field and every enum tag.  Used by
    /// the round-trip pins below.
    fn diverse_entries() -> Vec<WalEntry> {
        vec![
            // Write: length + payload_ref = Hash + Success.
            WalEntry {
                op: WalOp::Write,
                path: PathBuf::from("/@bundle/w"),
                extra_path: None,
                offset: None,
                length: Some(5),
                payload_ref: Some(PayloadRef::hash(b"hello")),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            // Truncate: offset + Failure { code }.
            WalEntry {
                op: WalOp::Truncate,
                path: PathBuf::from("/@bundle/t"),
                extra_path: None,
                offset: Some(1024),
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Failure { code: 42 },
            },
            // Chmod: mode_bits populated.
            WalEntry {
                op: WalOp::Chmod,
                path: PathBuf::from("/@bundle/c"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: Some(0o644),
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            // Chown: owner + group populated.
            WalEntry {
                op: WalOp::Chown,
                path: PathBuf::from("/@bundle/o"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: Some("alice".to_string()),
                group: Some("staff".to_string()),
                outcome: WalOutcome::Success,
            },
            // Rename: extra_path populated.
            WalEntry {
                op: WalOp::Rename,
                path: PathBuf::from("/@bundle/from"),
                extra_path: Some(PathBuf::from("/@bundle/to")),
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            // DeployRef payload (exercises the third payload_ref
            // variant).
            WalEntry {
                op: WalOp::WriteAt,
                path: PathBuf::from("/@bundle/w2"),
                extra_path: None,
                offset: Some(0),
                length: Some(32),
                payload_ref: Some(PayloadRef::DeployRef {
                    block_hash: [0xEEu8; 32],
                    deploy_index: 3,
                    arg_index: 1,
                }),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
        ]
    }

    /// End-to-end round-trip: `decode_wal_slice(encode_wal_slice
    /// (entries)) == entries` for a diverse fixture.  This is the
    /// load-bearing property both encoder and decoder must
    /// preserve.
    #[test]
    fn decode_roundtrips_encode_for_diverse_entries() {
        let entries = diverse_entries();
        let bytes = encode_wal_slice(&entries);
        let decoded = decode_wal_slice(&bytes).expect("decode ok");
        assert_eq!(decoded, entries);
    }

    #[test]
    fn decode_empty_slice_roundtrips() {
        let bytes = encode_wal_slice(&[]);
        let decoded = decode_wal_slice(&bytes).expect("decode ok");
        assert!(decoded.is_empty());
    }

    /// Round-trip pinned per-`WalOp` variant so a decoder
    /// regression in a single variant surfaces with a specific
    /// failure rather than getting lost in the diverse-fixture
    /// blob.
    #[test]
    fn decode_roundtrips_every_wal_op_variant() {
        for op in [
            WalOp::Write,
            WalOp::WriteAt,
            WalOp::Truncate,
            WalOp::Chmod,
            WalOp::Chown,
            WalOp::RemoveFile,
            WalOp::RemoveDir,
            WalOp::Rename,
            WalOp::CopyFile,
            WalOp::Read,
            WalOp::ReadAt,
            WalOp::Stat,
            WalOp::Entries,
            WalOp::Size,
            WalOp::EntriesStreamNext,
            WalOp::Exists,
            WalOp::BulkApply,
        ] {
            let entry = WalEntry {
                op,
                path: PathBuf::from("/@bundle/x"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            };
            let bytes = encode_wal_slice(&[entry.clone()]);
            let decoded = decode_wal_slice(&bytes).expect("decode ok");
            assert_eq!(decoded, vec![entry], "round-trip failed for {op:?}");
        }
    }

    // --- Error surface pins ---------------------------------------

    #[test]
    fn decode_empty_bytes_returns_truncated() {
        match decode_wal_slice(&[]) {
            Err(SnapshotError::Truncated { got: 0, need: 1 }) => (),
            other => panic!("expected Truncated {{ got: 0, need: 1 }}, got {other:?}"),
        }
    }

    #[test]
    fn decode_wrong_version_byte_returns_unsupported_version() {
        // Version byte 99 (non-6) with a valid-shape 0-count tail.
        let bad = vec![99u8, 0, 0, 0, 0];
        match decode_wal_slice(&bad) {
            Err(SnapshotError::UnsupportedVersion { got: 99, supported }) => {
                assert_eq!(supported, SNAPSHOT_FORMAT_VERSION);
            }
            other => panic!("expected UnsupportedVersion {{ got: 99 }}, got {other:?}"),
        }
    }

    #[test]
    fn decode_missing_count_bytes_returns_truncated() {
        // Version byte present, count truncated (only 2 of 4 bytes).
        let short = vec![SNAPSHOT_FORMAT_VERSION, 0, 0];
        match decode_wal_slice(&short) {
            Err(SnapshotError::Truncated { got: 3, need: 5 }) => (),
            other => panic!("expected Truncated {{ got: 3, need: 5 }}, got {other:?}"),
        }
    }

    /// Wire tag 0 is unused (wire tags start at 1; the offset
    /// from `WalOp`'s discriminants leaves 0 free as a natural
    /// "invalid tag" sentinel).  Decoder MUST reject.
    #[test]
    fn decode_op_tag_0_returns_malformed_blob() {
        // Header + 1 entry with op_tag = 0.
        let bytes = vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 1, 0];
        match decode_wal_slice(&bytes) {
            Err(SnapshotError::MalformedBlob { message, .. }) => {
                assert!(message.contains("unknown op tag 0"), "got {message:?}");
            }
            other => panic!("expected MalformedBlob for tag 0, got {other:?}"),
        }
    }

    /// Wire tag 18 (past the current tail of 17) is also invalid.
    #[test]
    fn decode_op_tag_past_tail_returns_malformed_blob() {
        let bytes = vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 1, 18];
        match decode_wal_slice(&bytes) {
            Err(SnapshotError::MalformedBlob { message, .. }) => {
                assert!(message.contains("unknown op tag 18"), "got {message:?}");
            }
            other => panic!("expected MalformedBlob for tag 18, got {other:?}"),
        }
    }

    /// Invalid outcome tag surfaces as MalformedBlob with a
    /// specific message.
    #[test]
    fn decode_invalid_outcome_tag_returns_malformed_blob() {
        // Start with a valid single-entry encoding, then flip the
        // outcome tag byte (last byte of encoding) to 99.
        let entry = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/a"),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let mut bytes = encode_wal_slice(&[entry]);
        *bytes.last_mut().unwrap() = 99;
        match decode_wal_slice(&bytes) {
            Err(SnapshotError::MalformedBlob { message, .. }) => {
                assert!(
                    message.contains("outcome tag") && message.contains("99"),
                    "got {message:?}"
                );
            }
            other => panic!("expected MalformedBlob for outcome tag 99, got {other:?}"),
        }
    }

    /// Invalid `PayloadRef` variant tag (3+) surfaces as
    /// MalformedBlob.
    #[test]
    fn decode_invalid_payload_ref_variant_tag_returns_malformed_blob() {
        // Craft: header + 1 entry with op=Write, path=/a, no
        // extra_path, no offset, no length, payload_ref variant
        // tag = 99.
        let mut bytes = vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 1];
        bytes.push(1); // op_tag = Write
        bytes.extend_from_slice(&2u32.to_be_bytes()); // path length 2
        bytes.extend_from_slice(b"/a");
        bytes.push(0); // extra_path = None
        bytes.push(0); // offset = None
        bytes.push(0); // length = None
        bytes.push(99); // payload_ref variant tag = 99 — invalid
        match decode_wal_slice(&bytes) {
            Err(SnapshotError::MalformedBlob { message, .. }) => {
                assert!(
                    message.contains("payload_ref") && message.contains("99"),
                    "got {message:?}"
                );
            }
            other => panic!("expected MalformedBlob for payload_ref tag 99, got {other:?}"),
        }
    }

    /// Invalid presence tag (2+) surfaces as MalformedBlob.  Pin
    /// for all three `Option` presence variants — this test
    /// exercises `offset` (opt_u64), but the error message shape
    /// is identical for opt_u32, opt_str, extra_path.
    #[test]
    fn decode_invalid_opt_u64_presence_tag_returns_malformed_blob() {
        // Header + entry with op=Write, path=/a, extra_path=None,
        // then offset presence tag = 99.
        let mut bytes = vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 1];
        bytes.push(1); // op_tag = Write
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(b"/a");
        bytes.push(0); // extra_path = None
        bytes.push(99); // offset presence = 99 — invalid
        match decode_wal_slice(&bytes) {
            Err(SnapshotError::MalformedBlob { message, .. }) => {
                assert!(
                    message.contains("opt-u64") && message.contains("99"),
                    "got {message:?}"
                );
            }
            other => panic!("expected MalformedBlob for opt-u64 tag 99, got {other:?}"),
        }
    }

    /// Non-UTF-8 owner / group string surfaces as MalformedBlob.
    /// Owner/group are declared as `String` at the type level, so
    /// a decoder that silently passed invalid bytes would leave
    /// downstream code with a corrupt `String`.
    #[test]
    fn decode_non_utf8_owner_returns_malformed_blob() {
        // Craft: valid entry up to owner, then owner = Some with
        // one byte that's invalid UTF-8 (0xFF).
        let mut bytes = vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 1];
        bytes.push(4); // op_tag = Chmod
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(b"/x");
        bytes.push(0); // extra_path = None
        bytes.push(0); // offset = None
        bytes.push(0); // length = None
        bytes.push(0); // payload_ref = None
        bytes.push(1); // mode_bits = Some
        bytes.extend_from_slice(&0o644u32.to_be_bytes());
        bytes.push(1); // owner = Some
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.push(0xFF); // 0xFF is invalid UTF-8 as a standalone byte
        match decode_wal_slice(&bytes) {
            Err(SnapshotError::MalformedBlob { message, .. }) => {
                assert!(
                    message.contains("UTF-8"),
                    "expected UTF-8 error message, got {message:?}"
                );
            }
            other => panic!("expected MalformedBlob for non-UTF-8 owner, got {other:?}"),
        }
    }

    /// Mid-entry truncation surfaces as Truncated (not
    /// MalformedBlob).
    #[test]
    fn decode_mid_entry_truncation_returns_truncated() {
        // Header + entry claiming 1 count, but no entry bytes.
        let bytes = vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 1];
        match decode_wal_slice(&bytes) {
            Err(SnapshotError::Truncated { .. }) => (),
            other => panic!("expected Truncated for missing entry, got {other:?}"),
        }
    }

    // --- SnapshotError Display / From ------------------------------

    #[test]
    fn snapshot_error_display_formats_each_variant() {
        let s = format!("{}", SnapshotError::UnsupportedVersion {
            got: 99,
            supported: 6
        });
        assert!(s.contains("99") && s.contains("6"), "got {s:?}");

        let s = format!("{}", SnapshotError::RootMismatch {
            expected: [0xAAu8; 32],
            got: [0xBBu8; 32],
        });
        assert!(
            s.contains("aaaaaaaaaaaaaaaa") && s.contains("bbbbbbbbbbbbbbbb"),
            "expected 16-char hex-short renders in {s:?}"
        );

        let s = format!("{}", SnapshotError::Truncated { got: 3, need: 5 });
        assert!(s.contains("3") && s.contains("5"), "got {s:?}");

        let s = format!("{}", SnapshotError::MalformedBlob {
            offset: 42,
            message: "test-message".into(),
        });
        assert!(s.contains("42") && s.contains("test-message"), "got {s:?}");

        let s = format!("{}", SnapshotError::MalformedManifest {
            line: 17,
            cause: "example-cause".into(),
        });
        assert!(s.contains("17") && s.contains("example-cause"), "got {s:?}");

        let s = format!("{}", SnapshotError::UnsignedManifestEntry);
        assert!(s.contains("unsigned"), "got {s:?}");

        let s = format!("{}", SnapshotError::ManifestSignatureInvalid);
        assert!(s.contains("signature verification failed"), "got {s:?}");

        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let s = format!("{}", SnapshotError::Io(io_err));
        assert!(s.contains("nope"), "got {s:?}");
    }

    #[test]
    fn snapshot_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: SnapshotError = io_err.into();
        assert!(matches!(err, SnapshotError::Io(_)));
    }

    // `SnapshotError: Send + Sync` — no runtime test needed; the
    // module-scope `_SNAPSHOT_ERROR_IS_SEND_SYNC` const witness
    // above enforces the bound at every `cargo build` (not only
    // `cargo test`).

    // --- Slice 3: disk I/O -----------------------------------------

    /// `snapshot_path` produces `{dir}/{root_hex}.wal` with a
    /// full 64-char (256-bit) hex encoding — no truncation, so
    /// accidental collision is impossible under Blake2b256
    /// preimage resistance.
    #[test]
    fn snapshot_path_uses_full_root_hex_with_wal_extension() {
        let dir = PathBuf::from("/tmp/snapshots");
        let root: [u8; 32] = [0xABu8; 32];
        let path = snapshot_path(&dir, &root);
        // 0xAB = "ab" in hex; 32 bytes → 64-char string "abab...ab".
        let hex64 = "ab".repeat(32);
        assert_eq!(path, dir.join(format!("{hex64}.wal")));
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some(format!("{hex64}.wal").as_str())
        );
    }

    #[test]
    fn snapshot_path_differs_by_root_byte() {
        let dir = PathBuf::from("/x");
        let a = [0u8; 32];
        let mut b = [0u8; 32];
        b[31] = 1; // flip the trailing byte
        assert_ne!(
            snapshot_path(&dir, &a),
            snapshot_path(&dir, &b),
            "different roots MUST produce different paths"
        );
        // Sanity: identical roots produce identical paths (idempotence).
        assert_eq!(snapshot_path(&dir, &a), snapshot_path(&dir, &a));
    }

    /// End-to-end write → read round-trip: the bytes we read
    /// back byte-equal the bytes we wrote, and the returned root
    /// matches `Blake2b256(bytes)`.
    #[test]
    fn write_snapshot_then_read_bytes_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = diverse_entries();
        let (path, root, merkle_root) = write_snapshot(tmp.path(), &entries).expect("write ok");
        // File exists at the content-addressed path.
        assert!(path.exists());
        assert_eq!(path, snapshot_path(tmp.path(), &root));
        // Reader returns the same bytes we produced.
        let bytes = read_snapshot_bytes(tmp.path(), &root).expect("read ok");
        assert_eq!(bytes, encode_wal_slice(&entries));
        // The reader-side hash-check confirms `root == Blake2b256(bytes)`
        // — no need to re-assert here.
        // Merkle root matches an independent computation.
        use super::super::snapshot_chunk::{chunk_snapshot, snapshot_merkle_root};
        let expected_merkle = snapshot_merkle_root(
            &chunk_snapshot(&bytes)
                .iter()
                .map(|c| c.hash)
                .collect::<Vec<_>>(),
        );
        assert_eq!(merkle_root, expected_merkle);
    }

    /// Writing the same entries twice is idempotent — both writes
    /// land at the same content-addressed path, and both leave
    /// byte-identical files.
    #[test]
    fn write_snapshot_is_idempotent_by_content_address() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![mk_write_entry(5, b"hello")];
        let (path_a, root_a, _) = write_snapshot(tmp.path(), &entries).expect("first write ok");
        let (path_b, root_b, _) = write_snapshot(tmp.path(), &entries).expect("second write ok");
        assert_eq!(path_a, path_b);
        assert_eq!(root_a, root_b);
        // Reader-side round-trip still works after the rewrite.
        let bytes = read_snapshot_bytes(tmp.path(), &root_a).expect("read ok");
        assert_eq!(bytes, encode_wal_slice(&entries));
    }

    /// Missing snapshot file surfaces as `SnapshotError::Io(NotFound)`
    /// via the `From<io::Error>` impl.
    #[test]
    fn read_snapshot_bytes_missing_file_returns_io_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let root = [0x11u8; 32];
        match read_snapshot_bytes(tmp.path(), &root) {
            Err(SnapshotError::Io(e)) => {
                assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected Io(NotFound), got {other:?}"),
        }
    }

    /// A snapshot whose on-disk bytes don't hash to the requested
    /// root surfaces as `RootMismatch`.  Simulates a corrupt or
    /// tampered file by writing custom bytes to a
    /// content-addressed path whose name says the root is X but
    /// the bytes hash to Y.
    #[test]
    fn read_snapshot_bytes_wrong_bytes_returns_root_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        // Pretend the caller requested this root.
        let claimed_root = [0x22u8; 32];
        let path = snapshot_path(tmp.path(), &claimed_root);
        // Write bytes that hash to something else.
        std::fs::write(&path, b"not the right bytes").unwrap();
        match read_snapshot_bytes(tmp.path(), &claimed_root) {
            Err(SnapshotError::RootMismatch { expected, got }) => {
                assert_eq!(expected, claimed_root);
                assert_ne!(got, claimed_root);
            }
            other => panic!("expected RootMismatch, got {other:?}"),
        }
    }

    /// Post-hash version check: if a file's hash matches but the
    /// leading version byte doesn't match `SNAPSHOT_FORMAT_VERSION`,
    /// return `UnsupportedVersion`.  Constructs the pathological
    /// case by hashing a bad-version blob and pointing the reader
    /// at the resulting root.
    #[test]
    fn read_snapshot_bytes_wrong_version_returns_unsupported_version() {
        let tmp = tempfile::tempdir().unwrap();
        // Deliberately bogus version byte.
        let bad_bytes: Vec<u8> = vec![99u8, 0, 0, 0, 0];
        let bad_root = {
            let h = Blake2b256::hash(bad_bytes.clone());
            let mut out = [0u8; 32];
            out.copy_from_slice(&h);
            out
        };
        let path = snapshot_path(tmp.path(), &bad_root);
        std::fs::write(&path, &bad_bytes).unwrap();
        match read_snapshot_bytes(tmp.path(), &bad_root) {
            Err(SnapshotError::UnsupportedVersion { got, supported }) => {
                assert_eq!(got, 99);
                assert_eq!(supported, SNAPSHOT_FORMAT_VERSION);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    /// A zero-byte file hashes to the empty-input Blake2b256
    /// digest.  If a caller requests THAT root, the read succeeds
    /// past the hash check but fails the version check with
    /// `Truncated { got: 0, need: 1 }`.  Pins the truncation
    /// guard's role as a defense against a "hash-matches-but-
    /// empty" file that could otherwise crash the version-byte
    /// indexer.
    #[test]
    fn read_snapshot_bytes_zero_length_at_matching_root_returns_truncated() {
        let tmp = tempfile::tempdir().unwrap();
        let empty_root = {
            let h = Blake2b256::hash(Vec::new());
            let mut out = [0u8; 32];
            out.copy_from_slice(&h);
            out
        };
        let path = snapshot_path(tmp.path(), &empty_root);
        std::fs::write(&path, b"").unwrap();
        match read_snapshot_bytes(tmp.path(), &empty_root) {
            Err(SnapshotError::Truncated { got: 0, need: 1 }) => (),
            other => panic!("expected Truncated {{ got: 0, need: 1 }}, got {other:?}"),
        }
    }

    // --- referenced_payload_hashes ---------------------------------

    #[test]
    fn referenced_payload_hashes_extracts_hash_variants_only() {
        let block_hash = [0xEE; 32];
        let entries = vec![
            // Skipped: no payload_ref.
            WalEntry {
                op: WalOp::Chmod,
                path: PathBuf::from("/a"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: Some(0o644),
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            // Included: Hash variant.
            mk_write_entry(3, b"aa"),
            mk_write_entry(3, b"bb"),
            // Skipped: DeployRef variant (bytes live in block
            // storage, not payload store).
            WalEntry {
                op: WalOp::WriteAt,
                path: PathBuf::from("/c"),
                extra_path: None,
                offset: Some(0),
                length: Some(8),
                payload_ref: Some(PayloadRef::DeployRef {
                    block_hash,
                    deploy_index: 0,
                    arg_index: 0,
                }),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
        ];
        let set = referenced_payload_hashes(&entries);
        assert_eq!(
            set.len(),
            2,
            "expected exactly the two Hash-variant entries"
        );
    }

    /// Duplicates dedupe via the HashSet — a WAL slice with N
    /// entries all referencing the same payload contributes ONE
    /// hash to the set.
    #[test]
    fn referenced_payload_hashes_deduplicates() {
        let entries = vec![
            mk_write_entry(5, b"same"),
            mk_write_entry(5, b"same"),
            mk_write_entry(5, b"same"),
        ];
        let set = referenced_payload_hashes(&entries);
        assert_eq!(
            set.len(),
            1,
            "three copies of the same payload dedupe to one hash"
        );
    }

    #[test]
    fn referenced_payload_hashes_empty_input_yields_empty_set() {
        let set = referenced_payload_hashes(&[]);
        assert!(set.is_empty());
    }

    // ---------------------------------------------------------------
    // Payload-hash sidecar (slice 4)
    // ---------------------------------------------------------------

    fn small_hash_set(bytes: &[u8]) -> std::collections::HashSet<[u8; 32]> {
        bytes
            .iter()
            .map(|b| {
                let mut h = [0u8; 32];
                h[0] = *b;
                h
            })
            .collect()
    }

    #[test]
    fn hashes_sidecar_path_uses_hex64_with_hashes_extension() {
        let dir = PathBuf::from("/x");
        let root = [0xABu8; 32];
        let path = hashes_sidecar_path(&dir, &root);
        // 32 bytes of 0xAB → 64-char hex of "ab" repeated.
        let expected_hex: String = "ab".repeat(32);
        assert_eq!(path.parent(), Some(dir.as_path()));
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some(format!("{expected_hex}.hashes").as_str())
        );
    }

    #[test]
    fn hashes_sidecar_path_colocated_with_snapshot() {
        let dir = PathBuf::from("/x");
        let root = [0x11u8; 32];
        let snap = snapshot_path(&dir, &root);
        let side = hashes_sidecar_path(&dir, &root);
        // Same parent, same stem, extensions differ.
        assert_eq!(snap.parent(), side.parent());
        assert_eq!(snap.file_stem(), side.file_stem());
        assert_eq!(snap.extension().and_then(|s| s.to_str()), Some("wal"));
        assert_eq!(side.extension().and_then(|s| s.to_str()), Some("hashes"));
    }

    #[test]
    fn hashes_sidecar_empty_set_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.hashes");
        let empty = std::collections::HashSet::new();
        write_hashes_sidecar(&path, &empty).expect("write ok");
        // On disk: just the u32-be zero count header.
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes, vec![0u8, 0, 0, 0], "empty set is 4-byte header");
        let back = read_hashes_sidecar(&path).expect("read ok");
        assert!(back.is_empty());
    }

    #[test]
    fn hashes_sidecar_nonempty_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonempty.hashes");
        let set = small_hash_set(&[1, 2, 3]);
        write_hashes_sidecar(&path, &set).expect("write ok");
        let back = read_hashes_sidecar(&path).expect("read ok");
        assert_eq!(back, set);
    }

    /// The on-disk layout is `[u32-be count][32-byte hash × count]`
    /// with hashes sorted lexicographically.  Pinning this so an
    /// accidental switch to `HashSet`'s iteration order (which is
    /// non-deterministic across runs) is caught at test time.
    #[test]
    fn hashes_sidecar_layout_sorted_for_determinism() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sorted.hashes");
        // Insertion order deliberately reversed vs. sort order.
        let set = small_hash_set(&[9, 5, 1]);
        write_hashes_sidecar(&path, &set).expect("write ok");
        let bytes = std::fs::read(&path).unwrap();
        // Header: count = 3 in u32-be.
        assert_eq!(&bytes[0..4], &[0, 0, 0, 3]);
        // Bodies: sorted by leading byte (1, 5, 9).
        assert_eq!(bytes[4], 1, "first hash body starts with 1");
        assert_eq!(bytes[4 + 32], 5, "second hash body starts with 5");
        assert_eq!(bytes[4 + 64], 9, "third hash body starts with 9");
        assert_eq!(
            bytes.len(),
            4 + 3 * 32,
            "total length = header + count * 32"
        );
    }

    /// Two independent invocations with equivalent hash sets
    /// produce byte-identical files (via the sort).
    #[test]
    fn hashes_sidecar_write_is_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let path_a = tmp.path().join("a.hashes");
        let path_b = tmp.path().join("b.hashes");
        let set = small_hash_set(&[7, 3, 2, 11, 5]);
        write_hashes_sidecar(&path_a, &set).expect("write a ok");
        write_hashes_sidecar(&path_b, &set).expect("write b ok");
        assert_eq!(
            std::fs::read(&path_a).unwrap(),
            std::fs::read(&path_b).unwrap()
        );
    }

    /// Corrupt-sidecar posture: a short header (< 4 bytes) does
    /// NOT propagate as an error; the reader returns an empty set
    /// so retention can proceed.
    #[test]
    fn read_hashes_sidecar_short_header_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("short.hashes");
        std::fs::write(&path, b"ab").unwrap();
        let set = read_hashes_sidecar(&path).expect("read ok (best-effort)");
        assert!(set.is_empty());
    }

    /// Header claims N entries but the body length disagrees →
    /// return empty (corrupt-sidecar posture).
    #[test]
    fn read_hashes_sidecar_length_mismatch_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mismatched.hashes");
        // Claim 5 hashes but write only 4 bytes of body.
        let mut bytes = vec![0u8, 0, 0, 5];
        bytes.extend_from_slice(&[0xAAu8; 4]);
        std::fs::write(&path, &bytes).unwrap();
        let set = read_hashes_sidecar(&path).expect("read ok (best-effort)");
        assert!(set.is_empty());
    }

    /// Header claims a count exceeding [`MAX_WAL_ENTRIES`] → return
    /// empty (defensive OOM cap).  Body length would be gigabytes
    /// under a naive read; the cap check fires before we look at
    /// the body length or attempt any allocation proportional to
    /// `count`.
    #[test]
    fn read_hashes_sidecar_count_exceeds_cap_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("huge.hashes");
        // u32-be = 0xFFFFFFFF (>> MAX_WAL_ENTRIES); body left empty.
        std::fs::write(&path, [0xFFu8, 0xFF, 0xFF, 0xFF]).unwrap();
        let set = read_hashes_sidecar(&path).expect("read ok (best-effort)");
        assert!(set.is_empty());
    }

    /// A missing file DOES propagate as an [`io::Error`] — callers
    /// that need to distinguish "file wasn't there" from "file was
    /// there but corrupt" get the NotFound signal.
    #[test]
    fn read_hashes_sidecar_missing_file_returns_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such.hashes");
        let err = read_hashes_sidecar(&missing).expect_err("should be NotFound");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn scan_retained_payload_hashes_missing_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such-dir");
        let set = scan_retained_payload_hashes(&missing).expect("missing dir is OK, empty");
        assert!(set.is_empty());
    }

    #[test]
    fn scan_retained_payload_hashes_empty_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let set = scan_retained_payload_hashes(tmp.path()).expect("empty dir is OK");
        assert!(set.is_empty());
    }

    /// Two sidecars with disjoint hash sets → union contains both.
    #[test]
    fn scan_retained_payload_hashes_unions_multiple_sidecars() {
        let tmp = tempfile::tempdir().unwrap();
        let root_a = [0x01u8; 32];
        let root_b = [0x02u8; 32];
        let set_a = small_hash_set(&[10, 20]);
        let set_b = small_hash_set(&[30, 40]);
        write_hashes_sidecar(&hashes_sidecar_path(tmp.path(), &root_a), &set_a).unwrap();
        write_hashes_sidecar(&hashes_sidecar_path(tmp.path(), &root_b), &set_b).unwrap();

        let union = scan_retained_payload_hashes(tmp.path()).expect("scan ok");
        let mut expected = set_a;
        expected.extend(set_b);
        assert_eq!(union, expected);
    }

    /// The scan skips non-`.hashes` entries — a `.wal` snapshot
    /// sitting in the same directory must NOT be interpreted as a
    /// sidecar.
    #[test]
    fn scan_retained_payload_hashes_skips_non_hashes_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = [0x03u8; 32];
        // Real sidecar with one hash.
        let set = small_hash_set(&[0x77]);
        write_hashes_sidecar(&hashes_sidecar_path(tmp.path(), &root), &set).unwrap();
        // Adversarial files with wrong extension.
        std::fs::write(tmp.path().join("stray.wal"), b"not a sidecar").unwrap();
        std::fs::write(tmp.path().join("stray.wal.tmp"), b"partial write").unwrap();
        std::fs::write(tmp.path().join("random.txt"), b"unrelated").unwrap();

        let union = scan_retained_payload_hashes(tmp.path()).expect("scan ok");
        assert_eq!(union, set, "only .hashes sidecar contributes");
    }

    /// A corrupt sidecar in the middle of a scan does NOT abort
    /// the pass — the valid sidecars still contribute to the
    /// union (via [`read_hashes_sidecar`]'s corrupt-returns-empty
    /// posture).
    #[test]
    fn scan_retained_payload_hashes_skips_corrupt_sidecars_silently() {
        let tmp = tempfile::tempdir().unwrap();
        let root = [0x04u8; 32];
        let set = small_hash_set(&[0x55]);
        write_hashes_sidecar(&hashes_sidecar_path(tmp.path(), &root), &set).unwrap();
        // Corrupt sidecar (short header).
        std::fs::write(tmp.path().join("bad.hashes"), b"xy").unwrap();

        let union = scan_retained_payload_hashes(tmp.path()).expect("scan ok despite corrupt file");
        assert_eq!(union, set);
    }

    /// End-to-end: [`write_snapshot`] threads through the sidecar
    /// write, so after a successful snapshot write the sidecar
    /// exists at the colocated path with the expected referenced-
    /// hash contents.  This is the LOAD-BEARING integration pin for
    /// the sidecar's role in the retention pipeline.
    #[test]
    fn write_snapshot_creates_sidecar_alongside_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = diverse_entries();
        let (snap_path, root, _merkle) = write_snapshot(tmp.path(), &entries).expect("write ok");
        assert!(snap_path.exists(), "snapshot file exists");

        let side_path = hashes_sidecar_path(tmp.path(), &root);
        assert!(side_path.exists(), "sidecar file exists next to snapshot");
        assert_eq!(side_path.parent(), snap_path.parent());
        assert_eq!(side_path.file_stem(), snap_path.file_stem());

        let recorded = read_hashes_sidecar(&side_path).expect("sidecar read ok");
        let expected = referenced_payload_hashes(&entries);
        assert_eq!(
            recorded, expected,
            "sidecar records exactly the entries' hash-variant payloads"
        );
        // Sanity: diverse_entries contains at least one Hash payload.
        assert!(
            !expected.is_empty(),
            "fixture MUST contain a Hash-variant payload for this test to be meaningful"
        );
    }

    /// Entries with no `Hash`-variant payloads → empty referenced
    /// set → the sidecar still gets written (as a 4-byte
    /// zero-count header) so retention doesn't confuse "no
    /// referenced payloads" with "sidecar-missing / assume corrupt".
    #[test]
    fn write_snapshot_with_no_hash_refs_creates_empty_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        // Chmod-only entry: no payload_ref at all.
        let entries = vec![WalEntry {
            op: WalOp::Chmod,
            path: PathBuf::from("/@bundle/f"),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: Some(0o644),
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }];
        let (_snap_path, root, _merkle) = write_snapshot(tmp.path(), &entries).expect("write ok");

        let side_path = hashes_sidecar_path(tmp.path(), &root);
        assert!(side_path.exists());
        assert_eq!(std::fs::read(&side_path).unwrap(), vec![0u8, 0, 0, 0]);
        let recorded = read_hashes_sidecar(&side_path).expect("sidecar read ok");
        assert!(recorded.is_empty());
    }

    /// Sidecar joins the scan-union naturally when written through
    /// [`write_snapshot`] — the end-to-end proof that a fresh
    /// install can go directly from "wrote snapshots" to
    /// "retention scans referenced payloads" with no intermediate
    /// wiring.
    #[test]
    fn scan_after_two_write_snapshot_calls_returns_union() {
        let tmp = tempfile::tempdir().unwrap();
        // First snapshot: diverse fixture (contains Hash-variant refs).
        let entries_a = diverse_entries();
        let (_, _, _) = write_snapshot(tmp.path(), &entries_a).expect("write a ok");
        // Second snapshot: a distinct Write with a different payload
        // so the referenced sets are disjoint (different Blake2b256).
        let entries_b = vec![WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/@bundle/w-second"),
            extra_path: None,
            offset: None,
            length: Some(9),
            payload_ref: Some(PayloadRef::hash(b"different")),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }];
        let (_, _, _) = write_snapshot(tmp.path(), &entries_b).expect("write b ok");

        let union = scan_retained_payload_hashes(tmp.path()).expect("scan ok");
        let mut expected = referenced_payload_hashes(&entries_a);
        expected.extend(referenced_payload_hashes(&entries_b));
        assert_eq!(
            union, expected,
            "scan unions payload hashes across both sidecars"
        );
    }

    // ---------------------------------------------------------------
    // Stale-tmp-file sweep (slice 5)
    // ---------------------------------------------------------------

    /// Set the mtime of `path` to `now - delta_secs` — the primitive
    /// every sweep test needs to prove the mtime cutoff fires (or
    /// doesn't).
    fn age_file(path: &Path, delta_secs: u64) {
        let now = std::time::SystemTime::now();
        let aged = now
            .checked_sub(std::time::Duration::from_secs(delta_secs))
            .expect("test clock underflow");
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open for chtime");
        let times = std::fs::FileTimes::new()
            .set_modified(aged)
            .set_accessed(aged);
        f.set_times(times).expect("set_times");
    }

    /// A missing snapshot dir SHOULD propagate as an error — sweep
    /// is a maintenance op the caller invoked explicitly.  Contrast
    /// [`scan_retained_payload_hashes`] which returns empty on a
    /// missing dir (that runs on every retention pass and shouldn't
    /// error a fresh install).
    #[test]
    fn sweep_stale_tmp_files_missing_dir_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such-dir");
        let err = sweep_stale_tmp_files(&missing, 0).expect_err("missing dir is an error");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn sweep_stale_tmp_files_empty_dir_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(sweep_stale_tmp_files(tmp.path(), 0).unwrap(), 0);
    }

    /// Files not matching a [`TMP_SUFFIXES`] entry must survive.
    /// A stray `.tmp` (without one of the recognized compound
    /// suffixes) also survives — sweep only removes what our own
    /// writers create.
    #[test]
    fn sweep_stale_tmp_files_leaves_non_tmp_files_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let keep_wal = tmp.path().join("abc.wal");
        let keep_hashes = tmp.path().join("abc.hashes");
        let keep_manifest = tmp.path().join("manifest.jsonl");
        let keep_stray_tmp = tmp.path().join("operators-own.tmp");
        for p in [&keep_wal, &keep_hashes, &keep_manifest, &keep_stray_tmp] {
            std::fs::write(p, b"content").unwrap();
            age_file(p, 3600);
        }
        assert_eq!(sweep_stale_tmp_files(tmp.path(), 0).unwrap(), 0);
        for p in [&keep_wal, &keep_hashes, &keep_manifest, &keep_stray_tmp] {
            assert!(p.exists(), "should survive: {}", p.display());
        }
    }

    #[test]
    fn sweep_stale_tmp_files_removes_wal_tmp() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("abc.12345-1-1.wal.tmp");
        std::fs::write(&path, b"leaked").unwrap();
        age_file(&path, 3600);
        assert_eq!(sweep_stale_tmp_files(tmp.path(), 0).unwrap(), 1);
        assert!(!path.exists());
    }

    /// New in slice 5 vs. the pre-sidecar fileio implementation:
    /// `.hashes.tmp` (from PR #507's sidecar writer) is also swept.
    /// A future writer whose suffix is missing from [`TMP_SUFFIXES`]
    /// would leak.  Load-bearing pin — mismatch between what
    /// [`atomic_write_file`] creates and what the sweep recognizes
    /// silently leaks disk space.
    #[test]
    fn sweep_stale_tmp_files_removes_hashes_tmp() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("abc.12345-1-1.hashes.tmp");
        std::fs::write(&path, b"leaked").unwrap();
        age_file(&path, 3600);
        assert_eq!(sweep_stale_tmp_files(tmp.path(), 0).unwrap(), 1);
        assert!(!path.exists());
    }

    /// A tmp file younger than the cutoff MUST be preserved — the
    /// mtime gate exists so this can run concurrently with an
    /// in-progress `atomic_write_file` without racing its tmp
    /// write.
    #[test]
    fn sweep_stale_tmp_files_respects_mtime_cutoff() {
        let tmp = tempfile::tempdir().unwrap();
        let fresh = tmp.path().join("abc.12345-1-1.wal.tmp");
        let stale = tmp.path().join("abc.12345-2-2.wal.tmp");
        std::fs::write(&fresh, b"fresh").unwrap();
        std::fs::write(&stale, b"stale").unwrap();
        // fresh: mtime "now" (default from write).
        age_file(&stale, 7200); // 2 hours old
                                // Cutoff of 3600 (1 hour) means stale (2h) is eligible,
                                // fresh (0s) is not.
        assert_eq!(sweep_stale_tmp_files(tmp.path(), 3600).unwrap(), 1);
        assert!(fresh.exists(), "fresh tmp preserved (younger than cutoff)");
        assert!(!stale.exists(), "stale tmp removed");
    }

    /// `older_than_secs = 0` → cutoff == now → every tmp file with
    /// mtime <= now is eligible.  Because we set the mtime slightly
    /// in the past via `age_file`, both tmp files here are removed;
    /// this pins the edge case that "sweep everything you own" is
    /// expressible without a giant cutoff value.
    #[test]
    fn sweep_stale_tmp_files_older_than_zero_removes_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("abc.12345-1-1.wal.tmp");
        let b = tmp.path().join("abc.12345-2-2.hashes.tmp");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        age_file(&a, 1);
        age_file(&b, 1);
        assert_eq!(sweep_stale_tmp_files(tmp.path(), 0).unwrap(), 2);
        assert!(!a.exists());
        assert!(!b.exists());
    }

    /// Sweep should not touch subdirectories (only file entries
    /// have tmp suffixes we care about; a `.wal.tmp` DIRECTORY
    /// is nothing we produce and removing it recursively would
    /// be an over-reach).
    #[test]
    fn sweep_stale_tmp_files_leaves_directories_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let subdir = tmp.path().join("weird.wal.tmp");
        std::fs::create_dir(&subdir).unwrap();
        // sweep should skip (remove_file on a dir fails, but our
        // debug-log-and-continue posture means the sweep still
        // returns Ok(0)).
        let removed = sweep_stale_tmp_files(tmp.path(), 0).unwrap();
        assert_eq!(removed, 0);
        assert!(subdir.exists() && subdir.is_dir());
    }

    /// Post-sweep integration: after a `write_snapshot` cycle,
    /// nothing sweep-eligible remains (rename consumed the tmp
    /// file).  Pins that the happy path does not leak tmp files
    /// under normal operation.
    #[test]
    fn sweep_stale_tmp_files_zero_after_successful_write_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = diverse_entries();
        write_snapshot(tmp.path(), &entries).expect("write ok");
        // Even at older_than_secs = 0, nothing to sweep — rename
        // consumed the tmp during the write.
        assert_eq!(sweep_stale_tmp_files(tmp.path(), 0).unwrap(), 0);
    }

    // ---------------------------------------------------------------
    // Manifest wire format (slice 6)
    // ---------------------------------------------------------------

    /// Coordinated-upgrade surface pin.  Bumping this value invalidates
    /// existing manifest lines cross the network and must land as a
    /// coordinated upgrade.
    #[test]
    fn manifest_format_version_pinned_at_1() {
        assert_eq!(MANIFEST_FORMAT_VERSION, 1);
    }

    #[test]
    fn manifest_filename_pinned_at_manifest_jsonl() {
        assert_eq!(MANIFEST_FILENAME, "manifest.jsonl");
    }

    /// Load-bearing wire-format pin: the exact JSON layout of a
    /// data entry, no whitespace, fixed field order.  Any refactor
    /// that reorders keys, adds whitespace, or drops the `v` field
    /// prefix trips this test.
    ///
    /// # H-4 coordination cost
    ///
    /// H-4 signing ([`sign_bytes`] + [`verify_with_pubkey`])
    /// signs [`ManifestEntry::to_line`]'s output byte-for-byte.
    /// Any change that trips this test would invalidate EVERY
    /// existing signature on the network — a coordinated
    /// fleet-wide upgrade, not a local refactor.  Peers
    /// producing differently-ordered JSON would produce
    /// signatures over different bytes and fail
    /// [`verify_with_pubkey`] on every joiner.
    #[test]
    fn manifest_data_entry_to_line_layout_pinned() {
        let entry = ManifestEntry {
            block_number: 42,
            root: Some([0xABu8; 32]),
            entries: 7,
            ts_ms: 1_700_000_000_000,
            sig: None,
        };
        let hex64 = "ab".repeat(32);
        let expected = format!(
            "{{\"v\":1,\"block_number\":42,\"root\":\"{hex64}\",\"entries\":7,\"ts_ms\":1700000000000}}"
        );
        assert_eq!(entry.to_line(), expected);
    }

    /// Empty-sentinel entry: `root` serializes as `null`, no
    /// quotes.  Distinct from `"null"` (which would be a bogus
    /// 4-char root hex).
    #[test]
    fn manifest_empty_entry_root_field_serializes_as_null() {
        let entry = ManifestEntry {
            block_number: 100,
            root: None,
            entries: 0,
            ts_ms: 1_700_000_000_000,
            sig: None,
        };
        let expected =
            "{\"v\":1,\"block_number\":100,\"root\":null,\"entries\":0,\"ts_ms\":1700000000000}";
        assert_eq!(entry.to_line(), expected);
    }

    /// `sig = Some(bytes)` appends `,"sig":"<hex>"` at the end
    /// (never in the middle — field ordering is contractual).
    #[test]
    fn manifest_signed_entry_appends_sig_field_at_end() {
        let entry = ManifestEntry {
            block_number: 1,
            root: Some([0x11u8; 32]),
            entries: 2,
            ts_ms: 3,
            sig: Some(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        };
        let root_hex = "11".repeat(32);
        let expected = format!(
            "{{\"v\":1,\"block_number\":1,\"root\":\"{root_hex}\",\"entries\":2,\"ts_ms\":3,\"sig\":\"deadbeef\"}}"
        );
        assert_eq!(entry.to_line(), expected);
    }

    /// Load-bearing round-trip: any entry we serialize we can
    /// parse back to structural equality.  Covers both `None`
    /// and `Some` for the root and sig options.
    #[test]
    fn manifest_entry_to_line_from_line_round_trips() {
        for entry in [
            ManifestEntry {
                block_number: -5,
                root: Some([0x33u8; 32]),
                entries: 12345,
                ts_ms: 999,
                sig: None,
            },
            ManifestEntry {
                block_number: 0,
                root: None,
                entries: 0,
                ts_ms: 0,
                sig: None,
            },
            ManifestEntry {
                block_number: i64::MAX,
                root: Some([0xAAu8; 32]),
                entries: u64::MAX,
                ts_ms: i64::MAX,
                sig: Some(vec![0x01, 0x02, 0x03, 0x04, 0x05]),
            },
        ] {
            let line = entry.to_line();
            let back = ManifestEntry::from_line(&line).expect("parse ok");
            assert_eq!(back, entry, "round-trip preserves entry: {line}");
        }
    }

    /// `data` constructor: root populated, sig=None, ts_ms from
    /// wall clock (best-effort, so we only assert it's non-zero
    /// under a normally-set clock).
    #[test]
    fn manifest_data_constructor_populates_root_and_wallclock() {
        let entry = ManifestEntry::data(7, [0x55u8; 32], 3);
        assert_eq!(entry.block_number, 7);
        assert_eq!(entry.root, Some([0x55u8; 32]));
        assert_eq!(entry.entries, 3);
        assert!(entry.ts_ms > 0, "ts_ms populated from wall clock");
        assert_eq!(entry.sig, None);
    }

    /// `empty` constructor: root=None, entries=0, sig=None.
    #[test]
    fn manifest_empty_constructor_has_null_root_and_zero_entries() {
        let entry = ManifestEntry::empty(10);
        assert_eq!(entry.block_number, 10);
        assert_eq!(entry.root, None);
        assert_eq!(entry.entries, 0);
        assert!(entry.ts_ms > 0);
        assert_eq!(entry.sig, None);
    }

    /// `v` field is mandatory: a line without it is rejected
    /// (defends against silent-decode-as-v1 for pre-versioned
    /// or corrupted lines).
    #[test]
    fn manifest_from_line_missing_v_rejected() {
        let line = "{\"block_number\":1,\"root\":null,\"entries\":0,\"ts_ms\":0}";
        let err = ManifestEntry::from_line(line).expect_err("missing v is rejected");
        assert!(
            err.contains("missing `v` field"),
            "error mentions the missing v field: {err}"
        );
    }

    /// `v` value not matching the current MANIFEST_FORMAT_VERSION
    /// surfaces a coordinated-upgrade message.
    #[test]
    fn manifest_from_line_wrong_v_rejected_with_upgrade_message() {
        let line = "{\"v\":99,\"block_number\":1,\"root\":null,\"entries\":0,\"ts_ms\":0}";
        let err = ManifestEntry::from_line(line).expect_err("wrong v is rejected");
        assert!(
            err.contains("unsupported manifest version 99") && err.contains("coordinated upgrade"),
            "error surfaces both the unsupported version and the upgrade guidance: {err}"
        );
    }

    /// `root` hex must be exactly 64 chars (32 bytes).
    #[test]
    fn manifest_from_line_short_root_hex_rejected() {
        let line = "{\"v\":1,\"block_number\":1,\"root\":\"abcd\",\"entries\":0,\"ts_ms\":0}";
        let err = ManifestEntry::from_line(line).expect_err("short root hex is rejected");
        assert!(
            err.contains("root hex must be 64 chars"),
            "error names the length constraint: {err}"
        );
    }

    /// An unknown key surfaces cleanly rather than being silently
    /// dropped — future producers introducing a new field without
    /// coordinating the upgrade would otherwise mint lines that
    /// existing consumers half-decode.
    #[test]
    fn manifest_from_line_unknown_key_rejected() {
        let line = "{\"v\":1,\"block_number\":1,\"root\":null,\"entries\":0,\"ts_ms\":0,\"future_key\":\"x\"}";
        let err = ManifestEntry::from_line(line).expect_err("unknown key is rejected");
        assert!(
            err.contains("unknown manifest key"),
            "error names the unknown key: {err}"
        );
    }

    /// Missing required field (other than v, which has its own
    /// dedicated test) surfaces cleanly.
    #[test]
    fn manifest_from_line_missing_required_field_rejected() {
        // Missing `entries`.
        let line = "{\"v\":1,\"block_number\":1,\"root\":null,\"ts_ms\":0}";
        let err = ManifestEntry::from_line(line).expect_err("missing entries is rejected");
        assert!(
            err.contains("missing entries"),
            "error names the missing field: {err}"
        );
    }

    /// Missing braces (not a JSON object at all) surfaces cleanly.
    #[test]
    fn manifest_from_line_missing_braces_rejected() {
        let line = "not a json object";
        let err = ManifestEntry::from_line(line).expect_err("no braces is rejected");
        assert!(
            err.contains("missing braces"),
            "error names the shape problem: {err}"
        );
    }

    /// The parser tolerates leading/trailing whitespace on the
    /// whole line (some editors add trailing newlines or spaces).
    #[test]
    fn manifest_from_line_tolerates_outer_whitespace() {
        let entry = ManifestEntry::empty(1);
        let line = format!("   {}   ", entry.to_line());
        let back = ManifestEntry::from_line(&line).expect("trimmed parse ok");
        assert_eq!(back, entry);
    }

    /// Inner whitespace (around `:` and `,`) is also tolerated —
    /// producers with different JSON-emitter style should still
    /// parse.  The trims on key/value happen after
    /// [`split_top_level_commas`], so this exercises both paths.
    #[test]
    fn manifest_from_line_tolerates_inner_whitespace() {
        let line = "{ \"v\" : 1 , \"block_number\" : 42 , \"root\" : null , \
                     \"entries\" : 0 , \"ts_ms\" : 1700000000000 }";
        let back = ManifestEntry::from_line(line).expect("inner-whitespace parse ok");
        assert_eq!(back, ManifestEntry {
            block_number: 42,
            root: None,
            entries: 0,
            ts_ms: 1_700_000_000_000,
            sig: None,
        });
    }

    /// Load-bearing when H-4 signing arrives: signatures cover
    /// [`ManifestEntry::to_line`] output byte-for-byte (fixed
    /// order), but [`ManifestEntry::from_line`] MUST tolerate
    /// arbitrary field orderings so a peer running a differently-
    /// implemented producer (or a future emitter that reorders)
    /// still parses cleanly.  Pins the write-vs-parse asymmetry:
    /// serialize is order-fixed, deserialize is order-agnostic.
    #[test]
    fn manifest_from_line_is_field_order_agnostic() {
        let line =
            "{\"entries\":0,\"v\":1,\"ts_ms\":1700000000000,\"block_number\":42,\"root\":null}";
        let back = ManifestEntry::from_line(line).expect("reordered parse ok");
        assert_eq!(back, ManifestEntry {
            block_number: 42,
            root: None,
            entries: 0,
            ts_ms: 1_700_000_000_000,
            sig: None,
        });
    }

    /// Pins the odd-length-hex guard on the `sig` field.  An odd
    /// number of hex characters is definitionally not a byte
    /// sequence; reject cleanly rather than silently truncating
    /// or panicking.
    #[test]
    fn manifest_from_line_odd_length_sig_hex_rejected() {
        let line =
            "{\"v\":1,\"block_number\":1,\"root\":null,\"entries\":0,\"ts_ms\":0,\"sig\":\"abc\"}";
        let err = ManifestEntry::from_line(line).expect_err("odd-length sig hex is rejected");
        assert!(
            err.contains("sig hex length must be even"),
            "error names the length constraint: {err}"
        );
    }

    // Helpers pins (small but the split-commas helper is subtle
    // enough to deserve one dedicated test).

    #[test]
    fn split_top_level_commas_ignores_commas_inside_strings() {
        let parts = split_top_level_commas("\"a,b\":\"c\",\"d\":\"e,f\"");
        assert_eq!(parts, vec![
            "\"a,b\":\"c\"".to_string(),
            "\"d\":\"e,f\"".to_string()
        ]);
    }

    #[test]
    fn hex_encode_decode_32_round_trips_arbitrary_bytes() {
        let mut b = [0u8; 32];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i * 7 + 13) as u8;
        }
        let hex = hex_encode(&b);
        assert_eq!(hex.len(), 64);
        let back = hex_decode_32(&hex).expect("decode ok");
        assert_eq!(back, b);
    }

    #[test]
    fn hex_decode_32_rejects_wrong_length() {
        assert!(hex_decode_32("abcd").is_err());
    }

    #[test]
    fn hex_decode_32_rejects_non_hex_chars() {
        let mostly_hex: String = std::iter::repeat_n('0', 63)
            .chain(std::iter::once('z'))
            .collect();
        assert!(hex_decode_32(&mostly_hex).is_err());
    }

    // ---------------------------------------------------------------
    // Manifest persistence (slice 7)
    // ---------------------------------------------------------------

    /// Fresh install: no manifest file yet.  Callers see an empty
    /// Vec, not an error.
    #[test]
    fn read_manifest_missing_file_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let out = read_manifest(tmp.path()).expect("missing manifest is OK, empty");
        assert!(out.is_empty());
    }

    /// Empty file (created but never written) also returns empty
    /// — semantically the same as "no snapshots advertised."
    #[test]
    fn read_manifest_empty_file_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(MANIFEST_FILENAME), b"").unwrap();
        assert!(read_manifest(tmp.path()).unwrap().is_empty());
    }

    /// First `append_manifest_entry` creates the file at the
    /// expected path.
    #[test]
    fn append_manifest_entry_creates_file_on_first_append() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(MANIFEST_FILENAME);
        assert!(!path.exists());
        append_manifest_entry(tmp.path(), ManifestEntry::empty(1)).expect("append ok");
        assert!(path.exists());
    }

    /// Append writes exactly `to_line() + "\n"` — the newline is
    /// added by the appender, not by `to_line` (which returns a
    /// bare line for composition into other contexts).
    #[test]
    fn append_manifest_entry_writes_line_with_trailing_newline() {
        let tmp = tempfile::tempdir().unwrap();
        let entry = ManifestEntry::empty(1);
        append_manifest_entry(tmp.path(), entry.clone()).expect("append ok");
        let bytes = std::fs::read_to_string(tmp.path().join(MANIFEST_FILENAME)).unwrap();
        assert_eq!(bytes, format!("{}\n", entry.to_line()));
    }

    /// Second `append_manifest_entry` on an existing file appends
    /// (not overwrites).  Load-bearing behavior: the manifest is
    /// meant to grow over the lifetime of the snapshot dir.
    #[test]
    fn append_manifest_entry_appends_to_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let e1 = ManifestEntry::empty(1);
        let e2 = ManifestEntry::empty(2);
        append_manifest_entry(tmp.path(), e1.clone()).unwrap();
        append_manifest_entry(tmp.path(), e2.clone()).unwrap();
        let bytes = std::fs::read_to_string(tmp.path().join(MANIFEST_FILENAME)).unwrap();
        assert_eq!(bytes, format!("{}\n{}\n", e1.to_line(), e2.to_line()));
    }

    /// End-to-end load-bearing round-trip: `read_manifest` after
    /// multiple `append_manifest_entry` calls returns the entries
    /// in append order.
    #[test]
    fn append_read_round_trips_multiple_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![
            ManifestEntry::data(1, [0x11u8; 32], 3),
            ManifestEntry::empty(2),
            ManifestEntry::data(3, [0x33u8; 32], 7),
        ];
        for e in &entries {
            append_manifest_entry(tmp.path(), e.clone()).expect("append ok");
        }
        let back = read_manifest(tmp.path()).expect("read ok");
        assert_eq!(
            back, entries,
            "read returns entries in append (= write) order"
        );
    }

    /// Blank lines within the manifest (e.g., from an editor
    /// pass) are tolerated — parsing skips them.  Not a
    /// well-formed producer's output, but a resilience posture
    /// against manual edits.
    #[test]
    fn read_manifest_skips_blank_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let e1 = ManifestEntry::empty(1);
        let e2 = ManifestEntry::empty(2);
        let contents = format!("{}\n\n\n   \n{}\n", e1.to_line(), e2.to_line());
        std::fs::write(tmp.path().join(MANIFEST_FILENAME), contents).unwrap();
        let back = read_manifest(tmp.path()).expect("read ok");
        assert_eq!(back, vec![e1, e2]);
    }

    /// A malformed line surfaces as [`SnapshotError::MalformedManifest`]
    /// with the 1-based line number as a structured field.  Pattern-
    /// matching this variant apart from [`SnapshotError::Io`] lets
    /// the join-protocol layer distinguish "manifest schema issue,
    /// retry with a different peer" from "real disk problem,
    /// escalate."
    #[test]
    fn read_manifest_malformed_line_returns_error_with_line_number() {
        let tmp = tempfile::tempdir().unwrap();
        let good = ManifestEntry::empty(1);
        // Second line is malformed (missing braces).
        let contents = format!("{}\nnot json here\n", good.to_line());
        std::fs::write(tmp.path().join(MANIFEST_FILENAME), contents).unwrap();
        let err = read_manifest(tmp.path()).expect_err("malformed line surfaces");
        match err {
            SnapshotError::MalformedManifest { line, cause } => {
                assert_eq!(line, 2, "1-based line number");
                assert!(
                    cause.contains("missing braces"),
                    "underlying parse error: {cause}"
                );
                // Also pin the Display shape for operator-facing text.
                let msg = format!("{}", SnapshotError::MalformedManifest { line, cause });
                assert!(msg.starts_with("manifest line 2:"), "Display shape: {msg}");
            }
            other => panic!("expected MalformedManifest, got: {other:?}"),
        }
    }

    /// Partial-view posture: parsing halts at the first malformed
    /// line — entries before it are not returned, even though
    /// they parsed successfully.  Callers wanting a "best-effort
    /// prefix" should retry with a fixed manifest.  Pins the
    /// all-or-nothing behavior.
    #[test]
    fn read_manifest_stops_at_first_malformed_line() {
        let tmp = tempfile::tempdir().unwrap();
        let good_a = ManifestEntry::empty(1);
        let good_b = ManifestEntry::empty(3);
        let contents = format!("{}\nnot json\n{}\n", good_a.to_line(), good_b.to_line());
        std::fs::write(tmp.path().join(MANIFEST_FILENAME), contents).unwrap();
        assert!(read_manifest(tmp.path()).is_err());
    }

    /// Append preserves numeric order under a single-writer
    /// cadence (block_number climbs monotonically per validator).
    /// This is a natural consequence of append-only + writer
    /// discipline; pinned here so a future SnapshotWriter that
    /// accidentally shuffles entries would be caught.
    #[test]
    fn append_preserves_block_number_order_under_single_writer() {
        let tmp = tempfile::tempdir().unwrap();
        for bn in 1..=5 {
            append_manifest_entry(tmp.path(), ManifestEntry::empty(bn)).unwrap();
        }
        let back = read_manifest(tmp.path()).expect("read ok");
        let bns: Vec<i64> = back.iter().map(|e| e.block_number).collect();
        assert_eq!(bns, vec![1, 2, 3, 4, 5]);
    }

    /// Explicit 0o644 mode on unix so shared-storage joiners see
    /// consistent metadata regardless of the leader's umask.
    /// Matches the `atomic_write_file` posture.
    #[cfg(unix)]
    #[test]
    fn append_manifest_entry_creates_file_with_0o644_mode() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        append_manifest_entry(tmp.path(), ManifestEntry::empty(1)).expect("append ok");
        let meta = std::fs::metadata(tmp.path().join(MANIFEST_FILENAME)).unwrap();
        // Low 9 bits are the rwxrwxrwx mode; higher bits are
        // file-type flags.  Mask to compare just the mode bits.
        assert_eq!(meta.permissions().mode() & 0o777, 0o644);
    }

    // ---------------------------------------------------------------
    // Directory pruning (slice 8)
    // ---------------------------------------------------------------

    /// Same posture as [`sweep_stale_tmp_files`]: an explicit
    /// maintenance op that surfaces a missing dir as an error
    /// rather than a no-op.
    #[test]
    fn prune_missing_dir_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such-dir");
        let err = prune_snapshot_dir(&missing, 1).expect_err("missing dir is an error");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn prune_empty_dir_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(prune_snapshot_dir(tmp.path(), 5).unwrap(), 0);
    }

    /// `keep_last_n >= count` leaves everything alone.
    #[test]
    fn prune_keep_last_n_greater_than_count_removes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..3 {
            let name = format!("{:064x}.wal", i);
            std::fs::write(tmp.path().join(&name), b"x").unwrap();
        }
        assert_eq!(prune_snapshot_dir(tmp.path(), 10).unwrap(), 0);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 3);
    }

    /// `keep_last_n = 0` removes every `.wal` file.
    #[test]
    fn prune_keep_last_n_zero_removes_everything() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..3 {
            let name = format!("{:064x}.wal", i);
            std::fs::write(tmp.path().join(&name), b"x").unwrap();
        }
        assert_eq!(prune_snapshot_dir(tmp.path(), 0).unwrap(), 3);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    /// Load-bearing: retention keeps the N newest by mtime, not
    /// by filename order.  Five snapshots with staggered mtimes,
    /// keep 2 → the two newest survive.
    #[test]
    fn prune_keeps_newest_n_by_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..5 {
            let name = format!("{:064x}.wal", i);
            let p = tmp.path().join(&name);
            std::fs::write(&p, b"x").unwrap();
            // Age file[i] by (5 - i) hours: file[0] is oldest,
            // file[4] is newest.
            age_file(&p, ((5 - i) as u64) * 3600);
            paths.push(p);
        }
        assert_eq!(prune_snapshot_dir(tmp.path(), 2).unwrap(), 3);
        // The two newest (indices 3, 4) survive.
        assert!(!paths[0].exists());
        assert!(!paths[1].exists());
        assert!(!paths[2].exists());
        assert!(paths[3].exists());
        assert!(paths[4].exists());
    }

    /// Sidecar pairing: pruning a `.wal` also removes its paired
    /// `.hashes` sidecar so stale sidecars don't outlive their
    /// snapshots (which would defeat retention's payload-hash
    /// union pass).
    #[test]
    fn prune_pairs_sidecar_removal() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = diverse_entries();
        // Two snapshots via the real writer so both `.wal` and
        // `.hashes` files exist naturally.
        let (path_a, _, _) = write_snapshot(tmp.path(), &entries).expect("write a ok");
        // Second snapshot must differ; add a synthetic entry.
        let mut entries_b = entries.clone();
        entries_b.push(WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/@bundle/second"),
            extra_path: None,
            offset: None,
            length: Some(4),
            payload_ref: Some(PayloadRef::hash(b"more")),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        });
        let (path_b, _, _) = write_snapshot(tmp.path(), &entries_b).expect("write b ok");
        let sidecar_a = path_a.with_extension("hashes");
        let sidecar_b = path_b.with_extension("hashes");
        assert!(sidecar_a.exists() && sidecar_b.exists());
        // Age snapshot_a so snapshot_b is strictly newer.
        age_file(&path_a, 3600);
        age_file(&sidecar_a, 3600);

        assert_eq!(prune_snapshot_dir(tmp.path(), 1).unwrap(), 1);
        assert!(!path_a.exists(), "old .wal removed");
        assert!(!sidecar_a.exists(), "paired .hashes sidecar removed");
        assert!(path_b.exists(), "new .wal survives");
        assert!(sidecar_b.exists(), "new .hashes survives");
    }

    /// A `.wal` without a paired sidecar (e.g., a pre-sidecar
    /// snapshot from before PR #507) still prunes cleanly —
    /// ENOENT on the sidecar removal is silently OK.
    #[test]
    fn prune_missing_sidecar_is_fine() {
        let tmp = tempfile::tempdir().unwrap();
        let name = format!("{:064x}.wal", 1);
        let p = tmp.path().join(&name);
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(prune_snapshot_dir(tmp.path(), 0).unwrap(), 1);
        assert!(!p.exists());
    }

    /// Non-`.wal` files survive prune untouched: an orphan
    /// `.hashes` (no matching `.wal`), the manifest itself, and
    /// stale `.wal.tmp` files.
    #[test]
    fn prune_leaves_non_wal_files_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let orphan_hashes = tmp.path().join(format!("{:064x}.hashes", 0xAB));
        let manifest = tmp.path().join(MANIFEST_FILENAME);
        let stale_tmp = tmp.path().join("something.12345-1-1.wal.tmp");
        for p in [&orphan_hashes, &manifest, &stale_tmp] {
            std::fs::write(p, b"x").unwrap();
        }
        assert_eq!(prune_snapshot_dir(tmp.path(), 0).unwrap(), 0);
        for p in [&orphan_hashes, &manifest, &stale_tmp] {
            assert!(p.exists(), "should survive prune: {}", p.display());
        }
    }

    /// Symlink `.wal` entries are skipped — never counted, never
    /// removed by name (which would just unlink the symlink,
    /// noisy).  Operator hygiene defense.
    #[cfg(unix)]
    #[test]
    fn prune_skips_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join(format!("{:064x}.wal", 1));
        let link = tmp.path().join(format!("{:064x}.wal", 2));
        std::fs::write(&real, b"real").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        age_file(&real, 3600);
        // keep_last_n=0: both real+link would be removed if the
        // symlink weren't skipped.  With the skip: only the real
        // file is a candidate; it gets removed, count = 1.  Link
        // itself is left alone (dangling, but that's operator's
        // problem to clean up).
        assert_eq!(prune_snapshot_dir(tmp.path(), 0).unwrap(), 1);
        assert!(!real.exists(), "real .wal removed");
        // symlink_metadata (lstat) so we check the link itself,
        // not the target.
        assert!(
            std::fs::symlink_metadata(&link).is_ok(),
            "symlink survives prune"
        );
    }

    /// Prune returns the count of `.wal` files SUCCESSFULLY
    /// removed — not the count of pruning candidates, and not
    /// counting sidecar removals.
    #[test]
    fn prune_returns_count_of_wal_removals_only() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..4 {
            let name = format!("{:064x}.wal", i);
            let p = tmp.path().join(&name);
            std::fs::write(&p, b"x").unwrap();
            // Also drop a sidecar so we can prove sidecar
            // removals don't inflate the count.
            let sidecar = p.with_extension("hashes");
            std::fs::write(&sidecar, b"y").unwrap();
            age_file(&p, ((10 - i) as u64) * 60);
        }
        // Keep 1; expect 3 `.wal` removals (and 3 silent sidecar
        // removals) — count is `.wal` only.
        assert_eq!(prune_snapshot_dir(tmp.path(), 1).unwrap(), 3);
    }

    /// Post-write happy path: after a single `write_snapshot`
    /// cycle, `prune_snapshot_dir(dir, 1)` removes nothing
    /// (there is exactly one snapshot).
    #[test]
    fn prune_after_write_snapshot_cycle_keeps_just_written_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = diverse_entries();
        let (path, _, _) = write_snapshot(tmp.path(), &entries).expect("write ok");
        let sidecar = path.with_extension("hashes");
        assert_eq!(prune_snapshot_dir(tmp.path(), 1).unwrap(), 0);
        assert!(path.exists() && sidecar.exists());
    }

    /// Load-bearing invariant: if the `.wal` removal fails (e.g.,
    /// permissions, filesystem quirk, locked by another process),
    /// the paired `.hashes` sidecar MUST be preserved.  Otherwise
    /// we create an orphan `.wal` without its sidecar, and the
    /// next [`scan_retained_payload_hashes`] pass would
    /// under-count the payload hashes this still-live snapshot
    /// references → payload-store retention would delete the
    /// referenced bytes → snapshot becomes unreadable.
    ///
    /// This is the exact "orphan payload" hazard the
    /// manifest-independent pruning design was engineered to
    /// avoid; this test closes the symmetric case at the
    /// `.wal`-removal-failure path.  Portable orchestration:
    /// create the `.wal` as a DIRECTORY so `remove_file` returns
    /// EISDIR on every POSIX system without needing special
    /// privileges or filesystem features.
    #[test]
    fn prune_wal_removal_failure_preserves_paired_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let wal_path = tmp.path().join(format!("{:064x}.wal", 1));
        let sidecar_path = wal_path.with_extension("hashes");
        // `.wal` as a directory: `remove_file` will fail with EISDIR.
        std::fs::create_dir(&wal_path).unwrap();
        // `.hashes` as a regular file: `remove_file` would
        // succeed if attempted — which the fix forbids.
        std::fs::write(&sidecar_path, b"preserve me").unwrap();

        // keep_last_n = 0 → every `.wal` candidate enters the
        // remove loop.  The directory's `remove_file` fails;
        // per the orphan-`.wal` invariant, the sidecar MUST
        // survive.
        let removed = prune_snapshot_dir(tmp.path(), 0).unwrap();
        assert_eq!(removed, 0, "no `.wal` was successfully removed");
        assert!(wal_path.exists(), "orphan `.wal` survives (remove failed)");
        assert!(
            sidecar_path.exists(),
            "paired sidecar MUST survive when `.wal` remove failed \
             (otherwise next retention pass under-counts hashes)"
        );
    }

    /// Same-mtime tiebreaker: two `.wal` files with identical
    /// mtimes must have deterministic pruning survivors (not
    /// dependent on `read_dir` order, which is OS/filesystem-
    /// specific).  The secondary sort key is filename, so with
    /// `keep_last_n = 1` the lexicographically-smaller name is
    /// the survivor (reverse mtime, then forward filename →
    /// smaller filename wins the tiebreaker when mtimes match).
    #[test]
    fn prune_same_mtime_tiebreaker_is_deterministic_by_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join(format!("{:064x}.wal", 1)); // "0000...0001.wal"
        let b = tmp.path().join(format!("{:064x}.wal", 2)); // "0000...0002.wal"
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"y").unwrap();
        // Force both to the same mtime so the tiebreaker fires.
        let aged = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        for p in [&a, &b] {
            let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
            let times = std::fs::FileTimes::new()
                .set_modified(aged)
                .set_accessed(aged);
            f.set_times(times).unwrap();
        }
        // keep_last_n = 1.  Sort key is (mtime desc, filename asc).
        // With equal mtimes, filename-ascending puts `a` (hex "0...1")
        // ahead of `b` (hex "0...2"), so `a` is "newest" per the
        // tiebreaker and survives.
        assert_eq!(prune_snapshot_dir(tmp.path(), 1).unwrap(), 1);
        assert!(a.exists(), "lexicographically-smaller filename survives");
        assert!(!b.exists());
    }

    // ---------------------------------------------------------------
    // H-4 signing (slice 9)
    // ---------------------------------------------------------------

    fn new_secp256k1_keypair() -> (Vec<u8>, Vec<u8>) {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        let (sk, pk) = Secp256k1.new_key_pair();
        (sk.bytes.to_vec(), pk.bytes.to_vec())
    }

    fn fixture_entry() -> ManifestEntry {
        ManifestEntry {
            block_number: 42,
            root: Some([0xABu8; 32]),
            entries: 7,
            ts_ms: 1_700_000_000_000,
            sig: None,
        }
    }

    /// `sign_bytes` is deterministic — same entry, same output.
    /// Load-bearing property for signatures to verify
    /// deterministically across runs.
    #[test]
    fn manifest_sign_bytes_is_deterministic() {
        let entry = fixture_entry();
        assert_eq!(entry.sign_bytes(), entry.sign_bytes());
    }

    /// The output is a 32-byte Blake2b256 digest.  Pins the
    /// hash primitive — a change to a different hash function
    /// (same size, different algorithm) would silently
    /// invalidate every existing signature.
    #[test]
    fn manifest_sign_bytes_is_32_byte_blake2b256_digest() {
        assert_eq!(fixture_entry().sign_bytes().len(), 32);
    }

    /// `sign_bytes` ignores the `sig` field.  A signature
    /// cannot cover its own output — if it did, flipping `sig`
    /// from `None` to `Some(sig_bytes)` after signing would
    /// invalidate the signature it just created.
    #[test]
    fn manifest_sign_bytes_ignores_sig_field() {
        let unsigned = fixture_entry();
        let mut signed_shape = unsigned.clone();
        signed_shape.sig = Some(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(unsigned.sign_bytes(), signed_shape.sign_bytes());
    }

    /// Each semantic field is covered by the signed message —
    /// changing block_number, root, entries, or ts_ms yields a
    /// different digest.  Prevents a tampering attack where an
    /// attacker modifies one field hoping the signature still
    /// matches (which it wouldn't, if sign_bytes covers it).
    #[test]
    fn manifest_sign_bytes_differs_by_each_semantic_field() {
        let base = fixture_entry();
        let base_hash = base.sign_bytes();

        let mut mutated = base.clone();
        mutated.block_number += 1;
        assert_ne!(
            mutated.sign_bytes(),
            base_hash,
            "block_number must affect digest"
        );

        let mut mutated = base.clone();
        mutated.root = Some([0xCDu8; 32]);
        assert_ne!(
            mutated.sign_bytes(),
            base_hash,
            "root value must affect digest"
        );

        let mut mutated = base.clone();
        mutated.root = None;
        assert_ne!(
            mutated.sign_bytes(),
            base_hash,
            "root presence must affect digest"
        );

        let mut mutated = base.clone();
        mutated.entries += 1;
        assert_ne!(
            mutated.sign_bytes(),
            base_hash,
            "entries must affect digest"
        );

        let mut mutated = base.clone();
        mutated.ts_ms += 1;
        assert_ne!(mutated.sign_bytes(), base_hash, "ts_ms must affect digest");
    }

    /// `signed()` populates the `sig` field; the returned
    /// entry's other fields are unchanged.
    #[test]
    fn manifest_signed_populates_sig_field_and_preserves_others() {
        let (sk, _pk) = new_secp256k1_keypair();
        let unsigned = fixture_entry();
        let signed = unsigned.clone().signed(&sk);
        assert!(signed.sig.is_some(), "sig populated after signed()");
        // Non-sig fields preserved.
        assert_eq!(signed.block_number, unsigned.block_number);
        assert_eq!(signed.root, unsigned.root);
        assert_eq!(signed.entries, unsigned.entries);
        assert_eq!(signed.ts_ms, unsigned.ts_ms);
    }

    /// Load-bearing round-trip: a freshly-signed entry verifies
    /// cleanly against the same public key.
    #[test]
    fn manifest_signed_verify_round_trips_with_matching_pubkey() {
        let (sk, pk) = new_secp256k1_keypair();
        let signed = fixture_entry().signed(&sk);
        signed.verify_with_pubkey(&pk).expect("verify ok");
    }

    /// A signature made with one key does NOT verify under a
    /// different public key.  Baseline cryptographic property;
    /// pinned to catch a future implementation bug that would,
    /// e.g., accidentally accept any pubkey.
    #[test]
    fn manifest_verify_with_wrong_pubkey_returns_signature_invalid() {
        let (sk, _pk) = new_secp256k1_keypair();
        let (_sk2, pk2) = new_secp256k1_keypair();
        let signed = fixture_entry().signed(&sk);
        match signed.verify_with_pubkey(&pk2) {
            Err(SnapshotError::ManifestSignatureInvalid) => (),
            other => panic!("expected ManifestSignatureInvalid, got: {other:?}"),
        }
    }

    /// Verify on an entry with `sig = None` surfaces
    /// `UnsignedManifestEntry` (distinct from "signed but
    /// doesn't verify") — the two-variant split lets join
    /// clients diagnose the operator-facing problem.
    #[test]
    fn manifest_verify_unsigned_returns_unsigned_error() {
        let (_sk, pk) = new_secp256k1_keypair();
        let unsigned = fixture_entry();
        match unsigned.verify_with_pubkey(&pk) {
            Err(SnapshotError::UnsignedManifestEntry) => (),
            other => panic!("expected UnsignedManifestEntry, got: {other:?}"),
        }
    }

    /// Tampering with ANY signed field (post-signing) breaks
    /// verification — the tamper-detection load-bearer.
    #[test]
    fn manifest_verify_fails_on_any_tampered_field() {
        let (sk, pk) = new_secp256k1_keypair();
        let signed = fixture_entry().signed(&sk);

        for mutate in [
            Box::new(|e: &mut ManifestEntry| e.block_number += 1)
                as Box<dyn Fn(&mut ManifestEntry)>,
            Box::new(|e: &mut ManifestEntry| e.root = Some([0xCDu8; 32])),
            Box::new(|e: &mut ManifestEntry| e.root = None),
            Box::new(|e: &mut ManifestEntry| e.entries += 1),
            Box::new(|e: &mut ManifestEntry| e.ts_ms += 1),
        ] {
            let mut tampered = signed.clone();
            mutate(&mut tampered);
            match tampered.verify_with_pubkey(&pk) {
                Err(SnapshotError::ManifestSignatureInvalid) => (),
                other => panic!("tampered entry should fail verify: {other:?}"),
            }
        }
    }

    /// End-to-end integration: sign an entry, serialize to a
    /// line, parse the line back, verify.  Load-bearing — pins
    /// that `to_line` / `from_line` preserve the `sig` field
    /// losslessly across the wire format.
    #[test]
    fn manifest_signed_to_line_from_line_round_trips_with_verify() {
        let (sk, pk) = new_secp256k1_keypair();
        let signed = fixture_entry().signed(&sk);
        let line = signed.to_line();
        let parsed = ManifestEntry::from_line(&line).expect("parse ok");
        assert_eq!(parsed, signed);
        parsed
            .verify_with_pubkey(&pk)
            .expect("verify ok after wire round-trip");
    }

    /// The signed message prefixes `MANIFEST_FORMAT_VERSION` so
    /// a v1-signed entry's bytes could not be mistaken for a
    /// hypothetical v2-signed entry's.  Pins the first byte of
    /// the pre-hash buffer by constructing it manually.
    #[test]
    fn manifest_sign_bytes_version_byte_prefix_is_pinned() {
        let entry = ManifestEntry {
            block_number: 0,
            root: None,
            entries: 0,
            ts_ms: 0,
            sig: None,
        };
        // Expected pre-hash buffer: [MANIFEST_FORMAT_VERSION, 0...0 (bn), 0 (root=None), 0...0 (entries), 0...0 (ts_ms)]
        let mut expected_prehash = Vec::with_capacity(1 + 8 + 1 + 8 + 8);
        expected_prehash.push(MANIFEST_FORMAT_VERSION);
        expected_prehash.extend_from_slice(&0i64.to_be_bytes());
        expected_prehash.push(0u8); // root presence = None
        expected_prehash.extend_from_slice(&0u64.to_be_bytes());
        expected_prehash.extend_from_slice(&0i64.to_be_bytes());
        let expected_hash = Blake2b256::hash(expected_prehash);
        assert_eq!(entry.sign_bytes(), expected_hash);
    }

    /// This crate's secp256k1 strategy uses `sign_prehash` (RFC
    /// 6979 deterministic nonces), so two signings of the same
    /// entry under the same key produce byte-identical `sig`
    /// bytes.  Pins the current strategy so a swap to a
    /// randomized-ECDSA backend (which would still verify but
    /// not byte-match) surfaces here instead of silently
    /// breaking a downstream consumer that assumed sig-equality.
    #[test]
    fn manifest_signed_is_deterministic_rfc6979() {
        let (sk, pk) = new_secp256k1_keypair();
        let a = fixture_entry().signed(&sk);
        let b = fixture_entry().signed(&sk);
        assert_eq!(
            a.sig, b.sig,
            "RFC 6979: two signings of the same entry under the same key \
             produce byte-identical sig bytes"
        );
        // Semantic contract still holds either way.
        a.verify_with_pubkey(&pk).unwrap();
        b.verify_with_pubkey(&pk).unwrap();
    }

    /// Garbage `sig` bytes (zero-length, random non-DER) must
    /// surface `ManifestSignatureInvalid` rather than panicking
    /// or returning Ok.  Pins the delegation contract with
    /// `Secp256k1::verify` — any bytes that aren't a valid
    /// signature over `sign_bytes()` under the pubkey are
    /// rejected cleanly.
    #[test]
    fn manifest_verify_malformed_sig_bytes_returns_signature_invalid() {
        let (_sk, pk) = new_secp256k1_keypair();
        let mut entry = fixture_entry();

        // Zero-length sig bytes.
        entry.sig = Some(Vec::new());
        assert!(matches!(
            entry.verify_with_pubkey(&pk),
            Err(SnapshotError::ManifestSignatureInvalid)
        ));

        // Short, non-DER garbage.
        entry.sig = Some(vec![0, 0, 0]);
        assert!(matches!(
            entry.verify_with_pubkey(&pk),
            Err(SnapshotError::ManifestSignatureInvalid)
        ));

        // 72 bytes (plausible length) but all zeros — not a
        // valid DER-encoded signature.
        entry.sig = Some(vec![0u8; 72]);
        assert!(matches!(
            entry.verify_with_pubkey(&pk),
            Err(SnapshotError::ManifestSignatureInvalid)
        ));
    }

    // ---------------------------------------------------------------
    // SnapshotWriter (slice 10)
    // ---------------------------------------------------------------

    fn mk_writer(dir: PathBuf, cadence: u64, retain: usize) -> SnapshotWriter {
        SnapshotWriter {
            dir,
            cadence,
            retain,
            signer_sk: None,
            payload_dir: None,
        }
    }

    /// Cadence miss: block_number not a multiple of cadence →
    /// Ok(None), no files touched.
    #[test]
    fn snapshot_writer_cadence_miss_returns_none_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let w = mk_writer(tmp.path().to_path_buf(), 10, 2);
        assert_eq!(w.maybe_write(5, &diverse_entries()).unwrap(), None);
        assert_eq!(
            std::fs::read_dir(tmp.path()).unwrap().count(),
            0,
            "cadence miss writes nothing"
        );
    }

    /// Negative block number → Ok(None) (pre-genesis sentinel
    /// value).
    #[test]
    fn snapshot_writer_negative_block_number_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let w = mk_writer(tmp.path().to_path_buf(), 1, 2);
        assert_eq!(w.maybe_write(-1, &diverse_entries()).unwrap(), None);
    }

    /// Genesis (block_number = 0) is a cadence hit for ANY
    /// cadence (0 % N == 0 for all N >= 1).
    #[test]
    fn snapshot_writer_genesis_block_zero_is_cadence_hit() {
        let tmp = tempfile::tempdir().unwrap();
        let w = mk_writer(tmp.path().to_path_buf(), 10, 2);
        let out = w.maybe_write(0, &diverse_entries()).unwrap();
        assert!(out.is_some(), "block 0 is a cadence hit for any cadence");
    }

    /// Cadence hit with non-empty entries: writes a `.wal` file,
    /// returns Ok(Some(root, merkle_root)) matching an
    /// independent `write_snapshot` call for the same entries.
    #[test]
    fn snapshot_writer_cadence_hit_writes_snapshot_and_returns_matching_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let w = mk_writer(tmp.path().to_path_buf(), 1, 10);
        let entries = diverse_entries();
        let (root, merkle_root) = w.maybe_write(1, &entries).unwrap().expect("Some");

        // Independent write (into a different dir) MUST produce
        // the same (root, merkle_root) — content-addressing is
        // deterministic across validators.
        let indep = tempfile::tempdir().unwrap();
        let (_, root2, merkle2) = write_snapshot(indep.path(), &entries).unwrap();
        assert_eq!(root, root2);
        assert_eq!(merkle_root, merkle2);

        // The writer's dir contains the `.wal` at the expected
        // content-addressed path.
        assert!(snapshot_path(tmp.path(), &root).exists());
    }

    /// Empty-entries cadence hit writes NO `.wal` file but DOES
    /// append an empty-sentinel manifest line.  Return value is
    /// Ok(None) — caller cannot distinguish from cadence miss.
    #[test]
    fn snapshot_writer_empty_entries_writes_sentinel_only() {
        let tmp = tempfile::tempdir().unwrap();
        let w = mk_writer(tmp.path().to_path_buf(), 1, 10);
        assert_eq!(w.maybe_write(5, &[]).unwrap(), None);
        // No `.wal` file.
        let wal_count = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("wal"))
            .count();
        assert_eq!(wal_count, 0, "no .wal file on empty-sentinel cadence hit");
        // Manifest has one entry: the empty sentinel.
        let entries = read_manifest(tmp.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].block_number, 5);
        assert_eq!(entries[0].root, None, "empty sentinel has root = None");
        assert_eq!(entries[0].entries, 0);
    }

    /// Load-bearing: a successful write appends a corresponding
    /// manifest entry.  The two sides of the write (snapshot +
    /// manifest) stay paired.
    #[test]
    fn snapshot_writer_appends_manifest_entry_on_successful_write() {
        let tmp = tempfile::tempdir().unwrap();
        let w = mk_writer(tmp.path().to_path_buf(), 1, 10);
        let entries = diverse_entries();
        let (root, _) = w.maybe_write(7, &entries).unwrap().expect("Some");
        let manifest = read_manifest(tmp.path()).unwrap();
        assert_eq!(manifest.len(), 1);
        assert_eq!(manifest[0].block_number, 7);
        assert_eq!(manifest[0].root, Some(root));
        assert_eq!(manifest[0].entries, entries.len() as u64);
    }

    /// `signer_sk = None` ships unsigned manifest entries (the
    /// pre-H-4 wire format).  Join clients MUST reject these in
    /// production, but tests and observer nodes may legitimately
    /// produce them.
    #[test]
    fn snapshot_writer_without_signer_produces_unsigned_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let w = mk_writer(tmp.path().to_path_buf(), 1, 10);
        w.maybe_write(1, &diverse_entries()).unwrap();
        let manifest = read_manifest(tmp.path()).unwrap();
        assert_eq!(manifest[0].sig, None);
    }

    /// Load-bearing H-4 end-to-end: `signer_sk = Some(sk)` emits
    /// a signed manifest entry that verifies under the matching
    /// pubkey.
    #[test]
    fn snapshot_writer_with_signer_produces_verifiable_signed_entry() {
        let (sk, pk) = new_secp256k1_keypair();
        let tmp = tempfile::tempdir().unwrap();
        let mut w = mk_writer(tmp.path().to_path_buf(), 1, 10);
        w.signer_sk = Some(sk);
        w.maybe_write(1, &diverse_entries()).unwrap();
        let manifest = read_manifest(tmp.path()).unwrap();
        assert_eq!(manifest.len(), 1);
        assert!(manifest[0].sig.is_some(), "signed entry has sig populated");
        manifest[0]
            .verify_with_pubkey(&pk)
            .expect("signed entry verifies under matching pubkey");
    }

    /// H-4 authenticity extends to empty-sentinel entries so a
    /// malicious actor cannot forge a fake "I had no WAL
    /// entries at block N" claim.
    #[test]
    fn snapshot_writer_with_signer_signs_empty_sentinel() {
        let (sk, pk) = new_secp256k1_keypair();
        let tmp = tempfile::tempdir().unwrap();
        let mut w = mk_writer(tmp.path().to_path_buf(), 1, 10);
        w.signer_sk = Some(sk);
        w.maybe_write(1, &[]).unwrap();
        let manifest = read_manifest(tmp.path()).unwrap();
        assert_eq!(manifest.len(), 1);
        assert_eq!(manifest[0].root, None, "sentinel");
        manifest[0]
            .verify_with_pubkey(&pk)
            .expect("signed sentinel verifies");
    }

    /// Prune integration: with `retain = 2` and 4 cadence hits
    /// at distinct block heights (each producing a distinct
    /// `.wal` via distinct content), only the 2 newest `.wal`
    /// files survive after the fourth write.
    ///
    /// Note on determinism: `prune_snapshot_dir` orders by mtime
    /// (then filename as tiebreaker, per PR #511).  Back-to-back
    /// `maybe_write` calls within the same mtime tick fall back
    /// to the filename tiebreaker.  To keep this test
    /// deterministic across clocks we only assert the SURVIVING
    /// COUNT, not which specific `.wal` survives.
    #[test]
    fn snapshot_writer_maybe_write_prunes_according_to_retain() {
        let tmp = tempfile::tempdir().unwrap();
        let w = mk_writer(tmp.path().to_path_buf(), 1, 2);
        for i in 1..=4 {
            // Each write uses a distinct payload so content
            // hashes (and thus `.wal` filenames) differ.
            let entries = vec![WalEntry {
                op: WalOp::Write,
                path: PathBuf::from("/@bundle/x"),
                extra_path: None,
                offset: None,
                length: Some(1),
                payload_ref: Some(PayloadRef::hash(&[i as u8])),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            }];
            w.maybe_write(i, &entries).unwrap();
        }
        let wal_count = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("wal"))
            .count();
        assert_eq!(wal_count, 2, "retain = 2 leaves exactly 2 .wal files");
    }

    /// Load-bearing end-to-end: two cadence hits produce two
    /// manifest entries in append order.  Pins the two-phase
    /// commit (snapshot write + manifest append) across
    /// multiple blocks.
    #[test]
    fn snapshot_writer_two_cadence_hits_produce_two_ordered_manifest_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let w = mk_writer(tmp.path().to_path_buf(), 1, 10);
        let entries_a = diverse_entries();
        let mut entries_b = entries_a.clone();
        entries_b.push(WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/@bundle/second"),
            extra_path: None,
            offset: None,
            length: Some(4),
            payload_ref: Some(PayloadRef::hash(b"more")),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        });
        w.maybe_write(1, &entries_a).unwrap().expect("Some");
        w.maybe_write(2, &entries_b).unwrap().expect("Some");
        let manifest = read_manifest(tmp.path()).unwrap();
        assert_eq!(manifest.len(), 2);
        assert_eq!(manifest[0].block_number, 1);
        assert_eq!(manifest[1].block_number, 2);
        assert_ne!(
            manifest[0].root, manifest[1].root,
            "distinct content → distinct roots"
        );
    }

    /// Best-effort manifest-append posture: when the manifest
    /// append fails (we orchestrate this by pre-creating
    /// `manifest.jsonl` as a directory), `maybe_write` still
    /// returns Ok(Some(...)) because the snapshot `.wal` is
    /// already durable.  Pins the "snapshot durability is the
    /// authoritative act; manifest is best-effort" rationale
    /// from the docstring.
    #[test]
    fn snapshot_writer_manifest_append_failure_does_not_fail_snapshot_write() {
        let tmp = tempfile::tempdir().unwrap();
        // Preempt the manifest-append with a path that can't be
        // opened as a file.
        std::fs::create_dir(tmp.path().join(MANIFEST_FILENAME)).unwrap();

        let w = mk_writer(tmp.path().to_path_buf(), 1, 10);
        let entries = diverse_entries();
        let out = w.maybe_write(1, &entries).unwrap();
        assert!(
            out.is_some(),
            "snapshot write succeeded despite manifest failure"
        );

        let (root, _) = out.unwrap();
        assert!(
            snapshot_path(tmp.path(), &root).exists(),
            ".wal file durable despite manifest-append failure"
        );
        // Manifest path is still a directory (the open+append
        // failed silently per best-effort posture).
        assert!(
            tmp.path().join(MANIFEST_FILENAME).is_dir(),
            "manifest.jsonl is still a directory (append silently failed)"
        );
    }

    /// Same best-effort posture applies to the empty-sentinel
    /// path: a manifest-append failure must not propagate.
    #[test]
    fn snapshot_writer_empty_sentinel_manifest_failure_does_not_propagate() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(MANIFEST_FILENAME)).unwrap();

        let w = mk_writer(tmp.path().to_path_buf(), 1, 10);
        // Must not panic or propagate.
        assert_eq!(w.maybe_write(1, &[]).unwrap(), None);
    }

    /// `with_default_retain` applies the documented
    /// `max(2, cadence * 2)` heuristic — including the floor of
    /// 2 when `cadence * 2 < 2` (i.e., `cadence = 0` is rejected
    /// so the floor fires at `cadence = 1`: `max(2, 2) = 2`).
    #[test]
    fn snapshot_writer_with_default_retain_applies_heuristic() {
        let dir = PathBuf::from("/x");
        assert_eq!(
            SnapshotWriter::with_default_retain(dir.clone(), 1, None, None).retain,
            2,
            "cadence=1 → floor at 2"
        );
        assert_eq!(
            SnapshotWriter::with_default_retain(dir.clone(), 10, None, None).retain,
            20,
            "cadence=10 → 10 * 2"
        );
        assert_eq!(
            SnapshotWriter::with_default_retain(dir.clone(), 100, None, None).retain,
            200,
            "cadence=100 → 100 * 2"
        );
        // Also pin that the heuristic constructor's result works
        // as a valid writer (no debug_assert fires on retain=2 /
        // cadence=1).
        let tmp = tempfile::tempdir().unwrap();
        let w = SnapshotWriter::with_default_retain(tmp.path().to_path_buf(), 1, None, None);
        assert!(w.maybe_write(1, &diverse_entries()).unwrap().is_some());
    }

    /// `with_default_retain` panics on `cadence = 0` — the one
    /// documented panic surface, caught at construction time
    /// rather than deferred to first `maybe_write` call (which
    /// would divide-by-zero in `is_multiple_of`).
    #[test]
    #[should_panic(expected = "cadence MUST be >= 1")]
    fn snapshot_writer_with_default_retain_panics_on_cadence_zero() {
        let _ = SnapshotWriter::with_default_retain(PathBuf::from("/x"), 0, None, None);
    }

    /// Debug build fires the `cadence >= 1` invariant assertion
    /// when a caller constructs the struct directly with
    /// `cadence = 0` (bypassing `with_default_retain`'s panic)
    /// and then calls `maybe_write`.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "cadence MUST be >= 1")]
    fn snapshot_writer_maybe_write_debug_asserts_cadence_invariant() {
        let tmp = tempfile::tempdir().unwrap();
        // Direct construction with cadence=0 — bypasses the
        // constructor's panic.
        let w = SnapshotWriter {
            dir: tmp.path().to_path_buf(),
            cadence: 0,
            retain: 2,
            signer_sk: None,
            payload_dir: None,
        };
        let _ = w.maybe_write(1, &diverse_entries());
    }

    /// Debug build fires the `retain >= 1` invariant assertion
    /// when a caller constructs the struct directly with
    /// `retain = 0`.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "retain MUST be >= 1")]
    fn snapshot_writer_maybe_write_debug_asserts_retain_invariant() {
        let tmp = tempfile::tempdir().unwrap();
        let w = SnapshotWriter {
            dir: tmp.path().to_path_buf(),
            cadence: 1,
            retain: 0,
            signer_sk: None,
            payload_dir: None,
        };
        let _ = w.maybe_write(1, &diverse_entries());
    }
}
