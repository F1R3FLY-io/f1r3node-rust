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

#[cfg(test)]
use super::canonical_path::decode_trie_path;
use super::pathmap_integration::{PathMapCreationResult, RholangPathMap};
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{EPathMap, Expr, Par, Var};
use super::rhoapi_ext::EntryTrie;

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
    /// U(m) — the GROUND-arm value serialization: the UNCOMPRESSED,
    /// trie-ordered, length-framed key stream `repeat( u32-LE keylen ++
    /// trie_key )`, produced ONCE by a PathMap read-zipper walk over `map`
    /// (no sort; trie order = canonical order). EMPTY for non-ground maps.
    /// The wire payload of proto field 8 (`serialized_paths`) and, for ground
    /// maps, the sole content of [`Self::canonical_prost`] / the digest
    /// preimage. NEVER deflated.
    pub path_stream: Vec<u8>,
    /// The canonical prost encoding of the source EPathMap — the digest
    /// preimage + K2 verify reference. For a GROUND map this is the field-8
    /// message (just `serialized_paths` = U(m); `locally_free`/
    /// `connective_used`/`remainder` are at prost defaults and omitted). For a
    /// non-ground map it is the pre-wire field walk (`ps` at tag 1 + 3/4/5).
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
/// amendment PM-7). P3: a filled shadow cell (on this map or any nested
/// entry map) turns the corresponding subtree walk into a memcpy of cached
/// bytes — same digest by the cell invariant.
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
///
/// P3 note: this streams `Message::encode_raw`, which uses the candidate's
/// shadow cell when filled — sound under the cell invariant (cached bytes ==
/// field bytes, debug-policed), and the store only calls it on cell-empty
/// candidates anyway (a filled cell short-circuits before the store).
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
/// ★ Reads the entry half — *"is every entry in the codec's ground domain?"* —
/// off the [`EntryTrie`]'s O(1) fold rather than by walking the entries.
///
/// That matters here specifically: this predicate is on the ENCODE path (it
/// selects proto field 8 versus the tag-1 field walk), and a ground map's
/// encoding needs only the trie's key stream — never the decoded entries. Asking
/// the projection would force a full `decode_trie_path` walk to answer a question
/// the trie already folded as the entries arrived.
///
/// ⚠ The fold is EXACT, not conservative. A conservative `false` would move a
/// ground map off field 8 and onto the field walk, which is a consensus-visible
/// byte change; see `EntryTrie::remove_greatest_entry`, which recomputes rather
/// than weakening it.
pub fn eval_stable_epathmap(e_pathmap: &EPathMap) -> bool {
    e_pathmap.remainder.is_none()
        && e_pathmap.locally_free.is_empty()
        && !e_pathmap.connective_used
        && e_pathmap.entry_trie().entries_stable()
}

/// A Par in ground normal form: either a single-expr carrier over the stable
/// expr alphabet, or the reflect GPrivate leaf. Every other Par field must
/// be empty and `locally_free`/`connective_used` at their ground defaults.
///
/// `pub(crate)` since the canonical path codec (`canonical_path.rs`) uses THIS
/// function as its ground-domain gate — the codec grammar and `eval_stable_par`
/// must remain ONE grammar, pinned by the codec's agreement property test.
/// ★★ Native `Par` levels one classification may descend before SUSPENDING onto
/// the heap.
///
/// # Why a budget and not a plain worklist
///
/// This predicate is on the **encode path** — it selects proto field 8 versus the
/// tag-1 field walk, and `canonical_path.rs` calls it once per segment of every
/// trie key. A plain worklist would allocate a `Vec` on every call, turning a
/// zero-allocation predicate into an allocating one on the hottest path there is.
/// A budget keeps the shallow case **exactly** as cheap as the recursion it
/// replaces (`Vec::new()` does not allocate until its first `push`) and makes the
/// deep case finite.
///
/// This is the shape `88ec2734` established for the derived `Clone`: the budget is
/// threaded **unchanged** through a residual hop and decremented **only** at a
/// cut-set site, so native frames are bounded by *schema and budget, never by the
/// term*. Here the cut-set site is a `Par` inside an `EList`/`ETuple`, which is
/// the only edge the classifier's recursion can cycle through.
///
/// # Why 64
///
/// Two native frames per `Par` level ([`eval_stable_par_budgeted`] and
/// [`eval_stable_expr_budgeted`]), both small — no arrays, no by-value `Par`, only
/// references and a `u32`. 64 levels is therefore a few KiB, which fits inside the
/// smallest stack this workspace measures on (a 256 KiB probe thread,
/// `models/tests/trie_escape_arm_stack.rs`) with two orders of magnitude of
/// headroom, while covering every term depth a real contract produces.
const STABILITY_DESCEND_BUDGET: u32 = 64;

/// ★★ A `Par` in ground normal form. **Iterative past
/// [`STABILITY_DESCEND_BUDGET`] levels.**
///
/// # The defect this replaces
///
/// `eval_stable_par` and `eval_stable_expr` were **mutually recursive** with no
/// bound, descending through `EList.ps` and `ETuple.ps`. That put a Θ(depth)
/// native-stack traversal on `canonical_path.rs::encode_trie_path`, which is
/// **required total** (R3F-2: *"trie keys must build for every legal runtime
/// value"*) and whose module header promises *"unlimited depth; iterative; no
/// panics."* It could not honour either: a sufficiently nested entry aborted the
/// process inside a function documented not to.
///
/// ★ It was found by *bisection*, not by reading: a 256 KiB probe thread that was
/// built to show the escape arm's protobuf encode had become stack-safe overflowed
/// anyway, and the encoder was not the frame that ran out.
///
/// ⚠ A crash is strictly worse than a rejection. A clean `Err` is a decision every
/// node reaches identically; a `SIGSEGV` is a liveness failure of whichever node
/// was asked first, on input a peer controls.
///
/// # Why the answer cannot change
///
/// The predicate is a **conjunction over a tree with no side effects**, so its
/// value does not depend on the order the conjuncts are evaluated in. `Iterator::
/// all` short-circuits at the first `false`; this version returns at the first
/// `false` it *reaches*, which may be a different one — and the boolean is the
/// same. Nothing else observes which. That matters because the answer selects a
/// wire arm: `eval_stable_par` is exact rather than conservative, and a
/// conservative `false` here would move a ground map off field 8, which is a
/// consensus-visible byte change.
pub(crate) fn eval_stable_par(par: &Par) -> bool {
    // ★ NOT preallocated, deliberately. `Vec::new()` performs no allocation, and
    // the overwhelming majority of calls never push: a term shallower than
    // `STABILITY_DESCEND_BUDGET` is classified entirely in native frames, exactly
    // as before. Reserving capacity here would add an allocation to every call on
    // the encode path in order to speed up the case that a hostile input reaches.
    let mut deferred: Vec<&Par> = Vec::new();
    if !eval_stable_par_budgeted(par, STABILITY_DESCEND_BUDGET, &mut deferred) {
        return false;
    }
    // Each suspended `Par` restarts with a FULL budget — the same rule the clone
    // driver uses: the budget bounds one descent's native frames, not the whole
    // traversal. Termination is by the term being finite: every iteration pops one
    // node and pushes only that node's proper descendants.
    while let Some(next) = deferred.pop() {
        if !eval_stable_par_budgeted(next, STABILITY_DESCEND_BUDGET, &mut deferred) {
            return false;
        }
    }
    true
}

/// One `Par` level of [`eval_stable_par`], with `budget` native levels left.
fn eval_stable_par_budgeted<'p>(
    par: &'p Par,
    budget: u32,
    deferred: &mut Vec<&'p Par>,
) -> bool {
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
        ([expr], []) => eval_stable_expr_budgeted(expr, budget, deferred),
        // Nil Pars, multi-expr Pars, expr+unforgeable mixes, multiple
        // unforgeables: all conservatively unstable.
        _ => false,
    }
}

/// The stable expr alphabet (see [`eval_stable_epathmap`]).
///
/// ⚠ `budget` is threaded **unchanged** into this hop and decremented only where a
/// child `Par` is entered. An `Expr` is not a level of the cycle — the cycle is
/// `Par → Expr → EList|ETuple → Par` — so charging it a level would make the
/// budget mean something other than "Par levels", and the residual relation
/// (everything except that one edge) is acyclic.
fn eval_stable_expr_budgeted<'p>(
    expr: &'p Expr,
    budget: u32,
    deferred: &mut Vec<&'p Par>,
) -> bool {
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
                && stable_children(&list.ps, budget, deferred)
        }
        Some(ExprInstance::ETupleBody(tuple)) => {
            tuple.locally_free.is_empty()
                && !tuple.connective_used
                && stable_children(&tuple.ps, budget, deferred)
        }
        // ★ O(1): the entry half is the `EntryTrie`'s fold, maintained as entries
        // arrive. This arm never descends, which is why an `EPathMap` nested inside
        // a key costs the classifier no depth at all.
        Some(ExprInstance::EPathmapBody(inner)) => eval_stable_epathmap(inner),
        // EVar, ESet, EMap, EMethod, EZipper, operators, connective exprs,
        // interpolation/concatenation, None: conservatively unstable.
        _ => false,
    }
}

/// ★ THE CUT-SET SITE — the one edge the classifier's recursion cycles through,
/// and therefore the one place the budget is spent.
///
/// With budget remaining, a child is entered natively (no allocation). With the
/// budget spent, it is pushed onto the heap worklist and
/// [`eval_stable_par`]'s loop picks it up with a fresh budget.
///
/// ⚠ Suspension is **not** an answer: a deferred child is `true`-so-far, and the
/// loop that owns it will return `false` if it turns out unstable. That is why the
/// deferral must go to the caller's worklist and not be dropped.
#[inline]
fn stable_children<'p>(children: &'p [Par], budget: u32, deferred: &mut Vec<&'p Par>) -> bool {
    match budget {
        0 => {
            deferred.extend(children.iter());
            true
        }
        remaining => children
            .iter()
            .all(|child| eval_stable_par_budgeted(child, remaining - 1, deferred)),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The intern store
// ─────────────────────────────────────────────────────────────────────────────

/// Intern an EPathMap: return the shared [`InternedEPathMap`] for its
/// canonical prost bytes.
///
/// P3: the instance's SHADOW CELL is consulted FIRST (`EPathMap::intern`) —
/// filled ⇒ an O(1) `Arc` clone with NO digest walk, NO store lock, NO K2
/// verify (the post-P2 profile's #1 residual was exactly this rendezvous
/// re-walking the map per call). Unfilled ⇒ one [`intern_epathmap_via_store`]
/// pass fills the cell (store hit or build), and the handle then travels
/// with every later `Clone` of the instance. All 56 P1 call sites and P2's
/// fused-chain rendezvous inherit the O(1) path through this one function.
pub fn interned_epathmap(e_pathmap: &EPathMap) -> Arc<InternedEPathMap> {
    e_pathmap.intern()
}

/// The store rendezvous behind the shadow cell (P1's intern body, unchanged
/// in substance): build (trie + canonical encoding + classifier) on first
/// sight, dedup by content otherwise.
///
/// Called ONLY from `EPathMap::intern`'s `OnceLock::get_or_init` (the cell
/// is empty for the whole call, so every `encode_raw` below is the field
/// walk; nested entry-level EPathMaps may still serve their own filled
/// cells, which is byte-correct under the cell invariant).
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
pub(crate) fn intern_epathmap_via_store(e_pathmap: &EPathMap) -> Arc<InternedEPathMap> {
    // GROUND fast-path (non-empty eval_stable map): content-address by U(m).
    // ★ The trie is no longer BUILT here — `e_pathmap` is holding it. What the
    // store still provides is the canonical BYTES (and, on a hit, one shared
    // `Arc` for every equal map in the process), which is a genuinely different
    // artifact and still costs a walk to produce.
    if eval_stable_epathmap(e_pathmap) && !e_pathmap.entry_trie().is_empty() {
        let path_stream = e_pathmap.path_stream();
        let canonical_prost = ground_canonical_prost(&path_stream);
        let digest = blake2b_256(&canonical_prost);
        {
            let mut store = intern_store()
                .lock()
                .expect("EPathMap intern store mutex poisoned");
            if let Some((last_use, bucket)) = store.get_mut(&digest) {
                for entry in bucket.iter() {
                    if entry.canonical_prost == canonical_prost {
                        *last_use = next_intern_tick();
                        return Arc::clone(entry);
                    }
                }
                note_digest_collision();
            }
        }
        let encoded_len = canonical_prost.len();
        return store_insert(Arc::new(InternedEPathMap {
            map: e_pathmap.entry_trie().trie().clone(),
            connective_used: e_pathmap.entry_trie().any_connective_used(),
            locally_free: e_pathmap.entry_trie().union_locally_free().to_vec(),
            path_stream,
            canonical_prost,
            encoded_len,
            digest,
            entry_count: e_pathmap.entry_trie().len(),
            eval_stable: true,
            serde_bytes: OnceLock::new(),
        }));
    }

    // NON-GROUND (or empty) map: the pre-wire field walk (`ps` at tag 1 + the
    // 3/4/5 metadata fields), unchanged — zero-alloc streamed-digest lookup,
    // build + insert on miss.
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
    let entry = Arc::new(InternedEPathMap {
        map: e_pathmap.entry_trie().trie().clone(),
        connective_used: e_pathmap.entry_trie().any_connective_used()
            || e_pathmap.remainder.is_some(),
        locally_free: e_pathmap.entry_trie().union_locally_free().to_vec(),
        path_stream: Vec::new(),
        canonical_prost,
        encoded_len,
        digest,
        entry_count: e_pathmap.entry_trie().len(),
        eval_stable: eval_stable_epathmap(e_pathmap),
        serde_bytes: OnceLock::new(),
    });

    store_insert(entry)
}

// ─────────────────────────────────────────────────────────────────────────────
// U(m) helpers + the shared store rendezvous (EPathMap native byte-array wire)
// ─────────────────────────────────────────────────────────────────────────────

/// Blake2b-256 of raw bytes — the digest of a `canonical_prost` preimage.
fn blake2b_256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// U(m) from a built trie: a read-zipper walk yielding, in trie order (NO
/// sort), `repeat( u32-LE keylen ++ trie_key )`. This is the uncompressed twin
/// of PathMap's `serialize_paths` walk (minus the disqualifying deflate) — the
/// canonical, insertion-order-independent value serialization of a ground map.
pub(crate) fn path_stream_of(map: &RholangPathMap) -> Vec<u8> {
    use pathmap::zipper::{ZipperIteration, ZipperMoving};
    let mut rz = map.read_zipper();
    let mut stream = Vec::new();
    while rz.to_next_val() {
        let key = rz.path();
        stream.extend_from_slice(&(key.len() as u32).to_le_bytes());
        stream.extend_from_slice(key);
    }
    stream
}

/// Emit proto field 8 (`serialized_paths`, length-delimited bytes) = U(m).
pub(crate) fn encode_ground_field8(path_stream: &[u8], buf: &mut impl BufMut) {
    prost::encoding::encode_key(8u32, prost::encoding::WireType::LengthDelimited, buf);
    prost::encoding::encode_varint(path_stream.len() as u64, buf);
    buf.put_slice(path_stream);
}

/// `encoded_len` of proto field 8 (`serialized_paths`) = U(m).
pub(crate) fn ground_field8_len(path_stream: &[u8]) -> usize {
    prost::encoding::key_len(8u32)
        + prost::encoding::encoded_len_varint(path_stream.len() as u64)
        + path_stream.len()
}

/// A ground map's canonical prost bytes: JUST field 8 (`serialized_paths` =
/// U(m)). `locally_free`/`connective_used`/`remainder` are at prost defaults
/// (eval_stable ⇒ empty) and omitted.
pub(crate) fn ground_canonical_prost(path_stream: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(ground_field8_len(path_stream));
    encode_ground_field8(path_stream, &mut buf);
    buf
}

/// The KEY-side reading of a trie's contents: every key decoded through
/// `decode_trie_path`, in trie order.
///
/// # ⚠ This is a CHECK, not an answer
///
/// `EntryTrie::view` — the projection every consumer reads — walks the VALUE
/// side, and the reason is measured rather than stylistic: **this function is
/// PARTIAL on the codec's own image.** The escape arm files a non-ground entry
/// as its canonical prost bytes, prost's decoder caps recursion at 100 levels
/// and prost's encoder caps nothing, so a sufficiently deep entry produces a key
/// that this function cannot decode (`RecursionLimitReached`) even though the
/// trie is perfectly well-formed. The `par_codec_differential` corpus contains
/// such a term; it is not a hypothetical.
///
/// What the key side is still good for is stating the trie ENTRY INVARIANT
/// (`∀(k,v). encode_trie_path(v) = k`) from the other direction. Note that
/// `pathmap_integration::trie_entry_divergences` and `EntryTrie::adopt_trie`
/// both check it in the ENCODE direction instead, precisely so that the check
/// stays total; this function is retained for the root-key divergence pin below,
/// which needs the decoding reading specifically.
#[cfg(test)]
pub(crate) fn canonical_ps_from_trie(map: &RholangPathMap) -> Vec<Par> {
    use pathmap::zipper::{ZipperIteration, ZipperMoving};
    let mut rz = map.read_zipper();
    let mut ps = Vec::new();
    while rz.to_next_val() {
        let key = rz.path();
        ps.push(decode_trie_path(key).expect("an intern trie key is always a valid codec path"));
    }
    ps
}

/// The shared store rendezvous: re-scan the digest bucket (a racing thread may
/// have interned the same bytes while we built) and reuse its `Arc`, else
/// insert (LRU-evicting a bucket when the store is at capacity). Keeps every
/// collision list pairwise byte-distinct.
fn store_insert(entry: Arc<InternedEPathMap>) -> Arc<InternedEPathMap> {
    let digest = entry.digest;
    let mut store = intern_store()
        .lock()
        .expect("EPathMap intern store mutex poisoned");
    let tick = next_intern_tick();
    if let Some((last_use, bucket)) = store.get_mut(&digest) {
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

/// TEST SEAM: the ground-domain predicate on ONE entry — the per-element half
/// of `eval_stable_epathmap`, exposed so an integration test can re-derive the
/// `EntryTrie`'s O(1) fold from the projection and check the two agree.
#[doc(hidden)]
pub fn eval_stable_par_for_test(par: &Par) -> bool {
    eval_stable_par(par)
}

/// TEST SEAM: number of digest buckets currently in the store.
///
/// ⚠ This is an ABSOLUTE reading of process-global state. A test may compare it
/// against a constant only when it has just called [`clear_intern_store_for_test`]
/// and holds exclusive access against every other store-touching test in its
/// binary. To ask the weaker, far more common question *"did this call touch the
/// store?"* use [`intern_store_touches_for_test`] instead — a bucket count
/// cannot answer it (see that function's note on LRU eviction).
#[doc(hidden)]
pub fn intern_store_len_for_test() -> usize {
    intern_store()
        .lock()
        .expect("EPathMap intern store mutex poisoned")
        .len()
}

/// TEST SEAM: the monotone count of STORE TOUCHES this process has performed —
/// the LRU tick, which is bumped exactly once per store hit and once per insert.
///
/// # Why this and not the bucket count
///
/// The question *"did this call reach the store?"* has three plausible
/// observables and two of them are unsound:
///
/// * **`intern_store_len_for_test() == 0`** is an absolute over process-global
///   state. It answers "has *anything in this process* ever interned", which is
///   a property of the test binary's schedule, not of the call under test. It is
///   the assertion this seam was introduced to replace.
/// * **the DELTA of `intern_store_len_for_test()`** is unsound twice over. A
///   concurrent thread may intern inside the measurement window; and — the
///   sharper failure — once the store reaches [`INTERN_CAPACITY`] every insert
///   LRU-evicts one bucket and adds one, so the length delta is **zero for a
///   call that did touch the store**. A full test binary drives the store to
///   capacity routinely, so that assertion would read green precisely where it
///   was supposed to read red.
/// * **this counter's delta** is immune to both. [`next_intern_tick`] is called
///   on every hit (`*last_use = next_intern_tick()`) and unconditionally by
///   [`store_insert`], always while the store mutex is held, and it is
///   monotone — eviction cannot mask it. So *store touched ⟺ this value
///   advanced*, and a zero delta over a window in which no other thread may
///   intern is exactly "the call did not reach the store".
///
/// The window still has to be exclusive: this is a process-global too, and the
/// delta is only attributable to the measured call if nothing else interns
/// while it runs. Under `cargo test` that means every store-touching test in
/// the same binary must be excluded for the duration (each `tests/*.rs` is its
/// own process, so the obligation is file-local).
#[doc(hidden)]
pub fn intern_store_touches_for_test() -> u64 {
    INTERN_TICK.load(Ordering::Relaxed)
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
    /// # ★ There is no conversion left to do
    ///
    /// This used to be *"build a trie from `ps`"*, and because that rebuild ran
    /// on EVERY EPathMap method dispatch it was worth an entire content-addressed
    /// intern store to avoid — a streamed Blake2b digest walk, a global mutex, a
    /// K2 full-prost byte verify, an LRU, and a shadow cell to skip all of it.
    ///
    /// The map is holding the trie. So the conversion is three field reads: an
    /// O(1) `clone()` of the trie handle (a refcount bump on the root
    /// `TrieNodeODRc`) and the two entry folds the [`EntryTrie`] maintains as
    /// entries arrive. **No digest, no lock, no verify, no cell** — not as an
    /// optimization but because there is nothing here that could go wrong to be
    /// protected against.
    ///
    /// The store survives for the artifact it still genuinely produces: the
    /// canonical BYTES (`canonical_prost`, `encoded_len`, the lazy
    /// `serde_bytes`), which cost a walk no matter who holds the trie, and the
    /// shared `Arc` that lets every equal map in the process quote them once.
    ///
    /// `remainder` folds into `connective_used` here rather than in the trie
    /// because it is the map's metadata, not one of its entries — the same
    /// division `create_pathmap_from_elements` made.
    ///
    /// Replay determinism and cost accounting are unaffected: no `reserve_*`
    /// charge lives in this layer, and the result is a pure function of the map.
    pub fn e_pathmap_to_rholang_pathmap(e_pathmap: &EPathMap) -> PathMapCreationResult {
        let entries = e_pathmap.entry_trie();
        PathMapCreationResult {
            map: entries.trie().clone(),
            connective_used: entries.any_connective_used() || e_pathmap.remainder.is_some(),
            locally_free: entries.union_locally_free().to_vec(),
        }
    }

    /// Convert from PathMap back to protobuf EPathMap.
    ///
    /// # ★ There is ONE reader, and it reads the KEYS
    ///
    /// This converter used to walk the trie's **values** and discard its keys,
    /// while [`canonical_ps_from_trie`] — the serde / event-hash reader — walked
    /// the **keys** and decoded them. Two readers, opposite directions, agreeing
    /// only while every value encoded back to the key it was filed under. The
    /// tree therefore carried an invariant, a `debug_assert` at this line, a
    /// divergence type and a renderer, purely to police that agreement — and it
    /// was policed because it had **already failed twice**: #89 (two readers
    /// reporting different contents for one map) and #108 (a reader building a
    /// key in the image of no `Par`).
    ///
    /// Both routes are now [`canonical_ps_from_trie`], so the agreement is no
    /// longer checked here. There is nothing left to check: **a single reader
    /// cannot disagree with itself.** #89 and #91 are not fixed at this line,
    /// they are unrepresentable — the second reader does not exist to be wrong.
    ///
    /// # Why the KEY side is the one that survives
    ///
    /// A `RholangPathMap` is a *set of `Par` entries indexed by their own codec
    /// path*, so the key IS the entry and the value is a redundant mirror of it.
    /// Of the two mirrors, the key is the one the rest of the system already
    /// trusts:
    ///
    /// * the trie's order is its keys' byte-lexicographic order, so this walk is
    ///   the SAME walk that produces `U(m)`, the consensus field-8 path stream.
    ///   The reducer's answer and the event-hash preimage are now one traversal
    ///   rather than two that must be kept in agreement;
    /// * `PathMap::iter()` YIELDS a value stored at the empty (root) key while
    ///   `ZipperIteration::to_next_val()` SKIPS it, so the value side could
    ///   report an entry that the codec can name no `Par` for — `[]` is in the
    ///   image of no `Par`;
    /// * `decode ∘ encode` is the codec's canonical fixed point, so an entry
    ///   read through its key comes back recursively canonical — a nested map
    ///   included. The value side reproduced whatever order its producer used,
    ///   which is the order this campaign has just finished removing from the
    ///   comparators.
    ///
    /// The `connective_used` / `locally_free` / `remainder` arguments are
    /// unchanged: they are the map's metadata, not its entries, and no trie
    /// carries them.
    ///
    /// # ⚠ The entry-invariant guard STAYS, and its subject has narrowed
    ///
    /// It would be tidy to delete the `debug_assert` below along with the value
    /// walk it used to protect. That would be wrong, and the reason is worth
    /// stating because it is easy to miss: **the value side is still read** —
    /// just not in bulk. `getLeaf` (`reduce.rs`), the fused chain's `get`
    /// (`fused_pathmap_chain.rs`) and `values_with_prefix`
    /// (`pathmap_native_query.rs`) all answer with `map.get(key)`, and that
    /// answer equals `decode_trie_path(key)` — what this converter now returns —
    /// only while `∀(k,v). encode_trie_path(v) = k`.
    ///
    /// So the invariant is not vacuous; its subject moved from *"the two bulk
    /// readers agree"* to *"a point lookup agrees with the enumeration"*. This
    /// converter remains the single point every trie in the system passes
    /// through on its way back to a value, which is what makes it the one place
    /// the check can be made at all, so the check stays here. Release builds
    /// (consensus nodes) compile it out; the whole test corpus runs with it on.
    ///
    /// It becomes genuinely vacuous only when the value slot stops mirroring the
    /// key — that is, when per-key values land — and it should be deleted THEN,
    /// with the redundancy it polices, and not before.
    pub fn rholang_pathmap_to_e_pathmap(
        map: &RholangPathMap,
        connective_used: bool,
        locally_free: &[u8],
        remainder: Option<Var>,
    ) -> EPathMap {
        #[cfg(debug_assertions)]
        {
            use crate::rust::pathmap_integration::{
                render_trie_entry_divergences, trie_entry_divergences,
            };
            let divergences = trie_entry_divergences(map);
            assert!(
                divergences.is_empty(),
                "trie entry invariant violated in {} of {} entries — a point \
                 lookup (`getLeaf`, the fused chain's `get`, `values_with_prefix`) \
                 answers with the VALUE while this converter answers with the KEY, \
                 so these entries make the two disagree, and distinct keys sharing \
                 one value COLLAPSE on the next re-insertion (entries are lost):{}",
                divergences.len(),
                map.iter().count(),
                render_trie_entry_divergences(&divergences),
            );
        }

        // ★ The trie is ADOPTED, not re-filed. `EPathMap::new(canonical_ps_from_trie(map))`
        // would decode every key to a `Par` and then immediately re-encode every
        // `Par` back to the key it came from — two walks to arrive at the trie
        // that was passed in. `EntryTrie::adopt_trie` keeps the trie, memoizes
        // the decode as the projection, and verifies the keys are canonical
        // (re-filing if they are not) IN RELEASE BUILDS, where the guard above
        // is compiled out.
        //
        // The result is a NEW value with an EMPTY shadow cell: it was just built
        // from a trie, and its canonical bytes are not those of any interned
        // source.
        EPathMap::new(
            EntryTrie::adopt_trie(map),
            locally_free.to_vec(),
            connective_used,
            remainder,
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE ROOT-KEY DIVERGENCE — pinned, because it is UNREACHABLE rather than
//   impossible, and the two trie WALKS do NOT agree about it
// ─────────────────────────────────────────────────────────────────────────────
//
// `PathMap::iter()` YIELDS a value stored at the empty (root) key;
// `ZipperIteration::to_next_val()` SKIPS it. A root value is therefore visible
// to one walk and invisible to the other.
//
// ⚠ This used to be a divergence between two READERS —
// [`PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap`] walked `iter()` and
// [`canonical_ps_from_trie`] walked `to_next_val()`, so a root value was kept by
// one and dropped by the other. Both converters now take the key side, so the
// two walks are no longer two answers about a map's contents. The pin remains,
// because the ASYMMETRY between the two walks remains and every future consumer
// of `iter()` inherits it.
//
// It is unreachable TODAY because `encode_trie_path` is total and emits at
// least one byte for every Par (the bare arm emits a tag; the split arm emits
// the `0x00` terminator), so no producer keying through the codec can file a
// value at `[]`. That is a property of the codec, not of the callers — which is
// exactly why it is asserted here rather than assumed. Note the consequence
// recorded below: `trie_entry_divergences` classifies a root value as a
// divergence FOR FREE, since `[]` is in the image of no Par.
#[cfg(test)]
mod root_key_divergence {
    use super::*;
    use crate::rhoapi::EList;
    use crate::rust::canonical_path::encode_trie_path;
    use crate::rust::pathmap_integration::{create_pathmap_from_elements, trie_entry_divergences};

    fn gint(i: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i)),
        }])
    }

    fn gstring(s: &str) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString(s.to_string())),
        }])
    }

    fn list(ps: Vec<Par>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    /// WHY the root key is unreachable: the codec never produces it. This is
    /// the premise the whole pin rests on, so it is checked, not asserted in
    /// prose — over both arms and the nested/empty edges.
    #[test]
    fn encode_trie_path_never_yields_the_empty_key() {
        for par in [
            Par::default(),
            gint(0),
            gint(-1),
            gstring(""),
            list(Vec::new()),
            list(vec![gint(1)]),
            list(vec![gstring("a"), list(vec![gint(2)])]),
        ] {
            assert!(
                !encode_trie_path(&par).is_empty(),
                "the codec emitted an EMPTY key for {par:?} — the root key would \
                 become reachable, and the two trie readers disagree about it"
            );
        }
    }

    /// ★ The two WALKS disagree about a root value — demonstrated on a trie
    /// that only a direct `insert` can build. This is why the empty key must
    /// stay outside the codec's image, and why a value found there is treated
    /// as a divergence rather than as an entry.
    #[test]
    fn a_root_value_is_kept_by_the_value_walk_and_dropped_by_the_key_walk() {
        let ordinary = gint(1);
        let mut map = RholangPathMap::new();
        map.insert(Vec::<u8>::new(), gstring("only reachable by hand"));
        map.insert(encode_trie_path(&ordinary), ordinary.clone());

        // VALUE-side reader: `iter()` yields the root value, so it is kept.
        let by_value: Vec<Par> = map.iter().map(|(_, par)| par.clone()).collect();
        assert_eq!(
            by_value.len(),
            2,
            "`iter()` yields the root value — the value-side reader keeps it"
        );

        // KEY-side reader: `to_next_val()` skips it, so it is dropped. This is
        // `canonical_ps_from_trie` itself, not a re-implementation — it would
        // panic on the root key's `decode_trie_path`, which is the second half
        // of the same divergence, so the walk is exercised through the guard
        // below instead of by calling it on this map.
        {
            use pathmap::zipper::{ZipperIteration, ZipperMoving};
            let mut rz = map.read_zipper();
            let mut keys = Vec::new();
            while rz.to_next_val() {
                keys.push(rz.path().to_vec());
            }
            assert_eq!(
                keys,
                vec![encode_trie_path(&ordinary)],
                "`to_next_val()` skips the root value — the key-side reader \
                 (`canonical_ps_from_trie`) drops it"
            );
        }

        // …and the invariant checker names it, WITHOUT a special case: `[]` is
        // in the image of no Par, so a root value is a divergence by the same
        // rule that catches every other mis-filed entry.
        let divergences = trie_entry_divergences(&map);
        assert_eq!(divergences.len(), 1, "exactly the root entry diverges");
        assert!(
            divergences[0].key.is_empty(),
            "the divergence named is the ROOT key"
        );
        assert!(
            !divergences[0].value_key.is_empty(),
            "…whose value claims a NON-empty key, which is the whole point"
        );
    }

    /// `canonical_ps_from_trie` on a well-formed trie agrees ENTRY FOR ENTRY
    /// with the value walk — the positive half of the same statement, so the pin
    /// above is a witness of divergence and not of a broken walk.
    ///
    /// ★ This is what keeps the entry invariant load-bearing after the bulk
    /// converters moved to the key side. The reducer still reads VALUES at every
    /// point lookup — `getLeaf` and the fused chain's `get` return
    /// `map.get(key)` — and those answers equal `decode_trie_path(key)` only
    /// while `∀(k,v). encode_trie_path(v) = k`. The invariant no longer
    /// reconciles two bulk readers; it reconciles the point lookups with the
    /// bulk one.
    #[test]
    fn the_point_lookup_value_agrees_with_the_key_when_the_invariant_holds() {
        let elements = vec![
            gint(1),
            list(vec![gint(1)]),
            gstring("a"),
            list(vec![gstring("a"), gstring("x")]),
        ];
        let built = create_pathmap_from_elements(&elements, None);
        assert!(
            trie_entry_divergences(&built.map).is_empty(),
            "the sole program-facing constructor upholds the invariant"
        );

        let by_key = canonical_ps_from_trie(&built.map);
        let by_value: Vec<Par> = built.map.iter().map(|(_, par)| par.clone()).collect();
        assert_eq!(
            by_key, by_value,
            "KEY-side and VALUE-side readers must return the same entries in \
             the same order when the invariant holds"
        );
    }
}
