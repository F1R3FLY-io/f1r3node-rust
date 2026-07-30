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
//! `EPathMap` is `extern_path`'d, so the descriptor-driven generator emits no
//! program for it and [`crate::rust::rholang::wire::EPATHMAP_PROGRAM`] is
//! hand-written. Its `ps` is the canonical projection of the map's entry trie
//! (`EPathMap::ps()`), memoized on the value, which is the same choice its
//! `Serialize` impl makes.
//!
//! ⚠ ★ **This section used to describe an ARENA.** A ground map's canonical
//! `ps` was *constructed* by re-reading a trie, and a constructed vector cannot
//! be served by a borrow-returning accessor, so the machine leaked it into a
//! `Vec<*mut Vec<Par>>` and released it with an `Op::DropOwned` pushed beneath
//! the subtree that read it — a raw-pointer arena with a hand-written soundness
//! argument, existing only because the canonical entries were not stored
//! anywhere. They are stored now, so `Machine::park`, `Machine::owned`,
//! `Machine::release_to`, `Op::DropOwned`, and the two `unsafe` blocks that
//! made them work are **deleted**, and the encoder holds no raw pointers at all.
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
    pathmap_ps, put_bytes, put_u64, Descent, PathmapPs, WireNode, WireSeq, NO_RESUME,
};

// ===========================================================================
// §A  Byte emission
// ===========================================================================
//
// ⚠ There is none here. Every layout primitive lives in
// [`crate::rust::rholang::wire`] (§A2) and is called from the GENERATED
// `wire_emit` bodies, so bincode's layout is written in exactly one place and
// the emission of a node's bounded fields is monomorphic — which is what makes
// it competitive with serde's derive. This module owns the TRAMPOLINE: the op
// stack, the counted repeat, the suspension discipline, the map iterator and
// the `EPathMap` interception. See `wire.rs` §A2 for the profile that forced
// the split.

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
    /// field was opened, and `len` is carried so the driver never makes a
    /// virtual call to re-ask — those calls were ~8.6% of the first profile.
    Seq {
        seq: &'a dyn WireSeq,
        /// ⚠ `u32`, not `usize`, so `Op` stays four words. A sequence with more
        /// than 4,294,967,295 elements cannot exist in memory (each `Par` is
        /// far more than one byte), and [`Machine::open_seq`] refuses one
        /// rather than truncating the cursor — a truncated cursor would emit
        /// fewer elements than the count prefix promises, which is a corrupt
        /// encoding rather than a slow one.
        index: u32,
        len: u32,
    },
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

// ★★ The pooling discipline and its soundness argument live ONCE, in
// `super::pooled_stack`. `prost_encode` allocates three fresh `Vec`s per encoder because
// this code was written here, in one file, and the next codec did not find it — that
// omission IS the 3.79× shallow regression. Declaring it through the macro is what makes it
// findable.
crate::pooled_stack! {
    // The pooled op-stack **allocation** for the bincode encoder.
    // 
    // ★ This is the last per-encode allocation. The output buffer is already reused;
    // without this, every `hash_produce` would still pay one `Vec::with_capacity(64)`
    // malloc/free pair — small, but per produce, and "zero allocation in the steady state"
    // is a stated acceptance criterion rather than an aspiration.
    pool OPS for Op, capacity = OP_STACK_CAPACITY, max = MAX_POOLED_OPS,
    take = take_ops, give = give_ops,
}

struct Machine<'a> {
    ops: Vec<Op<'a>>,
    /// One live iterator per ancestor `New` (the `injections` map).
    map_iters: Vec<btree_map::Iter<'a, String, Par>>,
    /// Op-stack high-water mark, tracked only when `TRACK` is on.
    high_water: usize,
}

impl<'a> Drop for Machine<'a> {
    /// Return the op-stack allocation to the thread-local pool.
    ///
    /// This used to also drain the parked-vector arena on a panic unwinding out
    /// of the loop. There is no arena any more, so teardown is one `mem::take`
    /// and touches no term structure — there is no deep `Par` here to dismantle.
    fn drop(&mut self) {
        give_ops(std::mem::take(&mut self.ops));
    }
}

impl<'a> Machine<'a> {
    fn new() -> Self {
        Machine {
            ops: take_ops(),
            map_iters: Vec::new(),
            high_water: 0,
        }
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
                Op::Seq { seq, index, len } => {
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
                                len,
                            });
                        }
                        self.ops.push(Op::Node {
                            node: seq.wire_get(index as usize),
                            field: 0,
                        });
                    }
                }
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
    ///
    /// ★ ONE virtual call — `wire_emit` — however many bounded fields it runs
    /// through. The first design asked the node for each field in turn and cost
    /// 21 indirect calls per `Par`; see `wire.rs` §A2.
    #[inline]
    fn run_node(&mut self, out: &mut Vec<u8>, node: &'a dyn WireNode, field: usize) {
        // ⚠ `EPathMap` cannot be entered at field 0: its `ps` may have to be
        // CONSTRUCTED in canonical order (§4), which no borrow-returning
        // emission can serve. Intercept, park, and rejoin at field 1.
        if field == 0 {
            if let Some(map) = node.wire_as_pathmap() {
                self.open_pathmap(out, node, map);
                return;
            }
        }
        match node.wire_emit(field, out) {
            Descent::Done => {}
            Descent::Node { resume, node: child } => {
                self.suspend(node, resume);
                self.ops.push(Op::Node {
                    node: child,
                    field: 0,
                });
            }
            Descent::Seq { resume, len, seq } => {
                self.suspend(node, resume);
                self.open_seq(seq, len);
            }
            Descent::Map { resume, map } => {
                self.suspend(node, resume);
                self.map_iters.push(map.iter());
                self.ops.push(Op::MapEntries);
            }
        }
    }

    /// Begin a counted repeat over `seq`.
    ///
    /// The length check is the price of a four-word `Op` (see [`Op::Seq`]); it
    /// is one compare against a constant, perfectly predicted, and it refuses
    /// rather than silently truncating a cursor.
    #[inline]
    fn open_seq(&mut self, seq: &'a dyn WireSeq, len: usize) {
        assert!(
            len <= u32::MAX as usize,
            "wire_encode: a sequence of {len} elements exceeds the u32 cursor. The count \
             prefix has already been written, so truncating here would emit fewer elements \
             than the stream promises — a corrupt encoding, not a slow one."
        );
        self.ops.push(Op::Seq {
            seq,
            index: 0,
            len: len as u32,
        });
    }

    /// Push a resume point for `node` at `resume`, unless the program is spent.
    ///
    /// ★ A pure comparison against [`NO_RESUME`] — no virtual call. The
    /// generator knows each program's length, so "is this the last field?" is
    /// answered at build time. Asking the node instead (`wire_program().len()`)
    /// cost one indirect call per DESCENT, which is what a deep term is made
    /// of: it was the whole of the residual 2.75% regression.
    ///
    /// Omitting the push is the encoder's TAIL CALL. Together with its sibling
    /// in `Op::Seq` (the last element of a sequence) it halves the per-level op
    /// cost of the deep-nesting shape, 4.000 → 2.000 entries.
    #[inline(always)]
    fn suspend(&mut self, node: &'a dyn WireNode, resume: u16) {
        if resume != NO_RESUME {
            self.ops.push(Op::Node {
                node,
                field: resume,
            });
        }
    }

    /// Open an `EPathMap`: emit its `ps` count, then arrange for fields 1..4.
    ///
    /// ⚠ It is still opened HERE rather than at field 0 of the generated
    /// walker, because `EPathMap`'s `ps` is a projection reached through
    /// `pathmap_ps` rather than a plain field read, and emitting the wrong one
    /// changes the event-hash preimage silently. `WireNode::wire_emit` refuses
    /// field 0 for exactly that reason.
    fn open_pathmap(&mut self, out: &mut Vec<u8>, node: &'a dyn WireNode, map: &'a EPathMap) {
        let PathmapPs::Stored(ps) = pathmap_ps(map);
        put_u64(out, ps.len() as u64);
        self.suspend(node, 1);
        if !ps.is_empty() {
            self.open_seq(ps, ps.len());
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
