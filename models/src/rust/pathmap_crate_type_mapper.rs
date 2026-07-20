//! EPathMap fix P1 — the content-addressed intern store (T1-lite).
//!
//! Evolves the 84a0fbe4 bounded trie memo IN PLACE into a content-addressed
//! intern store (ONE mechanism, ONE capacity — risk R3 forbids a second
//! cache): the store key is the Blake2b-256 digest of the canonical prost
//! encoding, computed by STREAMING the prost encode through a
//! `bytes::BufMut` hasher adapter (an O(map) walk with ZERO heap allocation
//! on the lookup path), and every digest hit is certified by the K2
//! FULL-PROST-FIDELITY structural verify: the candidate's encode is
//! re-streamed through a byte comparator against the stored canonical bytes,
//! which is byte-exact-by-construction and includes `locally_free` at every
//! level (the generated `AlwaysEqual` `==`/`Hash` IGNORE `locally_free` —
//! models/src/lib.rs — and are therefore UNUSABLE for keying; the P0 goldens
//! pin that prost RETAINS `locally_free`).
//!
//! Collision stance (user decision D1 = K2, 2026-07-20): digest buckets hold
//! a collision LIST; a verify mismatch on every candidate is treated as a
//! MISS (build + insert into the bucket's list) and emits a once-per-process
//! diagnostic log line — a ~2^-128 event worth loud evidence. This PRESERVES
//! the 84a0fbe4 documented stance that no truncated digest is ever trusted:
//! a hit is still certified by full-encoded-bytes equality; the digest only
//! selects the bucket.
//!
//! Cost/consensus invariants (unchanged from 84a0fbe4):
//!   * NO `reserve_*` cost charge lives in this layer — the store removes
//!     UNCHARGED host work only, so charge totals AND order are identical
//!     whether a call hits, misses, was evicted, or collided.
//!   * Results are value-identical on every path (pure content addressing);
//!     replay determinism is untouched.
//!   * Memory bound (risk R2): `INTERN_CAPACITY` buckets × (canonical prost
//!     bytes + trie + `locally_free`); `serde_bytes` stays lazy (P4).
//!   * Thread safety (risk R8): the `OnceLock<Mutex<…>>` std-sync idiom; a
//!     set-once race between two builders of the same bytes resolves to a
//!     single shared `Arc` (the re-lock re-scan below); `PathMap<Par>` is
//!     `Send + Sync` with atomic node refcounts.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use prost::bytes::buf::UninitSlice;
use prost::bytes::BufMut;
use prost::Message;

use super::pathmap_integration::{
    create_pathmap_from_elements, PathMapCreationResult, RholangPathMap,
};
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{EPathMap, Expr, Par, Var};

/// Bound on the number of digest buckets retained at once (the landed
/// 84a0fbe4 capacity, unchanged — one mechanism, one capacity).
///
/// The interpreter's PathMap query workloads (e.g. the rho_net set-automaton
/// index) issue many method calls against a small number of distinct EPathMap
/// values — typically one large index plus a handful of small transients — so
/// a small bound captures essentially all reuse while keeping worst-case
/// retained memory at `INTERN_CAPACITY × (canonical prost bytes + trie)`.
const INTERN_CAPACITY: usize = 64;

/// Fixed staging-chunk size for the streaming encode adapter. prost writes
/// through `BufMut::put_u8`/`put_slice`, whose `bytes` default impls loop
/// `chunk_mut()` → write ≤ chunk-len bytes → `advance_mut(cnt)`, so any
/// chunk size is contract-correct; 256 keeps the whole adapter on the stack.
const ENCODE_STREAM_CHUNK: usize = 256;

/// One interned EPathMap→trie conversion (the store entry, shared by `Arc`).
///
/// `map` is retained as a live `PathMap<Par>`; handing it to a caller is an
/// O(1) `clone()` (the pathmap crate bumps the refcount on the root
/// `TrieNodeODRc`). Callers that mutate their clone go through the crate's
/// `make_mut` copy-on-write path, so an interned trie can never be corrupted
/// by a caller (test-pinned since 84a0fbe4).
pub struct InternedEPathMap {
    /// The built trie (an O(1)-clonable handle).
    pub map: RholangPathMap,
    /// `connective_used` as computed by `create_pathmap_from_elements`
    /// (OR over entries; forced `true` by a `Some` remainder) — NOT the
    /// EPathMap's own field (landed semantics, value-identical).
    pub connective_used: bool,
    /// `locally_free` as computed by `create_pathmap_from_elements`
    /// (union over entries) — NOT the EPathMap's own field (landed
    /// semantics, value-identical).
    pub locally_free: Vec<u8>,
    /// The canonical prost encoding of the source EPathMap — the K2 verify
    /// reference (full fidelity: includes `locally_free` at every level).
    pub canonical_prost: Vec<u8>,
    /// `Message::encoded_len()` of the source EPathMap
    /// (`== canonical_prost.len()`), computed ONCE (amendment PM-7).
    pub encoded_len: usize,
    /// Blake2b-256 of `canonical_prost` — the store key.
    pub digest: [u8; 32],
    /// `ps.len()` of the source EPathMap.
    pub entry_count: usize,
    /// The PM-4(c) ground-normal-form classifier verdict: `true` iff the
    /// interpreter's EPathMap re-evaluation (reduce.rs:2687-2707) is
    /// PROVABLY the byte-exact identity on this map. Consumed by P2's
    /// method-chain fusion gate; conservative (anything unrecognized is
    /// `false` ⇒ fallback to today's path). See [`eval_stable_epathmap`].
    pub eval_stable: bool,
    /// Lazily-filled bincode-of-EPathMap serde bytes for P4's spliced event
    /// hashing. UNPOPULATED in P1 (P4 is the lazy consumer).
    pub serde_bytes: OnceLock<Vec<u8>>,
}

/// One digest bucket: the LRU last-use tick plus the K2 collision list
/// (pairwise byte-distinct entries sharing one digest — a ~2^-128 event;
/// the list is length 1 in every non-adversarial execution).
type InternBucket = (u64, Vec<Arc<InternedEPathMap>>);

/// Process-wide intern store. `std::sync::OnceLock` + `Mutex` matches the
/// interpreter's existing std-sync concurrency idiom (cf. the `OnceLock`
/// reducer cell in `rholang`'s `reduce.rs`) and introduces no new
/// dependencies.
static TRIE_INTERN: OnceLock<Mutex<HashMap<[u8; 32], InternBucket>>> = OnceLock::new();

/// Monotonic LRU tick source. Incremented only while the store mutex is
/// held, so bucket last-use ticks are strictly ordered by lock acquisition.
static INTERN_TICK: AtomicU64 = AtomicU64::new(0);

/// Count of digest-collision events: a digest bucket was HIT but the K2
/// byte verify matched NO candidate in its collision list (each such event
/// creates a new list entry). The first event emits the once-per-process
/// diagnostic log line.
static DIGEST_COLLISION_EVENTS: AtomicU64 = AtomicU64::new(0);

fn intern_store() -> &'static Mutex<HashMap<[u8; 32], InternBucket>> {
    TRIE_INTERN.get_or_init(|| Mutex::new(HashMap::with_capacity(INTERN_CAPACITY)))
}

fn next_intern_tick() -> u64 {
    INTERN_TICK.fetch_add(1, Ordering::Relaxed) + 1
}

fn note_digest_collision() {
    let prior = DIGEST_COLLISION_EVENTS.fetch_add(1, Ordering::Relaxed);
    if prior == 0 {
        tracing::error!(
            target: "models::epathmap_intern",
            "Blake2b-256 digest collision in the EPathMap intern store: a digest bucket was hit \
             but the full-prost-fidelity byte verify matched no stored candidate (probability \
             ~2^-128 per pair). The store disambiguated via the bucket's collision list and \
             continued value-correctly; preserve this log line as evidence of the event."
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Streaming encode adapter (zero-allocation digest + K2 byte verify)
// ─────────────────────────────────────────────────────────────────────────────

/// Consumer of the prost encode byte stream.
trait EncodeByteSink {
    fn consume(&mut self, bytes: &[u8]);
}

/// A `bytes::BufMut` that forwards every encoded byte to an [`EncodeByteSink`]
/// through a fixed stack-resident staging chunk — no heap allocation.
///
/// Contract fit: `bytes`' default `put_u8`/`put_slice`/`put_*` impls (the
/// only write paths prost 0.14 uses) loop `chunk_mut()` → copy `cnt ≤
/// chunk-len` bytes → `advance_mut(cnt)`, so every `advance_mut(cnt)` is
/// preceded by a write of exactly `chunk[..cnt]`, which `advance_mut` then
/// hands to the sink and logically resets the chunk.
struct EncodeStream<S: EncodeByteSink> {
    sink: S,
    chunk: [u8; ENCODE_STREAM_CHUNK],
}

impl<S: EncodeByteSink> EncodeStream<S> {
    fn new(sink: S) -> Self {
        EncodeStream {
            sink,
            chunk: [0u8; ENCODE_STREAM_CHUNK],
        }
    }
}

unsafe impl<S: EncodeByteSink> BufMut for EncodeStream<S> {
    fn remaining_mut(&self) -> usize {
        // The stream never fills up (`bytes`' `put_slice` pre-checks this
        // against the total source length before its chunk loop).
        usize::MAX
    }

    fn chunk_mut(&mut self) -> &mut UninitSlice {
        UninitSlice::new(&mut self.chunk)
    }

    unsafe fn advance_mut(&mut self, cnt: usize) {
        // BufMut contract: the caller wrote `cnt` bytes at the start of the
        // last `chunk_mut()` slice before advancing. The backing array is
        // fully initialized `u8`s, so reading `chunk[..cnt]` is always
        // defined; the range index panics loudly on any contract breach
        // (`cnt` can never exceed what `chunk_mut` handed out).
        self.sink.consume(&self.chunk[..cnt]);
    }
}

/// Sink that feeds the byte stream into a Blake2b-256 hasher.
struct DigestSink<'a> {
    hasher: &'a mut Blake2b<U32>,
}

impl EncodeByteSink for DigestSink<'_> {
    fn consume(&mut self, bytes: &[u8]) {
        self.hasher.update(bytes);
    }
}

/// Sink that compares the byte stream against an expected canonical encoding
/// (the K2 structural verify): full prost fidelity by construction —
/// `locally_free` at every level is part of the wire bytes (field 3), so it
/// participates in the compare without any per-field logic to forget.
struct CompareSink<'a> {
    expected: &'a [u8],
    position: usize,
    mismatch: bool,
}

impl EncodeByteSink for CompareSink<'_> {
    fn consume(&mut self, bytes: &[u8]) {
        if !self.mismatch {
            let end = self.position + bytes.len();
            if end > self.expected.len() || &self.expected[self.position..end] != bytes {
                self.mismatch = true;
            }
        }
        self.position += bytes.len();
    }
}

/// Blake2b-256 of the canonical prost encoding of `e_pathmap`, computed by
/// streaming `Message::encode_raw` through the hasher adapter — an O(map)
/// walk with zero heap allocation (`encode_raw` is called directly; only
/// `Message::encode` would re-walk `encoded_len` for its capacity check,
/// amendment PM-7).
pub fn canonical_prost_digest(e_pathmap: &EPathMap) -> [u8; 32] {
    let mut hasher = Blake2b::<U32>::new();
    let mut stream = EncodeStream::new(DigestSink {
        hasher: &mut hasher,
    });
    e_pathmap.encode_raw(&mut stream);
    hasher.finalize().into()
}

/// The K2 structural verify: `true` iff the canonical prost encoding of
/// `e_pathmap` is byte-identical to `canonical` — computed by re-streaming
/// the encode through a comparator, with zero heap allocation. Full prost
/// fidelity by construction (includes `locally_free` at every level, which
/// the generated `AlwaysEqual` `==` ignores).
pub fn matches_canonical_prost(e_pathmap: &EPathMap, canonical: &[u8]) -> bool {
    let mut stream = EncodeStream::new(CompareSink {
        expected: canonical,
        position: 0,
        mismatch: false,
    });
    e_pathmap.encode_raw(&mut stream);
    let CompareSink {
        position, mismatch, ..
    } = stream.sink;
    !mismatch && position == canonical.len()
}

// ─────────────────────────────────────────────────────────────────────────────
// The `eval_stable` classifier (amendment PM-4(c), consumed by P2)
// ─────────────────────────────────────────────────────────────────────────────

/// PM-4(c) ground-normal-form classifier: `true` iff the interpreter's
/// EPathMap re-evaluation is PROVABLY the byte-exact identity on this map,
/// so P2's method-chain fusion may skip it. Conservative by construction
/// (risk R6): anything unrecognized ⇒ `false` ⇒ fallback to today's path.
///
/// The re-evaluation being certified (reduce.rs:2687-2707) maps every entry
/// through `eval_expr` + `update_locally_free_par`, preserves the map's own
/// `locally_free`/`connective_used`, and FORCES `remainder = None`. The
/// grammar admits exactly the shapes for which that pipeline is the
/// identity, verified arm-by-arm at 602144bd:
///
///   * map/nested-map/list levels: `remainder == None` (the EPathMap arm
///     reduce.rs:2705 and the EList arm :2614 force `None` — a `Some`
///     remainder would be dropped, so any `Some` ⇒ unstable);
///     `locally_free` empty and `connective_used` false at EVERY level
///     (entry-level `locally_free` is recomputed via
///     `update_locally_free_par` :7152-7205 + `update_locally_free_elist`/
///     `_etuple` :7207-7226, which yield empty exactly when every inner
///     level is empty — has_locally_free.rs:156-236);
///   * an entry (or nested element) Par is EITHER a single-expr carrier —
///     exactly one expr, every other Par field empty (`eval_expr_to_par`'s
///     ground fall-through :1556 rebuilds `Par::default().with_exprs`, and
///     the `eval_expr` fold :7232-7252 concatenates it with the input's
///     non-expr fields, so identity requires them empty) — OR the reflect
///     GPrivate leaf: exactly one `GPrivateBody` unforgeable and no exprs
///     (`eval_expr` returns a zero-expr Par unchanged, and
///     `update_locally_free_par` draws nothing from `unforgeables`), the
///     shape `rho_net_lower::reflect_ground_term_par` emits as every
///     reflected term's head tag (the PM-4(c) execution-time check:
///     reflect's POSITIONAL alphabet — GPrivate tag + ground `EList` — is
///     inside the grammar; its AC collection carriers — send soups, `ESet`,
///     `EMap` — classify `false` conservatively);
///   * the expr alphabet: ground atoms (`GBool`/`GInt`/`GString`/`GUri`/
///     `GByteArray`/`GDouble`/`GBigInt`/`GBigRat`/`GFixedPoint` — all nine
///     are byte-identity arms in `eval_expr_to_expr` :1654-1689), `EList`,
///     `ETuple`, and nested `EPathMap` — recursively;
///   * NO `EVar` (evaluation substitutes it — and drops `var_eval_cost`),
///     no `ESet`/`EMap` (sorting arms), no connectives, no methods, no
///     operator exprs, no `EZipper`, and no non-expr Par fields
///     (sends/receives/news/matches/bundles/conditionals; also
///     `update_locally_free_par` unwrap-panics on a bundle with a `None`
///     body, reduce.rs:7199).
pub fn eval_stable_epathmap(e_pathmap: &EPathMap) -> bool {
    e_pathmap.remainder.is_none()
        && e_pathmap.locally_free.is_empty()
        && !e_pathmap.connective_used
        && e_pathmap.ps.iter().all(eval_stable_par)
}

/// A Par in ground normal form: either a single-expr carrier over the stable
/// expr alphabet, or the reflect GPrivate leaf. Every other Par field must
/// be empty and `locally_free`/`connective_used` at their ground defaults.
fn eval_stable_par(par: &Par) -> bool {
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.bundles.is_empty()
        || !par.connectives.is_empty()
        || !par.conditionals.is_empty()
        || !par.locally_free.is_empty()
        || par.connective_used
    {
        return false;
    }
    match (par.exprs.as_slice(), par.unforgeables.as_slice()) {
        // The reflect GPrivate leaf (an unforgeable INSTEAD of an expr; an
        // unforgeable riding alongside an expr — "in expr position" — falls
        // to the `_` arm and classifies unstable).
        ([], [unforgeable]) => matches!(
            unforgeable.unf_instance,
            Some(UnfInstance::GPrivateBody(_))
        ),
        // The single-expr carrier.
        ([expr], []) => eval_stable_expr(expr),
        // Nil Pars, multi-expr Pars, expr+unforgeable mixes, multiple
        // unforgeables: all conservatively unstable.
        _ => false,
    }
}

/// The stable expr alphabet (see [`eval_stable_epathmap`]).
fn eval_stable_expr(expr: &Expr) -> bool {
    match &expr.expr_instance {
        Some(
            ExprInstance::GBool(_)
            | ExprInstance::GInt(_)
            | ExprInstance::GString(_)
            | ExprInstance::GUri(_)
            | ExprInstance::GByteArray(_)
            | ExprInstance::GDouble(_)
            | ExprInstance::GBigInt(_)
            | ExprInstance::GBigRat(_)
            | ExprInstance::GFixedPoint(_),
        ) => true,
        Some(ExprInstance::EListBody(list)) => {
            list.remainder.is_none()
                && list.locally_free.is_empty()
                && !list.connective_used
                && list.ps.iter().all(eval_stable_par)
        }
        Some(ExprInstance::ETupleBody(tuple)) => {
            tuple.locally_free.is_empty()
                && !tuple.connective_used
                && tuple.ps.iter().all(eval_stable_par)
        }
        Some(ExprInstance::EPathmapBody(inner)) => eval_stable_epathmap(inner),
        // EVar, ESet, EMap, EMethod, EZipper, operators, connective exprs,
        // interpolation/concatenation, None: conservatively unstable.
        _ => false,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The intern store
// ─────────────────────────────────────────────────────────────────────────────

/// Intern an EPathMap: return the shared [`InternedEPathMap`] for its
/// canonical prost bytes, building (trie + canonical encoding + classifier)
/// on first sight.
///
/// Lookup path (zero heap allocation): one streamed digest walk selects the
/// bucket; each candidate in the bucket's collision list is certified by the
/// K2 re-streamed byte verify before being returned.
///
/// Miss path: `encoded_len()` is computed ONCE and the encoding written via
/// `encode_raw` into a `Vec` of exactly that capacity (amendment PM-7 —
/// `Message::encode` would re-walk `encoded_len` for its capacity check);
/// the trie is built OUTSIDE the lock (the landed double-insert-benign
/// discipline, upgraded to REUSE: the re-lock re-scan returns a racing
/// winner's `Arc`, keeping every collision list pairwise byte-distinct).
pub fn interned_epathmap(e_pathmap: &EPathMap) -> Arc<InternedEPathMap> {
    let digest = canonical_prost_digest(e_pathmap);

    {
        let mut store = intern_store()
            .lock()
            .expect("EPathMap intern store mutex poisoned");
        if let Some((last_use, bucket)) = store.get_mut(&digest) {
            for entry in bucket.iter() {
                if matches_canonical_prost(e_pathmap, &entry.canonical_prost) {
                    *last_use = next_intern_tick();
                    return Arc::clone(entry);
                }
            }
            // Digest hit but no byte match: a ~2^-128 collision. Treat as a
            // miss (build + insert into this bucket's list below).
            note_digest_collision();
        }
    }

    // Build outside the lock so a slow rebuild never serializes unrelated
    // conversions (landed discipline).
    let encoded_len = e_pathmap.encoded_len();
    let mut canonical_prost = Vec::with_capacity(encoded_len);
    e_pathmap.encode_raw(&mut canonical_prost);
    debug_assert_eq!(
        canonical_prost.len(),
        encoded_len,
        "prost encode_raw must write exactly encoded_len bytes"
    );
    let built = create_pathmap_from_elements(&e_pathmap.ps, e_pathmap.remainder.clone());
    let entry = Arc::new(InternedEPathMap {
        map: built.map,
        connective_used: built.connective_used,
        locally_free: built.locally_free,
        canonical_prost,
        encoded_len,
        digest,
        entry_count: e_pathmap.ps.len(),
        eval_stable: eval_stable_epathmap(e_pathmap),
        serde_bytes: OnceLock::new(),
    });

    let mut store = intern_store()
        .lock()
        .expect("EPathMap intern store mutex poisoned");
    let tick = next_intern_tick();
    if let Some((last_use, bucket)) = store.get_mut(&digest) {
        // Re-scan: a racing thread may have interned the same bytes while we
        // built. Reusing its Arc keeps the collision list pairwise
        // byte-distinct (both computed the same pure result — benign).
        for existing in bucket.iter() {
            if existing.canonical_prost == entry.canonical_prost {
                *last_use = tick;
                return Arc::clone(existing);
            }
        }
        *last_use = tick;
        bucket.push(Arc::clone(&entry));
    } else {
        if store.len() >= INTERN_CAPACITY {
            // Evict the least-recently-used bucket (O(INTERN_CAPACITY) scan —
            // negligible against the rebuild the store elides).
            if let Some(lru_digest) = store
                .iter()
                .min_by_key(|(_, (last_use, _))| *last_use)
                .map(|(bucket_digest, _)| *bucket_digest)
            {
                store.remove(&lru_digest);
            }
        }
        store.insert(digest, (tick, vec![Arc::clone(&entry)]));
    }
    entry
}

// ─────────────────────────────────────────────────────────────────────────────
// Test seams (hidden pub: integration tests cannot see `cfg(test)` items)
// ─────────────────────────────────────────────────────────────────────────────

/// TEST SEAM: number of digest buckets currently in the store.
#[doc(hidden)]
pub fn intern_store_len_for_test() -> usize {
    intern_store()
        .lock()
        .expect("EPathMap intern store mutex poisoned")
        .len()
}

/// TEST SEAM: drop every bucket (the LRU tick keeps advancing).
#[doc(hidden)]
pub fn clear_intern_store_for_test() {
    intern_store()
        .lock()
        .expect("EPathMap intern store mutex poisoned")
        .clear();
}

/// TEST SEAM: insert an entry under `entry.digest`, pushing into the
/// bucket's collision list (production insert discipline, minus the
/// byte-distinctness re-scan — forced-collision tests inject deliberately
/// mismatching entries).
#[doc(hidden)]
pub fn inject_intern_entry_for_test(entry: Arc<InternedEPathMap>) {
    let mut store = intern_store()
        .lock()
        .expect("EPathMap intern store mutex poisoned");
    let tick = next_intern_tick();
    if let Some((last_use, bucket)) = store.get_mut(&entry.digest) {
        *last_use = tick;
        bucket.push(entry);
    } else {
        if store.len() >= INTERN_CAPACITY {
            if let Some(lru_digest) = store
                .iter()
                .min_by_key(|(_, (last_use, _))| *last_use)
                .map(|(bucket_digest, _)| *bucket_digest)
            {
                store.remove(&lru_digest);
            }
        }
        let digest = entry.digest;
        store.insert(digest, (tick, vec![entry]));
    }
}

/// TEST SEAM: total digest-collision events observed by this process (the
/// first event also emitted the once-per-process diagnostic log line).
#[doc(hidden)]
pub fn digest_collision_events() -> u64 {
    DIGEST_COLLISION_EVENTS.load(Ordering::Relaxed)
}

pub struct PathMapCrateTypeMapper;

impl PathMapCrateTypeMapper {
    /// Convert from protobuf EPathMap to PathMap-based structure.
    ///
    /// A shim over [`interned_epathmap`] (P1): `create_pathmap_from_elements`
    /// is a PURE function of the EPathMap's `ps` and `remainder`
    /// (byte-identical inputs produce the same trie), so the conversion is
    /// interned process-wide behind the content-addressed store. A hit
    /// returns an O(1) clone of the interned trie instead of re-running the
    /// O(|ps| × element-size) rebuild that previously executed on EVERY
    /// EPathMap method dispatch. All 56 call sites (33 in reduce.rs, 23
    /// elsewhere) are unchanged.
    ///
    /// Replay determinism and cost accounting are unaffected: results are
    /// identical whether a call hits, misses, was evicted, or collided (the
    /// store is invisible in the value domain), and no `reserve_*` cost
    /// charge lives in this layer — the store removes UNCHARGED host work
    /// only.
    pub fn e_pathmap_to_rholang_pathmap(e_pathmap: &EPathMap) -> PathMapCreationResult {
        let interned = interned_epathmap(e_pathmap);
        PathMapCreationResult {
            map: interned.map.clone(),
            connective_used: interned.connective_used,
            locally_free: interned.locally_free.clone(),
        }
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
