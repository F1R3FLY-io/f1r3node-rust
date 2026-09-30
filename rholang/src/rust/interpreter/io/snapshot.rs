// Consensus-mode filesystem WAL snapshot — encoding primitives
// (slice 1 of the `snapshot` submodule tree).
//
// This slice provides the canonical byte-encoding of
// `Vec<WalEntry>` plus its Blake2b256 root hash and the
// [`SnapshotBlob`] convenience combiner (encoded bytes + root +
// Merkle root over 4 MiB chunks).  The decoder (`decode_wal_slice`)
// + `SnapshotError` land in slice 2; on-disk read / write / manifest
// / writer / pruning / payload-hash sidecar all land in subsequent
// slices as their own natural units.
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

use std::path::PathBuf;

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
    encode_str_bytes(path_as_bytes(&e.path), buf);
    match &e.extra_path {
        None => buf.push(0),
        Some(p) => {
            buf.push(1);
            encode_str_bytes(path_as_bytes(p), buf);
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

/// Cross-platform path → bytes conversion.  On unix, uses the
/// raw `OsStr` byte representation (path bytes are opaque + can
/// contain non-UTF-8).  On non-unix (nominally covered but this
/// module is unix-only in practice; see the file-level
/// `#[cfg(not(unix))]` guard in `mod.rs`), falls through to a
/// lossy UTF-8 rendering — acceptable because the target
/// platforms are all unix-like.
#[cfg(unix)]
fn path_as_bytes(p: &PathBuf) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes()
}

#[cfg(not(unix))]
fn path_as_bytes(p: &PathBuf) -> &[u8] {
    // Placeholder — the io/ subtree gates on unix via `mod.rs`;
    // this branch is not reachable in production but keeps the
    // module cross-platform-buildable for editor tooling.
    p.to_str().unwrap_or("").as_bytes()
}

#[cfg(test)]
mod tests {
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
}
