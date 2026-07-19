use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use prost::Message;

use super::pathmap_integration::{
    create_pathmap_from_elements, PathMapCreationResult, RholangPathMap,
};
use crate::rhoapi::{EPathMap, Var};

/// Bound on the number of memoized EPathMap→trie conversions retained at once.
///
/// The interpreter's PathMap query workloads (e.g. the rho_net set-automaton
/// index) issue many method calls against a small number of distinct EPathMap
/// values — typically one large index plus a handful of small transients — so a
/// small bound captures essentially all reuse while keeping worst-case retained
/// memory at `MEMO_CAPACITY × (encoded-map + trie)` bytes.
const MEMO_CAPACITY: usize = 64;

/// One memoized conversion result.
///
/// `map` is retained as a live `PathMap<Par>`; handing it to a caller is an
/// O(1) `clone()` (the pathmap crate's `PathMap::clone` bumps the refcount on
/// the root `TrieNodeODRc`). Callers that mutate their clone go through the
/// crate's `make_mut` copy-on-write path (nodes with refcount > 1 are cloned
/// before mutation), so a cached trie can never be corrupted by a caller.
struct MemoEntry {
    /// Monotonic last-use tick for LRU eviction.
    last_use: u64,
    map: RholangPathMap,
    connective_used: bool,
    locally_free: Vec<u8>,
}

/// Bounded LRU memo for `e_pathmap_to_rholang_pathmap`.
///
/// Keyed by the FULL prost encoding of the `EPathMap`. Collision stance: the
/// encoded bytes themselves are the `HashMap` key, so a hit is certified by
/// full-encoded-bytes equality (`HashMap` falls back to `Eq` on the complete
/// byte vector whenever hashes collide) — no truncated digest is ever trusted.
/// prost encoding is deterministic (fields in tag order, repeated fields in
/// element order), so byte-equality coincides with structural equality of
/// `(ps, locally_free, connective_used, remainder)`, a superset of the
/// conversion's true inputs `(ps, remainder)`; over-keying can only cause
/// spurious misses, never wrong hits.
struct TrieMemo {
    tick: u64,
    entries: HashMap<Vec<u8>, MemoEntry>,
}

/// Process-wide memo. `std::sync::OnceLock` + `Mutex` matches the
/// interpreter's existing std-sync concurrency idiom (cf. the `OnceLock`
/// reducer cell in `rholang`'s `reduce.rs`) and introduces no new
/// dependencies. `PathMap<Par>` is `Send + Sync` with atomic node refcounts,
/// so sharing built tries across interpreter threads is sound.
static TRIE_MEMO: OnceLock<Mutex<TrieMemo>> = OnceLock::new();

fn trie_memo() -> &'static Mutex<TrieMemo> {
    TRIE_MEMO.get_or_init(|| {
        Mutex::new(TrieMemo {
            tick: 0,
            entries: HashMap::with_capacity(MEMO_CAPACITY),
        })
    })
}

pub struct PathMapCrateTypeMapper;

impl PathMapCrateTypeMapper {
    /// Convert from protobuf EPathMap to PathMap-based structure.
    ///
    /// Memoized: `create_pathmap_from_elements` is a PURE function of the
    /// EPathMap's `ps` and `remainder` (byte-identical inputs produce the same
    /// trie), so its result is cached process-wide behind a bounded LRU (see
    /// [`TrieMemo`]). A hit returns an O(1) clone of the cached trie instead
    /// of re-running the O(|ps| × element-size) rebuild that previously
    /// executed on EVERY EPathMap method dispatch.
    ///
    /// Replay determinism and cost accounting are unaffected: results are
    /// identical whether a call hits, misses, or was evicted (the memo is
    /// invisible in the value domain), and no `reserve_*` cost charge lives in
    /// this layer — the memo removes UNCHARGED host work only.
    pub fn e_pathmap_to_rholang_pathmap(e_pathmap: &EPathMap) -> PathMapCreationResult {
        let key = e_pathmap.encode_to_vec();

        {
            let mut memo = trie_memo()
                .lock()
                .expect("EPathMap trie memo mutex poisoned");
            memo.tick += 1;
            let tick = memo.tick;
            if let Some(entry) = memo.entries.get_mut(&key) {
                entry.last_use = tick;
                return PathMapCreationResult {
                    map: entry.map.clone(),
                    connective_used: entry.connective_used,
                    locally_free: entry.locally_free.clone(),
                };
            }
        }

        // Build OUTSIDE the lock so a slow rebuild never serializes unrelated
        // conversions. If two threads race on the same key, both compute the
        // same pure result and the second insert overwrites the first with an
        // equivalent entry — benign and deterministic.
        let built = create_pathmap_from_elements(&e_pathmap.ps, e_pathmap.remainder.clone());

        let mut memo = trie_memo()
            .lock()
            .expect("EPathMap trie memo mutex poisoned");
        memo.tick += 1;
        let tick = memo.tick;
        if memo.entries.len() >= MEMO_CAPACITY && !memo.entries.contains_key(&key) {
            // Evict the least-recently-used entry (O(MEMO_CAPACITY) scan —
            // negligible against the rebuild this memo elides).
            if let Some(lru_key) = memo
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_use)
                .map(|(k, _)| k.clone())
            {
                memo.entries.remove(&lru_key);
            }
        }
        memo.entries.insert(
            key,
            MemoEntry {
                last_use: tick,
                map: built.map.clone(),
                connective_used: built.connective_used,
                locally_free: built.locally_free.clone(),
            },
        );

        built
    }

    /// Convert from PathMap back to protobuf EPathMap
    pub fn rholang_pathmap_to_e_pathmap(
        map: &RholangPathMap,
        connective_used: bool,
        locally_free: &[u8],
        remainder: Option<Var>,
    ) -> EPathMap {
        // Extract all values (flattened) from the trie as elements for proto EPathMap
        let mut ps = Vec::new();
        for (_, par) in map.iter() {
            ps.push(par.clone());
        }

        EPathMap {
            ps,
            locally_free: locally_free.to_vec(),
            connective_used,
            remainder,
        }
    }
}
