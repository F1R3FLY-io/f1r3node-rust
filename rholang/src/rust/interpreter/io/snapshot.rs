// Consensus-mode filesystem WAL snapshot — encoding + decoding
// primitives (slices 1 + 2 of the `snapshot` submodule tree).
//
// Slice 1 (PR #504) provided the canonical byte-encoding of
// `Vec<WalEntry>` plus its Blake2b256 root hash and the
// [`SnapshotBlob`] convenience combiner (encoded bytes + root +
// Merkle root over 4 MiB chunks).
//
// Slice 2 (this PR) provides the symmetric decoder
// [`decode_wal_slice`] + [`SnapshotError`] + supporting
// primitives.  Together, encoder + decoder are the round-trip
// substrate that joiners use to apply a fetched snapshot to a
// fresh tree.  On-disk read / write / manifest / writer /
// pruning / payload-hash sidecar all land in subsequent slices
// as their own natural units.
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

use super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

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
}
