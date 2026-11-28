//! Refcount-tracking [`ColdGc`] implementation (issue #1181).
//!
//! Maintains the invariant
//!
//! ```text
//! refcount[H] = |{ R ∈ retained_roots : H ∈ leaves(R) }|
//! ```
//!
//! over a backing `KeyValueStore` keyed by 32-byte leaf hashes and
//! encoding counts as little-endian `u64`. The casper driver and the
//! future GC worker both consult `refcount[H] == 0` as the "safe to
//! delete from `rspace/cold`" test.
//!
//! ## Recompute at expiry
//!
//! This implementation does not store `leaves(R)` per retained root.
//! Both `on_checkpoint` and `on_expire` recompute by walking the
//! supplied `history_store` via
//! [`leaves_from_roots`][crate::rspace::history::cold_gc::leaves_from_roots].
//! `on_expire` is called while the retiring root's trie nodes are still
//! present (the casper driver runs it BEFORE
//! `HistoryRepository::prune_history_atomic`), so the walk reaches
//! every leaf the root ever referenced.
//!
//! ## Init pass for flag flips
//!
//! When `enable-cold-gc-bookkeeping` is first enabled on a node that
//! has been running without it, both the refcount sub-DB and the
//! retention-set sub-DB are empty but the history store has many
//! retained roots that reference cold leaves. Without a seed, an
//! `on_expire` on one of those roots would saturating-subtract past
//! zero and silently drift. The first `advance_retention` call
//! detects this condition (both stores empty + non-empty
//! `new_retention_set`) and seeds refcounts by walking every root in
//! the new retention set.
//!
//! The one-time init cost is proportional to the retention window
//! size at flag-flip time. Operators should expect a startup delay on
//! the first pass.
//!
//! ## Serialization
//!
//! Every mutation (`on_checkpoint`, `on_expire`) is a
//! read-modify-write over many keys. We serialize them with an internal
//! [`Mutex`] rather than one write txn per batch because the
//! `KeyValueStore` trait does not expose transactions; at the LMDB
//! layer each `put`/`delete` call already takes the environment's
//! writer lock, and the Mutex keeps our get-then-put window from
//! racing against another refcount mutator on the same process.
//!
//! The Mutex is uncontended at steady state — checkpoints and expiries
//! happen seconds apart — so its cost is negligible. If future
//! measurement shows it matters, replace the trait-level backing with
//! a direct LMDB `env`/`db` handle and do the batch in one write txn.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use shared::rust::store::key_value_store::{KeyValueStore, SCAN_PAGE_LIMIT_MAX};

use crate::rspace::errors::HistoryError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_gc::{
    AUDIT_SAMPLE_CAP, AuditDivergence, AuditReport, ColdGc, SweepReport, leaves_from_roots,
    leaves_from_roots_strict,
};
use crate::rspace::history::prune_guard::PruneGuard;

/// LMDB-backed [`ColdGc`]. Three pieces of state:
///
/// * `refcount_store` — `Blake2b256Hash → u64` (little-endian). Zero values are
///   physically removed.
/// * `retention_set_store` — `Blake2b256Hash → ()` (empty value). Its key set
///   records the retention set of the previous GC pass. The casper driver's
///   current retention set is diffed against it in
///   [`advance_retention`][ColdGc::advance_retention] to produce the retiring
///   roots.
/// * `prune_guard` — shared [`PruneGuard`] whose exclusive side the sweep takes
///   per batch (see below). The host `HistoryRepositoryImpl` adopts this same
///   guard via [`ColdGc::prune_guard`], so `do_checkpoint`'s `enter_write`
///   shared acquisition fences against the sweep's `enter_sweep` exclusive one.
///   Without this, a cold-leaf `put_if_absent` in `do_checkpoint` could race
///   with an `enter_sweep` batch that deletes the fresh entry before its
///   refcount lands.
///
/// For production both stores are `LmdbKeyValueStore` handles on
/// dedicated sub-DBs. For tests, `InMemoryKeyValueStore` works.
pub struct LmdbColdGc {
    refcount_store: Arc<dyn KeyValueStore>,
    retention_set_store: Arc<dyn KeyValueStore>,
    prune_guard: Arc<PruneGuard>,
    /// Serialises `on_checkpoint`, `on_expire`, and
    /// `advance_retention` so their get+put windows on the refcount
    /// and retention-set stores are not interleaved.
    mu: Mutex<()>,
}

impl LmdbColdGc {
    pub fn new(
        refcount_store: Arc<dyn KeyValueStore>,
        retention_set_store: Arc<dyn KeyValueStore>,
    ) -> Self {
        Self {
            refcount_store,
            retention_set_store,
            prune_guard: Arc::new(PruneGuard::new()),
            mu: Mutex::new(()),
        }
    }

    /// Read-modify-write a batch of refcount updates. `delta` is applied
    /// to the current value of each key; if the result is zero the key
    /// is removed from the backing store. Caller holds `self.mu`.
    fn apply_delta(&self, hashes: &[Blake2b256Hash], delta: i64) -> Result<(), HistoryError> {
        // Deduplicate: a single root cannot contribute more than once to
        // a given leaf's refcount, so a leaf appearing multiple times in
        // the input (through the walk's internal dedup already does
        // this, but belt-and-suspenders) still counts for one.
        if hashes.is_empty() {
            return Ok(());
        }
        let keys: Vec<Vec<u8>> = hashes.iter().map(|h| h.bytes()).collect();
        let current = self.refcount_store.get(&keys).map_err(HistoryError::from)?;

        let mut puts: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut deletes: Vec<Vec<u8>> = Vec::new();
        for (key, prev) in keys.into_iter().zip(current.into_iter()) {
            let prev_val = match prev.as_deref() {
                Some(bytes) => decode_count(bytes),
                None => 0,
            };
            // Saturating arithmetic is defensive: an underflow means the
            // invariant was already broken (we tried to expire a root
            // that had not been committed under this GC's bookkeeping,
            // e.g., in init-pass failures). Logging + continuing is
            // safer than panicking the whole node; the audit comparator
            // is the authoritative canary.
            let next_val = if delta >= 0 {
                prev_val.saturating_add(delta as u64)
            } else {
                prev_val.saturating_sub(delta.unsigned_abs())
            };
            if next_val == 0 {
                if prev_val > 0 {
                    deletes.push(key);
                }
            } else {
                puts.push((key, encode_count(next_val)));
            }
        }

        if !puts.is_empty() {
            self.refcount_store.put(puts).map_err(HistoryError::from)?;
        }
        if !deletes.is_empty() {
            self.refcount_store
                .delete(deletes)
                .map_err(HistoryError::from)?;
        }
        Ok(())
    }
}

impl ColdGc for LmdbColdGc {
    fn on_checkpoint(
        &self,
        new_root: &Blake2b256Hash,
        _prev_root: &Blake2b256Hash,
        history_store: &dyn KeyValueStore,
    ) -> Result<(), HistoryError> {
        let _guard = self.mu.lock().unwrap_or_else(PoisonError::into_inner);
        // Idempotency check. A `reset`-then-`do_checkpoint` sequence
        // can produce the same root twice, and replays can re-invoke
        // on_checkpoint on a root that's already recorded. Without
        // this skip, the second call would +1 refcounts that the
        // single eventual on_expire cannot symmetrically -1, leaving
        // the leaves stuck above zero forever. The retention-set
        // store's membership is the authoritative record of
        // "committed-and-not-yet-retired."
        let already_recorded = self
            .retention_set_store
            .get_one(&new_root.bytes())
            .map_err(HistoryError::from)?
            .is_some();
        if already_recorded {
            return Ok(());
        }
        let leaves = leaves_from_roots(history_store, &[new_root.clone()])?;
        if !leaves.is_empty() {
            let hashes: Vec<Blake2b256Hash> = leaves.into_iter().collect();
            self.apply_delta(&hashes, 1)?;
        }
        // Record the committed root even when it has no leaves — a
        // future `advance_retention` that does NOT include this root
        // must still call on_expire so the invariant
        // `refcount[H] = |{retained R : H ∈ leaves(R)}|` holds (the
        // retention "roots that have been committed and not yet
        // retired via advance_retention").
        self.retention_set_store
            .put(vec![(new_root.bytes(), Vec::new())])
            .map_err(HistoryError::from)?;
        Ok(())
    }

    fn on_expire(
        &self,
        expired_root: &Blake2b256Hash,
        history_store: &dyn KeyValueStore,
    ) -> Result<(), HistoryError> {
        let leaves = leaves_from_roots(history_store, &[expired_root.clone()])?;
        if leaves.is_empty() {
            return Ok(());
        }
        let _guard = self.mu.lock().unwrap_or_else(PoisonError::into_inner);
        let hashes: Vec<Blake2b256Hash> = leaves.into_iter().collect();
        self.apply_delta(&hashes, -1)
    }

    fn refcount(&self, leaf_hash: &Blake2b256Hash) -> Result<u64, HistoryError> {
        let bytes = self
            .refcount_store
            .get_one(&leaf_hash.bytes())
            .map_err(HistoryError::from)?;
        Ok(bytes.as_deref().map(decode_count).unwrap_or(0))
    }

    fn advance_retention(
        &self,
        new_retention_set: &[Blake2b256Hash],
        history_store: &dyn KeyValueStore,
    ) -> Result<Vec<Blake2b256Hash>, HistoryError> {
        // Dedupe the caller's set so a duplicated root hash cannot
        // produce two increments. `compute_gc_retention_roots` dedupes
        // today, but we do not want to rely on that invariant from
        // this side.
        let new_set: HashSet<Blake2b256Hash> = new_retention_set.iter().cloned().collect();

        // Hold the mutex for the entire operation. Dropping it between
        // phases (e.g. around on_expire) would let a concurrent
        // on_checkpoint land a new root in retention_set_store after
        // Phase 1 scanned it, causing Phase 2b to classify that root
        // as "entering" and +1 its leaves a second time. Serializing
        // the whole call eliminates the window. The tree walks only
        // read history_store (LMDB handles its own txn semantics), so
        // they do not need to drop this mutex.
        let _guard = self.mu.lock().unwrap_or_else(PoisonError::into_inner);

        // Phase 0: init-pass detection for flag flips.
        //
        // On a node that just enabled cold-GC bookkeeping,
        // `on_checkpoint` never ran for the pre-flip commits, so both
        // the refcount store and the retention-set store are empty
        // but `new_retention_set` is non-empty. Seed refcounts by
        // walking every root in the retention set.
        //
        // On a node that has always had the flag on, `on_checkpoint`
        // has been maintaining both stores, so at least one of them
        // is non-empty when we get here; the init pass is skipped.
        let retention_empty = self
            .retention_set_store
            .scan_page(None, 1)
            .map_err(HistoryError::from)?
            .rows
            .is_empty();
        let refcount_empty = self
            .refcount_store
            .scan_page(None, 1)
            .map_err(HistoryError::from)?
            .rows
            .is_empty();
        if retention_empty && refcount_empty && !new_set.is_empty() {
            for root in &new_set {
                let leaves = leaves_from_roots(history_store, &[root.clone()])?;
                if !leaves.is_empty() {
                    let hashes: Vec<Blake2b256Hash> = leaves.into_iter().collect();
                    self.apply_delta(&hashes, 1)?;
                }
            }
            let inserts: Vec<(Vec<u8>, Vec<u8>)> =
                new_set.iter().map(|h| (h.bytes(), Vec::new())).collect();
            if !inserts.is_empty() {
                self.retention_set_store
                    .put(inserts)
                    .map_err(HistoryError::from)?;
            }
            tracing::info!(
                target: "f1r3.trace.cold_gc",
                retained_roots = new_set.len(),
                "cold-GC init pass complete: refcounts seeded for existing retained roots"
            );
            return Ok(Vec::new());
        }

        // Phase 1: enumerate the previous retention set and partition
        // into {retiring: not in new_set} and {surviving: in new_set}.
        let mut retiring: Vec<Blake2b256Hash> = Vec::new();
        let mut previous: HashSet<Blake2b256Hash> = HashSet::new();
        let mut cursor: Option<Vec<u8>> = None;
        loop {
            let page = self
                .retention_set_store
                .scan_page(cursor.as_deref(), SCAN_PAGE_LIMIT_MAX)
                .map_err(HistoryError::from)?;
            if page.rows.is_empty() {
                break;
            }
            for (key, _) in &page.rows {
                let h = Blake2b256Hash::from_bytes(key.clone());
                previous.insert(h.clone());
                if !new_set.contains(&h) {
                    retiring.push(h);
                }
            }
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }

        // Phase 2: for each retiring root, walk its tree and
        // decrement refcounts. Inlined (not a call to self.on_expire)
        // because we hold the mutex and on_expire would re-acquire.
        // Per-root failures are logged and skipped so one bad walk
        // does not stall every subsequent pass.
        for root in &retiring {
            let leaves = match leaves_from_roots(history_store, &[root.clone()]) {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(
                        target: "f1r3.trace.cold_gc",
                        error = %e,
                        root = %root,
                        "advance_retention: retiring-root walk failed; its refcount contribution \
                         will not be decremented. Audit comparator will surface the drift."
                    );
                    continue;
                }
            };
            if leaves.is_empty() {
                continue;
            }
            let hashes: Vec<Blake2b256Hash> = leaves.into_iter().collect();
            if let Err(e) = self.apply_delta(&hashes, -1) {
                tracing::warn!(
                    target: "f1r3.trace.cold_gc",
                    error = %e,
                    root = %root,
                    "advance_retention: refcount decrement failed"
                );
            }
        }

        // Phase 2b: increment refcounts for every root entering the
        // retention set (in new_set, not previously retained). In the
        // ordinary flow `on_checkpoint` already inserted every
        // committed root into the retention-set store and bumped
        // refcounts, so `entering` is empty here. Entering is
        // non-empty only when an operator's retention policy re-adds
        // a root that previously retired — in which case we walk
        // its tree and restore the contribution its earlier
        // `on_expire`-equivalent took away.
        let entering: Vec<Blake2b256Hash> = new_set
            .iter()
            .filter(|h| !previous.contains(h))
            .cloned()
            .collect();
        for root in &entering {
            let leaves = leaves_from_roots(history_store, &[root.clone()])?;
            if leaves.is_empty() {
                continue;
            }
            let hashes: Vec<Blake2b256Hash> = leaves.into_iter().collect();
            self.apply_delta(&hashes, 1)?;
        }

        // Phase 3: swap retention-set store contents.
        if !retiring.is_empty() {
            let retiring_keys: Vec<Vec<u8>> = retiring.iter().map(|h| h.bytes()).collect();
            self.retention_set_store
                .delete(retiring_keys)
                .map_err(HistoryError::from)?;
        }
        if !entering.is_empty() {
            let entering_kvs: Vec<(Vec<u8>, Vec<u8>)> =
                entering.iter().map(|h| (h.bytes(), Vec::new())).collect();
            self.retention_set_store
                .put(entering_kvs)
                .map_err(HistoryError::from)?;
        }

        Ok(retiring)
    }

    fn audit(&self, history_store: &dyn KeyValueStore) -> Result<AuditReport, HistoryError> {
        // Hold the mutex for the full audit so a concurrent
        // on_checkpoint / advance_retention cannot land an
        // intermediate state we'd report as a false divergence.
        let _guard = self.mu.lock().unwrap_or_else(PoisonError::into_inner);

        // Phase A: read the retention-set store into memory. One pass
        // through `scan_page`; retention sets are small (bounded by
        // the DAG's retention window × a small per-pass overhead),
        // not by cold-leaf count.
        let mut retention_set: Vec<Blake2b256Hash> = Vec::new();
        let mut cursor: Option<Vec<u8>> = None;
        loop {
            let page = self
                .retention_set_store
                .scan_page(cursor.as_deref(), SCAN_PAGE_LIMIT_MAX)
                .map_err(HistoryError::from)?;
            if page.rows.is_empty() {
                break;
            }
            for (key, _) in &page.rows {
                retention_set.push(Blake2b256Hash::from_bytes(key.clone()));
            }
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }

        // Phase B: compute expected refcounts via mark walk.
        // Each root is walked in strict mode so we can distinguish
        // "the bookkeeping is inconsistent" from "we couldn't walk
        // the whole tree." Roots whose walks are incomplete are
        // counted in `unwalkable_roots` and excluded from the
        // expected counts — their leaves would otherwise show as
        // false "extra" divergences.
        let mut expected: HashMap<Blake2b256Hash, u64> = HashMap::new();
        let mut unwalkable_roots: usize = 0;
        for root in &retention_set {
            let (leaves, complete) = leaves_from_roots_strict(history_store, &[root.clone()])?;
            if !complete {
                unwalkable_roots += 1;
                continue;
            }
            for leaf in leaves {
                *expected.entry(leaf).or_insert(0) += 1;
            }
        }

        // Phase C: scan refcount store into memory.
        let mut actual: HashMap<Blake2b256Hash, u64> = HashMap::new();
        let mut cursor: Option<Vec<u8>> = None;
        loop {
            let page = self
                .refcount_store
                .scan_page(cursor.as_deref(), SCAN_PAGE_LIMIT_MAX)
                .map_err(HistoryError::from)?;
            if page.rows.is_empty() {
                break;
            }
            for (key, value) in &page.rows {
                let h = Blake2b256Hash::from_bytes(key.clone());
                actual.insert(h, decode_count(value));
            }
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }

        // Phase D: diff. "Extras" from retention roots we couldn't
        // walk are suppressed: a refcount entry only counts as extra
        // when it's in refcount_store and no FULLY-walked retention
        // root contributes to it — i.e. it's missing from `expected`.
        // When unwalkable_roots > 0 some of those "extras" may be
        // real references in subtrees we couldn't reach; the report
        // surfaces this via unwalkable_roots so the operator can
        // discount accordingly.
        let mut report = AuditReport {
            retention_roots: retention_set.len(),
            unwalkable_roots,
            refcount_entries: actual.len(),
            expected_entries: expected.len(),
            divergence_total: 0,
            samples: Vec::new(),
        };
        for (h, &exp) in &expected {
            let (act, diverged) = match actual.get(h) {
                Some(&a) if a == exp => (a, false),
                Some(&a) => (a, true),
                None => (0, true),
            };
            if diverged {
                report.divergence_total += 1;
                if report.samples.len() < AUDIT_SAMPLE_CAP {
                    report
                        .samples
                        .push(AuditDivergence::classify(h.clone(), act, exp));
                }
            }
        }
        for (h, &act) in &actual {
            if !expected.contains_key(h) {
                report.divergence_total += 1;
                if report.samples.len() < AUDIT_SAMPLE_CAP {
                    report
                        .samples
                        .push(AuditDivergence::classify(h.clone(), act, 0));
                }
            }
        }

        Ok(report)
    }

    fn sweep(&self, cold_store: &dyn KeyValueStore) -> Result<SweepReport, HistoryError> {
        // Precondition. An empty retention set means either a fresh
        // node that has never committed, or a node that enabled the
        // bookkeeping flag but has not yet run its init pass. In
        // either case every cold entry's refcount is zero by default,
        // so unconditional sweeping here would delete the entire
        // cold store. Refuse early with a skipped result instead.
        let retention_empty = self
            .retention_set_store
            .scan_page(None, 1)
            .map_err(HistoryError::from)?
            .rows
            .is_empty();
        if retention_empty {
            tracing::warn!(
                target: "f1r3.trace.cold_gc",
                "cold-GC sweep skipped: retention-set store is empty (pre-init-pass or fresh \
                 node); unconditional sweep would delete every cold entry"
            );
            return Ok(SweepReport {
                skipped_preconditions_not_met: true,
                ..SweepReport::default()
            });
        }

        let mut report = SweepReport::default();
        let mut cursor: Option<Vec<u8>> = None;
        loop {
            // Take the PruneGuard's exclusive side BEFORE scanning
            // the page so the cold-leaf `put_if_absent` in
            // `do_checkpoint` — which runs under the shared side —
            // cannot land a fresh entry between our scan and our
            // delete. Modelled on `prune_history_atomic`'s
            // per-batch exclusive hold: brief enough not to starve
            // writers across the full sweep, broad enough to cover
            // the scan+check+delete window. `self.mu` layers on top
            // to keep concurrent `on_checkpoint` refcount writes
            // from interleaving with our refcount-store reads.
            let _sweep = self.prune_guard.enter_sweep();
            let _guard = self.mu.lock().unwrap_or_else(PoisonError::into_inner);

            let page = cold_store
                .scan_page(cursor.as_deref(), SCAN_PAGE_LIMIT_MAX)
                .map_err(HistoryError::from)?;
            if page.rows.is_empty() {
                break;
            }
            report.scanned += page.rows.len();
            // Cold-store keys were written via `bincode::serialize(&Blake2b256Hash)`
            // in `history_repository_impl::do_checkpoint`, which prepends an 8-byte
            // LE length to the 32 raw bytes. The refcount store, in contrast, keys
            // on the raw `Blake2b256Hash::bytes()` form. Normalise each cold-store
            // key to its raw form for the refcount lookup, but keep the original
            // encoded key for the delete so we actually remove the row we scanned.
            let original_keys: Vec<Vec<u8>> = page.rows.iter().map(|(k, _)| k.clone()).collect();
            let lookup_keys: Vec<Vec<u8>> = original_keys
                .iter()
                .map(|k| cold_key_to_refcount_key(k))
                .collect();

            let refs = self
                .refcount_store
                .get(&lookup_keys)
                .map_err(HistoryError::from)?;
            let orphans: Vec<Vec<u8>> = original_keys
                .into_iter()
                .zip(refs.into_iter())
                .filter_map(|(k, r)| r.is_none().then_some(k))
                .collect();
            if !orphans.is_empty() {
                let deleted = cold_store.delete(orphans).map_err(HistoryError::from)?;
                report.deleted += deleted;
            }
            report.batches += 1;
            let next = page.next;
            drop(_guard);
            drop(_sweep);

            match next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }

        Ok(report)
    }

    fn prune_guard(&self) -> Option<Arc<PruneGuard>> { Some(self.prune_guard.clone()) }
}

/// 8-byte little-endian `u64`. Chosen over bincode so a hex dump of the
/// refcount store is directly readable, and so a future reader in a
/// different language (e.g., Python ops tooling) does not need a bincode
/// library.
fn encode_count(n: u64) -> Vec<u8> { n.to_le_bytes().to_vec() }

fn decode_count(bytes: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let n = bytes.len().min(8);
    buf[..n].copy_from_slice(&bytes[..n]);
    u64::from_le_bytes(buf)
}

/// Normalise a cold-store key to the raw `Blake2b256Hash::bytes()` form used
/// by the refcount store. `do_checkpoint` writes cold-store keys via
/// `bincode::serialize(&Blake2b256Hash)`, which prepends an 8-byte LE length
/// (= 32) to the 32 hash bytes, so the on-disk key is 40 bytes. The historic
/// raw-bytes form (and all in-tree tests) uses the 32-byte hash directly; the
/// reader fallback in `rspace_history_reader_impl` accepts both. Anything else
/// is an unknown format — we return it as-is so sweep will not incorrectly
/// match a refcount entry; the caller treats a miss as "orphan" and will
/// delete it, which is the safer outcome for a corrupt/foreign key.
fn cold_key_to_refcount_key(key: &[u8]) -> Vec<u8> {
    const BINCODE_LEN_PREFIX: usize = 8;
    const HASH_BYTES: usize = 32;
    if key.len() == BINCODE_LEN_PREFIX + HASH_BYTES {
        let len = u64::from_le_bytes(key[..BINCODE_LEN_PREFIX].try_into().unwrap()) as usize;
        if len == HASH_BYTES {
            return key[BINCODE_LEN_PREFIX..].to_vec();
        }
    }
    key.to_vec()
}

#[cfg(test)]
mod tests {
    use shared::rust::store::key_value_store::KeyValueStore;

    use super::*;
    use crate::rspace::history::radix_tree::{Item, Node, empty_node, encode, hash_node};
    use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    fn put_node(store: &dyn KeyValueStore, node: &Node) -> Blake2b256Hash {
        let node_bytes = hash_node(node);
        store
            .put(vec![(node_bytes.0.clone(), encode(node))])
            .unwrap();
        Blake2b256Hash::from_bytes(node_bytes.0)
    }

    /// Build a trie whose root references two distinct cold leaves. The
    /// two leaf-hash values are fixed byte patterns so the test can
    /// assert refcounts against known keys.
    fn seeded_root_two_leaves()
    -> (Blake2b256Hash, Blake2b256Hash, Blake2b256Hash, Arc<dyn KeyValueStore>) {
        let history: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let leaf_a = Blake2b256Hash::from_bytes(vec![0xA1; 32]);
        let leaf_b = Blake2b256Hash::from_bytes(vec![0xB2; 32]);
        let mut root = empty_node();
        root[0] = Item::Leaf {
            prefix: vec![0x01],
            value: leaf_a.bytes(),
        };
        root[1] = Item::Leaf {
            prefix: vec![0x02],
            value: leaf_b.bytes(),
        };
        let root_hash = put_node(history.as_ref(), &root);
        (root_hash, leaf_a, leaf_b, history)
    }

    #[test]
    fn checkpoint_increments_every_reachable_leaf_once() {
        let (root, leaf_a, leaf_b, history) = seeded_root_two_leaves();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 1);
        assert_eq!(gc.refcount(&leaf_b).unwrap(), 1);
    }

    /// `on_checkpoint` is idempotent for the same root — the
    /// retention-set store is consulted and a repeat call is a
    /// no-op. This matters because `reset`-then-`do_checkpoint` or
    /// a replay can produce the same root twice; without the guard,
    /// the second call would +1 refcounts that the single eventual
    /// `on_expire` cannot symmetrically -1.
    #[test]
    fn on_checkpoint_is_idempotent_for_same_root() {
        let (root, leaf_a, leaf_b, history) = seeded_root_two_leaves();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 1);
        assert_eq!(gc.refcount(&leaf_b).unwrap(), 1);

        // After one on_expire the leaves fall to zero — which would
        // not happen under the pre-fix double-count bug.
        gc.on_expire(&root, history.as_ref()).unwrap();
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 0);
        assert_eq!(gc.refcount(&leaf_b).unwrap(), 0);
    }

    /// Two *distinct* roots that reference overlapping leaves each
    /// contribute +1 to the shared leaves. Covered by
    /// `shared_leaf_survives_until_last_referencing_root_expires`
    /// below — this test is the idempotency companion to that one.
    #[test]
    fn two_distinct_roots_both_increment_shared_leaves() {
        let history: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let leaf = Blake2b256Hash::from_bytes(vec![0xC0; 32]);
        let mut n1 = empty_node();
        n1[0] = Item::Leaf {
            prefix: vec![0],
            value: leaf.bytes(),
        };
        let r1 = put_node(history.as_ref(), &n1);
        let mut n2 = empty_node();
        n2[1] = Item::Leaf {
            prefix: vec![1],
            value: leaf.bytes(),
        };
        let r2 = put_node(history.as_ref(), &n2);
        assert_ne!(r1, r2);

        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&r1, &prev, history.as_ref()).unwrap();
        gc.on_checkpoint(&r2, &prev, history.as_ref()).unwrap();
        assert_eq!(gc.refcount(&leaf).unwrap(), 2);
    }

    #[test]
    fn expire_decrements_and_zero_deletes_the_key() {
        let (root, leaf_a, leaf_b, history) = seeded_root_two_leaves();
        let refcount_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let gc = LmdbColdGc::new(refcount_store.clone(), Arc::new(InMemoryKeyValueStore::new()));
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        gc.on_expire(&root, history.as_ref()).unwrap();
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 0);
        assert_eq!(gc.refcount(&leaf_b).unwrap(), 0);
        // Zero-valued keys are physically removed so the future GC
        // worker's "scan for refcount == 0" pass does not see
        // ambiguous residue.
        assert!(
            refcount_store.get_one(&leaf_a.bytes()).unwrap().is_none(),
            "zero refcount must delete the key"
        );
        assert!(refcount_store.get_one(&leaf_b.bytes()).unwrap().is_none());
    }

    #[test]
    fn expire_does_not_underflow_when_never_incremented() {
        let (root, leaf_a, _leaf_b, history) = seeded_root_two_leaves();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        // No prior on_checkpoint: refcounts start at implicit 0.
        gc.on_expire(&root, history.as_ref()).unwrap();
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 0);
    }

    /// Shared-leaf case: two roots reference the same cold hash. Each
    /// contributes +1; expiring one leaves the other's reference
    /// intact. This is the structural reason for refcounting rather
    /// than immediate deletion.
    #[test]
    fn shared_leaf_survives_until_last_referencing_root_expires() {
        let history: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let shared_leaf = Blake2b256Hash::from_bytes(vec![0xDE; 32]);

        // Root 1: slot 0 → shared_leaf.
        let mut root1_node = empty_node();
        root1_node[0] = Item::Leaf {
            prefix: vec![0x01],
            value: shared_leaf.bytes(),
        };
        let root1 = put_node(history.as_ref(), &root1_node);

        // Root 2: slot 7 → shared_leaf (same cold hash, different
        // channel-key position). Distinct root hash because the
        // encoded node bytes differ.
        let mut root2_node = empty_node();
        root2_node[7] = Item::Leaf {
            prefix: vec![0x07],
            value: shared_leaf.bytes(),
        };
        let root2 = put_node(history.as_ref(), &root2_node);
        assert_ne!(root1, root2);

        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let dummy_prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root1, &dummy_prev, history.as_ref())
            .unwrap();
        gc.on_checkpoint(&root2, &dummy_prev, history.as_ref())
            .unwrap();
        assert_eq!(gc.refcount(&shared_leaf).unwrap(), 2);

        gc.on_expire(&root1, history.as_ref()).unwrap();
        assert_eq!(
            gc.refcount(&shared_leaf).unwrap(),
            1,
            "root2 still references the shared leaf; its refcount must survive"
        );

        gc.on_expire(&root2, history.as_ref()).unwrap();
        assert_eq!(gc.refcount(&shared_leaf).unwrap(), 0);
    }

    #[test]
    fn encode_decode_round_trip() {
        for n in [0u64, 1, 42, u32::MAX as u64, u64::MAX] {
            assert_eq!(decode_count(&encode_count(n)), n);
        }
    }

    /// The invariant — `refcount[H] = |{retained R : H ∈ leaves(R)}|` —
    /// holds across arbitrary interleavings of `on_checkpoint` and
    /// `advance_retention`. Deterministic PRNG seed so the failure
    /// case is always the same sequence.
    ///
    /// Operation mix:
    /// * 70% commit: build a fresh radix root whose slots are filled with 1–20
    ///   random leaf hashes, record ground-truth membership, call
    ///   `on_checkpoint`.
    /// * 30% advance-retention: pick a random subset of all committed-so-far
    ///   roots as the new retention set.
    ///
    /// After every operation, the test asks `LmdbColdGc::refcount` for
    /// every leaf it has ever generated and compares against the
    /// reference count derived by walking the ground-truth state.
    #[test]
    fn property_refcount_matches_mark_walk_count_across_random_ops() {
        use std::collections::{HashMap, HashSet};

        use rand::rngs::StdRng;
        use rand::seq::IteratorRandom;
        use rand::{Rng, SeedableRng};

        const SEED: u64 = 0xC01DC01DC01DC01D;
        const NUM_OPS: usize = 200;
        const MAX_LEAVES_PER_ROOT: u8 = 20;

        let mut rng = StdRng::seed_from_u64(SEED);
        let history: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );

        // Ground truth.
        let mut all_leaves: HashSet<Blake2b256Hash> = HashSet::new();
        let mut committed_roots: Vec<Blake2b256Hash> = Vec::new();
        let mut leaves_of: HashMap<Blake2b256Hash, HashSet<Blake2b256Hash>> = HashMap::new();
        let mut retained: HashSet<Blake2b256Hash> = HashSet::new();

        let dummy_prev = Blake2b256Hash::from_bytes(vec![0; 32]);

        for op_idx in 0..NUM_OPS {
            let do_commit = rng.random_bool(0.7) || committed_roots.is_empty();
            if do_commit {
                let n_leaves = rng.random_range(1..=MAX_LEAVES_PER_ROOT) as usize;
                let mut node = empty_node();
                let mut leaves_for_root: HashSet<Blake2b256Hash> = HashSet::new();
                for slot in 0..n_leaves {
                    let mut bytes = [0u8; 32];
                    rng.fill(&mut bytes);
                    let leaf = Blake2b256Hash::from_bytes(bytes.to_vec());
                    leaves_for_root.insert(leaf.clone());
                    all_leaves.insert(leaf.clone());
                    node[slot] = Item::Leaf {
                        prefix: vec![slot as u8],
                        value: leaf.bytes(),
                    };
                }
                let root = put_node(history.as_ref(), &node);
                // Skip if we re-generated an identical root — would
                // make the ground-truth refcount model drift (a
                // second on_checkpoint on the same root +1s each leaf
                // again, but ground truth already had it marked
                // retained once).
                if leaves_of.contains_key(&root) {
                    continue;
                }
                committed_roots.push(root.clone());
                leaves_of.insert(root.clone(), leaves_for_root);
                gc.on_checkpoint(&root, &dummy_prev, history.as_ref())
                    .unwrap();
                retained.insert(root);
            } else {
                let n_retain = rng.random_range(0..=committed_roots.len());
                let new_set: Vec<Blake2b256Hash> = committed_roots
                    .iter()
                    .cloned()
                    .choose_multiple(&mut rng, n_retain);
                gc.advance_retention(&new_set, history.as_ref()).unwrap();
                retained = new_set.into_iter().collect();
            }

            for leaf in &all_leaves {
                let expected: u64 = retained
                    .iter()
                    .filter(|r| leaves_of[*r].contains(leaf))
                    .count() as u64;
                let actual = gc.refcount(leaf).unwrap();
                assert_eq!(
                    actual, expected,
                    "invariant broken after op {op_idx} for leaf {leaf}: expected {expected}, got \
                     {actual}. retained = {retained:?}"
                );
            }
        }
    }

    /// First call to `advance_retention` sees an empty retention store:
    /// nothing retires, everything in the new set enters.
    #[test]
    fn advance_retention_first_pass_retires_nothing() {
        let (root, _leaf_a, _leaf_b, history) = seeded_root_two_leaves();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let retiring = gc
            .advance_retention(&[root.clone()], history.as_ref())
            .unwrap();
        assert!(retiring.is_empty());
    }

    /// Retention-window slide: a root that was retained last pass but
    /// not this pass is surfaced as retiring and its leaves are
    /// decremented.
    #[test]
    fn advance_retention_decrements_on_retiring_root() {
        let (root, leaf_a, leaf_b, history) = seeded_root_two_leaves();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        // Simulate: the root was committed (refcount +1 per leaf) and
        // appeared in last pass's retention set.
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        gc.advance_retention(&[root.clone()], history.as_ref())
            .unwrap();
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 1);

        // Next pass: root is no longer retained.
        let retiring = gc.advance_retention(&[], history.as_ref()).unwrap();
        assert_eq!(retiring.len(), 1);
        assert_eq!(retiring[0], root);
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 0);
        assert_eq!(gc.refcount(&leaf_b).unwrap(), 0);
    }

    /// A root present in both last and current retention sets is NOT
    /// treated as retiring.
    #[test]
    fn advance_retention_survives_when_still_retained() {
        let (root, leaf_a, _leaf_b, history) = seeded_root_two_leaves();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        gc.advance_retention(&[root.clone()], history.as_ref())
            .unwrap();
        // Second pass with the same retention set — no retirement,
        // refcount stays put.
        let retiring = gc
            .advance_retention(&[root.clone()], history.as_ref())
            .unwrap();
        assert!(retiring.is_empty());
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 1);
    }

    /// Flag-flip init pass: refcount store starts empty (pre-flip
    /// commits never called `on_checkpoint`), so the first
    /// `advance_retention` call walks every root in the new retention
    /// set and seeds refcounts. Second call diffs normally.
    #[test]
    fn advance_retention_init_pass_seeds_refcounts_on_flag_flip() {
        let (root, leaf_a, leaf_b, history) = seeded_root_two_leaves();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        // No prior `on_checkpoint` — simulates a flag flip on a node
        // whose cold store already has leaves.
        gc.advance_retention(&[root.clone()], history.as_ref())
            .unwrap();
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 1, "init-pass seeded refcount");
        assert_eq!(gc.refcount(&leaf_b).unwrap(), 1);

        // Next pass drops the root — it retires and its leaves go to
        // zero. If the init pass had been wrong (double-counted), the
        // leaf refcount would be stuck at 1 after this call.
        let retiring = gc.advance_retention(&[], history.as_ref()).unwrap();
        assert_eq!(retiring.len(), 1);
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 0);
    }

    /// Fresh-node first call must NOT init-pass: `on_checkpoint` has
    /// already incremented refcounts, so another increment would
    /// double-count.
    #[test]
    fn advance_retention_fresh_node_first_call_does_not_double_count() {
        let (root, leaf_a, _leaf_b, history) = seeded_root_two_leaves();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 1);
        gc.advance_retention(&[root.clone()], history.as_ref())
            .unwrap();
        assert_eq!(gc.refcount(&leaf_a).unwrap(), 1, "no double-count on fresh-node first pass");
    }

    /// Audit returns a clean report on a consistent state.
    #[test]
    fn audit_reports_no_divergence_on_healthy_state() {
        let (root, _leaf_a, _leaf_b, history) = seeded_root_two_leaves();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        let report = gc.audit(history.as_ref()).unwrap();
        assert_eq!(report.divergence_total, 0, "healthy state has no drift");
        assert_eq!(report.retention_roots, 1);
        assert_eq!(report.unwalkable_roots, 0);
        assert_eq!(report.expected_entries, 2);
        assert_eq!(report.refcount_entries, 2);
        assert!(report.samples.is_empty());
    }

    /// Manually overwriting a refcount surfaces as a mismatched-count
    /// divergence. Exercises the bug class the audit is designed to
    /// catch (refcount drift from the invariant).
    #[test]
    fn audit_detects_mismatched_refcount() {
        use crate::rspace::history::cold_gc::AuditDivergenceKind;

        let (root, leaf_a, leaf_b, history) = seeded_root_two_leaves();
        let refcount: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let gc = LmdbColdGc::new(refcount.clone(), Arc::new(InMemoryKeyValueStore::new()));
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        // Corrupt leaf_a's refcount — simulate the exact drift bug
        // the audit is supposed to detect.
        refcount
            .put(vec![(leaf_a.bytes(), encode_count(5))])
            .unwrap();
        let report = gc.audit(history.as_ref()).unwrap();
        assert_eq!(report.divergence_total, 1);
        assert_eq!(report.samples.len(), 1);
        let sample = &report.samples[0];
        assert_eq!(sample.leaf, leaf_a);
        assert_eq!(sample.actual, 5);
        assert_eq!(sample.expected, 1);
        assert_eq!(sample.kind, AuditDivergenceKind::MismatchedCount);
        // leaf_b remains consistent.
        assert!(!report.samples.iter().any(|d| d.leaf == leaf_b));
    }

    /// A leaf missing from the refcount store but reachable from a
    /// retained root is classified as MissingFromRefcount.
    #[test]
    fn audit_detects_missing_refcount() {
        use crate::rspace::history::cold_gc::AuditDivergenceKind;
        let (root, leaf_a, leaf_b, history) = seeded_root_two_leaves();
        let refcount: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let gc = LmdbColdGc::new(refcount.clone(), Arc::new(InMemoryKeyValueStore::new()));
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        // Delete leaf_a's refcount entry entirely.
        refcount.delete(vec![leaf_a.bytes()]).unwrap();
        let report = gc.audit(history.as_ref()).unwrap();
        assert_eq!(report.divergence_total, 1);
        let sample = &report.samples[0];
        assert_eq!(sample.leaf, leaf_a);
        assert_eq!(sample.actual, 0);
        assert_eq!(sample.expected, 1);
        assert_eq!(sample.kind, AuditDivergenceKind::MissingFromRefcount);
        // leaf_b is still consistent.
        assert!(!report.samples.iter().any(|d| d.leaf == leaf_b));
    }

    /// An extra entry in the refcount store (no retention root
    /// references it) is classified as ExtraInRefcount.
    #[test]
    fn audit_detects_extra_refcount() {
        use crate::rspace::history::cold_gc::AuditDivergenceKind;

        let (root, _leaf_a, _leaf_b, history) = seeded_root_two_leaves();
        let refcount: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let gc = LmdbColdGc::new(refcount.clone(), Arc::new(InMemoryKeyValueStore::new()));
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();
        let orphan = Blake2b256Hash::from_bytes(vec![0xEE; 32]);
        refcount
            .put(vec![(orphan.bytes(), encode_count(3))])
            .unwrap();
        let report = gc.audit(history.as_ref()).unwrap();
        assert_eq!(report.divergence_total, 1);
        let sample = &report.samples[0];
        assert_eq!(sample.leaf, orphan);
        assert_eq!(sample.actual, 3);
        assert_eq!(sample.expected, 0);
        assert_eq!(sample.kind, AuditDivergenceKind::ExtraInRefcount);
    }

    /// A retention-set entry whose history tree is missing (joiner
    /// mid-sync, prior pruning) is counted in `unwalkable_roots` and
    /// its "would-be" leaves do NOT show as divergences. Guards
    /// against the walk-incomplete false-positive class.
    #[test]
    fn audit_tolerates_unwalkable_retention_root() {
        // An in-memory store with NO nodes; any root hash resolves
        // to no bytes, so the strict walk returns `complete = false`
        // for every root.
        let history: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let phantom_root = Blake2b256Hash::from_bytes(vec![0x7F; 32]);
        let retention_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        // Place the root directly into the retention-set store,
        // bypassing on_checkpoint (simulates: committed once, then
        // history was pruned, leaving the retention entry orphaned).
        retention_store
            .put(vec![(phantom_root.bytes(), Vec::new())])
            .unwrap();
        let gc = LmdbColdGc::new(Arc::new(InMemoryKeyValueStore::new()), retention_store);

        let report = gc.audit(history.as_ref()).unwrap();
        assert_eq!(report.retention_roots, 1);
        assert_eq!(report.unwalkable_roots, 1);
        assert_eq!(report.expected_entries, 0);
        assert_eq!(
            report.divergence_total, 0,
            "unwalkable roots must not be counted as divergences"
        );
    }

    /// Sample list is capped so the audit result stays small in log
    /// output even when a systemic bug makes every leaf diverge.
    #[test]
    fn audit_sample_list_is_bounded() {
        let history: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let refcount: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let gc = LmdbColdGc::new(refcount.clone(), Arc::new(InMemoryKeyValueStore::new()));
        // Seed the refcount store with lots of orphan entries (no
        // retention roots exist, so every entry diverges).
        let orphans: Vec<(Vec<u8>, Vec<u8>)> =
            (0..100u8).map(|i| (vec![i; 32], encode_count(1))).collect();
        refcount.put(orphans).unwrap();
        let report = gc.audit(history.as_ref()).unwrap();
        assert_eq!(report.divergence_total, 100);
        assert_eq!(report.samples.len(), AUDIT_SAMPLE_CAP);
    }

    /// Sweep refuses to run when the retention-set store is empty —
    /// the pre-init-pass / fresh-node signature. Unconditional
    /// deletion in that state would wipe every cold entry.
    #[test]
    fn sweep_skips_when_retention_set_is_empty() {
        let cold: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        // Seed the cold store so we would see mass deletion if the
        // precondition check failed.
        cold.put(vec![(vec![0xAA; 32], vec![1, 2, 3]), (vec![0xBB; 32], vec![4, 5, 6])])
            .unwrap();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let report = gc.sweep(cold.as_ref()).unwrap();
        assert!(report.skipped_preconditions_not_met);
        assert_eq!(report.deleted, 0);
        assert_eq!(report.scanned, 0);
        // Cold store is untouched.
        assert!(cold.get_one(&vec![0xAA; 32]).unwrap().is_some());
        assert!(cold.get_one(&vec![0xBB; 32]).unwrap().is_some());
    }

    /// With a properly-initialized retention set, sweep deletes
    /// cold entries that have no refcount entry and leaves the
    /// referenced ones alone.
    #[test]
    fn sweep_deletes_only_unreferenced_cold_entries() {
        let (root, leaf_a, leaf_b, history) = seeded_root_two_leaves();
        let cold: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        // Populate cold with the referenced leaves plus an orphan.
        let orphan = Blake2b256Hash::from_bytes(vec![0x44; 32]);
        cold.put(vec![
            (leaf_a.bytes(), vec![0x01]),
            (leaf_b.bytes(), vec![0x02]),
            (orphan.bytes(), vec![0x99]),
        ])
        .unwrap();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();

        let report = gc.sweep(cold.as_ref()).unwrap();
        assert!(!report.skipped_preconditions_not_met);
        assert_eq!(report.scanned, 3);
        assert_eq!(report.deleted, 1);
        // Referenced leaves survive.
        assert!(cold.get_one(&leaf_a.bytes()).unwrap().is_some());
        assert!(cold.get_one(&leaf_b.bytes()).unwrap().is_some());
        // Orphan deleted.
        assert!(cold.get_one(&orphan.bytes()).unwrap().is_none());
    }

    /// Production cold-store keys are `bincode::serialize(&Blake2b256Hash)`
    /// (8-byte LE length prefix + 32 raw bytes = 40 bytes), while the
    /// refcount store keys on the raw 32-byte form. Before the key-encoding
    /// fix, sweep would pass the 40-byte cold-store key straight into
    /// `refcount_store.get(...)`, every lookup would miss, and every
    /// retained leaf would be classified as an orphan and deleted. This
    /// test reproduces that scenario and asserts the referenced leaves
    /// survive.
    #[test]
    fn sweep_handles_bincode_encoded_cold_store_keys() {
        let (root, leaf_a, leaf_b, history) = seeded_root_two_leaves();
        let cold: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());

        // Match production: cold-store rows are keyed on bincode-serialized
        // `Blake2b256Hash`, not raw bytes. Include an orphan in the same
        // format to exercise the sweep's delete path.
        let orphan = Blake2b256Hash::from_bytes(vec![0x44; 32]);
        let encode = |h: &Blake2b256Hash| bincode::serialize(h).unwrap();
        cold.put(vec![
            (encode(&leaf_a), vec![0x01]),
            (encode(&leaf_b), vec![0x02]),
            (encode(&orphan), vec![0x99]),
        ])
        .unwrap();

        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();

        let report = gc.sweep(cold.as_ref()).unwrap();
        assert_eq!(report.scanned, 3);
        assert_eq!(report.deleted, 1, "only the orphan must be swept");
        assert!(
            cold.get_one(&encode(&leaf_a)).unwrap().is_some(),
            "retained leaf_a must survive sweep"
        );
        assert!(
            cold.get_one(&encode(&leaf_b)).unwrap().is_some(),
            "retained leaf_b must survive sweep"
        );
        assert!(cold.get_one(&encode(&orphan)).unwrap().is_none());
    }

    #[test]
    fn cold_key_to_refcount_key_strips_bincode_prefix() {
        let h = Blake2b256Hash::from_bytes(vec![0x7A; 32]);
        let encoded = bincode::serialize(&h).unwrap();
        assert_eq!(encoded.len(), 40);
        assert_eq!(cold_key_to_refcount_key(&encoded), h.bytes());
    }

    #[test]
    fn cold_key_to_refcount_key_passes_raw_32_bytes_through() {
        let raw = vec![0x5C; 32];
        assert_eq!(cold_key_to_refcount_key(&raw), raw);
    }

    /// After retiring a root, its leaves lose their refcount and
    /// become sweepable. End-to-end test of the Phase-1 → Phase-4
    /// pipeline.
    #[test]
    fn sweep_removes_leaves_of_retired_root() {
        let (root, leaf_a, leaf_b, history) = seeded_root_two_leaves();
        let cold: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        cold.put(vec![(leaf_a.bytes(), vec![0x01]), (leaf_b.bytes(), vec![0x02])])
            .unwrap();
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();

        // Before retirement: sweep is a no-op.
        let report = gc.sweep(cold.as_ref()).unwrap();
        assert_eq!(report.deleted, 0);

        // Retire the only retained root. Its leaves should now be
        // sweepable.
        gc.advance_retention(&[], history.as_ref()).unwrap();
        // Retention store is now empty — sweep correctly refuses.
        let report = gc.sweep(cold.as_ref()).unwrap();
        assert!(report.skipped_preconditions_not_met);
        assert_eq!(report.deleted, 0);

        // But if there's at least one other retained root to keep
        // the preconditions satisfied, the leaves of the retired
        // root sweep correctly. Seed a fresh retention entry.
        let other_leaf = Blake2b256Hash::from_bytes(vec![0xCC; 32]);
        let mut other_node = empty_node();
        other_node[0] = Item::Leaf {
            prefix: vec![0],
            value: other_leaf.bytes(),
        };
        let other_root = put_node(history.as_ref(), &other_node);
        cold.put(vec![(other_leaf.bytes(), vec![0xFF])]).unwrap();
        gc.on_checkpoint(&other_root, &prev, history.as_ref())
            .unwrap();

        let report = gc.sweep(cold.as_ref()).unwrap();
        assert!(!report.skipped_preconditions_not_met);
        assert_eq!(report.deleted, 2, "leaf_a and leaf_b (orphans) must sweep");
        // The still-referenced leaf survives.
        assert!(cold.get_one(&other_leaf.bytes()).unwrap().is_some());
        // Old leaves are gone.
        assert!(cold.get_one(&leaf_a.bytes()).unwrap().is_none());
        assert!(cold.get_one(&leaf_b.bytes()).unwrap().is_none());
    }

    /// The LmdbColdGc exposes a stable PruneGuard via the trait so
    /// `HistoryRepositoryImpl` can adopt it. Identity matters: the
    /// repository must fence against the SAME lock the sweep
    /// acquires, not a parallel one.
    #[test]
    fn exposes_a_stable_prune_guard() {
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let g1 = ColdGc::prune_guard(&gc).unwrap();
        let g2 = ColdGc::prune_guard(&gc).unwrap();
        assert!(
            Arc::ptr_eq(&g1, &g2),
            "LmdbColdGc must return the SAME Arc on repeated prune_guard() calls"
        );
    }

    /// Sweep's per-batch `enter_sweep` on the shared PruneGuard
    /// serialises against any writer holding `enter_write`. This
    /// directly tests the race fix for issue #1181 Phase 4: if
    /// `do_checkpoint` holds `enter_write` (around its cold+history
    /// writes), sweep cannot run a delete batch concurrently.
    #[test]
    fn sweep_blocks_while_prune_guard_write_is_held() {
        use std::sync::mpsc::{TryRecvError, channel};
        use std::thread;
        use std::time::Duration;

        let (root, _leaf_a, _leaf_b, history) = seeded_root_two_leaves();
        let cold: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        // Populate cold so sweep would actually have work to do.
        cold.put(vec![(vec![0xAA; 32], vec![0x01])]).unwrap();
        let gc = Arc::new(LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        ));
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        gc.on_checkpoint(&root, &prev, history.as_ref()).unwrap();

        let guard = ColdGc::prune_guard(&*gc).unwrap();
        let writer_lock = guard.enter_write();

        let (started_tx, started_rx) = channel::<()>();
        let (done_tx, done_rx) = channel::<()>();
        let gc_c = gc.clone();
        let cold_c = cold.clone();
        let sweeper = thread::spawn(move || {
            started_tx.send(()).unwrap();
            gc_c.sweep(cold_c.as_ref()).unwrap();
            done_tx.send(()).unwrap();
        });

        // Thread has been scheduled and is about to attempt sweep —
        // which needs to acquire enter_sweep. Give it 20ms.
        started_rx.recv().unwrap();
        thread::sleep(Duration::from_millis(20));
        match done_rx.try_recv() {
            Err(TryRecvError::Empty) => {}
            Ok(()) => panic!("sweep returned while writer held enter_write"),
            Err(TryRecvError::Disconnected) => panic!("sweep thread panicked"),
        }

        drop(writer_lock);
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("sweep must unblock within 5s after writer releases enter_write");
        sweeper.join().unwrap();
    }

    /// Multi-root pass across two retention windows: three roots retained
    /// at pass 1, two at pass 2. The one dropped must retire exactly
    /// once.
    #[test]
    fn advance_retention_handles_multiple_retiring_roots() {
        let history: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let mut roots = Vec::new();
        let mut leaves = Vec::new();
        for slot in 0u8..3 {
            let leaf = Blake2b256Hash::from_bytes(vec![0xE0 + slot; 32]);
            leaves.push(leaf.clone());
            let mut node = empty_node();
            node[slot as usize] = Item::Leaf {
                prefix: vec![slot],
                value: leaf.bytes(),
            };
            let r = put_node(history.as_ref(), &node);
            roots.push(r);
        }
        let gc = LmdbColdGc::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let prev = Blake2b256Hash::from_bytes(vec![0; 32]);
        for r in &roots {
            gc.on_checkpoint(r, &prev, history.as_ref()).unwrap();
        }
        gc.advance_retention(roots.as_slice(), history.as_ref())
            .unwrap();

        // Drop roots[0]; roots[1..=2] stay.
        let retiring = gc.advance_retention(&roots[1..], history.as_ref()).unwrap();
        assert_eq!(retiring.len(), 1);
        assert_eq!(retiring[0], roots[0]);
        assert_eq!(gc.refcount(&leaves[0]).unwrap(), 0);
        assert_eq!(gc.refcount(&leaves[1]).unwrap(), 1);
        assert_eq!(gc.refcount(&leaves[2]).unwrap(), 1);
    }
}
