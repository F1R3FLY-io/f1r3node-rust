// Consensus-mode Write-Ahead Log (slice 29, PB-M-14).
//
// The WAL journals every mutating filesystem operation performed on
// a `Consensus`-cmode cap.  It is the durability substrate for the
// consensus-replay guarantee: joining validators (or nodes recovering
// from crash) can reconstruct the filesystem state of any
// `consensus-static-*` bucket by replaying the WAL against a known
// snapshot (see slice 30).
//
// # Scope of this slice (MVP)
//
// Slice 29 adds the in-memory WAL data structure and the append hooks
// in FD-based write handlers (`fs_write`, `fs_write_at`,
// `fs_truncate`).  Path-based mutations (`fs_chmod`, `fs_chown`,
// `fs_removeFile`, `fs_removeDir`, `fs_rename`, `fs_copyFile`) will
// be wired in a follow-up slice — those handlers need their
// signatures extended to accept the caller's cmode, mirroring slice
// 26's threading through `fs_chown`.  Snapshotting + persistence +
// on-chain commitment of the WAL Merkle root are slice 30.
//
// # Payload references
//
// Per the plan §369 (2026-08-03 note): WAL rows carry a
// cryptographic hash of the bytes, not the bytes themselves — except
// deploy-derived writes, which reference the block position where
// the payload can be retrieved.
//
// This MVP uses `PayloadRef::Hash([u8; 32])` (Blake2b256) for every
// write.  The `DeployRef` optimization (block-hash + deploy-index +
// arg-index) requires plumbing the deploy context down through
// `fs_write`, which is a bigger change deferred to a future slice.
//
// # Determinism
//
// The WAL buffer is a per-runtime `Arc<Mutex<Vec<WalEntry>>>`.
// Every validator processing the same deploys observes identical
// hash-of-payload values (Blake2b256 is cryptographic and
// deterministic).  Append order is fixed by the deploy execution
// order which is itself deterministic (per Rholang small-step
// semantics + is_replay cache).  So the WAL is byte-identical across
// validators after processing the same block sequence.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b256::Blake2b256;

/// H-29-2 review-fix: per-runtime cap on WAL entries.  Prevents an
/// adversarial deploy from growing the WAL without bound.
/// Enforced in `Wal::append` which returns `Err(())` on overflow,
/// translated to `FSERR_QUOTA_EXCEEDED` by upstream handlers.  Set
/// to 65_536 as a rough analog to `MAX_OPEN_FDS = 1024` scaled up
/// for the higher-throughput write-op vs. long-lived-handle
/// distinction; final calibration is a Cost FIP concern.
pub const MAX_WAL_ENTRIES: usize = 65_536;

/// Opaque marker returned by `Wal::begin_deploy` and consumed by
/// `Wal::take_deploy_entries`.  Records the WAL length at the deploy
/// boundary so post-deploy drain covers exactly the entries this
/// deploy contributed.  Also usable by soft-checkpoint machinery
/// (H-29-1 review fix) as a snapshot to truncate back to on revert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalMark {
    len: usize,
}

/// One journaled mutation.  Ordered by insertion into the WAL — the
/// replay protocol applies entries in insertion order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalEntry {
    pub op: WalOp,
    /// Canonical host path of the target file (from
    /// `FileHandle::canon_path` for fd-based ops, or from the
    /// `canonRoot + rel` join for path-based ops when those are
    /// wired in a follow-up slice).
    pub path: PathBuf,
    /// Additional target path — only populated for `Rename` and
    /// `CopyFile` (the destination).
    pub extra_path: Option<PathBuf>,
    /// Offset for `WriteAt` and `Truncate`; `None` otherwise.
    pub offset: Option<u64>,
    /// Byte length for `Write` / `WriteAt` (total bytes written to
    /// the target); `None` for non-write ops.
    pub length: Option<u64>,
    /// Reference to the write payload.  `None` for non-write ops
    /// (chmod / chown / rename / remove / copy carry their args
    /// inline via other fields — payload_ref is specifically for
    /// byte content of a write).
    pub payload_ref: Option<PayloadRef>,
    /// Mode bits for `Chmod`; `None` otherwise.
    pub mode_bits: Option<u32>,
    /// Owner + group strings for `Chown`; empty otherwise.  Kept as
    /// String not uid/gid because chown at the syscall boundary
    /// accepts names (resolved via NSS on the host).  Replay is
    /// operator-responsible: they need matching NSS on every
    /// replaying node.
    pub owner: Option<String>,
    pub group: Option<String>,
}

/// Enumeration of consensus-observable filesystem operations
/// captured in the WAL.  Named "WalOp" for historical reasons but
/// now covers both mutations AND observation-preserving reads whose
/// results feed the tuplespace (slice 32, PB-M-14 read-hash).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WalOp {
    /// `fs_write(fd, bytes)` — sequential write; offset is None,
    /// length = bytes.len(), payload_ref = Hash(blake2b(bytes)).
    Write,
    /// `fs_write_at(fd, off, bytes)` — positional write;
    /// offset = off, length = bytes.len(), payload_ref = Hash(...).
    WriteAt,
    /// `fs_truncate(fd, n)` — file length becomes n; offset = n,
    /// length = None, payload_ref = None.
    Truncate,
    // The following variants are RESERVED for the follow-up slice
    // that wires path-based mutations; kept in the enum so the
    // WAL replay protocol can distinguish op types without a
    // discriminant clash.
    Chmod,
    Chown,
    RemoveFile,
    RemoveDir,
    Rename,
    CopyFile,
    /// `fs_read(fd, n) -> bytes` — sequential read.  Slice 32
    /// (PB-M-14 read-hash): records `Hash(returned_bytes)` so a
    /// joining validator can verify that a byte-identical read
    /// against reconstructed state produces the same hash.
    /// offset is None; length = returned_bytes.len();
    /// payload_ref = Hash(blake2b(returned_bytes)).
    Read,
    /// `fs_read_at(fd, off, n) -> bytes` — positional read.
    /// offset = off; length = returned_bytes.len();
    /// payload_ref = Hash(...).  See Read.
    ReadAt,
}

/// Reference to write payload bytes.  The MVP uses `Hash` only; the
/// `DeployRef` optimization lands with the block-context plumbing
/// slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadRef {
    /// Blake2b256 hash of the payload bytes.  Recipients of the WAL
    /// look up the actual bytes via the separate byte-payload
    /// distribution sub-protocol (also slice 30's concern).
    Hash([u8; 32]),
    /// Reference into a deploy from a specific block position:
    /// `(block_hash, deploy_index, arg_index)`.  Followers can
    /// reconstruct the payload directly from the on-chain deploy
    /// data, avoiding the byte-payload sub-protocol.
    ///
    /// Not yet emitted by this MVP; kept as a variant so the WAL
    /// consumer can pattern-match forward-compatibly.
    #[allow(dead_code)]
    DeployRef {
        block_hash: [u8; 32],
        deploy_index: u32,
        arg_index: u32,
    },
}

impl PayloadRef {
    /// Hash-only convenience constructor.
    pub fn hash(bytes: &[u8]) -> Self {
        let h = Blake2b256::hash(bytes.to_vec());
        // Blake2b256 returns Vec<u8> of length 32 in this codebase;
        // panic if it doesn't (indicates upstream misconfiguration).
        let mut buf = [0u8; 32];
        assert_eq!(
            h.len(),
            32,
            "Blake2b256 must produce 32-byte digest; got {}",
            h.len()
        );
        buf.copy_from_slice(&h);
        PayloadRef::Hash(buf)
    }
}

/// Per-runtime append-only WAL buffer.  Cloneable (shares the
/// underlying `Arc<Mutex<Vec<...>>>` via reference-counting) so every
/// handler closure can journal into the same list.
#[derive(Clone, Debug, Default)]
pub struct Wal {
    entries: Arc<Mutex<Vec<WalEntry>>>,
}

impl Wal {
    pub fn new() -> Self { Self::default() }

    /// Append one entry to the log.  H-29-2 review fix: returns
    /// `Err(())` if the log would exceed `MAX_WAL_ENTRIES` —
    /// callers translate to `FSERR_QUOTA_EXCEEDED`.  Unit error is
    /// deliberate: the only failure mode is cap-exceeded and all
    /// call sites map it to the same code.
    #[allow(clippy::result_unit_err)]
    pub fn append(&self, entry: WalEntry) -> Result<(), ()> {
        let mut guard = self.entries.lock().expect("Wal mutex poisoned");
        if guard.len() >= MAX_WAL_ENTRIES {
            return Err(());
        }
        guard.push(entry);
        Ok(())
    }

    /// Snapshot the current entries.  Cheap — returns a Vec clone.
    /// Intended for tests + slice-30 snapshot/checkpoint machinery.
    pub fn snapshot(&self) -> Vec<WalEntry> {
        let guard = self.entries.lock().expect("Wal mutex poisoned");
        guard.clone()
    }

    /// Number of journaled entries.
    pub fn len(&self) -> usize { self.entries.lock().expect("Wal mutex poisoned").len() }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// Clear all entries.  Called on `RhoRuntimeImpl::reset()`
    /// (H-29-F2 review fix — defense in depth) and by tests.
    pub fn clear(&self) { self.entries.lock().expect("Wal mutex poisoned").clear(); }

    /// H-29-1 review fix: rollback support for soft-checkpoints.
    /// Records the current length so a subsequent `truncate_to`
    /// can discard any entries appended past it (i.e., during a
    /// deploy attempt that gets reverted).
    pub fn snapshot_mark(&self) -> WalMark {
        let guard = self.entries.lock().expect("Wal mutex poisoned");
        WalMark { len: guard.len() }
    }

    /// H-29-1 review fix: truncate entries appended after the
    /// mark.  Called from `revert_to_soft_checkpoint` alongside
    /// `FileHandleTable::truncate_to`.  Monotonicity: a `mark`
    /// past the current length is a no-op (an oversized snapshot
    /// captured pre-clear-and-repopulate).
    pub fn truncate_to(&self, mark: WalMark) {
        let mut guard = self.entries.lock().expect("Wal mutex poisoned");
        if mark.len < guard.len() {
            guard.truncate(mark.len);
        }
    }

    /// Redesign: per-deploy boundary marker.  Called at the top of
    /// `play_deploy_with_cost_accounting` before the deploy runs.
    /// Paired with `take_deploy_entries` which returns exactly the
    /// entries this deploy contributed.  The pair lets slice 30
    /// attach a deploy's WAL contributions to its `ProcessedDeploy`
    /// (either via a proto-schema extension or via an out-of-band
    /// side-map keyed by deploy signature).
    pub fn begin_deploy(&self) -> WalMark { self.snapshot_mark() }

    /// Redesign: drain entries appended after `mark`.  Returns
    /// them (in insertion order) AND removes them from the WAL,
    /// so the underlying buffer stays bounded across deploys.
    /// Symmetric with `begin_deploy`.  If a caller wants to peek
    /// without draining, use `snapshot()` + `snapshot_mark()`
    /// arithmetic.
    pub fn take_deploy_entries(&self, mark: WalMark) -> Vec<WalEntry> {
        let mut guard = self.entries.lock().expect("Wal mutex poisoned");
        if mark.len >= guard.len() {
            return Vec::new();
        }
        guard.split_off(mark.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_ref_hash_matches_blake2b256() {
        let bytes = b"hello world";
        let pr = PayloadRef::hash(bytes);
        let expected = Blake2b256::hash(bytes.to_vec());
        match pr {
            PayloadRef::Hash(got) => {
                assert_eq!(&got[..], &expected[..], "hash mismatch");
            }
            other => panic!("expected PayloadRef::Hash, got {other:?}"),
        }
    }

    fn mk_write_entry(payload: &[u8]) -> WalEntry {
        WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: Some(payload.len() as u64),
            payload_ref: Some(PayloadRef::hash(payload)),
            mode_bits: None,
            owner: None,
            group: None,
        }
    }

    #[test]
    fn wal_append_and_snapshot() {
        let wal = Wal::new();
        assert!(wal.is_empty());
        wal.append(mk_write_entry(b"hello")).unwrap();
        assert_eq!(wal.len(), 1);
        let snap = wal.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].op, WalOp::Write);
        assert_eq!(snap[0].length, Some(5));
    }

    #[test]
    fn wal_clone_shares_buffer() {
        let wal_a = Wal::new();
        let wal_b = wal_a.clone();
        wal_b
            .append(WalEntry {
                op: WalOp::Truncate,
                path: PathBuf::from("/y"),
                extra_path: None,
                offset: Some(0),
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
            })
            .unwrap();
        assert_eq!(
            wal_a.len(),
            1,
            "wal clones must share the underlying buffer"
        );
    }

    #[test]
    fn wal_clear_empties_buffer() {
        let wal = Wal::new();
        for _ in 0..5 {
            wal.append(mk_write_entry(b"x")).unwrap();
        }
        assert_eq!(wal.len(), 5);
        wal.clear();
        assert_eq!(wal.len(), 0);
        assert!(wal.is_empty());
    }

    /// H-29-2 review fix: append past `MAX_WAL_ENTRIES` returns
    /// `Err(())` and does NOT push.
    #[test]
    fn wal_append_returns_err_at_cap() {
        let wal = Wal::new();
        // Fill to cap.
        for _ in 0..MAX_WAL_ENTRIES {
            wal.append(mk_write_entry(b"x")).expect("under cap ok");
        }
        assert_eq!(wal.len(), MAX_WAL_ENTRIES);
        // Overflow attempt must fail.
        let r = wal.append(mk_write_entry(b"y"));
        assert!(r.is_err(), "append past cap must return Err");
        assert_eq!(
            wal.len(),
            MAX_WAL_ENTRIES,
            "failed append must not grow buffer"
        );
    }

    /// H-29-1 review fix: `snapshot_mark` + `truncate_to` discard
    /// entries appended after the mark.
    #[test]
    fn wal_snapshot_mark_and_truncate_to() {
        let wal = Wal::new();
        wal.append(mk_write_entry(b"a")).unwrap();
        let mark = wal.snapshot_mark();
        wal.append(mk_write_entry(b"b")).unwrap();
        wal.append(mk_write_entry(b"c")).unwrap();
        assert_eq!(wal.len(), 3);
        wal.truncate_to(mark);
        assert_eq!(wal.len(), 1);
        // Truncating to a mark past current len is a no-op.
        wal.truncate_to(WalMark { len: 999 });
        assert_eq!(wal.len(), 1);
    }

    /// Redesign: per-deploy boundary — `begin_deploy` marks the
    /// start, `take_deploy_entries` drains everything after.
    #[test]
    fn wal_take_deploy_entries_drains_since_mark() {
        let wal = Wal::new();
        // Pre-deploy state.
        wal.append(mk_write_entry(b"pre")).unwrap();
        let mark = wal.begin_deploy();
        // Deploy contributions.
        wal.append(mk_write_entry(b"d1")).unwrap();
        wal.append(mk_write_entry(b"d2")).unwrap();
        wal.append(mk_write_entry(b"d3")).unwrap();
        let entries = wal.take_deploy_entries(mark);
        assert_eq!(
            entries.len(),
            3,
            "must drain exactly the deploy's contributions"
        );
        // Pre-deploy state remains.
        assert_eq!(wal.len(), 1);
        let remaining = wal.snapshot();
        assert_eq!(remaining[0].payload_ref, Some(PayloadRef::hash(b"pre")));
    }

    /// Redesign: `take_deploy_entries` with an empty deploy contributes
    /// nothing and doesn't touch the buffer.
    #[test]
    fn wal_take_deploy_entries_empty_deploy_returns_empty() {
        let wal = Wal::new();
        wal.append(mk_write_entry(b"pre")).unwrap();
        let mark = wal.begin_deploy();
        // No appends between begin and take.
        let entries = wal.take_deploy_entries(mark);
        assert!(entries.is_empty());
        assert_eq!(wal.len(), 1);
    }
}
