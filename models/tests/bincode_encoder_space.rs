//! # The SPACE gate — the encoder's heap, measured rather than intended
//!
//! Space is a co-equal acceptance criterion, not a footnote to throughput, and
//! every claim below is about a *mechanism* that no correctness test can see:
//!
//! | claim | why a correctness test cannot see it |
//! |---|---|
//! | the op stack is Θ(term **depth**), not Θ(term **size**) | a driver that pushed all `n` children eagerly emits byte-identical output |
//! | the op-stack entry is four words | an `Op` that silently grew multiplies the only per-call heap there is |
//! | the reused buffer converges and does not pin | a leak of capacity is invisible to every assertion about bytes |
//! | program addresses do **not** identify a type | the unsound downcast *worked* on every fixture until an `EList` met it |
//!
//! ⚠ The last row is not hypothetical. `EPATHMAP_PROGRAM` is
//! `[Seq, EmptyBytes, Bool, Opt]` — byte-for-byte identical to `ELIST_PROGRAM`,
//! `ESET_PROGRAM` and `EMAP_PROGRAM` — and the linker **merges** identical
//! read-only statics. A downcast keyed on the program's address therefore
//! reinterpreted an `EList` as an `EPathMap` and produced a `SIGSEGV` on the
//! differential's first run. [`program_addresses_do_not_identify_a_type`] keeps
//! that fact executable so the trick cannot return as an "optimization".

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par, Send};
use models::rust::rholang::bincode_encoder::{
    encode, encode_into, op_size, op_stack_high_water, program_address, with_encoded,
};
use models::rust::rholang::bincode_schema::BincodeNode;

mod par_corpus;
use par_corpus as corpus;

// ---------------------------------------------------------------------------
// term shapes, built ITERATIVELY so construction is never the constraint
// ---------------------------------------------------------------------------

/// A left-spine of `depth` nested `EList`s: DEEP and narrow.
fn deep(depth: usize) -> Par {
    let mut par = Par::default();
    for _ in 0..depth {
        par = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![par],
                    ..Default::default()
                })),
            }],
            ..Default::default()
        };
    }
    par
}

/// One node with `width` children: SHALLOW and wide. Same node COUNT as
/// `deep(width)`, so the two shapes isolate depth from size exactly.
fn wide(width: usize) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: (0..width).map(|i| corpus::gint(i as i64)).collect(),
                ..Default::default()
            })),
        }],
        ..Default::default()
    }
}

// ===========================================================================
// §1  ★ The op stack is Θ(DEPTH), not Θ(SIZE)
// ===========================================================================

/// **The verdict**, pure in its observations, so it can be shown a synthetic
/// Θ(size) ladder and required to reject it.
fn no_size_slope_verdict(
    label: &str,
    lo_width: usize,
    lo_high_water: usize,
    hi_width: usize,
    hi_high_water: usize,
) -> Result<(), String> {
    let growth = hi_high_water.saturating_sub(lo_high_water);
    // A width ladder spanning 1,000× may cost a small constant more (the count
    // prefix opens one `Seq` op), but nothing proportional.
    if growth <= 4 {
        return Ok(());
    }
    Err(format!(
        "Θ(SIZE) OP STACK for `{label}`: the high-water mark grew from {lo_high_water} at \
         width {lo_width} to {hi_high_water} at width {hi_width} — {:.4} entries per sibling. \
         `Op::Seq` must RE-PUSH ITSELF with `index + 1`, not push one op per child; a term \
         with a million siblings would otherwise cost a million-entry stack while emitting \
         byte-identical output.",
        growth as f64 / (hi_width - lo_width) as f64
    ))
}

#[test]
fn the_op_stack_is_flat_in_term_size() {
    let lo = op_stack_high_water(&wide(4));
    let hi = op_stack_high_water(&wide(65_536));
    if let Err(why) = no_size_slope_verdict("wide", 4, lo, 65_536, hi) {
        panic!("{why}");
    }
    println!("  op stack (width): {lo} entries at 4, {hi} at 65,536");

    // ★★ ANTI-VACUITY: the verdict must REJECT a Θ(size) ladder. Without this,
    // a checker that accepted everything would look identical to a pass.
    let synthetic = no_size_slope_verdict("synthetic-Θ(size)", 4, 4, 65_536, 65_536);
    let why = synthetic.expect_err("a Θ(size) ladder must be REJECTED");
    assert!(
        why.contains("entries per sibling"),
        "the rejection must name the per-sibling growth; got: {why}"
    );
}

#[test]
fn the_op_stack_grows_only_with_depth_and_only_by_a_bounded_amount() {
    // Depth IS allowed to cost entries — that is the whole point of an explicit
    // stack — but the cost per level must be a small constant, not a
    // multiplier. A `Par → Expr → EList → Par` level is four nodes.
    let lo_depth = 16usize;
    let hi_depth = 4_096usize;
    let lo = op_stack_high_water(&deep(lo_depth));
    let hi = op_stack_high_water(&deep(hi_depth));
    let per_level = (hi - lo) as f64 / (hi_depth - lo_depth) as f64;
    println!("  op stack (depth): {lo} at {lo_depth}, {hi} at {hi_depth} ({per_level:.3}/level)");
    assert!(
        per_level <= 2.0,
        "the op stack costs {per_level:.3} entries per nesting level, and the measured value \
         is 2.000. Each level is four schema nodes (Par → Expr → EList → Par), of which TWO \
         need a resume point: `Par` has trailing fields after `exprs`, and `EList` has \
         trailing fields after `ps`. The other two are tail calls — a spent program \
         (`Machine::suspend`) and the LAST element of a sequence (`Op::Seq`). A regression \
         here means one of the two tail calls stopped being taken; it doubles the encoder\'s \
         only per-call heap on the adversarial shape."
    );

    // And the heap that costs, stated in bytes, so the number is comparable to
    // the decoder's value stacks rather than to an entry count.
    let bytes = hi * op_size();
    assert!(
        bytes <= 320 * 1024,
        "a 4,096-deep term costs {bytes} B of op stack ({} entries × {} B); the encoder's \
         only per-call heap must stay small enough that it is never the reason a node runs \
         out of memory",
        hi,
        op_size()
    );
    println!(
        "  op stack (depth {hi_depth}): {bytes} B = {hi} × {} B",
        op_size()
    );
}

#[test]
fn the_op_stack_entry_stays_small() {
    // `&dyn` is two words; the widest arm is a `&dyn` plus a `usize` plus the
    // discriminant, which packs into four words on a 64-bit target.
    assert!(
        op_size() <= 4 * std::mem::size_of::<usize>(),
        "`Op` grew to {} B ({} words). Every entry is multiplied by the term's DEPTH, so an \
         arm that started carrying a large payload should move to a side stack — the \
         discipline `bincode_decoder`'s `ParFrame`/`ReceiveTail`/`NewFrame` already follow.",
        op_size(),
        op_size() / std::mem::size_of::<usize>()
    );
    println!("  size_of::<Op>() = {} B", op_size());
}

// ===========================================================================
// §2  ★ Zero per-encode allocation in the steady state
// ===========================================================================

/// A counting allocator, installed globally for this test binary.
///
/// ⚠ It is the only honest way to assert "allocates nothing": a massif profile
/// shows aggregate behaviour, whereas the acceptance criterion is a statement
/// about *one call*.
mod counting_alloc {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    // ⚠ PER-THREAD, not global. `cargo test` runs test functions on separate
    // threads by default, so an `AtomicUsize` would have every measurement
    // counting its neighbours' allocations — a number that changes with
    // `--test-threads` is not a measurement of this encoder at all. (Observed:
    // the same assertion passed at `--test-threads=1` and failed in the full
    // suite.)
    //
    // `const`-initialised `Cell<usize>` has no destructor, so it registers no
    // TLS teardown hook and cannot re-enter the allocator it lives inside.
    thread_local! {
        static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
        static BYTES: Cell<usize> = const { Cell::new(0) };
    }

    fn bump(bytes: usize) {
        // `try_with` because a thread in TLS teardown must not panic here.
        let _ = ALLOCATIONS.try_with(|c| c.set(c.get() + 1));
        let _ = BYTES.try_with(|c| c.set(c.get() + bytes));
    }

    fn read() -> (usize, usize) {
        (
            ALLOCATIONS.try_with(Cell::get).unwrap_or(0),
            BYTES.try_with(Cell::get).unwrap_or(0),
        )
    }

    pub struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            bump(layout.size());
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            bump(new_size);
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    /// Allocations and bytes attributable to `f`, **on this thread**.
    pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
        let (a0, b0) = read();
        let r = f();
        let (a1, b1) = read();
        (r, a1 - a0, b1 - b0)
    }
}

#[global_allocator]
static ALLOC: counting_alloc::Counting = counting_alloc::Counting;

/// ★★ **The steady-state allocation table** — the acceptance criterion, measured
/// per shape and against the derived path in the same run.
///
/// | shape | machine | derived |
/// |---|---|---|
/// | ordinary terms (`gint`, wide, deep, non-ground `EPathMap`) | **0** | 1 every call, forever |
/// | lf-bearing `EPathMap` (the CBR-043 memo path) | **0** | 1 every call, forever |
/// | ground `EPathMap` | 40 | 77 |
///
/// The zero is not an approximation: the output buffer is `clear()`ed and
/// reused, the op-stack *allocation* is pooled per thread, and every `FieldVal`
/// borrows. The derived path allocates a fresh exactly-sized `Vec` on every
/// call and can never converge, because it must hand out ownership.
///
/// ⚠ The ground-`EPathMap` row is **not** the encoder allocating: it is
/// `ground_canonical_ps`, which constructs the canonical entry order (deduped
/// and recursively canonical) so a ground map's event-hash preimage is a pure
/// function of its entry set. That cost is inherent to the normalization and
/// the derived `Serialize` pays it too — nearly twice over, which is why the
/// machine's number is the smaller one.
#[test]
fn the_steady_state_allocation_table() {
    let ordinary: Vec<(&str, Par)> = vec![
        ("gint", corpus::gint(1)),
        ("wide(64)", wide(64)),
        ("deep(16)", deep(16)),
        (
            "nonground_pathmap",
            pathmap_par(corpus::nonground_pathmap()),
        ),
        // ★★ THE CBR-043 ROW, and it is not a duplicate of the one above it.
        //
        // `nonground_pathmap`'s entries are `GInt`s — `entries_stable`, so
        // `EntryTrie::bincode_path_stream` answers O(1) off the fold and the memo
        // cell is never touched. This shape's entry is lf-bearing and NOT
        // `eval_stable`, which is the only route into `blanked_stream()`: the
        // blanked entries are built through the cold-store codec pair, a
        // throwaway trie is filed, its key stream is compared, and the result is
        // memoized.
        //
        // ⚠ That work is real and it allocates — ONCE. What this row measures is
        // that it stays BEHIND the memo: a warm encode of an lf-bearing map must
        // still borrow, exactly like every other shape. An implementation that
        // recomputed the blanked stream per encode would pass every correctness
        // test in the tree and fail here, which is what this file is for.
        ("lf_bearing_pathmap", pathmap_par(lf_bearing_pathmap())),
    ];
    for (name, par) in &ordinary {
        warm(par);
        let (len, allocations, bytes) = counting_alloc::measure(|| with_encoded(par, <[u8]>::len));
        let (_, oracle_allocations, oracle_bytes) =
            counting_alloc::measure(|| bincode::serialize(par).expect("oracle").len());
        println!(
            "  {name:20} {len:8} B | machine {allocations:3} allocs {bytes:7} B \
             | derived {oracle_allocations:3} allocs {oracle_bytes:7} B"
        );
        assert_eq!(
            allocations, 0,
            "`{name}`: a warm encode made {allocations} allocations ({bytes} B). The steady \
             state must be ZERO — the output buffer is reused, the op-stack allocation is \
             pooled, and every `FieldVal` borrows. A non-zero count means one of those three \
             stopped holding."
        );
        assert!(
            oracle_allocations > 0,
            "`{name}`: the derived path must allocate, or the comparison is measuring nothing"
        );
    }

    // ★ The ground map: the canonicalization did not stop happening — it MOVED
    // INTO THE VALUE, so the encoder does not perform it and does not allocate
    // for it. Asserted as a RELATION, not as a magic number.
    let ground = pathmap_par(corpus::ground_pathmap());
    warm(&ground);
    let (_, allocations, bytes) = counting_alloc::measure(|| with_encoded(&ground, <[u8]>::len));
    let (_, oracle_allocations, oracle_bytes) =
        counting_alloc::measure(|| bincode::serialize(&ground).expect("oracle").len());
    println!(
        "  {:20} {:8} | machine {allocations:3} allocs {bytes:7} B \
         | derived {oracle_allocations:3} allocs {oracle_bytes:7} B",
        "ground_pathmap", ""
    );
    // ★ This assertion is INVERTED, deliberately.
    //
    // It read: *"a GROUND map must allocate — `ground_canonical_ps` constructs
    // the canonical order. Zero here would mean the canonicalization stopped
    // happening."* That inference was sound while the canonical order was
    // CONSTRUCTED at serialize time out of a `Vec` in the producer's order: the
    // construction was an allocation, so the allocation was evidence of the
    // canonicalization, and the encoder had to park the constructed vector in a
    // raw-pointer arena to hand it out by reference.
    //
    // The map stores the trie now, so the canonical order is a memoized
    // projection ON THE VALUE. The encoder BORROWS it, the arena is deleted, and
    // the ground map joins every other shape at zero. The canonicalization is
    // still happening — the leg below is what checks that, by BYTES rather than
    // by an allocation count standing in for them.
    assert_eq!(
        allocations, 0,
        "a GROUND map must now allocate NOTHING in the steady state: its canonical \
         order is memoized on the value and the encoder borrows it. A non-zero count \
         means the encoder went back to constructing the order at serialize time."
    );
    // The direct evidence the allocation count used to stand in for: a permuted
    // construction of the same entry set writes the SAME bytes.
    {
        let mut permuted_entries = corpus::ground_pathmap().entry_trie().entries_owned();
        permuted_entries.reverse();
        let permuted = pathmap_par(models::rhoapi::EPathMap::new(
            permuted_entries,
            Vec::new(),
            false,
            None,
        ));
        assert_eq!(
            with_encoded(&ground, <[u8]>::to_vec),
            with_encoded(&permuted, <[u8]>::to_vec),
            "the canonicalization must still be happening: a permuted construction \
             of one entry set must write identical bytes"
        );
    }
    assert!(
        allocations < oracle_allocations,
        "the machine allocated {allocations} and the derived path {oracle_allocations}; the \
         single-walk emitter must not cost MORE than the two-walk one on the shape where \
         both must canonicalize"
    );
}

/// An `EPathMap` whose single entry is `¬eval_stable` **and** carries
/// `locally_free` — the one shape that reaches `EntryTrie::blanked_stream`.
///
/// A bound-variable `EVar` is non-ground by CONTENT, so the entry takes
/// `encode_trie_path`'s `0x0F` escape arm, whose payload is canonical prost
/// bytes — and prost retains the bitset. That is the exact route by which an
/// entry's `locally_free` used to reach the bincode wire, and therefore the
/// exact route the memo now covers.
fn lf_bearing_pathmap() -> models::rust::rhoapi_ext::EPathMap {
    let entry = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(models::rhoapi::EVar {
                v: Some(models::rhoapi::Var {
                    var_instance: Some(models::rhoapi::var::VarInstance::BoundVar(0)),
                }),
            })),
        }],
        locally_free: models::create_bit_vector(&[0]),
        ..Default::default()
    };
    models::rust::rhoapi_ext::EPathMap::new(vec![entry], Vec::new(), false, None)
}

/// One node holding one `EPathMap`.
fn pathmap_par(map: models::rust::rhoapi_ext::EPathMap) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }],
        ..Default::default()
    }
}

/// Drive the thread-local buffer and op-stack pool to their steady state.
fn warm(par: &Par) {
    for _ in 0..64 {
        with_encoded(par, <[u8]>::len);
    }
}

#[test]
fn encode_into_an_existing_buffer_reuses_its_capacity() {
    let par = corpus::all_par_fields();
    let size = encode(&par).len();

    let mut out = Vec::with_capacity(size * 4);
    for _ in 0..8 {
        out.clear();
        encode_into(&par, &mut out);
    }
    let before = out.capacity();
    let (_, _allocations, _) = counting_alloc::measure(|| {
        out.clear();
        encode_into(&par, &mut out);
    });
    assert_eq!(out.len(), size, "the encoding must be stable across reuse");
    assert_eq!(
        out.capacity(),
        before,
        "`encode_into` must not re-grow a buffer that already fits — appending into a \
         caller-owned buffer is what lets an intern-aware emitter splice without a copy"
    );
}

#[test]
fn the_reused_buffer_does_not_pin_a_pathological_high_water() {
    // One pathological term inflates the thread-local; the shrink policy must
    // return the capacity rather than hold it for the life of the thread.
    let huge = wide(200_000);
    let huge_len = with_encoded(&huge, <[u8]>::len);
    assert!(
        huge_len > 1 << 20,
        "the pathological fixture must exceed the shrink threshold, or this test proves \
         nothing (it encoded to {huge_len} B)"
    );

    // A small encode afterwards must not be paying for the huge one.
    let small = corpus::gint(1);
    for _ in 0..4 {
        with_encoded(&small, <[u8]>::len);
    }
    let (_, _, bytes) = counting_alloc::measure(|| {
        for _ in 0..16 {
            with_encoded(&small, <[u8]>::len);
        }
    });
    assert!(
        bytes < huge_len / 4,
        "after a {huge_len} B encode, sixteen small encodes allocated {bytes} B — the shrink \
         policy is not returning the pathological capacity"
    );
    println!("  after a {huge_len} B encode, 16 small encodes cost {bytes} B");
}

/// The op-stack POOL must not pin a deep term's high-water mark either.
#[test]
fn the_op_stack_pool_does_not_pin_a_deep_terms_high_water() {
    let deep_term = deep(8_192);
    let bytes = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let n = with_encoded(&deep_term, <[u8]>::len);
            std::mem::forget(deep_term);
            n
        })
        .expect("spawn")
        .join()
        .expect("a deep encode must survive a 2 MiB stack");
    assert!(bytes > 0);

    // ⚠ The deep encode ran on its OWN thread, so this thread's pool and buffer
    // were never touched by it — which is itself the point: the pool is
    // per-thread, so one thread's pathological term cannot inflate another's.
    // On the encoding thread the 8,192-deep stack (16,385 entries) exceeds
    // `MAX_POOLED_OPS` and is dropped rather than parked.
    let small = corpus::gint(1);
    warm(&small);
    let (_, allocations, _) = counting_alloc::measure(|| with_encoded(&small, <[u8]>::len));
    assert_eq!(
        allocations, 0,
        "a small encode after a deep one must still be allocation-free"
    );
}

// ===========================================================================
// §3  ⚠ Program addresses do NOT identify a type
// ===========================================================================

/// The measured fact behind [`models::rust::rholang::bincode_schema::BincodeNode::bincode_as_pathmap`].
///
/// If this test ever *fails* — i.e. the addresses become distinct — that is not
/// permission to reintroduce the pointer trick. Constant merging is a linker
/// and codegen-unit decision that varies with profile, LTO and target; a
/// downcast that is sound in one build and a type confusion in the next is
/// strictly worse than one that is always sound.
#[test]
fn program_addresses_do_not_identify_a_type() {
    let pathmap = corpus::ground_pathmap();
    let elist = EList::default();

    let same = program_address(&pathmap as &dyn BincodeNode)
        == program_address(&elist as &dyn BincodeNode);
    println!(
        "  EPATHMAP_PROGRAM @ {:#x}, ELIST_PROGRAM @ {:#x} — merged: {same}",
        program_address(&pathmap as &dyn BincodeNode),
        program_address(&elist as &dyn BincodeNode),
    );

    // Whatever the addresses are, the TYPE question must be answered by the
    // trait, and it must answer correctly.
    assert!(
        (&pathmap as &dyn BincodeNode)
            .bincode_as_pathmap()
            .is_some(),
        "an EPathMap must identify itself"
    );
    assert!(
        (&elist as &dyn BincodeNode).bincode_as_pathmap().is_none(),
        "an EList must NOT identify as an EPathMap — this is the type confusion that \
         SIGSEGV'd the differential on its first run"
    );
    for node in [
        &Par::default() as &dyn BincodeNode,
        &Send::default(),
        &Expr::default(),
        &models::rhoapi::ESet::default(),
        &models::rhoapi::EMap::default(),
    ] {
        assert!(
            node.bincode_as_pathmap().is_none(),
            "only EPathMap may answer to `bincode_as_pathmap`"
        );
    }

    // And the encoder survives the exact shape that used to crash: an `EList`
    // whose program is (content-)identical to `EPathMap`'s, nested under one.
    let mixed = models::par_from_default! {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![corpus::gint(1)],
                    ..Default::default()
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(corpus::ground_pathmap())),
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        encode(&mixed),
        bincode::serialize(&mixed).expect("oracle"),
        "an EList beside an EPathMap must still encode identically"
    );
}

// ===========================================================================
// §4  ★★ Zero per-CLONE allocation — and the RED that makes it a measurement
// ===========================================================================

/// ★★ The clone half of the acceptance criterion, which had no executed guard.
///
/// The generated `<Par as Clone>::clone` routes through `drive_with` on
/// **thread-local pooled** stacks, so its steady state must be **0 allocations**
/// for the driver — exactly the criterion §2 already asserts for the encoder. The
/// clone's *output* is owned, so it allocates for the term it produces; the
/// statement being pinned is therefore a **relation**, not a zero: the driven
/// clone must allocate no more than the derived oracle it is byte-identical to.
///
/// ## ⚠ Why the RED is the OWNING `drive` and not a mutilated visitor
///
/// `drive` and `drive_with` differ in exactly one respect — `drive` performs
/// `Vec::with_capacity` twice per call, `drive_with` takes both stacks by `&mut`.
/// That difference is the entire reason `drive_with` exists, and its justification
/// is a measured one: 95.43% of production terms are at depth 2, where a `Par`
/// clone touches ~6 nodes and two mallocs are a double-digit regression.
///
/// So the RED here is **the owning entry point, called on the same term**. If the
/// pooled path's advantage is real, the owning path allocates strictly more; if it
/// is not, this test cannot tell them apart and says so. ★ That is a sharper RED
/// than a deliberately-broken visitor would be, because it is a control that
/// *ships* — somebody could reasonably reach for `drive` on this path, and this
/// test is what tells them what it costs.
#[test]
fn the_clone_steady_state_allocates_nothing_for_the_driver_itself() {
    use models::rust::rholang::drive::{drive, Step};
    use models::rust::rholang::term_ops::{oracle_clone_par, CloneNode, CloneTraversal, CloneVal};

    for (name, par) in [
        ("gint", corpus::gint(1)),
        ("wide(64)", wide(64)),
        ("deep(16)", deep(16)),
    ] {
        // Warm the thread-local stack pool: the criterion is the STEADY state.
        for _ in 0..8 {
            let warmed = par.clone();
            models::rust::rholang::par_children::dismantle(warmed);
        }

        let (pooled_allocs, oracle_allocs, owning_allocs) = {
            let (c, pooled_allocs, pooled_bytes) = counting_alloc::measure(|| par.clone());
            models::rust::rholang::par_children::dismantle(c);
            let (o, oracle_allocs, oracle_bytes) =
                counting_alloc::measure(|| oracle_clone_par(&par));
            models::rust::rholang::par_children::dismantle(o);
            // ⚠ THE RED: the OWNING entry point, same visitor, same term.
            let (w, owning_allocs, owning_bytes) = counting_alloc::measure(|| {
                match drive(
                    &mut CloneTraversal,
                    &mut (),
                    Step::Descend(CloneNode::Par(&par)),
                )
                .expect("the clone traversal is infallible")
                {
                    CloneVal::Par(p) => p,
                }
            });
            models::rust::rholang::par_children::dismantle(w);
            println!(
                "  {name:20} pooled {pooled_allocs:3} allocs {pooled_bytes:8} B \
                 | derived {oracle_allocs:3} allocs {oracle_bytes:8} B \
                 | OWNING drive {owning_allocs:3} allocs {owning_bytes:8} B"
            );
            (pooled_allocs, oracle_allocs, owning_allocs)
        };

        assert!(
            pooled_allocs <= oracle_allocs,
            "`{name}`: the driven clone made {pooled_allocs} allocations against the derived \
             oracle's {oracle_allocs}. The two produce BYTE-IDENTICAL terms \
             (`models/tests/clone_equivalence_corpus.rs`, eight axes), so every allocation \
             beyond the oracle's is the DRIVER's and the thread-local pool has stopped \
             holding. `drive_with` exists for exactly this number."
        );
        assert!(
            owning_allocs > pooled_allocs,
            "★ THE RED IS INERT. The owning `drive` made {owning_allocs} allocations and the \
             pooled `drive_with` path made {pooled_allocs} — the owning path must cost \
             STRICTLY MORE, because it performs two `Vec::with_capacity` calls per clone \
             where the pooled path performs none. If they are equal, this test cannot \
             distinguish a pooled driver from an allocating one, the assertion above is \
             vacuous, and `drive_with`'s entire justification is unmeasured."
        );
    }
}
