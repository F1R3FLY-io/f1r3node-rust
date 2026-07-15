// C10 — Concurrent memoization safety for the finalized-floor / frontier cache.
//
// The finalized-floor feature memoizes two PURE, node-identical functions of a
// block into `KeyValueDagRepresentation`:
//
//   * `floor_index`    : block hash -> floor    (justification-derived floor)
//   * `frontier_index` : block hash -> frontier F(X) = parent_frontier(X, just(X))
//
// (see `block-storage/src/rust/dag/block_dag_key_value_storage.rs`:112,:119 and
// the `get_cached_floor`/`put_cached_floor`/`get_cached_frontier`/
// `put_cached_frontier` accessors ~:151,:162,:172,:183). Those accessors take
// `&self` and are DELIBERATELY NOT behind a global lock — each `get_cached_*`
// and `put_cached_*` is an independent single-key store operation. The value
// they store is a pure function of the block, so it is the SAME regardless of
// which thread computes it: a write-once memoization. The T-CACHE theorem
// (`Floor.frontier_cache_transparent`) proves this transparent SEQUENTIALLY;
// this file is the concurrent regression that a torn or regressed cached value
// is impossible under the Rust memory model.
//
// REAL guarantee (production): idempotence of the memoized write (every thread
// computes the identical value) PLUS the single-key MVCC of the LMDB-backed
// `KeyValueTypedStoreImpl` (a get is one read txn, a put is one write txn; a
// key is never partially visible). Loom cannot model LMDB; what it DOES check
// here is the Rust-memory-model SHAPE of the pattern: a shared index guarded by
// a lock, two writers racing the get-miss -> compute -> put memoization into the
// SAME key, and a concurrent reader — asserting on EVERY preemption-bounded
// interleaving that no third value and no regression can ever be observed.
//
// We model the index as `loom::sync::Mutex<HashMap<Hash, Hash>>` (the loom
// analog of the production store's atomic single-key get/put). `u64` stands in
// for `BlockHash` exactly as the sibling `loom_equivocations_tracker.rs` uses a
// `u64` counter as its observation point.
//
// Properties model-checked on every interleaving:
//   (1) DOMAIN     — any value observed for key `x` is in {absent, canonical};
//                    a torn/partial write (a THIRD value) is impossible.
//   (2) FINALITY   — after all writers quiesce, the memoized value is canonical.
//   (3) MONOTONE   — no read returns a value regressed below a prior read:
//                    once a reader observes `canonical`, it can never later
//                    observe `absent` (or anything else). The memo is write-once
//                    and never removed, so presence is monotone.
//
// Run with (matches how `loom_equivocations_tracker` is invoked — the tests use
// `loom::sync::*` types directly, so no `--cfg loom` flag is required):
//   cargo test -p block-storage --test loom_frontier_floor_cache
//
// See `block-storage/tests/loom_equivocations_tracker.rs` for the broader loom
// harness rationale (why the model lives in `block-storage`, next to the store).

use std::collections::HashMap;

use loom::sync::{Arc, Mutex};
use loom::thread;

/// The block-hash key under test. `u64` stands in for `BlockHash`.
const X: u64 = 0xA5;

/// The PURE, node-identical memoized function F(x). Every thread that misses in
/// the cache derives the SAME value from this — that is exactly what makes the
/// lost update benign. Deterministic; no interior mutability, no I/O.
fn compute(x: u64) -> u64 {
    // An arbitrary fixed bijection-ish mix; the only property that matters is
    // that it is a pure function of `x` (same input -> same output, always).
    x.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(17)
        ^ 0x0000_0000_DEAD_BEEF
}

type Index = Arc<Mutex<HashMap<u64, u64>>>;

/// Memoize `F(key)` into the shared index: get-miss -> compute -> put, with the
/// lock released across the pure compute (production holds no lock across the
/// floor/frontier walk). A concurrent writer may insert the identical value in
/// between; that lost update is benign because the value is identical.
fn memoize(index: &Index, key: u64) -> u64 {
    // GET — one store read (production: a single-key LMDB read txn).
    {
        let guard = index.lock().expect("index mutex poisoned");
        if let Some(&hit) = guard.get(&key) {
            return hit; // memo hit: another thread already wrote the canonical value
        }
    } // release before the pure compute — no lock spans F(x)

    // COMPUTE F(x) — pure, so every racing thread derives the SAME `value`.
    let value = compute(key);

    // PUT — one store write (production: a single-key LMDB write txn). If a
    // racing writer already inserted, this overwrites with the identical value.
    {
        let mut guard = index.lock().expect("index mutex poisoned");
        guard.insert(key, value);
    }
    value
}

/// A single atomic read of `key` from the shared index.
fn read_key(index: &Index, key: u64) -> Option<u64> {
    let guard = index.lock().expect("index mutex poisoned");
    guard.get(&key).copied()
}

/// Two writers race the SAME memoization into the SAME key while a reader
/// observes concurrently. On EVERY interleaving: every observed value is in
/// {absent, canonical}, the reader never regresses, and the final value is
/// canonical.
#[test]
fn concurrent_memoization_never_tears_or_regresses() {
    loom::model(|| {
        let canonical = compute(X);
        let index: Index = Arc::new(Mutex::new(HashMap::new()));

        let writer_a = {
            let index = index.clone();
            thread::spawn(move || {
                memoize(&index, X);
            })
        };
        let writer_b = {
            let index = index.clone();
            thread::spawn(move || {
                memoize(&index, X);
            })
        };

        // Concurrent reader: two observations, checked for the DOMAIN and
        // MONOTONE (no-regression) properties at the moment of observation.
        let reader = {
            let index = index.clone();
            thread::spawn(move || {
                let in_domain = |v: Option<u64>| v.is_none() || v == Some(canonical);

                let r1 = read_key(&index, X);
                assert!(
                    in_domain(r1),
                    "read #1 observed a THIRD value {:?} (not absent, not canonical {}) — \
                     a torn/partial memo write leaked",
                    r1, canonical
                );

                let r2 = read_key(&index, X);
                assert!(
                    in_domain(r2),
                    "read #2 observed a THIRD value {:?} (not absent, not canonical {}) — \
                     a torn/partial memo write leaked",
                    r2, canonical
                );

                // MONOTONE: the memo is write-once and never removed, so once the
                // canonical value is visible it must stay visible.
                if r1 == Some(canonical) {
                    assert_eq!(
                        r2, Some(canonical),
                        "cached value REGRESSED below a prior read: saw canonical then {:?}",
                        r2
                    );
                }
            })
        };

        writer_a.join().expect("writer A completes on every interleaving");
        writer_b.join().expect("writer B completes on every interleaving");
        reader.join().expect("reader completes on every interleaving");

        // FINALITY: after both writers quiesce the memoized value is canonical.
        assert_eq!(
            read_key(&index, X),
            Some(canonical),
            "final memoized value must be the canonical F(x)"
        );
    });
}
