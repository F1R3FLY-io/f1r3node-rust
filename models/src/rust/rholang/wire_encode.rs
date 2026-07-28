//! # `wire_encode` — the O(1)-native-stack, single-walk cold-store ENCODER
//!
//! The write-side twin of [`crate::rust::rholang::par_codec`]. Same
//! trampolining discipline, same generated table
//! ([`crate::rust::rholang::wire_schema`]), opposite direction — and one
//! structural asymmetry, spelled out in §2.
//!
//! ---
//!
//! ## 1. What this replaces, and why it is faster rather than merely flatter
//!
//! `bincode::serialize` **traverses the term twice** (bincode 1.3.3,
//! `src/internal.rs:25-37`):
//!
//! ```text
//!    let size = serialized_size(value)?;        ← a COMPLETE recursive walk
//!    let mut out = Vec::with_capacity(size);
//!    serialize_into(&mut out, value)?;          ← the SAME walk again
//! ```
//!
//! So the win here is not shaving a tight loop: it is **deleting an entire
//! traversal**. Everything else — the flat native stack, the reused output
//! buffer — is additive on top of that.
//!
//! ```text
//!                     traversals   steady-state allocation
//!    ───────────────  ──────────   ───────────────────────
//!    bincode 1.3.3         2       a fresh `Vec` every call
//!    one walk, fresh Vec   1       up to 2× over, every call
//!    ★ this module         1       ZERO (thread-local buffer)
//! ```
//!
//! That table is the whole argument for the design. Exact sizing buys minimal
//! space at the price of a second walk; geometric growth buys one walk at the
//! price of slack — and **buffer reuse dissolves the trade**, because a
//! `clear()`ed thread-local converges to its high-water mark and then
//! allocates nothing while still walking once. `hash_produce` runs per
//! produce, so this is the highest-frequency encode in the node.
//!
//! ## 2. ⚠ The asymmetry with the decoder
//!
//! The decoder reassembles **bottom-up**, so a child's value must be parked
//! until its parent is ready to take it — hence `par_codec`'s eighteen
//! per-type value stacks, and hence its `Drop`-time `dismantle_all` salvage (a
//! deep partial result must not abort the process while being *released*).
//!
//! The encoder walks **top-down over borrows**. Every op holds `&'a T` into a
//! term the caller owns:
//!
//! * there are **no value stacks** — nothing is built, only read;
//! * there are **no clones** and no ownership on the op stack, so
//! * there is **no `Drop` obligation**, and an error path would be O(1) native
//!   stack for free.
//!
//! ## 3. The machine
//!
//! ```text
//!            ┌──────────────────────────────────────────────────┐
//!            │  ops : Vec<Op<'a>>      (the OBLIGATION stack)    │
//!            │    …                                             │
//!            │    Node{ &Par,  field: 4 }   ← resume point       │
//!            │    Seq { &[Expr], index: 2 } ← counted repeat     │
//!            │    Node{ &Expr, field: 0 }   ← next to run        │  ◀── pop
//!            └──────────────────────────────────────────────────┘
//!                                  │ emits bytes, pushes successors
//!                                  ▼
//!            ┌──────────────────────────────────────────────────┐
//!            │  out : &mut Vec<u8>   (thread-local, reused)      │
//!            └──────────────────────────────────────────────────┘
//! ```
//!
//! Three properties make it flat, small, and single-pass:
//!
//! * **Leaves never touch the op stack.** [`Machine::run_node`] *loops* over a
//!   node's program, emitting bounded fields inline, and pushes a resume point
//!   only when it must descend. A node whose remaining fields are all scalars
//!   costs zero entries.
//!
//! * **Counted repeats are never materialised.** `Op::Seq` re-pushes itself
//!   with `index + 1` and exactly one child, so a 1,000,000-element sequence
//!   costs **one** entry, not a million. ★ This is the difference between an op
//!   stack that is Θ(term DEPTH) and one that is Θ(term SIZE), and it is
//!   asserted by *measurement* (`models/tests/wire_encode_space.rs`), because a
//!   bug that pushed children eagerly would pass every correctness test.
//!
//! * **A spent program is a tail call.** [`Machine::suspend`] pushes nothing
//!   when the descending field was the node's last, so the deep-nesting shape
//!   — `EList` whose only interesting field is its `ps` — does not accumulate
//!   resume points at all.
//!
//! ## 4. ⚠ `EPathMap` — the one program the descriptor cannot express
//!
//! `EPathMap` is `extern_path`'d, and its hand-written `Serialize` re-orders
//! `ps` into canonical trie order for GROUND maps, so that a ground map's
//! event-hash preimage is a pure function of its entry *set* (two producers
//! building the same map in different orders must not emit different bytes).
//! [`crate::rust::rholang::wire::pathmap_ps`] makes that same choice by
//! calling the same two functions.
//!
//! Canonicalization *constructs* a vector, which no borrow-returning accessor
//! can serve, so the machine owns it: [`Machine::park`] moves it into a small
//! arena and `Op::DropOwned` releases it when the subtree completes. At most
//! one parked vector per ancestor ground `EPathMap` is live.
//!
//! ## 5. What byte identity rests on
//!
//! bincode legacy has **no back-references, no framing, and no byte-length
//! prefixes**; structs are positional in declaration order and enums are a
//! `u32` declaration-order index then the payload. Emission order is therefore
//! exactly pre-order, which is exactly what an explicit stack yields. The
//! differential suite (`models/tests/wire_encode_differential.rs`) *confirms*
//! this against the derived `Serialize`; it does not establish it.

use std::cell::RefCell;
use std::collections::btree_map;

use crate::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::wire::{
    pathmap_ps, FieldVal, LeafVal, PathmapPs, Payload, WireNode, WireSeq, EPATHMAP_PROGRAM,
};

// ===========================================================================
// §A  Byte emission — the whole of bincode's legacy layout, in one place
// ===========================================================================

/// `u64` little-endian. Every length, count and `usize` on the wire is this.
#[inline(always)]
fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// A length-prefixed byte payload: `u64` LE length ++ raw bytes.
///
/// Both `serialize_bytes` and `Vec<u8>`-as-a-seq produce exactly this, because
/// each `u8` element of a seq is one byte — the two spellings coincide, so the
/// encoder needs only one.
#[inline(always)]
fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_u64(out, b.len() as u64);
    out.extend_from_slice(b);
}

/// Emit one bounded field. Never descends, never allocates.
#[inline]
fn put_leaf(out: &mut Vec<u8>, v: LeafVal<'_>) {
    match v {
        LeafVal::Bool(b) => out.push(b as u8),
        LeafVal::I32(n) => out.extend_from_slice(&n.to_le_bytes()),
        LeafVal::U32(n) => out.extend_from_slice(&n.to_le_bytes()),
        LeafVal::I64(n) => out.extend_from_slice(&n.to_le_bytes()),
        LeafVal::U64(n) => out.extend_from_slice(&n.to_le_bytes()),
        LeafVal::Bytes(b) => put_bytes(out, b),
        // ⚠ The serialize-only `locally_free` normalization: EIGHT ZERO BYTES,
        // whatever is stored. `models/build.rs` injects the equivalent
        // `serialize_with` on the derived path; the DECODER reads the stream's
        // real length. See `wire::FieldKind`.
        LeafVal::EmptyBytes => put_u64(out, 0),
        LeafVal::Str(s) => put_bytes(out, s.as_bytes()),
        LeafVal::StrSeq(ss) => {
            put_u64(out, ss.len() as u64);
            for s in ss {
                put_bytes(out, s.as_bytes());
            }
        }
        LeafVal::BytesSeq(bs) => {
            put_u64(out, bs.len() as u64);
            for b in bs {
                put_bytes(out, b);
            }
        }
    }
}

// ===========================================================================
// §B  The opcode alphabet
// ===========================================================================

/// One suspended obligation.
///
/// `Copy` and small — `&dyn` is two words, so the widest arm is four.
/// `wire_encode_space::the_op_stack_entry_stays_small` pins the size, because
/// an op that quietly grew would multiply the only per-call heap here.
#[derive(Clone, Copy)]
enum Op<'a> {
    /// Emit fields `[field..]` of `node`.
    Node { node: &'a dyn WireNode, field: u16 },
    /// Emit elements `[index..]` of `seq`. The count was written when the
    /// field was opened.
    Seq { seq: &'a dyn WireSeq, index: usize },
    /// Release parked vectors down to `mark`. Pushed *beneath* the subtree
    /// that reads them, so LIFO makes the release exact.
    DropOwned { mark: usize },
    /// Emit the remaining entries of the `BTreeMap` iterator on top of
    /// `map_iters`: one key, then descend into its value.
    MapEntries,
}

// ===========================================================================
// §C  The machine
// ===========================================================================

/// Preallocated op-stack capacity.
///
/// ⚠ Deliberately a **constant**, not a function of the term. Deriving it from
/// `par_depth` would add a Θ(size) pass costing more than the handful of
/// reallocations it saves, and on the dominant shallow terms it would be pure
/// loss. Geometric growth handles the tail.
const OP_STACK_CAPACITY: usize = 64;

/// Initial capacity of a fresh output buffer.
const OUT_CAPACITY: usize = 4096;

/// The largest op-stack allocation worth keeping in the per-thread pool.
///
/// 4,096 entries × 32 B = 128 KiB, which covers a 2,048-deep term. A deeper
/// one-off is *not* parked, so a single pathological encode cannot pin its
/// high-water mark for the life of the thread — the same policy, and the same
/// reasoning, as [`SHRINK_THRESHOLD`] for the output buffer.
const MAX_POOLED_OPS: usize = 4096;

thread_local! {
    /// The pooled op-stack **allocation**.
    ///
    /// ★ This is the last per-encode allocation. The output buffer is already
    /// reused; without this, every `hash_produce` would still pay one
    /// `Vec::with_capacity(64)` malloc/free pair — small, but per produce, and
    /// "zero allocation in the steady state" is a stated acceptance criterion
    /// rather than an aspiration.
    ///
    /// ⚠ The parameter is `'static` because a thread-local cannot be generic
    /// over a caller's lifetime. It is **always empty while parked**, so no
    /// `Op<'static>` value ever exists — see [`take_ops`].
    static OPS: RefCell<Vec<Op<'static>>> = RefCell::new(Vec::with_capacity(OP_STACK_CAPACITY));
}

/// Borrow the pooled op-stack allocation with the caller's lifetime.
///
/// # Soundness
///
/// The parked vector is **empty** (asserted), so the transmute re-types zero
/// live values — only the heap allocation is carried across, which is the
/// standard buffer-recycling idiom. `Op<'a>` and `Op<'static>` are
/// layout-identical: lifetimes are erased before codegen and appear in no
/// discriminant, size or alignment. [`give_ops`] clears before parking, so the
/// emptiness invariant is restored on every path, including a panic (the
/// machine's [`Drop`] returns the buffer).
///
/// If the slot is already taken — a nested encode, which the spliced event-hash
/// emitter can produce — a private vector is used instead of aliasing.
fn take_ops<'a>() -> Vec<Op<'a>> {
    OPS.with(|cell| match cell.try_borrow_mut() {
        Ok(mut parked) if parked.is_empty() && parked.capacity() > 0 => {
            let recycled = std::mem::take(&mut *parked);
            debug_assert!(recycled.is_empty(), "the pooled op stack must be parked empty");
            unsafe { std::mem::transmute::<Vec<Op<'static>>, Vec<Op<'a>>>(recycled) }
        }
        _ => Vec::with_capacity(OP_STACK_CAPACITY),
    })
}

/// Return an op-stack allocation to the pool.
///
/// Parks only if the slot is vacant and the capacity is worth keeping, so
/// neither a nested encode nor a one-off deep term can pin memory.
fn give_ops(mut ops: Vec<Op<'_>>) {
    ops.clear();
    if ops.capacity() == 0 || ops.capacity() > MAX_POOLED_OPS {
        return;
    }
    // SAFETY: emptied immediately above; see `take_ops`.
    let parked: Vec<Op<'static>> = unsafe { std::mem::transmute(ops) };
    OPS.with(|cell| {
        if let Ok(mut slot) = cell.try_borrow_mut() {
            // Keep the LARGER of the two, so the pool converges upward to the
            // working set instead of oscillating.
            if slot.capacity() < parked.capacity() {
                *slot = parked;
            }
        }
    });
}

struct Machine<'a> {
    ops: Vec<Op<'a>>,
    /// Canonical `ps` vectors for ground `EPathMap`s, owned for the duration
    /// of their subtree. See [`Machine::park`] for the soundness argument.
    owned: Vec<*mut Vec<Par>>,
    /// One live iterator per ancestor `New` (the `injections` map).
    map_iters: Vec<btree_map::Iter<'a, String, Par>>,
    /// Op-stack high-water mark, tracked only when `TRACK` is on.
    high_water: usize,
}

impl<'a> Drop for Machine<'a> {
    /// Release anything still parked, and return the op-stack allocation.
    ///
    /// The normal path drains `owned` through `Op::DropOwned`; this covers a
    /// panic unwinding out of the loop. Unlike the decoder's teardown this is
    /// O(number of parked vectors) and touches no term structure, because the
    /// encoder owns nothing else — there is no deep `Par` here to dismantle.
    fn drop(&mut self) {
        self.release_to(0);
        give_ops(std::mem::take(&mut self.ops));
    }
}

impl<'a> Machine<'a> {
    fn new() -> Self {
        Machine {
            ops: take_ops(),
            owned: Vec::new(),
            map_iters: Vec::new(),
            high_water: 0,
        }
    }

    /// Drop parked vectors above `mark`.
    fn release_to(&mut self, mark: usize) {
        while self.owned.len() > mark {
            let raw = self.owned.pop().expect("len > mark implies non-empty");
            // SAFETY: `raw` came from `Box::into_raw` in `park` and is popped
            // exactly once, here.
            drop(unsafe { Box::from_raw(raw) });
        }
    }

    /// Park a constructed vector and hand back a reference valid for the rest
    /// of this run.
    ///
    /// # Soundness
    ///
    /// The `Box` is leaked into `self.owned` and freed only by `release_to`,
    /// which is reached either through the `Op::DropOwned` pushed *beneath*
    /// every op that reads the vector (LIFO, so strictly afterwards) or through
    /// [`Drop`] when the machine itself goes away. The returned reference is
    /// stored only in `self.ops`, which cannot outlive the machine, so its real
    /// use window is a subset of the allocation's lifetime. The `'a`
    /// annotation is an over-approximation used to satisfy the borrow checker;
    /// no reference derived from it escapes [`Machine::run`].
    fn park(&mut self, ps: Vec<Par>) -> (usize, &'a Vec<Par>) {
        let mark = self.owned.len();
        let raw = Box::into_raw(Box::new(ps));
        self.owned.push(raw);
        (mark, unsafe { &*raw })
    }

    /// Run to completion.
    ///
    /// `TRACK` is a const generic so the high-water sampling monomorphises
    /// away entirely on the production path — the instrumented and production
    /// machines are then provably the *same* machine, which a separate sampler
    /// loop could not claim.
    fn run<const TRACK: bool>(&mut self, out: &mut Vec<u8>) {
        loop {
            if TRACK && self.ops.len() > self.high_water {
                self.high_water = self.ops.len();
            }
            let Some(op) = self.ops.pop() else { break };
            match op {
                Op::Node { node, field } => self.run_node(out, node, field as usize),
                Op::Seq { seq, index } => {
                    let len = seq.wire_len();
                    if index < len {
                        // ★ Re-push SELF, not `n` children: the op stack stays
                        // Θ(depth) however WIDE the sequence is (measured flat
                        // from 4 to 65,536 siblings).
                        //
                        // ★★ …and not even self, when this is the LAST element.
                        // The final child is a tail call exactly as a node's
                        // final field is, and skipping the re-push halves the
                        // per-level cost of the adversarial shape — a chain of
                        // single-element sequences — from four entries to two.
                        // Measured: 4.000 → 2.000 entries per nesting level.
                        if index + 1 < len {
                            self.ops.push(Op::Seq {
                                seq,
                                index: index + 1,
                            });
                        }
                        self.ops.push(Op::Node {
                            node: seq.wire_get(index),
                            field: 0,
                        });
                    }
                }
                Op::DropOwned { mark } => self.release_to(mark),
                Op::MapEntries => {
                    let next = self
                        .map_iters
                        .last_mut()
                        .expect("wire_encode: MapEntries with no live iterator")
                        .next();
                    match next {
                        Some((key, value)) => {
                            // serde emits a map as COUNT then key/value PAIRS:
                            // the key here, then the arbitrarily deep value.
                            put_bytes(out, key.as_bytes());
                            self.ops.push(Op::MapEntries);
                            self.ops.push(Op::Node {
                                node: value,
                                field: 0,
                            });
                        }
                        None => {
                            self.map_iters.pop();
                        }
                    }
                }
            }
        }
    }

    /// Emit fields `[field..]` of `node`, suspending at the first descent.
    fn run_node(&mut self, out: &mut Vec<u8>, node: &'a dyn WireNode, field: usize) {
        // ⚠ `EPathMap` cannot enter the generic loop at field 0: its `ps` may
        // have to be CONSTRUCTED in canonical order (§4), which no
        // borrow-returning accessor can serve. Intercept, park, rejoin at 1.
        if field == 0 {
            if let Some(map) = node.wire_as_pathmap() {
                self.open_pathmap(out, node, map);
                return;
            }
        }

        let program = node.wire_program();
        let mut i = field;
        while i < program.len() {
            match node.wire_field(i) {
                FieldVal::Leaf(v) => {
                    put_leaf(out, v);
                    i += 1;
                }
                FieldVal::Opt(child) => {
                    out.push(u8::from(child.is_some()));
                    match child {
                        Some(child) => {
                            self.suspend(node, i + 1, program.len());
                            self.ops.push(Op::Node {
                                node: child,
                                field: 0,
                            });
                            return;
                        }
                        None => i += 1,
                    }
                }
                FieldVal::Seq(seq) => {
                    let n = seq.wire_len();
                    put_u64(out, n as u64);
                    if n == 0 {
                        i += 1;
                    } else {
                        self.suspend(node, i + 1, program.len());
                        self.ops.push(Op::Seq { seq, index: 0 });
                        return;
                    }
                }
                FieldVal::Map(map) => {
                    put_u64(out, map.len() as u64);
                    if map.is_empty() {
                        i += 1;
                    } else {
                        self.suspend(node, i + 1, program.len());
                        self.map_iters.push(map.iter());
                        self.ops.push(Op::MapEntries);
                        return;
                    }
                }
                FieldVal::Oneof(variant) => {
                    out.push(u8::from(variant.is_some()));
                    match variant {
                        Some((index, payload)) => {
                            // An enum is a `u32` LE DECLARATION-ORDER index —
                            // never the proto tag — then the payload.
                            out.extend_from_slice(&index.to_le_bytes());
                            match payload {
                                Payload::Leaf(v) => {
                                    put_leaf(out, v);
                                    i += 1;
                                }
                                Payload::Node(child) => {
                                    self.suspend(node, i + 1, program.len());
                                    self.ops.push(Op::Node {
                                        node: child,
                                        field: 0,
                                    });
                                    return;
                                }
                            }
                        }
                        None => i += 1,
                    }
                }
            }
        }
    }

    /// Push a resume point for `node` at `field`, unless the program is spent.
    #[inline]
    fn suspend(&mut self, node: &'a dyn WireNode, field: usize, len: usize) {
        if field < len {
            self.ops.push(Op::Node {
                node,
                field: field as u16,
            });
        }
    }

    /// Open an `EPathMap`: emit its `ps` count, park a canonical vector when
    /// the map is ground, and arrange for fields 1..4 to follow.
    fn open_pathmap(&mut self, out: &mut Vec<u8>, node: &'a dyn WireNode, map: &'a EPathMap) {
        match pathmap_ps(map) {
            PathmapPs::Stored(ps) => {
                put_u64(out, ps.len() as u64);
                self.suspend(node, 1, EPATHMAP_PROGRAM.len());
                if !ps.is_empty() {
                    self.ops.push(Op::Seq { seq: ps, index: 0 });
                }
            }
            PathmapPs::Canonical(ps) => {
                put_u64(out, ps.len() as u64);
                let (mark, parked) = self.park(ps);
                // ⚠ ORDER. `DropOwned` goes on FIRST so it pops LAST — after
                // the whole subtree that reads the slot has been emitted.
                self.ops.push(Op::DropOwned { mark });
                self.suspend(node, 1, EPATHMAP_PROGRAM.len());
                if !parked.is_empty() {
                    self.ops.push(Op::Seq {
                        seq: parked,
                        index: 0,
                    });
                }
            }
        }
    }
}

// ===========================================================================
// §D  Thread-local reuse — where the zero-allocation steady state comes from
// ===========================================================================

thread_local! {
    /// The reused output buffer. `clear()` keeps the capacity, so after warm-up
    /// an encode allocates **nothing at all** while still walking once.
    ///
    /// ⚠ High-water pinning is real: one pathological term would otherwise hold
    /// its capacity for the life of the thread. [`SHRINK_THRESHOLD`] bounds it.
    static OUT: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(OUT_CAPACITY));
}

/// Above this capacity the reused buffer is shrunk back after a call.
///
/// Chosen so the buffer absorbs every ordinary datum without a single
/// reallocation, while a one-off multi-megabyte term cannot pin that memory for
/// the life of the thread. Both halves are measured by
/// `wire_encode_space::the_reused_buffer_does_not_pin_a_pathological_high_water`.
const SHRINK_THRESHOLD: usize = 1 << 20; // 1 MiB

/// Encode `value`, **appending** to `out`.
///
/// The lowest-level entry point: it is what composes, and it is what an
/// intern-aware splicing emitter needs.
pub fn encode_into<T: WireNode>(value: &T, out: &mut Vec<u8>) {
    let mut m = Machine::new();
    m.ops.push(Op::Node {
        node: value,
        field: 0,
    });
    m.run::<false>(out);
    debug_assert!(
        m.map_iters.is_empty(),
        "wire_encode: a map iterator outlived its field"
    );
}

/// Encode `value` into a fresh `Vec<u8>`.
///
/// Byte-identical to `bincode::serialize(value).expect(..)`, in one traversal.
/// Prefer [`with_encoded`] on a hot path: this one hands out ownership and so
/// cannot reuse the thread-local buffer.
pub fn encode<T: WireNode>(value: &T) -> Vec<u8> {
    with_encoded(value, <[u8]>::to_vec)
}

/// Encode `value` into the **reused thread-local buffer** and hand the bytes
/// to `f`.
///
/// ★ This is the zero-allocation steady state: the buffer is `clear()`ed, not
/// dropped, so after the first calls on a thread it has converged to the
/// high-water mark and an encode allocates nothing.
pub fn with_encoded<T, R>(value: &T, f: impl FnOnce(&[u8]) -> R) -> R
where
    T: WireNode,
{
    OUT.with(|cell| {
        // ⚠ Re-entrancy: `f` may itself encode. A borrow conflict would panic,
        // so fall back to a private buffer rather than assume `f` is a leaf.
        match cell.try_borrow_mut() {
            Ok(mut out) => {
                out.clear();
                encode_into(value, &mut out);
                let result = f(&out);
                if out.capacity() > SHRINK_THRESHOLD {
                    out.shrink_to(OUT_CAPACITY);
                }
                result
            }
            Err(_) => {
                let mut out = Vec::with_capacity(OUT_CAPACITY);
                encode_into(value, &mut out);
                f(&out)
            }
        }
    })
}

// ===========================================================================
// §E  The four cold-store roots
// ===========================================================================
//
// C, P, A, K of `RSpace<Par, BindPattern, ListParWithRandom,
// TaggedContinuation>` — the Rholang instantiation, and exactly the set
// `par_codec` decodes. Every other schema type is reachable only as a child of
// one of these.

/// Encode a channel / pattern / datum / continuation byte-identically to
/// `bincode::serialize`, in one traversal and O(1) native stack.
pub trait ColdStoreEncode {
    fn cold_encode(&self) -> Vec<u8>;
    fn cold_encode_into(&self, out: &mut Vec<u8>);
}

macro_rules! cold_store_encode {
    ($($t:ty),* $(,)?) => {$(
        impl ColdStoreEncode for $t {
            #[inline]
            fn cold_encode(&self) -> Vec<u8> { encode(self) }
            #[inline]
            fn cold_encode_into(&self, out: &mut Vec<u8>) { encode_into(self, out) }
        }
    )*};
}

cold_store_encode!(Par, BindPattern, ListParWithRandom, TaggedContinuation);

// ===========================================================================
// §F  Introspection for the space gate
// ===========================================================================

/// The op-stack high-water mark reached while encoding `value`.
///
/// ★ Exists because "the op stack is Θ(term DEPTH), not Θ(term SIZE)" is a
/// claim about a *mechanism*: a bug that pushed children eagerly would turn a
/// 10-deep, 1,000,000-node term into a 1,000,000-entry stack while every
/// correctness test still passed. `models/tests/wire_encode_space.rs` measures
/// it on both shapes.
///
/// The measurement runs the production loop with `TRACK = true`, so it cannot
/// drift into describing a different machine.
pub fn op_stack_high_water<T: WireNode>(value: &T) -> usize {
    let mut out = Vec::with_capacity(OUT_CAPACITY);
    let mut m = Machine::new();
    m.ops.push(Op::Node {
        node: value,
        field: 0,
    });
    m.run::<true>(&mut out);
    m.high_water
}

/// `size_of::<Op>()`, exposed so the space gate can pin it.
pub const fn op_size() -> usize {
    std::mem::size_of::<Op<'static>>()
}

/// The ADDRESS of `node`'s program, as an opaque integer.
///
/// ⚠ Exposed so `wire_encode_space` can demonstrate that program addresses do
/// **not** identify a type: `&'static` slices with identical contents are
/// merged by the linker, and `EPATHMAP_PROGRAM` is byte-for-byte
/// `ELIST_PROGRAM`. A downcast built on address identity therefore
/// reinterprets an `EList` as an `EPathMap` — which is exactly the `SIGSEGV`
/// that produced [`WireNode::wire_as_pathmap`]. The gate keeps that fact
/// executable so the "optimization" cannot be reintroduced.
pub fn program_address(node: &dyn WireNode) -> usize {
    node.wire_program().as_ptr() as usize
}
