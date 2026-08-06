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
    /// H-6 fix (2026-08-06): whether the underlying syscall
    /// SUCCEEDED or FAILED on the leader.  Reserve-pattern
    /// callers (`journal_write` / `journal_truncate`) append
    /// with `Success` optimistically before the syscall runs; a
    /// post-syscall finalize (`finalize_failure_journal`) updates
    /// the entry to `Failure { code }` when the reply carries an
    /// error.  Replayers reading a `Failure` entry MUST NOT
    /// apply it to reconstructed state — the leader never wrote
    /// anything to disk.  Without this field the WAL commits
    /// "requested payload was written" for syscalls that
    /// actually returned EIO/ENOSPC/EROFS, forcing followers to
    /// diverge from the leader's on-disk state.
    pub outcome: WalOutcome,
}

/// H-6 fix (2026-08-06): outcome of the syscall the WAL entry
/// represents.  Encoded at the tail of `encode_entry` (item #9 of
/// the hard-fork surface catalog).  Bumping the layout or the
/// numeric tags is a hard fork of the WAL root — coordinate via
/// `SNAPSHOT_FORMAT_VERSION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WalOutcome {
    /// The syscall completed successfully.  Reserve-pattern
    /// placeholders default to this — the leader will finalize
    /// to `Failure` if the syscall reply carries an error.
    Success,
    /// The syscall failed with the given upstream FSERR_* code.
    /// Followers MUST NOT apply the entry's mutation to
    /// reconstructed state.
    Failure { code: u32 },
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
///
/// # H-R3 fix (slice 30c H-R3 PoC): log-order-derived drain
///
/// The `entries` Vec is scheduler-order (tokio-work-stealing);
/// under `Par` two runs of the same deploy can populate it in
/// different orders — non-deterministic WAL root.  The sidecar
/// `ack_hashes` Vec (index-aligned) records each entry's ack-
/// channel Blake2b256 hash at append time.  At drain, the caller
/// can either use `take_deploy_entries(mark)` (insertion order —
/// scheduler-dependent, kept for tests / soft-checkpoint) OR the
/// new `take_deploy_entries_in_log_order(mark, deploy_log)`
/// which walks the log's Produce events, matches each Produce's
/// `channel_hash` against sidecar hashes, and emits in log order.
///
/// Log order is canonical per block (captured on the leader's
/// initial play, then frozen in the block; followers consume the
/// same log verbatim during replay), so log-order drain is
/// deterministic across validators + across re-executions on the
/// same validator regardless of `Par` scheduling.
#[derive(Clone, Debug, Default)]
pub struct Wal {
    entries: Arc<Mutex<Vec<WalEntry>>>,
    /// Slice 30c H-R3 PoC: parallel to `entries`, index-aligned.
    /// Each element is the Blake2b256 hash of the ack channel Par
    /// the syscall published its reply on.  Every fs syscall's ack
    /// is a fresh unforgeable per call, so this is a unique key
    /// within a deploy.  A subsequent `take_deploy_entries_in_log_order`
    /// walk finds each entry by matching its sidecar hash to a
    /// Produce event's `channel_hash` in the deploy_log.
    ///
    /// Stored as `Vec<[u8; 32]>` rather than `Vec<Blake2b256Hash>`
    /// to keep the `wal.rs` module free of `rspace_plus_plus`
    /// dependency (the caller in `handlers.rs` computes the hash
    /// via `stable_hash_provider::hash(ack)` and passes bytes).
    ack_hashes: Arc<Mutex<Vec<[u8; 32]>>>,
}

impl Wal {
    pub fn new() -> Self { Self::default() }

    /// Legacy append: no ack-hash sidecar populated (records
    /// `[0u8; 32]`, which is guaranteed not to match any real
    /// Produce event's channel_hash for a fresh unforgeable).
    /// Kept for tests + soft-checkpoint machinery.  Production
    /// handler paths should use `append_with_ack`.
    ///
    /// H-29-2 review fix: returns `Err(())` if the log would
    /// exceed `MAX_WAL_ENTRIES` — callers translate to
    /// `FSERR_QUOTA_EXCEEDED`.
    #[allow(clippy::result_unit_err)]
    pub fn append(&self, entry: WalEntry) -> Result<(), ()> {
        self.append_with_ack(entry, [0u8; 32])
    }

    /// Slice 30c H-R3 PoC: append an entry with its ack-channel
    /// hash for later log-order-based drain.  The hash comes from
    /// `stable_hash_provider::hash(ack_par)` on the handler side.
    /// A sentinel `[0u8; 32]` disables log-order matching for
    /// that entry (falls back to insertion order).
    #[allow(clippy::result_unit_err)]
    pub fn append_with_ack(&self, entry: WalEntry, ack_hash: [u8; 32]) -> Result<(), ()> {
        let mut guard = self.entries.lock().expect("Wal mutex poisoned");
        if guard.len() >= MAX_WAL_ENTRIES {
            return Err(());
        }
        guard.push(entry);
        // Mutex order matters: `entries` lock is held while we
        // grab `ack_hashes` so the two Vecs stay index-aligned
        // under concurrent appends.
        self.ack_hashes
            .lock()
            .expect("Wal ack_hashes mutex poisoned")
            .push(ack_hash);
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
    pub fn clear(&self) {
        self.entries.lock().expect("Wal mutex poisoned").clear();
        self.ack_hashes
            .lock()
            .expect("Wal ack_hashes mutex poisoned")
            .clear();
    }

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
            // Slice 30c H-R3 PoC: keep sidecar index-aligned.
            self.ack_hashes
                .lock()
                .expect("Wal ack_hashes mutex poisoned")
                .truncate(mark.len);
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
    ///
    /// **Insertion-order caveat (H-R3):** the returned Vec's
    /// order reflects `Par` scheduling on this run and is
    /// therefore scheduler-dependent.  Callers that will hash
    /// this Vec into a consensus commitment (e.g., a snapshot
    /// root) should use `take_deploy_entries_in_log_order`
    /// instead — that path re-orders by the canonical event log
    /// and is deterministic across validators.
    pub fn take_deploy_entries(&self, mark: WalMark) -> Vec<WalEntry> {
        let mut guard = self.entries.lock().expect("Wal mutex poisoned");
        // Also drain the ack_hash sidecar to keep it aligned.
        let mut ack_guard = self
            .ack_hashes
            .lock()
            .expect("Wal ack_hashes mutex poisoned");
        if mark.len >= guard.len() {
            return Vec::new();
        }
        let _ = ack_guard.split_off(mark.len);
        guard.split_off(mark.len)
    }

    /// Slice 30c M-29-3 fix: find the entry with the given
    /// ack_hash (from a prior `append_with_ack`) and replace it
    /// with `new_entry`.  Returns `true` if a match was found and
    /// updated, `false` otherwise (no change).
    ///
    /// Used by `fs_write` / `fs_write_at` on partial writes: the
    /// pre-syscall reservation records a placeholder with the
    /// REQUESTED payload hash + length; if the syscall returns
    /// `n < requested`, the post-syscall finalize replaces the
    /// entry with the ACTUAL payload hash + length (`hash(bytes[..n])`).
    /// Both leader (`n` from reply) and follower (`n` from `previous`
    /// cache) perform this finalize deterministically, so the two
    /// sides end with byte-identical entries even under partial-
    /// write divergence between hosts.
    ///
    /// Search is O(n) worst case but starts from the tail (the
    /// entry was appended recently), so common case is O(1).  A
    /// duplicate ack_hash across entries (shouldn't happen for
    /// fresh unforgeables) updates the most-recent one.
    pub fn update_last_entry_by_ack_hash(&self, ack_hash: [u8; 32], new_entry: WalEntry) -> bool {
        let mut entries_guard = self.entries.lock().expect("Wal mutex poisoned");
        let ack_guard = self
            .ack_hashes
            .lock()
            .expect("Wal ack_hashes mutex poisoned");
        debug_assert_eq!(
            entries_guard.len(),
            ack_guard.len(),
            "Wal invariant: entries and ack_hashes must be index-aligned"
        );
        for i in (0..ack_guard.len()).rev() {
            if ack_guard[i] == ack_hash {
                entries_guard[i] = new_entry;
                return true;
            }
        }
        false
    }

    /// H-6 fix (2026-08-06): update ONLY the `outcome` field of
    /// the entry matching `ack_hash`.  Used by
    /// `finalize_failure_journal` on the leader (and its follower
    /// mirror) to flip a placeholder from `Success` to
    /// `Failure { code }` when the syscall reply carries an
    /// error.  All other fields (op, path, offset, length,
    /// payload_ref, ...) are preserved so consumers can see WHAT
    /// the caller asked for and WHY replay should skip it.
    ///
    /// Returns `true` if a match was found and updated.
    /// Search starts from the tail — the placeholder was appended
    /// moments ago in the same handler.
    pub fn update_outcome_by_ack_hash(&self, ack_hash: [u8; 32], outcome: WalOutcome) -> bool {
        let mut entries_guard = self.entries.lock().expect("Wal mutex poisoned");
        let ack_guard = self
            .ack_hashes
            .lock()
            .expect("Wal ack_hashes mutex poisoned");
        debug_assert_eq!(
            entries_guard.len(),
            ack_guard.len(),
            "Wal invariant: entries and ack_hashes must be index-aligned"
        );
        for i in (0..ack_guard.len()).rev() {
            if ack_guard[i] == ack_hash {
                entries_guard[i].outcome = outcome;
                return true;
            }
        }
        false
    }

    /// Slice 30c H-R3 fix (option B PoC): drain entries appended
    /// after `mark` in DETERMINISTIC log order.
    ///
    /// Walks `produce_channel_hashes` (sequence of
    /// `channel_hash` bytes from the deploy's `deploy_log`'s
    /// Produce events, in log order).  For each hash present in
    /// the WAL's ack_hash sidecar, emits the corresponding
    /// entry.  Entries whose sidecar hash never appears in the
    /// log (should not happen for well-formed fs syscalls, but
    /// defense in depth) are appended at the end in insertion
    /// order so nothing is silently dropped.
    ///
    /// The caller drains + removes from the buffer just like
    /// `take_deploy_entries`.
    ///
    /// Correctness relies on: the deploy_log's produces are in a
    /// canonical order (frozen when the leader publishes the
    /// block; followers consume the same log verbatim during
    /// replay).  So the re-ordered output is byte-identical
    /// across validators and across re-executions on the same
    /// validator, regardless of `Par` scheduling on this run.
    pub fn take_deploy_entries_in_log_order(
        &self,
        mark: WalMark,
        produce_channel_hashes: &[[u8; 32]],
    ) -> Vec<WalEntry> {
        let mut entries_guard = self.entries.lock().expect("Wal mutex poisoned");
        let mut ack_guard = self
            .ack_hashes
            .lock()
            .expect("Wal ack_hashes mutex poisoned");
        if mark.len >= entries_guard.len() {
            return Vec::new();
        }
        let drained_entries: Vec<WalEntry> = entries_guard.split_off(mark.len);
        let drained_acks: Vec<[u8; 32]> = ack_guard.split_off(mark.len);
        debug_assert_eq!(
            drained_entries.len(),
            drained_acks.len(),
            "Wal invariant: entries and ack_hashes must be index-aligned"
        );
        // Build ack_hash → entry-index map for O(1) lookup.
        // Duplicate ack hashes shouldn't occur (fresh unforgeables)
        // but if they did, first-wins mirrors insertion order.
        use std::collections::HashMap;
        let mut index_by_ack: HashMap<[u8; 32], usize> = HashMap::with_capacity(drained_acks.len());
        for (i, h) in drained_acks.iter().enumerate() {
            index_by_ack.entry(*h).or_insert(i);
        }
        let mut ordered: Vec<WalEntry> = Vec::with_capacity(drained_entries.len());
        let mut emitted = vec![false; drained_entries.len()];
        for h in produce_channel_hashes {
            if let Some(&i) = index_by_ack.get(h) {
                if !emitted[i] {
                    ordered.push(drained_entries[i].clone());
                    emitted[i] = true;
                }
            }
        }
        // Defense in depth: any drained entries not matched by
        // the log walk (sentinel ack_hash `[0u8; 32]` or a
        // future-refactor gap) get appended at the end so
        // nothing is silently dropped.
        for (i, e) in drained_entries.into_iter().enumerate() {
            if !emitted[i] {
                ordered.push(e);
            }
        }
        ordered
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
            outcome: WalOutcome::Success,
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
                outcome: WalOutcome::Success,
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

    // ---------------------------------------------------------------
    // Slice 30c H-R3 fix (option B PoC) tests: log-order drain
    // produces the same output regardless of insertion order,
    // so long as the log walk is the same.
    // ---------------------------------------------------------------

    #[test]
    fn log_order_drain_permutes_insertion_order_to_match_log_order() {
        let wal = Wal::new();
        let mark = wal.begin_deploy();
        // Two entries with distinct ack hashes.  Their INSERTION
        // order into the WAL differs from the LOG order (imagine
        // two Par branches: A appended first (won the scheduler
        // race) but B's produce landed first in the log).
        let ack_a = [0xAAu8; 32];
        let ack_b = [0xBBu8; 32];
        wal.append_with_ack(mk_write_entry(b"A"), ack_a).unwrap();
        wal.append_with_ack(mk_write_entry(b"B"), ack_b).unwrap();
        // Log walks produces in order: B first, then A.
        let log_order = vec![ack_b, ack_a];
        let drained = wal.take_deploy_entries_in_log_order(mark, &log_order);
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].payload_ref, Some(PayloadRef::hash(b"B")));
        assert_eq!(drained[1].payload_ref, Some(PayloadRef::hash(b"A")));
    }

    #[test]
    fn log_order_drain_is_deterministic_across_insertion_permutations() {
        // Same set of entries + same log order → identical output
        // regardless of which order the scheduler happened to
        // append them in.  This IS the H-R3 fix.
        let ack_a = [0xAAu8; 32];
        let ack_b = [0xBBu8; 32];
        let ack_c = [0xCCu8; 32];
        let log_order = vec![ack_c, ack_a, ack_b];

        // Permutation 1: insertion = A, B, C
        let wal1 = Wal::new();
        let mark1 = wal1.begin_deploy();
        wal1.append_with_ack(mk_write_entry(b"A"), ack_a).unwrap();
        wal1.append_with_ack(mk_write_entry(b"B"), ack_b).unwrap();
        wal1.append_with_ack(mk_write_entry(b"C"), ack_c).unwrap();
        let drained1 = wal1.take_deploy_entries_in_log_order(mark1, &log_order);

        // Permutation 2: insertion = C, A, B (different scheduler)
        let wal2 = Wal::new();
        let mark2 = wal2.begin_deploy();
        wal2.append_with_ack(mk_write_entry(b"C"), ack_c).unwrap();
        wal2.append_with_ack(mk_write_entry(b"A"), ack_a).unwrap();
        wal2.append_with_ack(mk_write_entry(b"B"), ack_b).unwrap();
        let drained2 = wal2.take_deploy_entries_in_log_order(mark2, &log_order);

        // Permutation 3: insertion = B, C, A (yet another scheduler)
        let wal3 = Wal::new();
        let mark3 = wal3.begin_deploy();
        wal3.append_with_ack(mk_write_entry(b"B"), ack_b).unwrap();
        wal3.append_with_ack(mk_write_entry(b"C"), ack_c).unwrap();
        wal3.append_with_ack(mk_write_entry(b"A"), ack_a).unwrap();
        let drained3 = wal3.take_deploy_entries_in_log_order(mark3, &log_order);

        // All three drain to the same sequence: C, A, B (log order).
        assert_eq!(drained1, drained2);
        assert_eq!(drained2, drained3);
        assert_eq!(drained1[0].payload_ref, Some(PayloadRef::hash(b"C")));
        assert_eq!(drained1[1].payload_ref, Some(PayloadRef::hash(b"A")));
        assert_eq!(drained1[2].payload_ref, Some(PayloadRef::hash(b"B")));
    }

    #[test]
    fn log_order_drain_appends_unmatched_entries_at_end() {
        // Defense in depth: any drained entry whose ack_hash
        // doesn't appear in the log gets emitted at the tail
        // rather than being silently dropped.  Guards against
        // future refactor gaps (missed handler, sentinel-hash
        // use, etc.).
        let wal = Wal::new();
        let mark = wal.begin_deploy();
        let ack_a = [0xAAu8; 32];
        let ack_orphan = [0x99u8; 32]; // NOT in the log
        wal.append_with_ack(mk_write_entry(b"A"), ack_a).unwrap();
        wal.append_with_ack(mk_write_entry(b"orphan"), ack_orphan)
            .unwrap();
        // Log only contains ack_a.
        let drained = wal.take_deploy_entries_in_log_order(mark, &[ack_a]);
        assert_eq!(drained.len(), 2, "orphan must not be dropped");
        assert_eq!(drained[0].payload_ref, Some(PayloadRef::hash(b"A")));
        assert_eq!(drained[1].payload_ref, Some(PayloadRef::hash(b"orphan")));
    }

    #[test]
    fn log_order_drain_ignores_log_entries_without_matching_wal() {
        // Log contains produce channel hashes for non-fs syscalls
        // (they're the vast majority in a real deploy log — every
        // Rholang send emits a Produce).  Only produce hashes that
        // match a WAL ack sidecar produce output.
        let wal = Wal::new();
        let mark = wal.begin_deploy();
        let ack_a = [0xAAu8; 32];
        wal.append_with_ack(mk_write_entry(b"A"), ack_a).unwrap();
        let log_order = vec![
            [0x11u8; 32], // unrelated Rholang send
            [0x22u8; 32], // unrelated Rholang send
            ack_a,        // the fs syscall's ack
            [0x33u8; 32], // unrelated Rholang send
        ];
        let drained = wal.take_deploy_entries_in_log_order(mark, &log_order);
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].payload_ref, Some(PayloadRef::hash(b"A")));
    }

    #[test]
    fn log_order_drain_empty_mark_returns_empty() {
        let wal = Wal::new();
        wal.append_with_ack(mk_write_entry(b"pre"), [0x55u8; 32])
            .unwrap();
        let mark = wal.begin_deploy();
        let drained = wal.take_deploy_entries_in_log_order(mark, &[[0x55u8; 32]]);
        assert!(drained.is_empty());
        assert_eq!(wal.len(), 1);
    }

    #[test]
    fn ack_sidecar_stays_index_aligned_across_soft_checkpoint_revert() {
        // If truncate_to didn't also truncate the ack sidecar,
        // subsequent take_deploy_entries_in_log_order would
        // read stale ack hashes and misalign.
        let wal = Wal::new();
        wal.append_with_ack(mk_write_entry(b"pre"), [0x11u8; 32])
            .unwrap();
        let checkpoint = wal.snapshot_mark();
        wal.append_with_ack(mk_write_entry(b"revert_me"), [0x22u8; 32])
            .unwrap();
        wal.truncate_to(checkpoint);
        // After revert, appending a new entry should place its
        // ack at index 1 (right after the surviving pre entry),
        // not at index 2 (which would leak the stale revert_me
        // sidecar).
        wal.append_with_ack(mk_write_entry(b"post_revert"), [0x33u8; 32])
            .unwrap();
        let drained =
            wal.take_deploy_entries_in_log_order(WalMark { len: 0 }, &[[0x11u8; 32], [0x33u8; 32]]);
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].payload_ref, Some(PayloadRef::hash(b"pre")));
        assert_eq!(
            drained[1].payload_ref,
            Some(PayloadRef::hash(b"post_revert"))
        );
    }

    // ---------------------------------------------------------------
    // Slice 30c M-29-3 tests: partial-write finalize.
    //
    // The pre-syscall placeholder records the REQUESTED bytes'
    // hash; a post-syscall `update_last_entry_by_ack_hash` swaps
    // in the ACTUAL bytes' hash when `n < requested`.  Both leader
    // (n from syscall reply) and follower (n from `previous`
    // cache) finalize deterministically, so the two sides converge
    // on identical WAL entries.
    // ---------------------------------------------------------------

    #[test]
    fn update_last_entry_by_ack_hash_replaces_matching_entry() {
        let wal = Wal::new();
        let ack = [0xAAu8; 32];
        // Pre-syscall placeholder: full-length write.
        wal.append_with_ack(mk_write_entry(b"full requested payload"), ack)
            .unwrap();
        // Simulate partial write: syscall wrote only "full req".
        let updated = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: Some(8),
            payload_ref: Some(PayloadRef::hash(b"full req")),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        assert!(wal.update_last_entry_by_ack_hash(ack, updated.clone()));
        let snap = wal.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0], updated);
    }

    #[test]
    fn update_last_entry_by_ack_hash_returns_false_when_no_match() {
        let wal = Wal::new();
        wal.append_with_ack(mk_write_entry(b"anything"), [0xAAu8; 32])
            .unwrap();
        let bogus = mk_write_entry(b"other");
        // Different ack hash — no match.
        assert!(!wal.update_last_entry_by_ack_hash([0xBBu8; 32], bogus));
        // Original entry unchanged.
        let snap = wal.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].payload_ref, Some(PayloadRef::hash(b"anything")));
    }

    #[test]
    fn update_last_entry_finalize_replaces_only_most_recent_match() {
        // Duplicate ack hashes shouldn't occur for fresh unforgeables,
        // but defense-in-depth: only the LAST match gets updated.
        let wal = Wal::new();
        let ack = [0xCCu8; 32];
        wal.append_with_ack(mk_write_entry(b"first"), ack).unwrap();
        wal.append_with_ack(mk_write_entry(b"second"), ack).unwrap();
        let updated = mk_write_entry(b"second_updated");
        assert!(wal.update_last_entry_by_ack_hash(ack, updated.clone()));
        let snap = wal.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].payload_ref, Some(PayloadRef::hash(b"first")));
        assert_eq!(snap[1], updated);
    }

    // ---------------------------------------------------------------
    // H-6 fix (2026-08-06) tests: `WalOutcome` + failure finalize.
    // ---------------------------------------------------------------

    /// Optimistic-placeholder pattern: a fresh entry defaults to
    /// `Success`; `update_outcome_by_ack_hash` flips it to
    /// `Failure { code }` while leaving every other field intact.
    #[test]
    fn update_outcome_by_ack_hash_flips_success_to_failure() {
        let wal = Wal::new();
        let ack = [0xDDu8; 32];
        wal.append_with_ack(mk_write_entry(b"tried to write"), ack)
            .unwrap();
        let snap_pre = wal.snapshot();
        assert_eq!(snap_pre[0].outcome, WalOutcome::Success);

        let flipped = wal.update_outcome_by_ack_hash(ack, WalOutcome::Failure { code: 2 });
        assert!(flipped);

        let snap_post = wal.snapshot();
        assert_eq!(snap_post[0].outcome, WalOutcome::Failure { code: 2 });
        // Payload / length / op preserved — a Failure entry is
        // still self-describing about WHAT was attempted.
        assert_eq!(snap_post[0].payload_ref, snap_pre[0].payload_ref);
        assert_eq!(snap_post[0].length, snap_pre[0].length);
        assert_eq!(snap_post[0].op, snap_pre[0].op);
    }

    /// A miss (no matching ack) is a no-op — necessary for the
    /// leader path where finalize_failure_journal is called
    /// unconditionally in the error branch, even for non-Consensus
    /// caps that never appended a placeholder.
    #[test]
    fn update_outcome_by_ack_hash_returns_false_when_no_match() {
        let wal = Wal::new();
        wal.append_with_ack(mk_write_entry(b"x"), [0x11u8; 32])
            .unwrap();
        let hit = wal.update_outcome_by_ack_hash([0x22u8; 32], WalOutcome::Failure { code: 3 });
        assert!(!hit);
        // Original entry unchanged.
        let snap = wal.snapshot();
        assert_eq!(snap[0].outcome, WalOutcome::Success);
    }

    /// Two Success entries + only the middle one flips — surrounding
    /// entries stay Success.  Guards against accidentally flipping
    /// ALL matches instead of just the ack-matched one.
    #[test]
    fn update_outcome_by_ack_hash_only_touches_matched_entry() {
        let wal = Wal::new();
        wal.append_with_ack(mk_write_entry(b"a"), [0x0Au8; 32])
            .unwrap();
        wal.append_with_ack(mk_write_entry(b"b"), [0x0Bu8; 32])
            .unwrap();
        wal.append_with_ack(mk_write_entry(b"c"), [0x0Cu8; 32])
            .unwrap();
        assert!(wal.update_outcome_by_ack_hash([0x0Bu8; 32], WalOutcome::Failure { code: 5 }));
        let snap = wal.snapshot();
        assert_eq!(snap[0].outcome, WalOutcome::Success);
        assert_eq!(snap[1].outcome, WalOutcome::Failure { code: 5 });
        assert_eq!(snap[2].outcome, WalOutcome::Success);
    }
}
