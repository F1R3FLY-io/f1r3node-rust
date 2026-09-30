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
// Slice 4 (this PR) adds the payload-hash sidecar:
// [`hashes_sidecar_path`] (colocated `.hashes` path derivation),
// [`read_hashes_sidecar`] (best-effort read that returns an
// empty set on corruption rather than propagating), and
// [`scan_retained_payload_hashes`] (union across a snapshot
// directory).  The sidecar write is threaded into
// [`write_snapshot`] as a best-effort tail — sidecar write
// failures log at warn but do not fail the snapshot write, since
// the snapshot bytes are already durable and a missing sidecar
// only means the corresponding payload hashes go un-counted on
// the next retention pass (over-eager delete, safe).
//
// Manifest / writer / pruning all land in subsequent slices as
// their own natural units.
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
pub const SNAPSHOT_FORMAT_VERSION: u8 = 6;

crate::register_consensus_constant!(order = 15, name = SNAPSHOT_FORMAT_VERSION, u8_raw);

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
/// applying a snapshot to a fresh tree (yet-to-land wal_applier).
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Hard-fork surface pin.  A change here rolls every WAL root
    /// on the network and requires a coordinated fleet upgrade.
    /// The fingerprint fold catches drift at peering; this test
    /// catches it before the binary even ships.
    #[test]
    fn snapshot_format_version_pinned_at_6() {
        assert_eq!(SNAPSHOT_FORMAT_VERSION, 6);
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
        const EXPECTED: &str = "311f801a5d9cee1bc306cdac23a240fb8fcd1f797ed6d1b8c8bbfbc71c71553a";
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

    /// Wire tag 17 (past the current tail of 16) is also invalid.
    #[test]
    fn decode_op_tag_past_tail_returns_malformed_blob() {
        let bytes = vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 1, 17];
        match decode_wal_slice(&bytes) {
            Err(SnapshotError::MalformedBlob { message, .. }) => {
                assert!(message.contains("unknown op tag 17"), "got {message:?}");
            }
            other => panic!("expected MalformedBlob for tag 17, got {other:?}"),
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
}
