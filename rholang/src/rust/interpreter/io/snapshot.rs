// Consensus-mode filesystem WAL snapshot (slice 30, PB-M-15 — MVP).
//
// # Scope
//
// Slice 30 MVP delivers the *content-addressed serialization + Merkle
// root* substrate that follow-up slices build on to:
//
//   - Persist per-block WAL slices to disk at operator-configured
//     cadence (`storage.consensus-fs-snapshot-cadence`).
//   - Serve the snapshot bytes to joining validators.
//   - Let a joining validator replay from the last snapshot forward.
//
// This module is deliberately narrow: canonical byte encoding of
// `Vec<WalEntry>`, a Blake2b256 root hash over that encoding, and
// filesystem read/write on a content-addressed path.  The cadence
// loop, the joining-node fetch protocol, and the on-chain commitment
// of the root (a hard-fork proto change) are deferred to slice 30c.
//
// # Snapshot semantics — log-structured (F-30-2 review decision)
//
// A snapshot in this module IS a canonical byte-encoding of
// `Vec<WalEntry>` — that is, a WAL slice or checkpoint, not a
// materialized filesystem-state image.  Joining validators replay
// the concatenation of all snapshots from genesis forward against
// an empty base image.  This commits the system to
// **log-structured checkpointing**: prevention of unbounded WAL
// replay for a late joiner depends on the operator's cadence being
// short enough that the cumulative WAL slice size stays under the
// target replay budget.
//
// The alternative — materialized fs state snapshots — was
// considered and deferred; the current design is chosen because it:
//   * keeps a single content-addressed encoding for both per-block
//     WAL entries (already used for on-chain root commitment) and
//     for on-disk snapshots — no separate serialization schema;
//   * requires no fs-state serializer (which for consensus-static
//     buckets would need to snapshot every file's contents, sizes
//     comparable to the total on-disk footprint);
//   * lets the joining protocol be a plain byte-stream fetch.
//
// A future slice may add a materialized-state snapshot alongside
// the log-structured one for operators who want a faster join
// path; the on-disk format is versioned via the WAL root prefix so
// both can coexist.
//
// # Canonical encoding
//
// The encoding is prefix-length + big-endian; no protobuf, no serde.
// This keeps the byte layout independent of any code-generator's
// choices and makes the resulting root hash a stable consensus
// commitment.  A hard-fork of the encoding format is a hard-fork of
// the WAL root hash.
//
// The WAL slice header is:
//   version:  u8 (= SNAPSHOT_FORMAT_VERSION)
//   entries:  u32-be count + [entry bytes] × count
//
// The leading version byte (added in slice 34, MED-1 FIPS fix) makes
// reads self-describing: `read_snapshot_bytes` rejects unknown
// versions with `SnapshotError::UnsupportedVersion` at parse time
// rather than mis-decoding.  A future format change bumps the
// version, so a hybrid rollout can accept both v1 and vN blobs
// during transition.
//
// The exact per-entry schema (see `encode_entry`):
//   op:           u8
//   path:         u32-be length prefix + UTF-8 bytes
//   extra_path:   u8 flag (0 = None, 1 = Some) + optional u32+bytes
//   offset:       u8 flag + optional u64-be
//   length:       u8 flag + optional u64-be
//   payload_ref:  u8 tag (0 = None, 1 = Hash, 2 = DeployRef) + variant bytes
//   mode_bits:    u8 flag + optional u32-be
//   owner:        u8 flag + optional u32+UTF-8
//   group:        u8 flag + optional u32+UTF-8
//
// Order matters: `Vec<WalEntry>` is encoded in its insertion order.
// The caller (per-block accumulator in casper) is responsible for
// producing a deterministic ordering — slice 29 round-2 finding H-R3
// (Par-parallel WAL ordering) is orthogonal to this encoding.
//
// # Hard-fork surface catalog (MED-1, slice 34)
//
// EVERY item below is consensus-observable.  Changing any one of
// them changes the WAL root computed over the same logical inputs,
// which is a hard fork of the network.  Bump
// `SNAPSHOT_FORMAT_VERSION` and update the golden-hex test IF AND
// ONLY IF you intend a hard fork and have coordinated it.  If
// you're adding a new item to this surface (a new field, a new
// op variant, a new PayloadRef variant), append it to this list
// AND to `hard_fork_surface_catalog_is_pinned` in the test module.
//
// 1. **Format version byte** — `SNAPSHOT_FORMAT_VERSION` at
//    the front of `encode_wal_slice`.  Bump on any format change.
// 2. **Op tag bytes** — `op_tag(WalOp)` maps enum variants to
//    stable u8 values 1..=15 (Write, WriteAt, Truncate, Chmod,
//    Chown, RemoveFile, RemoveDir, Rename, CopyFile, Read, ReadAt,
//    Stat, Entries, Size, EntriesStreamNext).
//    Pinned by `op_tags_are_stable` + `encode_entry_uses_op_tag_values`.
// 3. **Hash function = Blake2b256** — `hash_of`,
//    `PayloadRef::hash`, `compute_wal_root`.
// 4. **Length-prefix widths**:
//      - u32-be for entry-count and every string-byte-count;
//      - u8 flags for Option<...> present/absent tags.
// 5. **Field widths / endianness**:
//      - u64-be for `offset` and `length`;
//      - u32-be for `mode_bits`, DeployRef's `deploy_index` and
//        `arg_index`, and every string-byte-count prefix.
// 6. **`PayloadRef` variant tags** — `Hash = 1`, `DeployRef = 2`,
//    `None = 0`.  Pinned by
//    `deploy_ref_encoding_is_big_endian_and_field_order_is_stable`.
// 7. **Field order inside `encode_entry`** — op, path, extra_path,
//    offset, length, payload_ref, mode_bits, owner, group, outcome.
//    A reorder that keeps all field-encoders unchanged still forks
//    the network because the concatenation order differs.
// 8. **Path encoding** — `PathBuf::as_os_str().as_encoded_bytes()`.
//    Unix-only; see `# Platform scope` above.  A Windows port MUST
//    bump the version and switch to logical bucket keys.
// 9. **Outcome encoding** (H-6, version 2) — `WalOutcome` at the
//    tail of `encode_entry`.  `Success = 0` (single byte tag);
//    `Failure = 1` followed by u32-be `code` (from `fserr_to_code`,
//    itself a stable enumeration in `errors.rs`).  Reordering
//    outcome tags, adding a new variant without extending
//    `fserr_to_code`, or changing the u32-be layout of `code` is
//    a hard fork of the WAL root.  Pinned by
//    `encode_entry_outcome_layout_is_stable`.
// 11. **WAL entry cap** (M-8, 2026-08-06) — `MAX_WAL_ENTRIES`
//    in `wal.rs`.  Consensus-observable because callers see
//    `FSERR_QUOTA_EXCEEDED` on the overflow write; a divergent
//    cap would produce different reply distributions on
//    identical inputs.  Pinned by
//    `max_wal_entries_pinned_at_65536`.  A change here is a
//    hard fork of the tuplespace-level reply behavior.
// 10. **Manifest wire format** (M-1, 2026-08-06) —
//    `MANIFEST_FORMAT_VERSION` at `v` in every JSON line;
//    field order `v, block_number, root, entries, ts_ms, [sig]`;
//    `root` as 64-char lowercase hex or `null`; `sig` as
//    variable-length lowercase hex; `sign_bytes` layout
//    `MANIFEST_FORMAT_VERSION | block_number | root_present | root |
//    entries | ts_ms` (all big-endian).  This is INDEPENDENT of
//    catalog items 1-9 (which cover the .wal snapshot bytes);
//    manifest schema evolves on its own version cadence.  Pinned
//    by `manifest_line_format_is_pinned_at_v1` +
//    `manifest_sign_bytes_layout_is_pinned` +
//    `from_line_rejects_unknown_manifest_version`.
// 12. **Range-lock caps** (Phase 8 slice 8a, 2026-08-12) —
//    `MAX_RANGES_PER_FILE` and `LOCK_ID_CEILING` in `lock.rs`.
//    Both govern when `fs_lock_range` returns `FSERR_QUOTA_EXCEEDED`,
//    a consensus-observable reply.  A validator with a different
//    cap would produce a divergent reply on the same call
//    sequence.  Pinned by `max_ranges_per_file_pinned_at_1024`
//    and `lock_id_ceiling_pinned` in `lock.rs`'s test module.
//    A change to either is a hard fork of the fs_lock_range
//    reply distribution.  Note: `LockId` VALUES themselves are
//    per-runtime and NOT consensus-observable; only the
//    QuotaExceeded THRESHOLD is.
//
// # Platform scope (H-30-3 review note)
//
// This module uses `PathBuf::as_os_str().as_encoded_bytes()` to
// serialize paths.  On Unix that returns the raw byte sequence; on
// Windows it returns WTF-8 (an encoding of the internal UTF-16).
// Consequently a Unix validator and a Windows validator processing
// the same block would produce **different WAL roots for the same
// logical path**.
//
// The File I/O FIP scopes the node to Unix (see slice 25 provisioning
// docs and slice 27/28 review notes).  Any future Windows port MUST
// resolve this encoding divergence — the standard fix is to encode
// the *logical bucket key* (`"app/data.bin"` from the provisioning
// map) rather than the host canonical path.  That fix is deferred to
// the Windows-port slice; today the assumption is Unix-only and the
// canonical `PathBuf` bytes serve as the consensus commitment.

use std::path::{Path, PathBuf};

use crypto::rust::hash::blake2b256::Blake2b256;

use super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

/// Slice 34 (MED-1): version byte at the front of every encoded
/// WAL slice.  Bumping this is a hard fork of the WAL root; see
/// `# Hard-fork surface catalog` in the module docstring.
///
/// # Version history
/// - `1`: initial slice-34 layout — op, path, extra_path,
///   offset, length, payload_ref, mode_bits, owner, group.
/// - `2`: H-6 fix (2026-08-06) — appended `outcome`
///   (`WalOutcome::Success` = tag 0, `WalOutcome::Failure` =
///   tag 1 + u32-be code) at the tail of every entry so
///   followers can distinguish a leader's successful syscall
///   from a syscall that returned an error (EIO/ENOSPC/EROFS).
/// - `3`: M-5 fix (2026-08-06) — added `Stat` (op tag 12),
///   `Entries` (13), `Size` (14) WalOp variants.  State-read
///   handlers on Consensus caps journal into the WAL so
///   tuplespace divergence traceable to filesystem drift
///   surfaces as "stat reply differed at path X" rather than
///   as an opaque `check_replay_data` mismatch downstream.
/// - `4`: Streaming-backing slice Step 3 (2026-08-25) — added
///   `EntriesStreamNext` (op tag 15).  Each `entriesStreamNext`
///   call on a Consensus-cap dir stream journals its reply
///   (entryRecord or EOS marker) with `payload_ref =
///   Hash(reply_par)`, symmetric on leader + follower.  Length
///   field encodes `1` for yielded entries and `0` for
///   EOS/error terminators so a replay-side auditor can count
///   the stream without re-parsing.
/// - `5`: Consensus-fs Shape A / Task 0.4 (2026-08-31) — the
///   encoding itself is unchanged, but the semantics of every
///   Consensus-cap entry's `path` field flip from an absolute
///   on-disk canon_path to a **bundle-relative** logical form
///   (typically `/@bundle/<logical_name>` — see
///   `BUNDLE_ROOT_PREFIX` in `casper/src/rust/genesis/contracts/
///   fs_genesis.rs`).  A pre-Shape-A joiner reading a Shape A
///   snapshot would fall through the applier's registry lookup
///   and syscall against `/@bundle/...` on its local disk (ENOENT
///   at best; worse if a shell alias resolves `@` somewhere real).
///   The version bump makes the mis-decode surface as
///   `SnapshotError::UnsupportedVersion` before any syscall.
///   Non-Consensus (Oracular) entries continue to carry absolute
///   on-disk paths and are handled by the resolver's identity
///   fall-through unchanged.  No running network per auto-memory
///   `f1r3node_no_running_network.md` — the bump is a normal edit.
pub const SNAPSHOT_FORMAT_VERSION: u8 = 5;

/// M-1 fix (2026-08-06): manifest.jsonl line-format version.
/// Distinct from `SNAPSHOT_FORMAT_VERSION` because the two are
/// independent wire surfaces: the .wal file bytes and the
/// manifest text lines evolve on separate cadences.  A WAL
/// encoding change (added field, new op tag) does NOT
/// invalidate existing manifest signatures; a manifest schema
/// change (added key, renamed field) does NOT invalidate
/// existing .wal snapshots.
///
/// # Wire-format contract (see spec §Join-protocol manifest)
///
/// - The version is embedded as `"v":<n>` in the JSON line so
///   readers can multi-decode across versions cleanly.
/// - Bumping this value is a hard fork of the manifest protocol.
///   All producers and consumers on the network MUST upgrade
///   before the change activates.
/// - Item #10 of the hard-fork surface catalog.  Golden-hex pin
///   in `manifest_line_format_is_pinned_at_v1`.
///
/// # Version history
///
/// - `1`: initial slice-30c Phase C layout + H-4 sig field +
///   M-1 `"v"` version tag.  Fields: `v`, `block_number`,
///   `root` (hex string or null), `entries`, `ts_ms`, `sig`
///   (optional hex string).
pub const MANIFEST_FORMAT_VERSION: u8 = 1;

/// Result of encoding + hashing a WAL slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotBlob {
    /// The full canonical byte encoding.
    pub bytes: Vec<u8>,
    /// Blake2b256 of `bytes`.  The content address.
    pub root: [u8; 32],
    /// Phase 7b-1 Merkle root over per-chunk hashes.  Derived from
    /// `bytes` via `snapshot_chunk::chunk_snapshot` +
    /// `snapshot_merkle_root`.  Empty snapshot → `EMPTY_SNAPSHOT_ROOT`
    /// = `[0u8; 32]`.  This is the anchor a joining validator uses
    /// to verify individual chunks fetched via the `get_snapshot_chunk`
    /// wire opcode (follow-up slice).  See
    /// `rholang/src/rust/interpreter/io/snapshot_chunk.rs` for the
    /// chunker primitives.
    pub merkle_root: [u8; 32],
}

/// Encode a WAL slice to canonical bytes.  Deterministic across
/// validators for identical `entries` input.
///
/// Layout: `[SNAPSHOT_FORMAT_VERSION: u8][count: u32-be][entry × count]`.
/// The leading version byte lets `read_snapshot_bytes` reject
/// unknown-version blobs cleanly instead of mis-decoding.
pub fn encode_wal_slice(entries: &[WalEntry]) -> Vec<u8> {
    // Cap at u32::MAX entries.  In practice `MAX_WAL_ENTRIES` = 65_536
    // per runtime keeps this well below the cap.
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
pub fn compute_wal_root(entries: &[WalEntry]) -> [u8; 32] {
    let bytes = encode_wal_slice(entries);
    hash_of(&bytes)
}

/// Encode + hash together.  Post-2026-08-27 also computes the
/// Phase 7b-1 Merkle root over per-chunk hashes so joining
/// validators can verify individual 4 MiB chunks against a
/// canonical anchor.  See `SnapshotBlob::merkle_root`.
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
    assert_eq!(
        h.len(),
        32,
        "Blake2b256 must produce 32-byte digest; got {}",
        h.len()
    );
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

fn encode_entry(e: &WalEntry, buf: &mut Vec<u8>) {
    buf.push(op_tag(e.op));
    encode_str_bytes(e.path.as_os_str().as_encoded_bytes(), buf);
    match &e.extra_path {
        None => buf.push(0),
        Some(p) => {
            buf.push(1);
            encode_str_bytes(p.as_os_str().as_encoded_bytes(), buf);
        }
    }
    encode_opt_u64(e.offset, buf);
    encode_opt_u64(e.length, buf);
    encode_payload_ref(&e.payload_ref, buf);
    encode_opt_u32(e.mode_bits, buf);
    encode_opt_str(e.owner.as_deref(), buf);
    encode_opt_str(e.group.as_deref(), buf);
    // H-6 fix (2026-08-06): outcome tail — version 2.
    encode_outcome(e.outcome, buf);
}

fn encode_outcome(o: WalOutcome, buf: &mut Vec<u8>) {
    // Tag 0 = Success (no payload); tag 1 = Failure + u32-be code.
    // Renumbering here is a hard fork (catalog item #9).
    match o {
        WalOutcome::Success => buf.push(0),
        WalOutcome::Failure { code } => {
            buf.push(1);
            buf.extend_from_slice(&code.to_be_bytes());
        }
    }
}

fn op_tag(op: WalOp) -> u8 {
    // Explicit numeric tags — DO NOT reorder or renumber.  Changing
    // these is a hard-fork of the WAL root.  Slice 32 (PB-M-14 read-
    // hash) adds Read/ReadAt at the end of the reserved-tag range.
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
        // M-5 fix (2026-08-06): state-read journaling on
        // Consensus caps.  Tags 12-14 appended at the tail of
        // the reserved range.  Version bumped 2 → 3.
        WalOp::Stat => 12,
        WalOp::Entries => 13,
        WalOp::Size => 14,
        // Streaming-backing slice Step 3 (2026-08-25): per-call
        // journal on `entriesStreamNext` for Consensus-cap dir
        // streams.  Tag 15 appended at the tail; version bumped
        // 3 → 4.
        WalOp::EntriesStreamNext => 15,
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

/// Decode canonical WAL bytes back to a `Vec<WalEntry>`.  Symmetric
/// inverse of `encode_wal_slice`; used by joiners applying a
/// snapshot to a fresh tree via the Phase 7b-2 payload-fetch flow.
///
/// # Version handling
///
/// Only accepts `SNAPSHOT_FORMAT_VERSION`.  A newer version byte
/// returns `SnapshotError::UnsupportedVersion` (same shape as
/// `read_snapshot_bytes`) so a lagging validator refuses to
/// mis-decode a hard-forked network's snapshots.
///
/// # Errors
///
/// - `Truncated` — insufficient bytes to satisfy the declared
///   entry count / a field's declared length.
/// - `UnsupportedVersion` — the leading version byte doesn't match
///   this validator's `SNAPSHOT_FORMAT_VERSION`.
/// - `MalformedBlob` — an op tag, PayloadRef variant tag, or
///   outcome tag outside the allowed set, or a `RemoveDir` /
///   `Rename` / `CopyFile` etc. missing its documented field.
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
    let path = os_str_bytes_to_pathbuf(path_bytes);
    let extra_path = match decode_u8(bytes, cursor)? {
        0 => None,
        1 => Some(os_str_bytes_to_pathbuf(decode_str_bytes(bytes, cursor)?)),
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

#[cfg(unix)]
fn os_str_bytes_to_pathbuf(bytes: &[u8]) -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(OsStr::from_bytes(bytes))
}

/// I/O errors from snapshot read/write.
#[derive(Debug)]
pub enum SnapshotError {
    Io(std::io::Error),
    RootMismatch {
        expected: [u8; 32],
        got: [u8; 32],
    },
    /// Slice 34 (MED-1): the on-disk blob starts with a version byte
    /// this validator does not recognize.  A joining validator
    /// running an older binary can see a snapshot produced by a
    /// newer, hard-forked network — surface a clean error rather
    /// than mis-decoding.
    UnsupportedVersion {
        got: u8,
        supported: u8,
    },
    /// Blob is too short to contain even the version byte.
    Truncated {
        got: usize,
        need: usize,
    },
    /// Slice 34+ (Phase 7b-2 item (c), 2026-08-28):
    /// `decode_wal_slice` encountered a byte pattern that doesn't
    /// match any valid encoding.  Includes an unknown op tag, an
    /// unknown PayloadRef variant tag, an unknown outcome tag, or
    /// truncated data mid-entry.  A joiner surfacing this error
    /// should treat the assembled snapshot as byzantine and
    /// re-fetch from a different peer set.
    MalformedBlob {
        offset: usize,
        message: String,
    },
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
            SnapshotError::MalformedBlob { offset, message } => write!(
                f,
                "snapshot blob malformed at offset {offset}: {message}"
            ),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<std::io::Error> for SnapshotError {
    fn from(e: std::io::Error) -> Self { SnapshotError::Io(e) }
}

fn hex_short(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(16);
    for b in &bytes[..8] {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Content-addressed on-disk snapshot path.
///
/// Layout: `{snapshot_dir}/{root_hex}.wal`.  The filename IS the
/// content hash, so a joining validator can request a snapshot by
/// root and verify byte-for-byte after fetch.
pub fn snapshot_path(snapshot_dir: &Path, root: &[u8; 32]) -> PathBuf {
    let mut hex = String::with_capacity(64);
    for b in root {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    snapshot_dir.join(format!("{hex}.wal"))
}

/// Write a snapshot to `snapshot_dir` under its content-addressed
/// filename.  Idempotent: writing the same content twice produces the
/// same file (overwrites atomically via a tmp+rename dance).  Returns
/// `(path, root, merkle_root)` where `root` is the atomic Blake2b256
/// of the whole blob (content-addressed filename) and `merkle_root`
/// is the Phase 7b-1 Merkle root over 4 MiB chunk hashes (used by
/// joiners to verify chunks fetched via `get_snapshot_chunk`).
pub fn write_snapshot(
    snapshot_dir: &Path,
    entries: &[WalEntry],
) -> Result<(PathBuf, [u8; 32], [u8; 32]), SnapshotError> {
    let blob = snapshot_blob(entries);
    let final_path = snapshot_path(snapshot_dir, &blob.root);
    // Phase 7b-2 retention sidecar (DD-7b-1 (y), 2026-08-27):
    // extract the unique payload hashes referenced by this
    // snapshot's WAL entries and stash them next to the snapshot
    // file so payload-store retention can union across all
    // retained snapshots without decoding the full WAL bytes.
    // Sidecar failures are best-effort: a missing sidecar just
    // means the corresponding payload hashes are not counted in
    // the retained set (they will be over-eagerly deleted from the
    // payload store on the next retention pass).  Deferred to the
    // very end of `write_snapshot` so an error here does not
    // shadow the more important snapshot-write result.
    let referenced_hashes = referenced_payload_hashes(entries);
    // Ensure directory exists.  Callers should have validated this at
    // boot; a race that removed it mid-flight surfaces here as ENOENT.
    if let Some(parent) = final_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // M-30-1 review fix (round 2): the tmp filename includes both the
    // process ID and a nanosecond timestamp so two concurrent writers
    // in the SAME PROCESS writing the SAME content do not race on the
    // same tmp path.  Pre-fix used `final_path.with_extension("wal.tmp")`
    // which is deterministic per-root, so two threads writing the same
    // content had `std::fs::write` calls stomping the same tmp file
    // between each other's rename.  Even for content-addressed writes
    // (where either writer produces identical bytes), a mid-write
    // observer could see truncated bytes.
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_name = format!(
        "{}.{}-{}.wal.tmp",
        final_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("snapshot"),
        std::process::id(),
        now_nanos
    );
    let tmp_path = final_path.with_file_name(tmp_name);
    // F-30-12 review fix (slice 30b): fsync the tmp file before
    // rename so a crash between write and rename leaves either
    // no snapshot at all (recovery is safe — the WAL cadence loop
    // re-emits on next block) OR a fully durable snapshot at the
    // final path.  Without fsync, `write` + `rename` can leave a
    // renamed file whose contents are still in the page cache;
    // post-crash the operator's snapshot file exists but reads as
    // truncated or garbled — the read-time root check catches this
    // as `RootMismatch`, but preventing it upstream avoids the
    // false-alarm.
    {
        use std::io::Write as _;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        // M-30b-4 review fix (round 2): explicit 0o644 mode on Unix
        // so the snapshot's uid/gid/mode do NOT leak the leader's
        // umask to any joiner reading over shared storage.  Content
        // is deterministic across validators (canonical WAL bytes),
        // but the file metadata was previously umask-dependent.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o644);
        }
        let mut file = opts.open(&tmp_path)?;
        file.write_all(&blob.bytes)?;
        file.sync_all()?;
    }
    // POSIX rename is atomic on the same filesystem; two concurrent
    // writes with distinct tmp files still race on the final rename,
    // but since the content is byte-identical (content-addressed) the
    // outcome is one of two identical files — observationally
    // indistinguishable.
    std::fs::rename(&tmp_path, &final_path)?;
    // Also fsync the directory so the rename entry itself is durable
    // (POSIX: rename's atomicity does not imply metadata durability).
    // M-30b-5 review fix: log fsync failures instead of swallowing.
    // Some filesystems (tmpfs, some network fs) reject dir fsync
    // with a legitimate errno — that's fine.  But an ext4/xfs EIO
    // (real hardware fault) should surface to operators.
    if let Some(parent) = final_path.parent() {
        match std::fs::File::open(parent) {
            Ok(dir_file) => {
                if let Err(e) = dir_file.sync_all() {
                    tracing::debug!(
                        target: "f1r3fly.fs_wal.snapshot",
                        parent = %parent.display(),
                        error = %e,
                        "dir fsync after rename failed (fs may not support dir fsync)"
                    );
                }
            }
            Err(e) => {
                tracing::debug!(
                    target: "f1r3fly.fs_wal.snapshot",
                    parent = %parent.display(),
                    error = %e,
                    "opening parent dir for fsync failed"
                );
            }
        }
    }
    // Sidecar write — best-effort.  Failures logged; don't abort
    // the snapshot write (the snapshot file itself is already
    // durable at this point).
    let sidecar_path = hashes_sidecar_path(snapshot_dir, &blob.root);
    if let Err(e) = write_hashes_sidecar(&sidecar_path, &referenced_hashes) {
        tracing::warn!(
            target: "f1r3fly.fs_wal.payload_store",
            path = %sidecar_path.display(),
            error = %e,
            "Phase 7b-2 hashes sidecar write failed; payload retention will \
             miss this snapshot's referenced hashes on the next pass"
        );
    }
    Ok((final_path, blob.root, blob.merkle_root))
}

/// Phase 7b-2 (2026-08-27): extract the set of unique payload
/// hashes referenced by a WAL slice.  Skips entries whose
/// `payload_ref` is None or `DeployRef` (only `Hash` variant
/// references bytes that live in the payload store).  Deduplicates
/// automatically via the HashSet.
pub fn referenced_payload_hashes(
    entries: &[WalEntry],
) -> std::collections::HashSet<[u8; 32]> {
    let mut set = std::collections::HashSet::new();
    for e in entries {
        if let Some(PayloadRef::Hash(h)) = e.payload_ref {
            set.insert(h);
        }
    }
    set
}

/// Phase 7b-2 (2026-08-27): path to the hashes-sidecar file for
/// a snapshot with content-address `root`.  Colocated with the
/// snapshot itself so `prune_snapshot_dir` can pair them up.
pub fn hashes_sidecar_path(snapshot_dir: &Path, root: &[u8; 32]) -> PathBuf {
    snapshot_dir.join(format!("{}.hashes", hex::encode(root)))
}

/// Phase 7b-2 sidecar format: `[u32-be count][32-byte hash × count]`.
/// Writes atomically via tmp + rename so a mid-write crash leaves
/// either no sidecar or a fully-durable one (never partial).
fn write_hashes_sidecar(
    sidecar_path: &Path,
    hashes: &std::collections::HashSet<[u8; 32]>,
) -> std::io::Result<()> {
    use std::io::Write as _;
    let count: u32 = hashes.len().try_into().unwrap_or(u32::MAX);
    let mut buf = Vec::with_capacity(4 + hashes.len() * 32);
    buf.extend_from_slice(&count.to_be_bytes());
    // Sorted for deterministic on-disk byte layout across runs
    // that produce equivalent hash sets (aids diff-review and
    // makes the sidecar itself content-addressable if we ever
    // need it).
    let mut sorted: Vec<[u8; 32]> = hashes.iter().copied().collect();
    sorted.sort();
    for h in sorted {
        buf.extend_from_slice(&h);
    }
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_name = format!(
        "{}.{}-{}.hashes.tmp",
        sidecar_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("sidecar"),
        std::process::id(),
        now_nanos
    );
    let tmp_path = sidecar_path.with_file_name(tmp_name);
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o644);
        }
        let mut file = opts.open(&tmp_path)?;
        file.write_all(&buf)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp_path, sidecar_path)?;
    Ok(())
}

/// Phase 7b-2 (2026-08-27): read a hashes sidecar back into a
/// HashSet.  On format/version issues returns an empty set +
/// logs at debug — a corrupt sidecar just means the corresponding
/// snapshot's payloads won't be counted in retention (over-eager
/// prune on the next pass, which is safe if the operator hasn't
/// added any new joiners).
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

/// Phase 7b-2 (2026-08-27): union the payload hashes referenced
/// by every retained snapshot in `snapshot_dir` via the
/// `.hashes` sidecars.  Missing sidecars are skipped silently
/// (see `write_snapshot`'s best-effort sidecar write).
///
/// Returns the union set — callers pass it to
/// `wal_payload_server::prune_payload_store` to delete any
/// non-referenced entries from the on-disk payload store.
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
        // Skip symlinks — matches prune_snapshot_dir's posture.
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

/// M-30b-3 review fix (slice 30b round 2): sweep stale `.wal.tmp`
/// files from `snapshot_dir`.  On crash between `sync_all` and
/// `rename`, the tmp file lives forever; `prune_snapshot_dir` filters
/// on extension `wal` (not `tmp`), so it never GC's them.  Call this
/// on startup or periodically to bound tmp-file accumulation.
///
/// `older_than_secs` filters by mtime — only tmp files older than
/// this threshold are removed, so we don't race in-progress writes.
/// Returns number of files removed.  Individual failures are
/// logged, not propagated.
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
        // Match `*.wal.tmp` by suffix (not extension, which is only
        // the last component after the final dot).
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if !name.ends_with(".wal.tmp") {
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
                target: "f1r3fly.fs_wal.snapshot",
                path = %path.display(),
                error = %e,
                "sweep_stale_tmp_files: remove failed; continuing"
            ),
        }
    }
    Ok(removed)
}

/// Slice 30b cadence-writer configuration.  Attached to
/// `RhoRuntimeImpl::fs_snapshot_writer` at boot time when the
/// operator has provisioned `consensus-static-*` buckets AND set
/// `storage.consensus-fs-snapshot-cadence` +
/// `storage.consensus-fs-snapshot-dir`.  `maybe_write` is called
/// once per block from `play_deploys_for_state`; it decides whether
/// this block's height hits the cadence and, if so, persists the
/// snapshot bytes and prunes old snapshots.
#[derive(Debug, Clone)]
pub struct SnapshotWriter {
    /// Canonicalized directory to write snapshots into.  Guaranteed
    /// absolute + symlink-resolved by
    /// `snapshot_config::validate_snapshot_config`.
    pub dir: PathBuf,
    /// Block interval between snapshots.  Guaranteed `>= 1` by boot
    /// validation.
    pub cadence: u64,
    /// How many snapshots to retain on disk.  Older ones are pruned
    /// after each successful write.
    ///
    /// # Default heuristic and its ratification
    ///
    /// Defaults to `max(2, cadence * 2)` snapshots when the
    /// operator has not set `storage.consensus-fs-snapshot-retain`
    /// explicitly (via `snapshot_config::build_snapshot_writer`).
    /// The resulting on-disk history window is roughly
    /// `2 * cadence²` blocks, which scales quadratically in
    /// cadence — almost certainly overprovisioned for small
    /// cadences and underprovisioned for very large ones.
    ///
    /// Slice 30c F-30b-1 disposition: the `cadence * 2` formula
    /// is RATIFIED AS A PLACEHOLDER.  It ships as the default
    /// because (a) it has a defensible minimum-viable rationale
    /// ("keep at least 2 × current cadence's worth of history"),
    /// (b) the retention floor of 2 guarantees any joining
    /// validator can always fetch at least prior + current, and
    /// (c) it's operator-tunable per-node via
    /// `storage.consensus-fs-snapshot-retain` (slice 35).
    /// Operators with concrete joining-SLA targets should set
    /// this explicitly using the sizing formula in the HOCON
    /// docstring: `retain = ceil(N_blocks / cadence) + 1`.
    ///
    /// A future slice may replace the default with a principled
    /// formula (fixed N-blocks joining window regardless of
    /// cadence).  That change would flip existing operator
    /// defaults, so it wants a coordinated rollout with an
    /// explicit config-schema migration.  Until then, the
    /// heuristic ships with this ratification note.
    pub retain: usize,

    /// H-4 fix (2026-08-06): optional secp256k1 secret key bytes
    /// for signing manifest entries at write time.  `None` = no
    /// signing (produces the pre-H-4 unsigned wire format for
    /// backward compatibility, e.g., in tests that don't need
    /// authenticity).  Populated from
    /// `conf.casper.validator_private_key` at boot via
    /// `snapshot_config::build_snapshot_writer`; observer nodes
    /// without an identity key get `None` and log a warning.
    ///
    /// Pre-H-4 the manifest was written unsigned in the operator's
    /// snapshot dir (mode 0o644); any local attacker with write
    /// access could inject bogus `(block_number, root)` entries
    /// and joining validators would fetch and replay against
    /// attacker-chosen roots.  With this field set, every
    /// manifest line carries a signature the join protocol can
    /// verify against the writer's known pubkey before trusting
    /// `root`.
    pub signer_sk: Option<Vec<u8>>,

    /// Phase 7b-2 retention (DD-7b-1 (y), 2026-08-27): operator-
    /// configured on-disk payload store directory (typically
    /// `<data-dir>/wal_payload_store/`).  When Some, the LFB-
    /// triggered snapshot writer in the casper finalization runner
    /// prunes the payload store alongside `prune_snapshot_dir` —
    /// deleting any content-addressed payload file whose hash is
    /// NOT referenced by any currently-retained snapshot.
    ///
    /// `None` on nodes without the payload store wired (test
    /// harnesses, observer nodes with no on-disk cache).  When
    /// None, no retention runs; the store grows unbounded (this
    /// matches the pre-DD-7b-1-y posture).
    ///
    /// Populated at boot in setup.rs from
    /// `<data-dir>/wal_payload_store/`.  Reads through the
    /// finalization runner's `writer_opt.payload_dir.clone()`
    /// call — the field is intentionally NOT wrapped in
    /// `Arc<RwLock<>>` because the payload dir path is a
    /// per-node config value set once at boot and never mutated.
    pub payload_dir: Option<PathBuf>,
}

impl SnapshotWriter {
    /// Try to persist a snapshot for `block_number` given the block's
    /// consensus WAL contribution.  Returns Ok(None) on cadence miss
    /// (no snapshot written), Ok(Some((root, merkle_root))) on
    /// successful persist.  `root` is the atomic content-address
    /// (Blake2b256 of the whole blob) used as the on-disk filename;
    /// `merkle_root` is the Phase 7b-1 Merkle root over 4 MiB chunk
    /// hashes, used by joiners to verify individual chunks fetched
    /// via `get_snapshot_chunk`.
    ///
    /// Cadence math: writes on blocks where `block_number % cadence == 0`.
    /// Genesis (block_number = 0) writes a snapshot too — cheap and
    /// useful for joining validators as an early-warning content hash.
    pub fn maybe_write(
        &self,
        block_number: i64,
        entries: &[WalEntry],
    ) -> Result<Option<([u8; 32], [u8; 32])>, SnapshotError> {
        // Block number is i64 in the block-data ref; treat negative
        // as "no block yet" and skip.
        if block_number < 0 {
            return Ok(None);
        }
        let bn = block_number as u64;
        if !bn.is_multiple_of(self.cadence) {
            return Ok(None);
        }
        if entries.is_empty() {
            // Slice 30c Phase C (F-30b-4 fix): empty-slice cadence
            // hit.  Pre-30c this was a silent skip — indistinguishable
            // from a cadence miss to a joining validator.  Now we
            // append an "empty" sentinel to the manifest so joiners
            // can verify they have not missed a snapshot boundary.
            // No `.wal` file is written on disk (empty payloads all
            // hash to the same content-address; one file per empty
            // slice would waste disk without adding replay value).
            // H-4 fix: sign the sentinel entry if a signer key is
            // present so joining validators can verify authenticity.
            let sentinel = ManifestEntry::empty(block_number);
            let sentinel = match &self.signer_sk {
                Some(sk) => sentinel.signed(sk),
                None => sentinel,
            };
            let _ = append_manifest_entry(&self.dir, sentinel);
            return Ok(None);
        }
        let (_, root, merkle_root) = write_snapshot(&self.dir, entries)?;
        tracing::info!(
            target: "f1r3fly.fs_wal.snapshot",
            block_number,
            root = %{
                let mut s = String::with_capacity(16);
                for b in &root[..8] {
                    use std::fmt::Write;
                    let _ = write!(s, "{b:02x}");
                }
                s
            },
            merkle_root = %{
                let mut s = String::with_capacity(16);
                for b in &merkle_root[..8] {
                    use std::fmt::Write;
                    let _ = write!(s, "{b:02x}");
                }
                s
            },
            n_entries = entries.len(),
            "snapshot persisted"
        );
        // Slice 30c Phase C: append to the join-protocol manifest so
        // peers can enumerate this validator's available snapshots
        // by block_number without probing the directory.  Best-
        // effort: manifest append failures do not abort the write
        // path (the snapshot file is already durable; missing
        // manifest lines can be reconstructed by directory scan).
        // H-4 fix: sign the manifest entry with the writer's identity
        // key so joining validators can verify authenticity before
        // trusting `root`.
        let data_entry = ManifestEntry::data(block_number, root, entries.len());
        let data_entry = match &self.signer_sk {
            Some(sk) => data_entry.signed(sk),
            None => data_entry,
        };
        if let Err(e) = append_manifest_entry(&self.dir, data_entry) {
            tracing::warn!(
                target: "f1r3fly.fs_wal.snapshot.manifest",
                block_number,
                error = %e,
                "manifest append failed; snapshot persisted but join-protocol \
                 enumeration will need to fall back to directory scan"
            );
        }
        // Best-effort retention prune.  Failures logged, not
        // propagated — retention is bounded by future writes anyway.
        let _ = prune_snapshot_dir(&self.dir, self.retain);
        Ok(Some((root, merkle_root)))
    }
}

// ---------------------------------------------------------------
// Slice 30c Phase C: join-protocol manifest substrate.
//
// A joining validator catching up from genesis (or after a long
// downtime) needs to know WHICH snapshots each peer has on disk
// without listing directories or probing every possible content-
// hash.  The manifest is an append-only file at
// `<snapshot-dir>/manifest.jsonl` recording one line per
// cadence-hit block: block_number, snapshot root (or the "empty"
// sentinel for F-30b-4), entry count, timestamp.
//
// The line format is intentionally simple (JSON per line, no
// serde dependency) so a joining validator's client can parse it
// with a hand-rolled reader if it lives outside the Rust node
// (e.g., a browser-side lightweight explorer, an operator's
// diagnostic script).
//
// # What this slice delivers
//
// - Append side: `SnapshotWriter.maybe_write` emits a manifest
//   entry per cadence hit (both data and empty-sentinel).
// - Read side: `read_manifest(dir)` for peers to publish + for
//   catch-up clients to plan fetches.
// - Empty-slice sentinel: distinguishes "block had zero
//   mutations" from "cadence miss" (F-30b-4 resolution).
//
// # What this slice deliberately DOES NOT deliver
//
// - Network transport (`GET /snapshots/<root_hex>` or similar
//   peer-level RPC): the actual client/server for fetching
//   snapshot bytes across the network.  Requires comm-crate
//   integration, new protobuf messages, retry/peer-selection
//   logic, and a state machine for progress tracking.  A
//   follow-up slice (30c-4 or similar) wires this once the
//   catch-up state machine design is agreed.
// - Manifest signing / attestation.  Currently the manifest is
//   unsigned per-node reporting.  A joining validator asking
//   multiple peers can cross-check root hashes (content-
//   addressed, so mismatches surface as hash mismatches at
//   fetch time).  A signed-manifest protocol may lift this to
//   an attested claim about "which snapshots exist" for
//   audit; deferred pending threat-model review.
// - Automatic reconstruction from directory scan if the manifest
//   is absent.  For now, missing/corrupt manifest → peer serves
//   no discovery (their snapshots are still fetchable by hash
//   if the joiner learns the hash elsewhere).

/// One line in the manifest.  Serialized to a compact JSON
/// object with fixed field ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Block_number the snapshot corresponds to.  For LFB-cadence
    /// (slice 30c Phase B, deferred): the finalized-block height
    /// at which this snapshot was written.
    pub block_number: i64,
    /// `Some(root)` for a data snapshot; `None` for the empty-
    /// slice sentinel (F-30b-4).
    pub root: Option<[u8; 32]>,
    /// Number of `WalEntry` records the snapshot encodes.  Zero
    /// for the empty sentinel; strictly positive for data.
    pub entries: u64,
    /// Wall-clock write timestamp, ms since UNIX_EPOCH.  Best-
    /// effort — a validator whose clock is skewed still produces
    /// a valid manifest, but comparisons across peers are noisy.
    pub ts_ms: i64,
    /// H-4 fix (2026-08-06): optional secp256k1 signature over
    /// `sign_bytes()` (Blake2b256 of the canonical serialization
    /// of the other four fields).  Signed by the validator's
    /// identity key at write time via `signed(...)`; joining
    /// validators verify with `verify_with_pubkey(...)` before
    /// trusting `root`.  Pre-H-4 the manifest was written
    /// unsigned with mode 0o644 in the operator's snapshot dir;
    /// any local attacker with write access could inject bogus
    /// (block_number, root) entries and joining validators would
    /// fetch and replay against attacker-chosen roots.  Post-H-4
    /// a joining validator that trusts the writer's pubkey
    /// rejects lines with missing or invalid `sig`.
    ///
    /// `None` = unsigned line (parsed for backward compat with
    /// pre-H-4 manifests; join protocol MUST reject in production
    /// unless an explicit "trust local disk" override is set).
    /// `Some(sig)` = 64-byte secp256k1 sig; caller verifies with
    /// the writer's public key.
    pub sig: Option<Vec<u8>>,
}

impl ManifestEntry {
    pub fn data(block_number: i64, root: [u8; 32], entries: usize) -> Self {
        Self {
            block_number,
            root: Some(root),
            entries: entries as u64,
            ts_ms: now_ms(),
            sig: None,
        }
    }

    pub fn empty(block_number: i64) -> Self {
        Self {
            block_number,
            root: None,
            entries: 0,
            ts_ms: now_ms(),
            sig: None,
        }
    }

    /// H-4 fix (2026-08-06): canonical byte-encoding of the four
    /// non-`sig` fields, hashed via Blake2b256 to produce the
    /// message the signature covers.  Anything that changes the
    /// canonicalization is a hard-fork of the manifest format
    /// (bump `MANIFEST_FORMAT_VERSION`; see hard-fork catalog
    /// item #10).
    ///
    /// M-1 fix (2026-08-06): version byte is `MANIFEST_FORMAT_VERSION`
    /// (was `SNAPSHOT_FORMAT_VERSION` pre-fix — that conflated
    /// two distinct wire surfaces; bumping WAL encoding would
    /// have invalidated existing manifest sigs unnecessarily).
    ///
    /// Format: `MANIFEST_FORMAT_VERSION | block_number | root_present |
    /// root | entries | ts_ms` (all big-endian, root omitted when
    /// absent).  Deterministic across writer/verifier as long as
    /// they agree on this module's version byte.
    pub fn sign_bytes(&self) -> Vec<u8> {
        use crypto::rust::hash::blake2b256::Blake2b256;
        let mut buf = Vec::with_capacity(1 + 8 + 1 + 32 + 8 + 8);
        // Version byte tying the signature format to the manifest
        // wire format (independent from the .wal snapshot format).
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

    /// H-4 fix (2026-08-06): return a copy of `self` with `sig`
    /// populated by signing `sign_bytes()` with the provided
    /// secp256k1 secret key.  The caller is responsible for
    /// invoking this before writing to the manifest; the
    /// `append_manifest_entry_signed` helper wraps the two-step
    /// (sign + append).
    pub fn signed(mut self, sk_bytes: &[u8]) -> Self {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        let msg = self.sign_bytes();
        let sig = Secp256k1.sign(&msg, sk_bytes);
        self.sig = Some(sig);
        self
    }

    /// H-4 fix (2026-08-06): verify the entry's signature against
    /// a public key.  Returns Err on missing sig, wrong-length
    /// sig, or verification failure.  Joining-protocol layer
    /// MUST call this on every manifest line before treating
    /// `root` as authoritative.
    pub fn verify_with_pubkey(&self, pk_bytes: &[u8]) -> Result<(), SnapshotError> {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        let sig = self.sig.as_deref().ok_or_else(|| {
            SnapshotError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "manifest entry is unsigned",
            ))
        })?;
        let msg = self.sign_bytes();
        if !Secp256k1.verify(&msg, sig, pk_bytes) {
            return Err(SnapshotError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "manifest entry signature verification failed",
            )));
        }
        Ok(())
    }

    /// Serialize to a single JSON line (no trailing newline).
    /// Fixed field order + minimal whitespace so peers parsing
    /// with a hand-rolled reader don't have to canonicalize.
    ///
    /// H-4 fix (2026-08-06): if `sig` is `Some`, appends a
    /// trailing `,"sig":"<hex>"` field.  `None` omits the field
    /// entirely.
    ///
    /// M-1 fix (2026-08-06): `"v":<MANIFEST_FORMAT_VERSION>` is
    /// the first key of every emitted line.  Consumers rejecting
    /// an unknown `v` surface `SnapshotError::UnsupportedManifestVersion`
    /// cleanly rather than silently mis-decoding a future schema.
    /// Field order: `v`, `block_number`, `root`, `entries`,
    /// `ts_ms`, [`sig`].  Item #10 of the hard-fork surface catalog.
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
                MANIFEST_FORMAT_VERSION,
                self.block_number,
                root_field,
                self.entries,
                self.ts_ms,
            ),
        }
    }

    /// Parse a single manifest line.  Rejects any input that
    /// doesn't match the strict schema — a corrupted line
    /// surfaces here rather than mid-catchup.
    pub fn from_line(line: &str) -> Result<Self, String> {
        // Hand-rolled parser — deliberately not serde to keep
        // the line format independent of any Rust crate's
        // deserializer behavior and to make the wire format
        // reproducible in other languages.
        let trimmed = line.trim();
        let inner = trimmed
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .ok_or_else(|| format!("manifest line missing braces: {line:?}"))?;
        let mut version: Option<u8> = None;
        let mut block_number: Option<i64> = None;
        let mut root: Option<Option<[u8; 32]>> = None;
        let mut entries: Option<u64> = None;
        let mut ts_ms: Option<i64> = None;
        // H-4 fix: `sig` is optional in the wire format for backward
        // compat with pre-H-4 unsigned manifests.  Absence -> None;
        // presence -> parse hex to Vec<u8>.
        let mut sig: Option<Vec<u8>> = None;
        for part in split_top_level_commas(inner) {
            let (key, value) = part
                .split_once(':')
                .ok_or_else(|| format!("manifest kv missing `:` in {part:?}"))?;
            let key = key.trim().trim_matches('"');
            let value = value.trim();
            match key {
                // M-1 fix (2026-08-06): explicit version field.
                // Absence means pre-M-1 unversioned line; treat
                // as version 1 for backward compat but require a
                // present `v` to equal `MANIFEST_FORMAT_VERSION`
                // (unknown values → UnsupportedManifestVersion).
                "v" => {
                    let v: u8 = value.parse().map_err(|e| format!("v parse: {e}"))?;
                    if v != MANIFEST_FORMAT_VERSION {
                        return Err(format!(
                            "unsupported manifest version {v} (this validator understands \
                             version {MANIFEST_FORMAT_VERSION}); a coordinated upgrade may \
                             be needed"
                        ));
                    }
                    version = Some(v);
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
                    // bytes typical, up to ~72).  Accept any even-length hex
                    // that decodes cleanly.
                    if hex.len() % 2 != 0 {
                        return Err(format!("sig hex length must be even; got {}", hex.len()));
                    }
                    let mut bytes = Vec::with_capacity(hex.len() / 2);
                    for i in (0..hex.len()).step_by(2) {
                        let hi = u8::from_str_radix(&hex[i..i + 2], 16)
                            .map_err(|e| format!("sig hex byte {i}: {e}"))?;
                        bytes.push(hi);
                    }
                    sig = Some(bytes);
                }
                other => {
                    return Err(format!("unknown manifest key `{other}`"));
                }
            }
        }
        // M-1 fix (2026-08-06): `v` is mandatory.  A missing `v`
        // is either a pre-M-1 unversioned line or a corrupted
        // one; both are treated as untrusted and rejected so a
        // silent decode of "someone's future schema as v1" can
        // never happen.
        let _ = version.ok_or_else(|| {
            format!(
                "missing `v` field (mandatory since M-1 fix 2026-08-06); \
                 expected v = {MANIFEST_FORMAT_VERSION}"
            )
        })?;
        Ok(Self {
            block_number: block_number.ok_or("missing block_number")?,
            root: root.ok_or("missing root")?,
            entries: entries.ok_or("missing entries")?,
            ts_ms: ts_ms.ok_or("missing ts_ms")?,
            sig,
        })
    }
}

/// Manifest filename inside the snapshot directory.
pub const MANIFEST_FILENAME: &str = "manifest.jsonl";

/// Append a manifest entry to `<snapshot_dir>/manifest.jsonl`.
/// Creates the file with `0o644` on first append.  Uses
/// `O_APPEND` semantics — multiple concurrent processes writing
/// to the same manifest see line-atomic appends (POSIX
/// guarantee for writes ≤ PIPE_BUF; a manifest line is under
/// that limit).
pub fn append_manifest_entry(
    snapshot_dir: &Path,
    entry: ManifestEntry,
) -> Result<(), SnapshotError> {
    use std::io::Write;
    let path = snapshot_dir.join(MANIFEST_FILENAME);
    let mut file = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o644)
                .open(&path)?
        }
        #[cfg(not(unix))]
        {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?
        }
    };
    let mut line = entry.to_line();
    line.push('\n');
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// Read every manifest entry from `<snapshot_dir>/manifest.jsonl`.
/// Returns entries in file order (append order = block-number
/// order under a single writer; under concurrent-writer append
/// races two entries may interleave at line boundaries, still
/// well-formed).  A corrupt line halts parsing at that line
/// and surfaces the error — join clients treat "we have manifest
/// entries up to line K" as a valid partial view.
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
        let entry = ManifestEntry::from_line(line).map_err(|e| {
            SnapshotError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("manifest line {}: {}", i + 1, e),
            ))
        })?;
        out.push(entry);
    }
    Ok(out)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn hex_decode_32(hex: &str) -> Result<[u8; 32], String> {
    if hex.len() != 64 {
        return Err(format!("expected 64 hex chars; got {}", hex.len()));
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("hex byte {i}: {e}"))?;
        out[i] = b;
    }
    Ok(out)
}

/// Split a JSON-object body on top-level commas (no depth
/// tracking beyond top level — manifest entries are flat, so a
/// naive splitter is correct).  Handles quoted strings so a
/// comma inside a string literal (there shouldn't be one in
/// well-formed entries, but defense) doesn't split.
fn split_top_level_commas(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_string = false;
    let mut escape = false;
    for c in inner.chars() {
        if escape {
            buf.push(c);
            escape = false;
            continue;
        }
        match c {
            '\\' if in_string => {
                buf.push(c);
                escape = true;
            }
            '"' => {
                in_string = !in_string;
                buf.push(c);
            }
            ',' if !in_string => {
                out.push(std::mem::take(&mut buf));
            }
            _ => buf.push(c),
        }
    }
    if !buf.trim().is_empty() {
        out.push(buf);
    }
    out
}

/// F-30-5 review fix (slice 30b): retention policy for the snapshot
/// directory.  Deletes all `*.wal` files EXCEPT the `keep_last_n`
/// most recently modified.  Best-effort — I/O failures on individual
/// files are logged but do not abort the sweep (the goal is to bound
/// disk usage, not to guarantee removal).
///
/// The retention window is measured by filesystem mtime, not by
/// block height, because slice 30b's cadence loop calls this after
/// each snapshot write and the mtime ordering matches the write
/// order.  A future slice may switch to an operator-visible manifest
/// file if cross-restart retention becomes important.
///
/// Returns the number of files removed.
pub fn prune_snapshot_dir(snapshot_dir: &Path, keep_last_n: usize) -> std::io::Result<usize> {
    let mut wal_files: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(snapshot_dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("wal") {
                return None;
            }
            // M-P7-2 review fix: use `file_type()` (lstat, does NOT
            // follow symlinks) to detect and skip symlink `.wal`
            // entries.  Attacker-planted `evil.wal -> /etc/passwd`
            // would otherwise get a fresh mtime from its target and
            // persist across restarts.  `path.symlink_metadata()` for
            // the mtime — also lstat-based, so a symlink's mtime is
            // the symlink's own mtime, not the target's.  We do NOT
            // unlink symlinks here (that's a separate operator
            // hygiene concern; `remove_file` on Unix unlinks the
            // symlink itself, not the target — safe, but noisy).
            let file_type = entry.file_type().ok()?;
            if file_type.is_symlink() {
                tracing::debug!(
                    target: "f1r3fly.fs_wal.snapshot",
                    path = %path.display(),
                    "prune_snapshot_dir: skipping symlink .wal entry \
                     (operator hygiene: snapshot dir should be exclusively owned)"
                );
                return None;
            }
            std::fs::symlink_metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|mtime| (path, mtime))
        })
        .collect();
    // Sort newest-first, then keep the first `keep_last_n`.
    wal_files.sort_by(|a, b| b.1.cmp(&a.1));
    let mut removed = 0;
    for (path, _) in wal_files.into_iter().skip(keep_last_n) {
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) => tracing::warn!(
                target: "f1r3fly.fs_wal.snapshot",
                path = %path.display(),
                error = %e,
                "prune_snapshot_dir: failed to remove old snapshot; continuing"
            ),
        }
        // Phase 7b-2 (2026-08-27): also remove the `.hashes`
        // sidecar so it doesn't outlive the snapshot it was
        // paired with.  Otherwise stale sidecars keep contributing
        // to the retention union forever, defeating retention.
        // Best-effort: missing sidecar is fine (pre-Phase-7b-2
        // snapshots don't have one; ENOENT is not a failure).
        let sidecar_path = path.with_extension("hashes");
        match std::fs::remove_file(&sidecar_path) {
            Ok(()) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(
                target: "f1r3fly.fs_wal.payload_store",
                path = %sidecar_path.display(),
                error = %e,
                "prune_snapshot_dir: failed to remove hashes sidecar; \
                 the sidecar file will leak but this is not a correctness bug \
                 (retention over-counts hashes and prunes less aggressively)"
            ),
        }
    }
    Ok(removed)
}

/// Read a snapshot by content hash and verify byte-for-byte before
/// returning.  Returns the raw bytes (decoding is a separate concern
/// for the replay engine).
///
/// Slice 34 (MED-1) also validates the leading `SNAPSHOT_FORMAT_VERSION`
/// byte and rejects any other value with `UnsupportedVersion` —
/// clean-fail semantics for an older validator that fetches a
/// snapshot from a newer, hard-forked network.  Truncated blobs
/// (fewer than 1 byte) surface `Truncated`.
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
    // Version check runs AFTER hash verification: an attacker who
    // handed us a byte string that hashed to the requested root but
    // carried a bogus version byte would be caught here even if the
    // root check somehow passed (defense in depth).  In practice
    // Blake2b256 preimage resistance already makes forging bytes to
    // a target root computationally infeasible.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_write_entry(payload: &[u8], path: &str) -> WalEntry {
        WalEntry {
            op: WalOp::Write,
            path: PathBuf::from(path),
            extra_path: None,
            offset: None,
            length: Some(payload.len() as u64),
            payload_ref: Some(PayloadRef::hash(payload)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    /// Slice 34 (MED-1): empty slice is now version byte (1) +
    /// four-byte count of zero.  Pre-slice-34 layout was just the
    /// four zero bytes.  Bumping golden hex is intentional here —
    /// the version byte is an added consensus commitment.
    #[test]
    fn empty_slice_encodes_to_version_byte_plus_four_zero_bytes() {
        let bytes = encode_wal_slice(&[]);
        assert_eq!(bytes, vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 0]);
    }

    #[test]
    fn empty_slice_root_is_deterministic() {
        let r1 = compute_wal_root(&[]);
        let r2 = compute_wal_root(&[]);
        assert_eq!(r1, r2, "same input → same root");
    }

    #[test]
    fn same_entries_yield_identical_root() {
        let a = vec![
            mk_write_entry(b"aa", "/root/x"),
            mk_write_entry(b"bb", "/root/y"),
        ];
        let b = vec![
            mk_write_entry(b"aa", "/root/x"),
            mk_write_entry(b"bb", "/root/y"),
        ];
        assert_eq!(compute_wal_root(&a), compute_wal_root(&b));
    }

    #[test]
    fn different_order_yields_different_root() {
        let a = vec![
            mk_write_entry(b"aa", "/root/x"),
            mk_write_entry(b"bb", "/root/y"),
        ];
        let b = vec![
            mk_write_entry(b"bb", "/root/y"),
            mk_write_entry(b"aa", "/root/x"),
        ];
        assert_ne!(
            compute_wal_root(&a),
            compute_wal_root(&b),
            "reordering entries must change the root (append order is consensus-observable)"
        );
    }

    #[test]
    fn different_payload_yields_different_root() {
        let a = vec![mk_write_entry(b"aa", "/root/x")];
        let b = vec![mk_write_entry(b"ab", "/root/x")];
        assert_ne!(compute_wal_root(&a), compute_wal_root(&b));
    }

    #[test]
    fn different_path_yields_different_root() {
        let a = vec![mk_write_entry(b"aa", "/root/x")];
        let b = vec![mk_write_entry(b"aa", "/root/y")];
        assert_ne!(compute_wal_root(&a), compute_wal_root(&b));
    }

    #[test]
    fn snapshot_blob_round_trip_via_disk() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_write_entry(b"hello", "/root/greeting"), WalEntry {
            op: WalOp::Truncate,
            path: PathBuf::from("/root/greeting"),
            extra_path: None,
            offset: Some(5),
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }];
        let (path, root, _merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        assert!(path.exists(), "snapshot file must be created");
        let hex_name = path.file_name().unwrap().to_string_lossy();
        assert!(
            hex_name.ends_with(".wal"),
            "filename must end in .wal, got {hex_name}"
        );
        let bytes = read_snapshot_bytes(dir.path(), &root).unwrap();
        assert_eq!(hash_of(&bytes), root, "round-tripped bytes match root");
        assert_eq!(bytes, encode_wal_slice(&entries));
    }

    /// Phase 7b-2 item (c) (2026-08-28): `decode_wal_slice` inverts
    /// `encode_wal_slice` byte-for-byte across every op tag and
    /// PayloadRef variant.  A regression here would mean joiners
    /// apply corrupt WAL slices on fresh trees → downstream state
    /// hashes diverge.
    #[test]
    fn decode_wal_slice_round_trips_all_variants() {
        let entries = vec![
            mk_write_entry(b"data", "/root/w.bin"),
            WalEntry {
                op: WalOp::WriteAt,
                path: PathBuf::from("/root/wa.bin"),
                extra_path: None,
                offset: Some(42),
                length: Some(4),
                payload_ref: Some(PayloadRef::hash(b"payload-at")),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            WalEntry {
                op: WalOp::Truncate,
                path: PathBuf::from("/root/w.bin"),
                extra_path: None,
                offset: Some(128),
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            WalEntry {
                op: WalOp::Chmod,
                path: PathBuf::from("/root/cm.bin"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: Some(0o644),
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            WalEntry {
                op: WalOp::Chown,
                path: PathBuf::from("/root/co.bin"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: Some("nobody".to_string()),
                group: Some("nogroup".to_string()),
                outcome: WalOutcome::Success,
            },
            WalEntry {
                op: WalOp::Rename,
                path: PathBuf::from("/root/rn/from"),
                extra_path: Some(PathBuf::from("/root/rn/to")),
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            WalEntry {
                op: WalOp::CopyFile,
                path: PathBuf::from("/root/cp/from"),
                extra_path: Some(PathBuf::from("/root/cp/to")),
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Failure {
                    code: super::super::errors::FSERR_CODE_PERM,
                },
            },
            WalEntry {
                op: WalOp::EntriesStreamNext,
                path: PathBuf::from("/root/dir"),
                extra_path: None,
                offset: None,
                length: Some(1),
                payload_ref: Some(PayloadRef::hash(b"stream-reply")),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
        ];
        let bytes = encode_wal_slice(&entries);
        let decoded = decode_wal_slice(&bytes).expect("decode round-trip");
        assert_eq!(decoded, entries);
    }

    /// Empty WAL slice is a legit encoding (version byte + four
    /// zeros); decoder returns an empty Vec cleanly.
    #[test]
    fn decode_wal_slice_empty() {
        let bytes = encode_wal_slice(&[]);
        let decoded = decode_wal_slice(&bytes).expect("decode empty");
        assert!(decoded.is_empty());
    }

    /// Byte stream shorter than the version + count prefix surfaces
    /// as `Truncated` instead of panicking or returning nonsense.
    #[test]
    fn decode_wal_slice_truncated_prefix() {
        let bytes = vec![SNAPSHOT_FORMAT_VERSION, 0, 0]; // 3 bytes < 5
        let err = decode_wal_slice(&bytes);
        assert!(
            matches!(err, Err(SnapshotError::Truncated { .. })),
            "expected Truncated on short prefix, got {err:?}"
        );
    }

    /// Version byte in the future → `UnsupportedVersion`, not a
    /// silent mis-decode.  Same shape as `read_snapshot_bytes`
    /// rejecting a hard-forked network's snapshots.
    #[test]
    fn decode_wal_slice_rejects_future_version() {
        let mut bytes = encode_wal_slice(&[]);
        bytes[0] = SNAPSHOT_FORMAT_VERSION + 1;
        let err = decode_wal_slice(&bytes);
        assert!(
            matches!(err, Err(SnapshotError::UnsupportedVersion { .. })),
            "expected UnsupportedVersion, got {err:?}"
        );
    }

    /// A byzantine peer that returns bytes claiming a well-formed
    /// prefix but with an unknown op tag mid-entry surfaces as
    /// `MalformedBlob`, not a panic.
    #[test]
    fn decode_wal_slice_rejects_unknown_op_tag() {
        let mut bytes = encode_wal_slice(&[mk_write_entry(b"x", "/a")]);
        // Overwrite the op tag byte (right after version + count).
        bytes[5] = 0xFF;
        let err = decode_wal_slice(&bytes);
        assert!(
            matches!(err, Err(SnapshotError::MalformedBlob { .. })),
            "expected MalformedBlob on bad op tag, got {err:?}"
        );
    }

    /// 2026-08-28 hardening: op tag 0 is not a valid variant (tags
    /// start at 1).  Explicit boundary pin — the general
    /// `unknown_op_tag` test used 0xFF; this locks in the low end
    /// so a future refactor that starts numbering at 0 breaks
    /// HERE rather than in production.
    #[test]
    fn decode_wal_slice_rejects_op_tag_zero() {
        let mut bytes = encode_wal_slice(&[mk_write_entry(b"x", "/a")]);
        bytes[5] = 0;
        let err = decode_wal_slice(&bytes);
        assert!(
            matches!(err, Err(SnapshotError::MalformedBlob { .. })),
            "op tag 0 must be MalformedBlob, got {err:?}"
        );
    }

    /// Boundary companion: op tag 16 is one past the last valid
    /// variant (EntriesStreamNext = 15).  If a future slice adds a
    /// tag, this pin flips and the new-variant maintainer must add
    /// coverage for it.
    #[test]
    fn decode_wal_slice_rejects_op_tag_sixteen() {
        let mut bytes = encode_wal_slice(&[mk_write_entry(b"x", "/a")]);
        bytes[5] = 16;
        let err = decode_wal_slice(&bytes);
        assert!(
            matches!(err, Err(SnapshotError::MalformedBlob { .. })),
            "op tag 16 must be MalformedBlob (guards the tail of \
             the reserved range); if you added a variant, bump this \
             pin to the next unassigned tag: got {err:?}"
        );
    }

    /// Round-trip via disk: write, read back, decode.  Composes
    /// `write_snapshot` + `read_snapshot_bytes` + `decode_wal_slice`
    /// — the full joiner-side pipeline for reconstructing a WAL
    /// slice from an assembled snapshot.
    #[test]
    fn decode_via_disk_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![
            mk_write_entry(b"aa", "/root/x"),
            mk_write_entry(b"bb", "/root/y"),
        ];
        let (_path, root, _merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let bytes = read_snapshot_bytes(dir.path(), &root).unwrap();
        let decoded = decode_wal_slice(&bytes).unwrap();
        assert_eq!(decoded, entries);
    }

    #[test]
    fn read_snapshot_rejects_tampered_file() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_write_entry(b"aa", "/root/x")];
        let (path, root, _merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        // Tamper: append a byte.
        let mut tampered = std::fs::read(&path).unwrap();
        tampered.push(0xff);
        std::fs::write(&path, &tampered).unwrap();
        let err = read_snapshot_bytes(dir.path(), &root);
        assert!(
            matches!(err, Err(SnapshotError::RootMismatch { .. })),
            "tampered snapshot must fail root verification"
        );
    }

    #[test]
    fn write_snapshot_is_idempotent_for_same_content() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_write_entry(b"aa", "/root/x")];
        let (p1, r1, _m1) = write_snapshot(dir.path(), &entries).unwrap();
        let (p2, r2, _m2) = write_snapshot(dir.path(), &entries).unwrap();
        assert_eq!(p1, p2, "same content → same path");
        assert_eq!(r1, r2, "same content → same root");
        assert!(p1.exists());
    }

    /// L-7 fix (2026-08-06): pin the empty-slice write path.  A
    /// snapshot of an empty WAL slice produces a file that contains
    /// only the SNAPSHOT_FORMAT_VERSION byte + 4 zero bytes (u32-be
    /// entry count).  Round-trip through `read_snapshot_bytes` must
    /// succeed and yield an empty WalEntry Vec.  Pre-fix no test
    /// covered this — a regression that panicked or errored on the
    /// zero-entries path would slip through until an operator hit
    /// a cadence boundary with no consensus mutations.
    #[test]
    fn write_snapshot_of_empty_slice_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let (path, root, _merkle_root) = write_snapshot(dir.path(), &[]).unwrap();
        assert!(path.exists(), "empty-slice write must produce a file");
        // Exactly 5 bytes: version + u32-be count(0).
        let contents = std::fs::read(&path).unwrap();
        assert_eq!(
            contents.len(),
            5,
            "empty-slice on-disk file is version byte + u32-be zero count"
        );
        assert_eq!(contents[0], SNAPSHOT_FORMAT_VERSION);
        assert_eq!(&contents[1..], &[0u8, 0, 0, 0]);
        // Root is the Blake2b256 of those 5 bytes (deterministic).
        assert_eq!(root, hash_of(&contents));
        // Read back cleanly via the dir + root API.
        let read_back = read_snapshot_bytes(dir.path(), &root).unwrap();
        assert_eq!(read_back, contents);
    }

    #[test]
    fn op_tags_are_stable() {
        // Any renumbering here is a hard-fork of the WAL root — pin
        // the values so a reorderer in the enum can't silently change
        // the tag.
        assert_eq!(op_tag(WalOp::Write), 1);
        assert_eq!(op_tag(WalOp::WriteAt), 2);
        assert_eq!(op_tag(WalOp::Truncate), 3);
        assert_eq!(op_tag(WalOp::Chmod), 4);
        assert_eq!(op_tag(WalOp::Chown), 5);
        assert_eq!(op_tag(WalOp::RemoveFile), 6);
        assert_eq!(op_tag(WalOp::RemoveDir), 7);
        assert_eq!(op_tag(WalOp::Rename), 8);
        assert_eq!(op_tag(WalOp::CopyFile), 9);
        assert_eq!(op_tag(WalOp::Read), 10);
        assert_eq!(op_tag(WalOp::ReadAt), 11);
        // M-5 fix (2026-08-06): state-read journaling ops.
        assert_eq!(op_tag(WalOp::Stat), 12);
        assert_eq!(op_tag(WalOp::Entries), 13);
        assert_eq!(op_tag(WalOp::Size), 14);
        // Streaming-backing slice Step 3 (2026-08-25).
        assert_eq!(op_tag(WalOp::EntriesStreamNext), 15);
    }

    // ------------------------------------------------------------------
    // Round-2 review-fix tests (H-30-4 / H-30-5 / H-30-7 / coverage M1/M2)
    // ------------------------------------------------------------------

    /// H-30-4: golden-hex root pin.  Any change to the canonical
    /// encoding (Blake2b256 config, prefix width, endianness,
    /// `PathBuf::as_encoded_bytes()` semantics) will flip this hash.
    /// A validator upgrade that inadvertently changes the encoding
    /// diverges on-network; this test catches it in CI before
    /// deployment.  If you're updating this hash, you are proposing a
    /// hard fork of the WAL commitment scheme — bump the WAL version
    /// and coordinate.
    #[test]
    fn compute_wal_root_golden_hex() {
        // A minimal, fully-populated entry that exercises every field
        // (op, path, no extra_path, offset, length, hash payload,
        // no mode_bits, no owner, no group).
        let entries = vec![WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/root/canonical.bin"),
            extra_path: None,
            offset: Some(0),
            length: Some(5),
            payload_ref: Some(PayloadRef::hash(b"hello")),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }];
        let root = compute_wal_root(&entries);
        let hex = root.iter().fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        });
        // Golden value re-pinned 2026-08-31 (Task 0.4 / Consensus-fs
        // Shape A: bumped `SNAPSHOT_FORMAT_VERSION` from 4 to 5 to
        // distinguish the pre-Shape-A snapshot semantics — Consensus
        // WAL entries' `path` field is now bundle-relative, not an
        // absolute on-disk canon_path).  Only the version byte
        // changed for THIS test's entry shape (Write op), so the
        // hash differs solely because of the leading version-byte
        // increment.  Prior anchors:
        //   pre-Shape-A (v=4): 0db9a41865abc2e7e00e96f66a26267f2b9e1815ef55490c237675bff1c60a73
        //   pre-streaming-slice (v=3): 9f2553c38cce8b72bbf6ad78c22f4b32f195b8bed781c952403f5404c25891c4
        //   pre-M-5 (v=2): eaeb49f95ec12631c4d59da9520f23cd9558c98e60529deda1fbc42395b5811a
        //   pre-H-6 (v=1): 532eea9096eb6962acbb48374e79167149960ec132f8e95838678e20e2fa38b2
        //   pre-slice-34: 06a8ce938471c2a9722aa3592209e04dbe9230b759af36a5088dea677f93b825
        // Regenerate via
        //   cargo test -p rholang --lib -- compute_wal_root_golden_hex --nocapture
        // ONLY when intentionally hard-forking the encoding.
        const EXPECTED: &str = "1bdd5f4536180b811f139ea762c6659f29bf6cf5008ec85b970bb618d10786c9";
        assert_eq!(
            hex, EXPECTED,
            "WAL root golden-hex mismatch — did you accidentally change the encoding? \
             If intentional, bump the WAL version and update EXPECTED."
        );
    }

    /// H-30-5: prove that `encode_entry` actually uses the `op_tag`
    /// values.  Pre-fix, the `op_tags_are_stable` test only checked
    /// the private `op_tag` fn in isolation — an `encode_entry`
    /// refactor that inlined `e.op as u8` (auto-discriminants: 0-8
    /// not 1-9) would silently drift while `op_tags_are_stable`
    /// still passed.  This test encodes one entry per WalOp variant
    /// and asserts the first byte of the entry region (index 4,
    /// after the u32-be count prefix) matches the expected tag.
    #[test]
    fn encode_entry_uses_op_tag_values() {
        for (op, tag) in [
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
        ] {
            let e = WalEntry {
                op,
                path: PathBuf::from("/x"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            };
            let bytes = encode_wal_slice(&[e]);
            // Slice 34: byte 0 = version, bytes 1..5 = count(1) BE,
            // byte 5 = op tag.
            assert_eq!(bytes[0], SNAPSHOT_FORMAT_VERSION);
            assert_eq!(&bytes[1..5], &1u32.to_be_bytes());
            assert_eq!(
                bytes[5], tag,
                "encode_entry byte 5 for {op:?} must be {tag}, got {}",
                bytes[5]
            );
        }
    }

    /// H-30-7: DeployRef byte-layout stability.  Verifies
    /// (a) the tag byte is 2, (b) block_hash comes first, then
    /// deploy_index big-endian u32, then arg_index big-endian u32,
    /// (c) swapping deploy_index and arg_index produces different
    /// roots, (d) endianness is big-endian (not native).
    #[test]
    fn deploy_ref_encoding_is_big_endian_and_field_order_is_stable() {
        let block_hash = [0xAAu8; 32];
        let e1 = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: Some(PayloadRef::DeployRef {
                block_hash,
                deploy_index: 0x0102_0304,
                arg_index: 0x0506_0708,
            }),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let mut e2 = e1.clone();
        if let Some(PayloadRef::DeployRef {
            deploy_index,
            arg_index,
            ..
        }) = &mut e2.payload_ref
        {
            std::mem::swap(deploy_index, arg_index);
        }
        assert_ne!(
            compute_wal_root(&[e1.clone()]),
            compute_wal_root(&[e2]),
            "swapping deploy_index and arg_index MUST change the root"
        );

        // Byte-layout pin: encode e1 and locate the payload_ref bytes.
        let bytes = encode_wal_slice(&[e1]);
        // Slice 34 layout: version(1) count(4) op(1) path_len(4)
        //   path("/x" = 2) extra_flag(1) offset_flag(1) length_flag(1)
        //   payload_tag(1) ...
        // Walk 1+4+1+4+2+1+1+1 = 15 to reach the payload tag.
        assert_eq!(bytes[0], SNAPSHOT_FORMAT_VERSION, "version byte");
        assert_eq!(bytes[15], 2, "payload_ref tag must be 2 for DeployRef");
        // Next 32 bytes = block_hash (all 0xAA).
        assert!(
            bytes[16..16 + 32].iter().all(|&b| b == 0xAA),
            "block_hash bytes must appear directly after tag byte"
        );
        // Next 4 bytes = deploy_index big-endian.
        assert_eq!(
            &bytes[16 + 32..16 + 32 + 4],
            &0x0102_0304u32.to_be_bytes(),
            "deploy_index must be big-endian"
        );
        // Next 4 bytes = arg_index big-endian.
        assert_eq!(
            &bytes[16 + 32 + 4..16 + 32 + 8],
            &0x0506_0708u32.to_be_bytes(),
            "arg_index must be big-endian and follow deploy_index"
        );
    }

    /// Coverage M1: every WalOp variant round-trips through
    /// `encode_wal_slice` without panicking, and every op produces
    /// distinct bytes when combined with the same other fields
    /// (proves the op is part of the encoding, not silently
    /// stripped).
    #[test]
    fn every_walop_variant_encodes_distinctly() {
        let mut roots = std::collections::HashSet::new();
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
        ] {
            let e = WalEntry {
                op,
                path: PathBuf::from("/x"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            };
            let root = compute_wal_root(&[e]);
            assert!(
                roots.insert(root),
                "op {op:?} produced a duplicate root — the op tag is not part of the encoding"
            );
        }
        assert_eq!(roots.len(), 9);
    }

    /// Coverage M2: every `Option::Some` field arm exercises the
    /// Some-branch of its encoder helper.  Pre-fix, only `None`
    /// arms of `extra_path`, `mode_bits`, `owner`, `group` were
    /// covered by any test — a bug in the Some encoding would slip
    /// through.
    #[test]
    fn every_option_some_arm_encodes_distinctly_from_none() {
        let base = WalEntry {
            op: WalOp::Rename,
            path: PathBuf::from("/from"),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let base_root = compute_wal_root(&[base.clone()]);

        // extra_path: None vs Some(distinct path).
        let mut e = base.clone();
        e.extra_path = Some(PathBuf::from("/to"));
        assert_ne!(
            compute_wal_root(&[e]),
            base_root,
            "extra_path Some must diff"
        );

        // mode_bits: None vs Some.
        let mut e = base.clone();
        e.mode_bits = Some(0o755);
        assert_ne!(
            compute_wal_root(&[e]),
            base_root,
            "mode_bits Some must diff"
        );

        // owner: None vs Some.
        let mut e = base.clone();
        e.owner = Some("alice".to_string());
        assert_ne!(compute_wal_root(&[e]), base_root, "owner Some must diff");

        // group: None vs Some.
        let mut e = base.clone();
        e.group = Some("wheel".to_string());
        assert_ne!(compute_wal_root(&[e]), base_root, "group Some must diff");

        // Empty vs None (distinguishability): "" is NOT None.
        let mut e = base.clone();
        e.owner = Some(String::new());
        assert_ne!(
            compute_wal_root(&[e]),
            base_root,
            "Some(empty string) must be distinguishable from None"
        );

        // H-6 fix (2026-08-06): outcome Success vs Failure must
        // diff.  This is the whole point of the outcome field —
        // a leader's failed syscall MUST NOT hash the same as a
        // successful one, else the H-6 attack surface is silently
        // reopened by a maintainer who patches a wire format
        // without noticing.
        let mut e = base.clone();
        e.outcome = WalOutcome::Failure {
            code: super::super::errors::FSERR_CODE_IO,
        };
        assert_ne!(
            compute_wal_root(&[e]),
            base_root,
            "outcome Success vs Failure must diff — H-6 hard-fork surface"
        );

        // Two distinct Failure codes must also diff.  Guards
        // against a maintainer accidentally dropping the u32-be
        // code encoding.
        let mut e_a = base.clone();
        e_a.outcome = WalOutcome::Failure {
            code: super::super::errors::FSERR_CODE_IO,
        };
        let mut e_b = base.clone();
        e_b.outcome = WalOutcome::Failure {
            code: super::super::errors::FSERR_CODE_PERM,
        };
        assert_ne!(
            compute_wal_root(&[e_a]),
            compute_wal_root(&[e_b]),
            "distinct Failure codes must diff"
        );
    }

    /// H-6 fix (2026-08-06): pin the on-wire layout of the
    /// outcome tail — item #9 of the hard-fork surface catalog.
    ///
    /// Success = single tag byte 0.
    /// Failure = tag byte 1 + u32-be `code`.
    /// A refactor that changes the tag numbering, the code width,
    /// or the tail position (e.g., inserting outcome BEFORE group
    /// instead of after) forks the WAL root.  Pin the byte
    /// positions so a regression fails HERE rather than diverging
    /// consensus in production.
    #[test]
    fn encode_entry_outcome_layout_is_stable() {
        let base = |outcome: WalOutcome| WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome,
        };
        let bytes_success = encode_wal_slice(&[base(WalOutcome::Success)]);
        let bytes_failure = encode_wal_slice(&[base(WalOutcome::Failure { code: 0x0203_0405 })]);
        // Success has exactly one more byte (the outcome tag = 0)
        // than the Failure encoding is minus its 4-byte code.
        assert_eq!(
            bytes_failure.len(),
            bytes_success.len() + 4,
            "Failure adds a u32 code after the tag; Success is tag-only"
        );
        // Last byte of Success is the outcome tag 0.
        assert_eq!(
            *bytes_success.last().unwrap(),
            0,
            "Success outcome tag must be 0"
        );
        // Failure ends with tag=1 followed by big-endian u32 of code.
        let n = bytes_failure.len();
        assert_eq!(
            bytes_failure[n - 5],
            1,
            "Failure outcome tag must be 1 (position -5 = tag, -4..0 = u32-be code)"
        );
        assert_eq!(
            &bytes_failure[n - 4..],
            &0x0203_0405u32.to_be_bytes(),
            "Failure code must be u32-big-endian at the tail"
        );
    }

    /// F-30-5 slice-30b: `prune_snapshot_dir` keeps the newest N and
    /// removes older ones.
    #[test]
    fn prune_snapshot_dir_keeps_last_n() {
        let dir = tempfile::tempdir().unwrap();
        // Write 5 snapshots with distinct content (distinct hashes,
        // distinct filenames).  Sleep 10ms between writes so mtimes
        // are monotonically distinguishable.
        let mut roots = Vec::new();
        for i in 0..5u8 {
            let entries = vec![mk_write_entry(&[i], "/x")];
            let (_, root, _merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
            roots.push(root);
            // L-30-COV-1 review fix: bump from 20ms to 1100ms so
            // macOS APFS (which historically had 1-second mtime
            // granularity on some volumes) reliably orders these
            // writes.  Slow but robust; retention tests run once
            // per CI so the added seconds are negligible.
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        assert_eq!(
            std::fs::read_dir(dir.path())
                .unwrap()
                .filter(|e| e
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "wal"))
                .count(),
            5
        );
        // Keep the 2 newest.
        let removed = prune_snapshot_dir(dir.path(), 2).unwrap();
        assert_eq!(removed, 3, "3 old snapshots should be removed");
        // The newest two files are the last two roots we wrote.
        for root in &roots[3..] {
            assert!(
                snapshot_path(dir.path(), root).exists(),
                "root {:?} must survive prune",
                &root[..4]
            );
        }
        for root in &roots[..3] {
            assert!(
                !snapshot_path(dir.path(), root).exists(),
                "root {:?} must be pruned",
                &root[..4]
            );
        }
    }

    /// prune_snapshot_dir on an empty dir is a no-op.
    #[test]
    fn prune_snapshot_dir_no_op_on_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(prune_snapshot_dir(dir.path(), 5).unwrap(), 0);
    }

    // ------------------------------------------------------------------
    // Slice 30b: SnapshotWriter cadence + retention tests.
    // ------------------------------------------------------------------

    /// Phase 7b-1 (2026-08-27): `SnapshotBlob` now carries a
    /// `merkle_root` derived from the 4 MiB chunker + Merkle tree.
    /// For a single-chunk snapshot the Merkle root equals the
    /// chunk hash, which is `Blake2b256(bytes)` — i.e., identical
    /// to `SnapshotBlob.root` (the atomic content-address).
    /// Multi-chunk snapshots diverge (Merkle root is
    /// `hash(chunk_hashes[0] || chunk_hashes[1] || ...)`), so a
    /// regression that collapsed the two fields would trip the
    /// distinct-in-multi-chunk pin below.
    #[test]
    fn snapshot_blob_carries_merkle_root() {
        let entries = vec![mk_write_entry(b"hi", "/x")];
        let blob = snapshot_blob(&entries);
        assert_ne!(blob.merkle_root, [0u8; 32]);
        // Single-chunk case: merkle_root == blob.root == hash(bytes).
        // The blob is well under 4 MiB, so one chunk.
        assert!(blob.bytes.len() < crate::rust::interpreter::io::snapshot_chunk::CHUNK_SIZE);
        assert_eq!(
            blob.merkle_root, blob.root,
            "single-chunk snapshot's merkle_root == chunk_hash == blob.root"
        );
    }

    /// Multi-chunk divergence pin: a snapshot bigger than
    /// `CHUNK_SIZE` produces a `merkle_root` that differs from
    /// `Blake2b256(whole_blob)`.  Real WAL slices are capped at
    /// `MAX_WAL_ENTRIES = 65536` (< 4 MiB), so we can't hit multi-
    /// chunk via `snapshot_blob(entries)` at runtime.  We exercise
    /// the divergence at the primitive level: chunk a raw >4 MiB
    /// blob and check `snapshot_merkle_root` != `Blake2b256`.
    /// Guards against a regression that computed
    /// `merkle_root = Blake2b256(bytes)` (defeating the chunker's
    /// point).  See `snapshot_chunk::tests` for chunker-specific
    /// coverage.
    #[test]
    fn multi_chunk_merkle_root_differs_from_atomic_hash() {
        use crate::rust::interpreter::io::snapshot_chunk::{
            chunk_snapshot, snapshot_merkle_root, CHUNK_SIZE,
        };
        let bytes = vec![0x77u8; CHUNK_SIZE + 100];
        let chunks = chunk_snapshot(&bytes);
        let hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
        let merkle = snapshot_merkle_root(&hashes);
        let atomic = hash_of(&bytes);
        assert_ne!(
            merkle, atomic,
            "multi-chunk merkle root MUST differ from atomic Blake2b256(bytes); \
             a regression that returned atomic would defeat per-chunk verification",
        );
    }

    #[test]
    fn snapshot_writer_cadence_skips_non_boundary_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 5,
            retain: 3,
            signer_sk: None,
            payload_dir: None,
        };
        let entries = vec![mk_write_entry(b"a", "/x")];
        // Blocks 1..5 (not aligned to cadence=5 boundary; 5 is aligned)
        assert!(writer.maybe_write(1, &entries).unwrap().is_none());
        assert!(writer.maybe_write(2, &entries).unwrap().is_none());
        assert!(writer.maybe_write(3, &entries).unwrap().is_none());
        assert!(writer.maybe_write(4, &entries).unwrap().is_none());
        // Block 5: cadence hit.
        let res = writer.maybe_write(5, &entries).unwrap();
        assert!(res.is_some(), "cadence-hit block must persist");
        let (root, _merkle) = res.unwrap();
        assert!(snapshot_path(dir.path(), &root).exists());
    }

    #[test]
    fn snapshot_writer_genesis_block_writes_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 10,
            retain: 5,
            signer_sk: None,
            payload_dir: None,
        };
        // Block 0 is % 10 == 0 → cadence hit.
        let entries = vec![mk_write_entry(b"genesis", "/genesis")];
        let root = writer.maybe_write(0, &entries).unwrap();
        assert!(root.is_some(), "genesis (block 0) is a cadence-hit");
    }

    #[test]
    fn snapshot_writer_skips_empty_entries_even_on_cadence_hit() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1,
            retain: 3,
            signer_sk: None,
            payload_dir: None,
        };
        // Cadence=1 means every block is a hit, but empty entries
        // don't produce a `.wal` file (empty payloads all hash to
        // the same content-address; one file per empty slice would
        // waste disk).  Slice 30c Phase C (F-30b-4 resolution):
        // empty cadence hits DO append a manifest sentinel line so
        // joining validators can distinguish "block had no fs
        // mutations" from "cadence miss."
        assert!(writer.maybe_write(0, &[]).unwrap().is_none());
        assert!(writer.maybe_write(1, &[]).unwrap().is_none());
        // No `.wal` files — the byte-heavy artifact is what we
        // skip on empty slices.
        let wal_files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|s| s == "wal"))
            .collect();
        assert!(
            wal_files.is_empty(),
            "empty slice must NOT write a .wal file; got {} files",
            wal_files.len()
        );
        // But manifest.jsonl DOES exist and contains 2 sentinel
        // entries (one per cadence-hit block).
        let manifest = read_manifest(dir.path()).unwrap();
        assert_eq!(manifest.len(), 2, "one sentinel per cadence-hit block");
        for e in &manifest {
            assert!(e.root.is_none(), "sentinel entries have root = None");
        }
    }

    #[test]
    fn snapshot_writer_negative_block_number_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 5,
            retain: 3,
            signer_sk: None,
            payload_dir: None,
        };
        let entries = vec![mk_write_entry(b"x", "/x")];
        // Negative block numbers indicate "no block yet" state and
        // must not persist.
        assert!(writer.maybe_write(-1, &entries).unwrap().is_none());
        assert!(writer.maybe_write(-100, &entries).unwrap().is_none());
    }

    /// Slice 30c F-30b-3 fix pin: `BlockData::empty()` returns
    /// `block_number = -1` as a sentinel meaning "no block yet."
    /// A runtime that ever calls `maybe_write` in that state must
    /// not persist a snapshot (would otherwise land at block 0 or
    /// some cadence-hit due to a bogus initial value).  The
    /// negative-block-number skip above catches this; this test
    /// pins the intent explicitly.
    #[test]
    fn snapshot_writer_skips_block_data_empty_sentinel() {
        use crate::rust::interpreter::system_processes::BlockData;
        let sentinel_block_number = BlockData::empty().block_number;
        assert!(
            sentinel_block_number < 0,
            "BlockData::empty must use a negative sentinel (F-30b-3); got {sentinel_block_number}"
        );
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1, // any cadence — negative < 0 skip triggers first
            retain: 3,
            signer_sk: None,
            payload_dir: None,
        };
        let entries = vec![mk_write_entry(b"x", "/x")];
        assert!(
            writer
                .maybe_write(sentinel_block_number, &entries)
                .unwrap()
                .is_none(),
            "BlockData::empty sentinel must NOT trigger a snapshot write"
        );
        // Companion pin: manifest is also NOT touched.
        let manifest = read_manifest(dir.path()).unwrap();
        assert!(
            manifest.is_empty(),
            "sentinel-block writes must not append manifest entries"
        );
    }

    #[test]
    fn snapshot_writer_prunes_to_retention_bound() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1, // every block
            retain: 2,
            signer_sk: None,
            payload_dir: None,
        };
        // Write snapshots for blocks 1..=5 with distinct content.
        for i in 1..=5u8 {
            let entries = vec![mk_write_entry(&[i], "/x")];
            writer.maybe_write(i as i64, &entries).unwrap();
            // L-30-COV-1 review fix: bump from 20ms to 1100ms so
            // macOS APFS (which historically had 1-second mtime
            // granularity on some volumes) reliably orders these
            // writes.  Slow but robust; retention tests run once
            // per CI so the added seconds are negligible.
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        // Only 2 should remain (retention = 2).
        let wal_count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "wal")
            })
            .count();
        assert_eq!(
            wal_count, 2,
            "retention bound must cap the on-disk snapshot count"
        );
    }

    // ------------------------------------------------------------------
    // Round-2 review fixes (slice 30b): fsync mode, tmp sweep,
    // cadence boundaries, retention edges, multi-snapshot
    // concatenation, O_EXCL semantics.
    // ------------------------------------------------------------------

    /// M-30b-4: on Unix, written snapshot files have 0o644 mode
    /// regardless of the process's umask.
    #[cfg(unix)]
    #[test]
    fn write_snapshot_sets_explicit_0o644_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_write_entry(b"aa", "/x")];
        let (path, _root, _merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let perms = std::fs::metadata(&path).unwrap().permissions();
        let mode = perms.mode() & 0o777;
        assert_eq!(
            mode, 0o644,
            "snapshot file mode must be 0o644 regardless of umask; got 0o{mode:o}"
        );
    }

    /// H-30b-3: `write_snapshot` uses `create_new` (O_EXCL) on the
    /// tmp file.  A regression to `create(true).truncate(true)`
    /// would silently work in the round-trip test but reopen the
    /// M-30-1 concurrent-writer / M-30-3 symlink race.  We can't
    /// guess the exact tmp filename (pid + nanos), but we can pin
    /// the semantic: pre-place a file matching the `.wal.tmp`
    /// pattern with attacker content and confirm `write_snapshot`
    /// either (a) fails hard or (b) succeeds without touching the
    /// pre-existing file (because it chose a different nanos).
    #[test]
    fn write_snapshot_never_truncates_existing_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        // Pre-place a decoy at a `.wal.tmp` path.  The real tmp
        // filename includes pid + nanos, so we can't collide by
        // guess — but we can verify decoy files are UNTOUCHED after
        // a snapshot write (proving `write_snapshot` doesn't open
        // ANY existing file).
        let decoy = dir.path().join("some-other.wal.tmp");
        let decoy_content = b"attacker owns me";
        std::fs::write(&decoy, decoy_content).unwrap();

        let entries = vec![mk_write_entry(b"real", "/x")];
        write_snapshot(dir.path(), &entries).unwrap();

        // Decoy contents are intact — write_snapshot did not
        // truncate or open it.
        assert_eq!(
            std::fs::read(&decoy).unwrap(),
            decoy_content,
            "write_snapshot must never touch existing files"
        );
    }

    /// M-30b-3: `sweep_stale_tmp_files` removes old `.wal.tmp`
    /// files (leftover from crashes) but leaves recent ones alone.
    #[test]
    fn sweep_stale_tmp_files_removes_old_but_keeps_recent() {
        let dir = tempfile::tempdir().unwrap();
        let old_tmp = dir.path().join("crashed-1.wal.tmp");
        let recent_tmp = dir.path().join("in-progress.wal.tmp");
        std::fs::write(&old_tmp, b"crashed").unwrap();
        std::fs::write(&recent_tmp, b"recent").unwrap();
        // Simplest strategy (without pulling in `filetime`): sweep
        // with `older_than_secs = 0` so ALL tmp files are older than
        // "now-0"; verify both are removed.  Then re-check with
        // older_than_secs=3600 to verify a just-created recent tmp
        // file is preserved.
        let removed = sweep_stale_tmp_files(dir.path(), 0).unwrap();
        assert_eq!(
            removed, 2,
            "with cutoff=0, both tmp files should be removed"
        );

        // Re-create for the "preserve recent" test.
        std::fs::write(&recent_tmp, b"recent").unwrap();
        let removed = sweep_stale_tmp_files(dir.path(), 3600).unwrap();
        assert_eq!(
            removed, 0,
            "with cutoff=3600s, a just-created tmp file must not be removed"
        );
        assert!(recent_tmp.exists());
    }

    /// M-30b-3 companion: sweep ignores files that don't match the
    /// `.wal.tmp` suffix.  Operators may put other files in the
    /// snapshot dir; sweep must not touch them.
    #[test]
    fn sweep_stale_tmp_files_ignores_non_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), b"notes").unwrap();
        std::fs::write(dir.path().join("real.wal"), b"data").unwrap();
        std::fs::write(dir.path().join("crashed.wal.tmp"), b"tmp").unwrap();
        let removed = sweep_stale_tmp_files(dir.path(), 0).unwrap();
        assert_eq!(removed, 1, "only the .wal.tmp file should be swept");
        assert!(dir.path().join("README").exists());
        assert!(dir.path().join("real.wal").exists());
        assert!(!dir.path().join("crashed.wal.tmp").exists());
    }

    /// H-30b-5: log-structured semantics — concatenating multiple
    /// snapshots is byte-consistent with a single-blob write of the
    /// union (post-count-prefix concatenation, since the count
    /// prefix is per-slice).  This is a joining-protocol pin: a
    /// late-joining validator reads each snapshot independently and
    /// applies its entries in the encoded order.
    #[test]
    fn multi_snapshot_concat_matches_ordered_entry_replay() {
        let dir = tempfile::tempdir().unwrap();
        let block_1 = vec![mk_write_entry(b"a", "/x"), mk_write_entry(b"b", "/y")];
        let block_2 = vec![mk_write_entry(b"c", "/z")];
        let (_, root_1, _merkle_1) = write_snapshot(dir.path(), &block_1).unwrap();
        let (_, root_2, _merkle_2) = write_snapshot(dir.path(), &block_2).unwrap();
        // Read each snapshot back independently.
        let bytes_1 = read_snapshot_bytes(dir.path(), &root_1).unwrap();
        let bytes_2 = read_snapshot_bytes(dir.path(), &root_2).unwrap();
        // Each blob independently encodes its own count-prefixed slice.
        assert_eq!(bytes_1, encode_wal_slice(&block_1));
        assert_eq!(bytes_2, encode_wal_slice(&block_2));
        // The joining-replay semantic: after applying both slices in
        // order, the effective entry sequence equals concatenation.
        let concatenated = [block_1.as_slice(), block_2.as_slice()].concat();
        let single_blob_root = compute_wal_root(&concatenated);
        // Note: `single_blob_root` != `root_1` and != `root_2` because
        // the count prefix differs (2 vs 3 entries).  This test pins
        // the correct semantic: the joining protocol replays entries
        // in order across snapshots; it does NOT concatenate bytes.
        assert_ne!(single_blob_root, root_1);
        assert_ne!(single_blob_root, root_2);
    }

    /// M-30b-6: `SnapshotWriter::maybe_write` cadence boundary at
    /// `2 * cadence` and `3 * cadence`.  Existing `cadence=5, block=5`
    /// test would pass a regression to `block % cadence == cadence - 1`;
    /// this test adds the extra boundaries.
    #[test]
    fn snapshot_writer_cadence_hits_multiples_of_cadence() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 5,
            retain: 100,
            signer_sk: None,
            payload_dir: None,
        };
        let entries = vec![mk_write_entry(b"x", "/x")];
        for bn in [5i64, 10, 15, 100, 1000] {
            let r = writer.maybe_write(bn, &entries).unwrap();
            assert!(
                r.is_some(),
                "block {bn} (multiple of 5) must be a cadence hit"
            );
        }
        for bn in [1i64, 4, 6, 9, 11, 999] {
            let r = writer.maybe_write(bn, &entries).unwrap();
            assert!(
                r.is_none(),
                "block {bn} (not multiple of 5) must be a cadence miss"
            );
        }
    }

    /// M-30b-6 companion: very-large block numbers hit cadence
    /// deterministically via `is_multiple_of`.
    #[test]
    fn snapshot_writer_handles_large_block_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1_000_000,
            retain: 3,
            signer_sk: None,
            payload_dir: None,
        };
        let entries = vec![mk_write_entry(b"x", "/x")];
        // A random-looking block number that's an exact multiple.
        let boundary = 42_000_000i64;
        assert_eq!(boundary % 1_000_000, 0);
        assert!(writer.maybe_write(boundary, &entries).unwrap().is_some());
        // One off the boundary should miss.
        assert!(writer
            .maybe_write(boundary + 1, &entries)
            .unwrap()
            .is_none());
    }

    /// M-30b-7: retain edge — `retain = 0` prunes everything after
    /// each write.  Pinned as behavioral spec (unusual but valid
    /// operator config for a "snapshot-only, no retention" mode).
    #[test]
    fn snapshot_writer_retain_zero_prunes_all_after_each_write() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1,
            retain: 0,
            signer_sk: None,
            payload_dir: None,
        };
        let entries_1 = vec![mk_write_entry(b"a", "/x")];
        let entries_2 = vec![mk_write_entry(b"b", "/y")];
        writer.maybe_write(1, &entries_1).unwrap();
        // Prune with retain=0 removes the just-written file.
        let count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "wal")
            })
            .count();
        assert_eq!(
            count, 0,
            "retain=0 must prune every snapshot immediately after write"
        );
        writer.maybe_write(2, &entries_2).unwrap();
        let count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "wal")
            })
            .count();
        assert_eq!(count, 0);
    }

    /// M-30b-7 companion: retain = usize::MAX means prune is a
    /// no-op (retention window exceeds anything the operator could
    /// realistically produce).
    #[test]
    fn snapshot_writer_retain_max_keeps_everything() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1,
            retain: usize::MAX,
            signer_sk: None,
            payload_dir: None,
        };
        for i in 1..=5u8 {
            let entries = vec![mk_write_entry(&[i], "/x")];
            writer.maybe_write(i as i64, &entries).unwrap();
        }
        let count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "wal")
            })
            .count();
        assert_eq!(count, 5, "retain=usize::MAX must never prune");
    }

    /// prune_snapshot_dir ignores non-`.wal` files (e.g. operator's
    /// README, backup files).
    #[test]
    fn prune_snapshot_dir_ignores_non_wal_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), b"operator notes").unwrap();
        std::fs::write(dir.path().join("backup.tar"), b"data").unwrap();
        let entries = vec![mk_write_entry(b"a", "/x")];
        write_snapshot(dir.path(), &entries).unwrap();
        assert_eq!(prune_snapshot_dir(dir.path(), 0).unwrap(), 1);
        // Non-WAL files survive.
        assert!(dir.path().join("README").exists());
        assert!(dir.path().join("backup.tar").exists());
    }

    /// Coverage: `read_snapshot_bytes` on non-existent file returns
    /// `SnapshotError::Io`.  Pins the error-mapping so a regression
    /// swallowing NotFound as an empty slice would fail.
    #[test]
    fn read_snapshot_nonexistent_file_returns_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let bogus_root = [0u8; 32];
        let err = read_snapshot_bytes(dir.path(), &bogus_root);
        assert!(
            matches!(err, Err(SnapshotError::Io(_))),
            "expected Io error for non-existent file, got {err:?}"
        );
    }

    #[test]
    fn deploy_ref_payload_is_encoded_distinctly_from_hash() {
        // Verify the DeployRef branch encodes differently from Hash
        // (even if the raw bytes overlap): the leading tag byte
        // discriminates.
        let e_hash = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: Some(0),
            payload_ref: Some(PayloadRef::Hash([0u8; 32])),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let e_ref = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: Some(0),
            payload_ref: Some(PayloadRef::DeployRef {
                block_hash: [0u8; 32],
                deploy_index: 0,
                arg_index: 0,
            }),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        assert_ne!(
            compute_wal_root(&[e_hash]),
            compute_wal_root(&[e_ref]),
            "Hash and DeployRef payload refs must produce different roots"
        );
    }

    // ------------------------------------------------------------------
    // Slice 34 (MED-1) tests: version byte + hard-fork catalog pins.
    // ------------------------------------------------------------------

    /// Every encoded slice begins with `SNAPSHOT_FORMAT_VERSION`
    /// (currently `1`).  Regression pin against a refactor that
    /// drops the version prefix.
    #[test]
    fn encoded_slice_starts_with_format_version_byte() {
        let bytes = encode_wal_slice(&[]);
        assert_eq!(
            bytes.first().copied(),
            Some(SNAPSHOT_FORMAT_VERSION),
            "encoded WAL slice must start with SNAPSHOT_FORMAT_VERSION"
        );
        let e = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let bytes = encode_wal_slice(&[e]);
        assert_eq!(bytes.first().copied(), Some(SNAPSHOT_FORMAT_VERSION));
    }

    /// `read_snapshot_bytes` accepts a well-formed v1 blob (the
    /// happy path).  Also documents by construction that
    /// `write_snapshot` + `read_snapshot_bytes` round-trip.
    #[test]
    fn read_snapshot_bytes_accepts_current_version() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: Some(3),
            payload_ref: Some(PayloadRef::hash(b"abc")),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }];
        let (_, root, _merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let bytes = read_snapshot_bytes(dir.path(), &root).expect("v1 round-trip");
        assert_eq!(bytes.first().copied(), Some(SNAPSHOT_FORMAT_VERSION));
    }

    /// A blob whose leading version byte is NOT
    /// `SNAPSHOT_FORMAT_VERSION` fails cleanly with
    /// `UnsupportedVersion` — the joining-validator use case where
    /// an older binary sees a snapshot from a newer, hard-forked
    /// network.
    #[test]
    fn read_snapshot_bytes_rejects_unknown_version() {
        let dir = tempfile::tempdir().unwrap();
        // Fabricate a blob with a bogus version byte.  Compute its
        // Blake2b256 so read_snapshot_bytes's hash check passes and
        // we exercise the version-check code path.
        let mut bogus = vec![99u8]; // version 99 — nonexistent
        bogus.extend_from_slice(&0u32.to_be_bytes()); // count = 0
        let root = hash_of(&bogus);
        let path = snapshot_path(dir.path(), &root);
        std::fs::write(&path, &bogus).unwrap();

        match read_snapshot_bytes(dir.path(), &root) {
            Err(SnapshotError::UnsupportedVersion { got, supported }) => {
                assert_eq!(got, 99);
                assert_eq!(supported, SNAPSHOT_FORMAT_VERSION);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    /// A completely empty blob (zero bytes) surfaces `Truncated`
    /// rather than mis-decoding.  The hash check passes vacuously
    /// (hash of empty is well-defined), so the version-length
    /// guard is what catches this.
    #[test]
    fn read_snapshot_bytes_rejects_truncated_blob() {
        let dir = tempfile::tempdir().unwrap();
        let empty: Vec<u8> = Vec::new();
        let root = hash_of(&empty);
        let path = snapshot_path(dir.path(), &root);
        std::fs::write(&path, &empty).unwrap();

        match read_snapshot_bytes(dir.path(), &root) {
            Err(SnapshotError::Truncated { got, need }) => {
                assert_eq!(got, 0);
                assert_eq!(need, 1);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    /// The hard-fork surface catalog in the module docstring must
    /// enumerate all consensus-critical encoding decisions.  This
    /// test scans the source for the 8 catalog headings — a
    /// regression that removes an item from the docstring (or
    /// silently adds a new consensus-observable field without
    /// listing it) surfaces here.  The catalog is a MAINTAINER
    /// contract: additions to any of these surfaces are hard forks.
    #[test]
    fn hard_fork_surface_catalog_is_pinned() {
        let src = include_str!("snapshot.rs");
        // The section header itself.
        assert!(
            src.contains("# Hard-fork surface catalog"),
            "module docstring must contain the catalog section header"
        );
        // Each of the 8 catalog items — labelled by their leading
        // number in the numbered list.  The test bodies check for
        // the distinctive keyword too, so a rename of the item
        // (e.g. "Format version byte" → "Version prefix") that
        // preserves the numbering still fails here — forcing the
        // maintainer to think about whether the change is a
        // consensus-neutral wording tweak or a real semantics
        // change that needs a version bump.
        let items = [
            ("1.", "Format version byte"),
            ("2.", "Op tag bytes"),
            ("3.", "Hash function = Blake2b256"),
            ("4.", "Length-prefix widths"),
            ("5.", "Field widths / endianness"),
            ("6.", "`PayloadRef` variant tags"),
            ("7.", "Field order inside `encode_entry`"),
            ("8.", "Path encoding"),
            ("9.", "Outcome encoding"),
            ("10.", "Manifest wire format"),
            ("11.", "WAL entry cap"),
        ];
        for (num, keyword) in items {
            let needle = format!("// {num} **{keyword}");
            assert!(
                src.contains(&needle),
                "catalog item missing or renamed: `{needle}` — if you're \
                 adding a new consensus-critical surface, extend both the \
                 docstring AND this test's `items` array"
            );
        }
    }

    // ------------------------------------------------------------------
    // Slice 30c Phase C: manifest / join-protocol substrate tests.
    // ------------------------------------------------------------------

    #[test]
    fn manifest_entry_data_round_trips_through_line_format() {
        let root = [0xABu8; 32];
        let e = ManifestEntry {
            block_number: 12345,
            root: Some(root),
            entries: 42,
            ts_ms: 1_722_400_000_000,
            sig: None,
        };
        let line = e.to_line();
        // Sanity: line must be single-line JSON (no interior newlines).
        assert!(!line.contains('\n'));
        assert!(line.starts_with('{') && line.ends_with('}'));
        let parsed = ManifestEntry::from_line(&line).expect("round-trip");
        assert_eq!(parsed, e);
    }

    #[test]
    fn manifest_entry_empty_sentinel_round_trips() {
        let e = ManifestEntry::empty(999);
        let line = e.to_line();
        assert!(
            line.contains("\"root\":null"),
            "empty entry must serialize root as null; got {line}"
        );
        let parsed = ManifestEntry::from_line(&line).expect("round-trip");
        assert_eq!(parsed, e);
        assert!(parsed.root.is_none());
        assert_eq!(parsed.entries, 0);
    }

    // -------------------- H-4 regression pins (2026-08-06) --------------------

    /// Deterministic secp256k1 keypair for tests.  Not a real
    /// validator key — just any 32-byte scalar that yields a valid
    /// key pair.  Copied-pattern from `standard_deploys::FS_GENERATOR_PK`
    /// (a checked-in test-shard-signing key with the same
    /// "deterministic constant" role).
    fn test_keypair() -> (Vec<u8>, Vec<u8>) {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        // Some deterministic 32-byte value.  0x11..0x30.
        let sk: Vec<u8> = (0x11u8..0x11 + 32).collect();
        let pk_bytes = Secp256k1
            .to_public(&crypto::rust::private_key::PrivateKey::from_bytes(&sk))
            .bytes;
        (sk, pk_bytes.to_vec())
    }

    /// H-4 signing round-trip: an entry signed with sk verifies with
    /// the matching pk; the sig field is populated after `signed()`.
    #[test]
    fn manifest_entry_signed_round_trips_and_verifies() {
        let (sk, pk) = test_keypair();
        let raw = ManifestEntry::data(100, [0x55u8; 32], 7);
        let signed = raw.clone().signed(&sk);
        assert!(signed.sig.is_some(), "signed() must populate the sig field");
        // Verify with matching pk succeeds.
        signed
            .verify_with_pubkey(&pk)
            .expect("valid sig must verify");
        // Verify with a wrong pk fails.
        let (_, wrong_pk) = test_keypair_wrong();
        assert!(
            signed.verify_with_pubkey(&wrong_pk).is_err(),
            "wrong pubkey must reject the signature"
        );
        // Serialize + parse preserves the sig.
        let line = signed.to_line();
        assert!(
            line.contains("\"sig\":\""),
            "signed line must include sig field; got {line}"
        );
        let parsed = ManifestEntry::from_line(&line).expect("round-trip signed line");
        assert_eq!(parsed.sig, signed.sig);
        parsed.verify_with_pubkey(&pk).expect("post-parse verify");
    }

    /// H-4: mutation of any signed field invalidates the signature.
    /// If an attacker rewrites the manifest to point at a bogus
    /// root, the sig no longer matches.
    #[test]
    fn manifest_entry_signature_binds_to_root_field() {
        let (sk, pk) = test_keypair();
        let signed = ManifestEntry::data(50, [0x11u8; 32], 3).signed(&sk);
        // Attacker mutates the root.
        let mut tampered = signed.clone();
        tampered.root = Some([0x99u8; 32]);
        assert!(
            tampered.verify_with_pubkey(&pk).is_err(),
            "signature must reject a root-tampered entry"
        );
    }

    /// H-4: an unsigned entry (pre-H-4 backward-compat wire format)
    /// returns an error on verify — the joining protocol must
    /// treat unsigned entries as untrusted.
    #[test]
    fn manifest_entry_unsigned_verify_returns_err() {
        let (_, pk) = test_keypair();
        let unsigned = ManifestEntry::data(1, [0u8; 32], 0);
        assert!(
            unsigned.verify_with_pubkey(&pk).is_err(),
            "unsigned entries must not verify"
        );
    }

    /// A second deterministic keypair for negative-verify tests.
    fn test_keypair_wrong() -> (Vec<u8>, Vec<u8>) {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        let sk: Vec<u8> = (0x31u8..0x31 + 32).collect();
        let pk_bytes = Secp256k1
            .to_public(&crypto::rust::private_key::PrivateKey::from_bytes(&sk))
            .bytes;
        (sk, pk_bytes.to_vec())
    }

    /// End-to-end: SnapshotWriter with `signer_sk = Some(sk)` writes
    /// a manifest line whose sig verifies with the matching pk.
    #[test]
    fn snapshot_writer_with_signer_produces_verifiable_manifest() {
        let (sk, pk) = test_keypair();
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1,
            retain: 10,
            signer_sk: Some(sk),
            payload_dir: None,
        };
        let entries = vec![mk_write_entry(b"payload", "/x")];
        writer
            .maybe_write(5, &entries)
            .expect("write")
            .expect("cadence hit yields root");
        let manifest = read_manifest(dir.path()).unwrap();
        assert_eq!(manifest.len(), 1);
        manifest[0]
            .verify_with_pubkey(&pk)
            .expect("SnapshotWriter-signed line must verify");
    }

    #[test]
    fn append_and_read_manifest_preserves_order() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![
            ManifestEntry::data(100, [0x11; 32], 5),
            ManifestEntry::empty(200),
            ManifestEntry::data(300, [0x33; 32], 7),
        ];
        for e in &entries {
            append_manifest_entry(dir.path(), e.clone()).unwrap();
        }
        let read = read_manifest(dir.path()).unwrap();
        assert_eq!(read.len(), 3);
        for (i, (r, w)) in read.iter().zip(entries.iter()).enumerate() {
            assert_eq!(
                r, w,
                "manifest entry {i} order mismatch: read={r:?}, wrote={w:?}"
            );
        }
    }

    #[test]
    fn read_manifest_returns_empty_when_file_absent() {
        let dir = tempfile::tempdir().unwrap();
        let read = read_manifest(dir.path()).unwrap();
        assert!(
            read.is_empty(),
            "absent manifest is not an error — empty is the valid partial view for a joining validator"
        );
    }

    #[test]
    fn read_manifest_surfaces_corrupt_line_as_error() {
        let dir = tempfile::tempdir().unwrap();
        append_manifest_entry(dir.path(), ManifestEntry::data(100, [0x11; 32], 5)).unwrap();
        // Corrupt the manifest by appending a bogus line.
        use std::io::Write;
        let path = dir.path().join(MANIFEST_FILENAME);
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(b"this is not JSON\n").unwrap();
        drop(f);
        let err = read_manifest(dir.path()).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("manifest line 2"),
            "error must identify the corrupt line number; got {msg}"
        );
    }

    /// F-30b-4 (slice 30c Phase C fix): `SnapshotWriter.maybe_write`
    /// appends an "empty" sentinel manifest entry on a cadence-hit
    /// block with zero WAL entries.  Pre-30c this was a silent
    /// skip — joiners couldn't distinguish "block had no fs
    /// mutations" from "cadence miss."  Now the sentinel makes the
    /// former explicit.
    #[test]
    fn maybe_write_empty_slice_appends_sentinel_manifest_entry() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 10,
            retain: 4,
            signer_sk: None,
            payload_dir: None,
        };
        // Block 20: cadence hit (20 % 10 == 0) with zero entries.
        let res = writer.maybe_write(20, &[]).unwrap();
        assert!(res.is_none(), "empty slice returns None (no snapshot file)");
        let manifest = read_manifest(dir.path()).unwrap();
        assert_eq!(manifest.len(), 1, "empty sentinel must land in manifest");
        assert_eq!(manifest[0].block_number, 20);
        assert!(manifest[0].root.is_none(), "sentinel entry: root = null");
        assert_eq!(manifest[0].entries, 0);
    }

    /// Data slice writes both the snapshot file AND a manifest
    /// entry with the concrete root.  A joiner reading the
    /// manifest can then fetch the snapshot bytes by the root
    /// hex via `read_snapshot_bytes`.
    #[test]
    fn maybe_write_data_slice_appends_data_manifest_entry() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 5,
            retain: 4,
            signer_sk: None,
            payload_dir: None,
        };
        let entries = vec![WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: Some(3),
            payload_ref: Some(PayloadRef::hash(b"abc")),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }];
        let (root, _merkle) = writer.maybe_write(5, &entries).unwrap().unwrap();
        let manifest = read_manifest(dir.path()).unwrap();
        assert_eq!(manifest.len(), 1);
        assert_eq!(manifest[0].block_number, 5);
        assert_eq!(manifest[0].root, Some(root));
        assert_eq!(manifest[0].entries, 1);
        // The manifest root points at a real snapshot file the
        // joiner can fetch.
        let bytes = read_snapshot_bytes(dir.path(), &root).unwrap();
        assert!(!bytes.is_empty());
    }

    /// Cadence-miss blocks must NOT touch the manifest.  Manifest
    /// is only for cadence-hit blocks (data + empty sentinel);
    /// cadence-miss blocks contribute nothing to the join-protocol
    /// view.
    #[test]
    fn maybe_write_cadence_miss_does_not_touch_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 100,
            retain: 4,
            signer_sk: None,
            payload_dir: None,
        };
        // Block 3 is a cadence miss (3 % 100 != 0).
        let res = writer.maybe_write(3, &[]).unwrap();
        assert!(res.is_none());
        let manifest = read_manifest(dir.path()).unwrap();
        assert!(
            manifest.is_empty(),
            "cadence miss must not contribute to manifest"
        );
    }

    #[test]
    fn manifest_entry_from_line_rejects_wrong_root_hex_length() {
        // 63 chars (not 64) — a truncated root that would silently
        // hash-mismatch if accepted.
        let bogus = "{\"block_number\":1,\"root\":\"aa\",\"entries\":1,\"ts_ms\":1}";
        let err = ManifestEntry::from_line(bogus).unwrap_err();
        assert!(
            err.contains("64 hex chars") || err.contains("64 chars"),
            "must reject wrong-length root hex; got {err}"
        );
    }

    #[test]
    fn manifest_entry_from_line_rejects_missing_field() {
        // Missing ts_ms.  Include `v` so the earlier version-check
        // arm doesn't short-circuit before we reach the ts_ms check.
        let bogus = "{\"v\":1,\"block_number\":1,\"root\":null,\"entries\":0}";
        let err = ManifestEntry::from_line(bogus).unwrap_err();
        assert!(err.contains("missing ts_ms"), "got {err}");
    }

    // ------------------------------------------------------------------
    // M-1 fix (2026-08-06) manifest wire-format pins.  Item #10 of the
    // hard-fork surface catalog.
    // ------------------------------------------------------------------

    /// M-1 pin: the current-version manifest line is byte-exact.
    /// A regression that reorders fields, changes quoting, or
    /// omits the `v` prefix flips this literal.
    #[test]
    fn manifest_line_format_is_pinned_at_v1() {
        let e = ManifestEntry {
            block_number: 42,
            root: Some([0xABu8; 32]),
            entries: 7,
            ts_ms: 1_722_400_000_000,
            sig: None,
        };
        let line = e.to_line();
        let expected = "{\"v\":1,\"block_number\":42,\"root\":\"\
            abababababababababababababababababababababababababababababababab\",\
            \"entries\":7,\"ts_ms\":1722400000000}";
        assert_eq!(
            line, expected,
            "M-1: manifest line format is a hard-fork surface (catalog #10); \
             a regression here means bumping MANIFEST_FORMAT_VERSION and \
             coordinating a network upgrade"
        );
    }

    /// M-1 pin: `sign_bytes` layout is byte-exact.  Signature
    /// verification only works cross-node if writers + verifiers
    /// agree on this canonicalization.
    #[test]
    fn manifest_sign_bytes_layout_is_pinned() {
        // Data entry: version | i64-be block_number | 1 |
        // 32-byte root | u64-be entries | i64-be ts_ms.
        let e = ManifestEntry {
            block_number: 0x0102_0304_0506_0708_i64,
            root: Some([0xCDu8; 32]),
            entries: 0x0000_0000_0000_0009_u64,
            ts_ms: 0x000A_0B0C_0D0E_0F10_i64,
            sig: None,
        };
        // The blake2b hash output is fixed (32 bytes), so pin the
        // pre-hash bytes assembled by sign_bytes.  Reproduce by
        // manually laying out the specified format.
        let mut expected_prehash = Vec::new();
        expected_prehash.push(MANIFEST_FORMAT_VERSION);
        expected_prehash.extend_from_slice(&e.block_number.to_be_bytes());
        expected_prehash.push(1); // root present
        expected_prehash.extend_from_slice(&[0xCDu8; 32]);
        expected_prehash.extend_from_slice(&e.entries.to_be_bytes());
        expected_prehash.extend_from_slice(&e.ts_ms.to_be_bytes());
        let expected_hash: Vec<u8> =
            crypto::rust::hash::blake2b256::Blake2b256::hash(expected_prehash);
        assert_eq!(
            e.sign_bytes(),
            expected_hash,
            "M-1: sign_bytes canonicalization is a hard-fork surface; changing \
             field order, endianness, or the version byte breaks cross-node sig \
             verification"
        );

        // Empty sentinel: root absent → tag byte 0, no root bytes.
        let empty = ManifestEntry {
            block_number: 5,
            root: None,
            entries: 0,
            ts_ms: 12345,
            sig: None,
        };
        let mut expected_prehash_empty = Vec::new();
        expected_prehash_empty.push(MANIFEST_FORMAT_VERSION);
        expected_prehash_empty.extend_from_slice(&empty.block_number.to_be_bytes());
        expected_prehash_empty.push(0); // root absent
        expected_prehash_empty.extend_from_slice(&empty.entries.to_be_bytes());
        expected_prehash_empty.extend_from_slice(&empty.ts_ms.to_be_bytes());
        let expected_hash_empty: Vec<u8> =
            crypto::rust::hash::blake2b256::Blake2b256::hash(expected_prehash_empty);
        assert_eq!(empty.sign_bytes(), expected_hash_empty);
    }

    /// M-1 pin: an unknown `v` value gets rejected explicitly.
    /// Protects against silent mis-decoding of a future v2 line
    /// by a v1-only validator.
    #[test]
    fn from_line_rejects_unknown_manifest_version() {
        let future = "{\"v\":99,\"block_number\":1,\"root\":null,\"entries\":0,\"ts_ms\":1}";
        let err = ManifestEntry::from_line(future).unwrap_err();
        assert!(
            err.contains("unsupported manifest version 99"),
            "must reject unknown version explicitly; got {err}"
        );
    }

    /// M-1 pin: a pre-M-1 line (no `v` field) gets rejected.
    /// A validator upgraded past the M-1 fix MUST NOT silently
    /// decode a pre-fix manifest as if it were v1 — that would
    /// mask an operator's stale directory.
    #[test]
    fn from_line_rejects_missing_manifest_version() {
        let pre_m1 = "{\"block_number\":1,\"root\":null,\"entries\":0,\"ts_ms\":1}";
        let err = ManifestEntry::from_line(pre_m1).unwrap_err();
        assert!(
            err.contains("missing `v`"),
            "must reject unversioned line explicitly; got {err}"
        );
    }

    // ---------------------------------------------------------------
    // Phase 7b-2 retention (DD-7b-1 (y), 2026-08-27) — sidecar tests.
    // ---------------------------------------------------------------

    /// `referenced_payload_hashes` extracts unique Hash refs and
    /// skips None + DeployRef variants.  DeployRef is not currently
    /// emitted by any handler but must be gracefully ignored here
    /// so future write-payload-determinism reducer work doesn't
    /// accidentally leak DeployRef bytes into the payload store.
    #[test]
    fn referenced_payload_hashes_extracts_only_hash_variants() {
        let bytes_a = b"aaa".to_vec();
        let bytes_b = b"bbb".to_vec();
        let hash_a = hash_of(&bytes_a);
        let hash_b = hash_of(&bytes_b);
        let entries = vec![
            WalEntry {
                op: WalOp::Write,
                path: PathBuf::from("/a"),
                extra_path: None,
                offset: Some(0),
                length: Some(3),
                payload_ref: Some(PayloadRef::Hash(hash_a)),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            WalEntry {
                op: WalOp::Chmod,
                path: PathBuf::from("/b"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: Some(0o600),
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            WalEntry {
                op: WalOp::WriteAt,
                path: PathBuf::from("/c"),
                extra_path: None,
                offset: Some(0),
                length: Some(3),
                payload_ref: Some(PayloadRef::Hash(hash_b)),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            // DeployRef must be skipped.
            WalEntry {
                op: WalOp::Write,
                path: PathBuf::from("/d"),
                extra_path: None,
                offset: Some(0),
                length: Some(1),
                payload_ref: Some(PayloadRef::DeployRef {
                    block_hash: [0u8; 32],
                    deploy_index: 0,
                    arg_index: 0,
                }),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            // Duplicate of hash_a — still one entry in the set.
            WalEntry {
                op: WalOp::Write,
                path: PathBuf::from("/e"),
                extra_path: None,
                offset: Some(0),
                length: Some(3),
                payload_ref: Some(PayloadRef::Hash(hash_a)),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
        ];
        let set = referenced_payload_hashes(&entries);
        assert_eq!(set.len(), 2);
        assert!(set.contains(&hash_a));
        assert!(set.contains(&hash_b));
    }

    /// Hashes sidecar round-trips: write → read produces the
    /// same set.  Uses sorted-write order internally so the bytes
    /// are deterministic across identical hash sets.
    #[test]
    fn hashes_sidecar_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("root.hashes");
        let mut set = std::collections::HashSet::new();
        set.insert([0x11u8; 32]);
        set.insert([0x22u8; 32]);
        set.insert([0x33u8; 32]);
        write_hashes_sidecar(&path, &set).unwrap();
        let round = read_hashes_sidecar(&path).unwrap();
        assert_eq!(round, set);
    }

    /// Corrupt sidecars (wrong header count, truncated body) are
    /// treated as empty rather than propagating an error — a
    /// corrupt sidecar just means the corresponding snapshot's
    /// hashes don't count toward retention (over-eager prune on
    /// the next pass, which is safe).
    #[test]
    fn hashes_sidecar_read_treats_corrupt_bytes_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        // Header claims 5 hashes, body has 0.
        let path = dir.path().join("bad.hashes");
        std::fs::write(&path, [0, 0, 0, 5]).unwrap();
        let set = read_hashes_sidecar(&path).unwrap();
        assert!(set.is_empty());
        // Header claims 1 hash, body has 16 bytes (too short).
        let path2 = dir.path().join("short.hashes");
        let mut buf = vec![0, 0, 0, 1];
        buf.extend_from_slice(&[0u8; 16]);
        std::fs::write(&path2, &buf).unwrap();
        let set2 = read_hashes_sidecar(&path2).unwrap();
        assert!(set2.is_empty());
    }

    /// `write_snapshot` populates a `.hashes` sidecar next to the
    /// `.wal` file whose contents union to the entry set.
    #[test]
    fn write_snapshot_creates_hashes_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"snapshot-sidecar".to_vec();
        let entries = vec![mk_write_entry(&payload, "/f")];
        let (final_path, root, _) = write_snapshot(dir.path(), &entries).unwrap();
        assert!(final_path.exists());
        let sidecar = hashes_sidecar_path(dir.path(), &root);
        assert!(sidecar.exists(), "hashes sidecar must be written");
        let set = read_hashes_sidecar(&sidecar).unwrap();
        assert_eq!(set.len(), 1);
        assert!(set.contains(&hash_of(&payload)));
    }

    /// `scan_retained_payload_hashes` unions across all `.hashes`
    /// sidecars in the dir and skips corrupt/missing ones.
    #[test]
    fn scan_retained_payload_hashes_unions_across_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let entries_a = vec![mk_write_entry(b"one", "/a")];
        let entries_b = vec![mk_write_entry(b"two", "/b")];
        write_snapshot(dir.path(), &entries_a).unwrap();
        write_snapshot(dir.path(), &entries_b).unwrap();
        // A stray non-.hashes file — must be ignored.
        std::fs::write(dir.path().join("README"), b"docs").unwrap();
        let union = scan_retained_payload_hashes(dir.path()).unwrap();
        assert_eq!(union.len(), 2);
        assert!(union.contains(&hash_of(b"one")));
        assert!(union.contains(&hash_of(b"two")));
    }

    /// `prune_snapshot_dir` removes `.hashes` sidecars alongside
    /// the `.wal` files it prunes.  Without this, sidecars pile
    /// up forever and inflate the retention set → payload store
    /// retention becomes a no-op.
    #[tokio::test]
    async fn prune_snapshot_dir_also_removes_hashes_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1,
            retain: 2,
            signer_sk: None,
            payload_dir: None,
        };
        // Write 3 snapshots with distinct payloads; retain=2 means
        // the oldest gets pruned.  Each maybe_write on a distinct
        // slice produces a distinct root + a distinct sidecar.
        // Sleep 20ms between writes so mtimes differ enough for
        // prune's newest-first sort.
        let mut roots: Vec<[u8; 32]> = Vec::new();
        for i in 0..3 {
            let entries = vec![mk_write_entry(format!("payload-{i}").as_bytes(), "/x")];
            let (root, _merkle_root) = writer.maybe_write(i, &entries).unwrap().unwrap();
            roots.push(root);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        // After 3 writes with retain=2, the oldest snapshot AND
        // its sidecar are gone.
        let oldest_wal = snapshot_path(dir.path(), &roots[0]);
        let oldest_sidecar = hashes_sidecar_path(dir.path(), &roots[0]);
        assert!(
            !oldest_wal.exists(),
            "oldest .wal must be pruned by prune_snapshot_dir"
        );
        assert!(
            !oldest_sidecar.exists(),
            "oldest .hashes sidecar must be pruned alongside .wal"
        );
        // Newest two survived.
        assert!(snapshot_path(dir.path(), &roots[1]).exists());
        assert!(hashes_sidecar_path(dir.path(), &roots[1]).exists());
        assert!(snapshot_path(dir.path(), &roots[2]).exists());
        assert!(hashes_sidecar_path(dir.path(), &roots[2]).exists());
    }
}
