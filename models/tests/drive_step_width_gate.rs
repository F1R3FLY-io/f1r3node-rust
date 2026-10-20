//! # The driver's per-level width, pinned — and the byte facts every argument rests on
//!
//! Two numbers decide whether a change to `models/src/rust/rholang/drive.rs` is
//! affordable, and until this file existed **neither of them was asserted
//! anywhere**:
//!
//! | number | why it decides things | measured |
//! |---|---|---|
//! | `size_of::<Step<'_, CloneTraversal>>()` | the work stack holds one `Step` per pending obligation, so this multiplies by **depth**. At depth 4,096 one extra word is +32 kB of live heap on the validator path. | **24 B** (16 B before the merge of feature/cost-accounted-rho) |
//! | `size_of::<Par>()` | every byte-movement argument in every report about the clone conversion is denominated in it — "three extra 248-byte moves per node" is the whole hypothesis | **296 B** (248 B before the merge of feature/cost-accounted-rho) |
//!
//! Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size pin
//! test): the merge added two `Vec` fields to `Par` and grew the clone cut set to
//! three members (`Par`, `CostSignature`, `CostStack`). This file failed on both
//! numbers. `models/codegen/schema.rs` records the new measurement (MEASURED
//! 2026-10-20). The user accepted the extra word per level on 2026-10-20, and the
//! pins below now hold the merged widths.
//!
//! ## ★ Why a gate and not a comment
//!
//! `models/tests/bincode_encoder_space.rs` already pins the encoder's op at
//! `size_of::<Op>() <= 4 * size_of::<usize>()`, and that assertion is the reason
//! anybody knows the encoder has **zero** headroom. The driver's `Step` had no
//! such statement, so *"`Outcome::Tail` does not widen `Step`"* — the claim that
//! answers the 32-byte-ceiling objection to `Tail` — was unfalsifiable. It is
//! now checked, and `size_of::<Outcome<..>>()` is checked *not* to be bounded,
//! which is the more interesting half of the same statement.
//!
//! ## ⚠ The RED this gate is watched at
//!
//! A width assertion that has only ever been satisfied is indistinguishable from
//! one that accepts everything, so the mechanism is demonstrated on a
//! **deliberately widened node** — [`Wide`], a `Node` type carrying an extra
//! 16 bytes — and the assertion that its `Step` is *larger* is what shows the
//! `size_of` is really reading the layout rather than a constant. ★ That RED is
//! **executed on every run**, in-process, because a width mismatch is a
//! compile-time-computable fact and needs no child process to observe.
//!
//! ★★ And one measured surprise, recorded here because it refuted a published
//! attribution: widening `CloneKont` from 8 B to 16 B **does not widen `Step` at
//! all.** `Step` is a two-variant enum whose payloads are `&Par`-shaped, and rustc
//! packs the discriminant into the non-null pointer niche, so `Step` stays 16 B.
//! An earlier report charged a measured regression to *"the wider `Kont` (8→16 B,
//! so `Step` 16→24 B)"*; the second half of that never happened. [`niche_packing`]
//! below is that fact, executed.
//!
//! Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size pin
//! test): the paragraph above describes the one-member cut set. In the merged tree
//! `CloneNode` and `CloneKont` each carry a tag word, so `Step` is 24 B.
//! [`niche_packing`] still checks the niche fact on a fixed control shape.

// Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size pin
// test): `CostSignature` is the widest variant of the merged `CloneVal`.
// use models::rhoapi::Par;
use models::rhoapi::{CostSignature, Par};
use models::rust::rholang::drive::{Outcome, Step, Traversal};
use models::rust::rholang::term_ops::{CloneKont, CloneNode, CloneTraversal, CloneVal};

/// One machine word. Every budget in this file is stated in words, because that is
/// the unit a stack cell is actually allocated in.
const WORD: usize = std::mem::size_of::<usize>();

/// The pinned width of one work-stack cell for the generated clone traversal, in
/// words.
///
/// `Step` is `enum { Descend(CloneNode<'t>), Combine(CloneKont<'t>) }`.
///
/// Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size
/// pin test): the clone cut set now has three members. So `CloneNode` is a tag
/// word and a reference (2 words), and `CloneKont` is a tag word, `src` and `base`
/// (3 words). rustc stores `Step`'s discriminant in unused values of
/// `CloneKont`'s tag, so the cell is 3 words. The user accepted the extra word per
/// level on 2026-10-20, after the measurement in `models/codegen/schema.rs`.
/// Before the merge the cell was a discriminant plus a pointer, 2 words.
// const STEP_WORDS: usize = 2;
const STEP_WORDS: usize = 3;

/// `size_of::<Par>()`, asserted rather than quoted.
// Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size pin
// test): the merge added `cost_signed_terms` and `cost_stacks` to `Par`, two `Vec`
// fields of 24 B each.
// const PAR_BYTES: usize = 248;
const PAR_BYTES: usize = 296;

// ---------------------------------------------------------------------------
// The RED subject: a deliberately widened node
// ---------------------------------------------------------------------------

/// A `Node` 16 bytes wider than [`CloneNode`], and otherwise identical in kind.
///
/// ⚠ Not a straw man: this is the shape **destination-passing descent** would
/// need. That design widens `Step::Descend` with a destination pointer, and the
/// point of pinning `STEP_WORDS` is that such a change must be *measured against
/// this gate* rather than assumed to re-pack.
#[derive(Clone, Copy)]
#[allow(dead_code)]
// Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size pin
// test): the merged `Step` is 24 B, as wide as the old `Wide`, so the control was
// INERT. One more `u64` keeps `Wide` 16 bytes wider than the merged `CloneNode`,
// and its `Step` is wider than the clone's again.
// struct Wide<'t>(&'t Par, u64, u64);
struct Wide<'t>(&'t Par, u64, u64, u64);

/// A traversal whose `Node` is [`Wide`]. Never driven — it exists only so
/// `size_of::<Step<'_, WideTraversal>>()` is a thing this file can read.
struct WideTraversal;

impl Traversal for WideTraversal {
    type Node<'t>
        = Wide<'t>
    where Self: 't;
    type Val = ();
    type Kont<'t>
        = &'t Par
    where Self: 't;
    type State = ();
    type Err = std::convert::Infallible;

    fn descend<'t>(
        &mut self,
        _st: &mut (),
        _node: Wide<'t>,
        _work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<()>,
    ) -> Result<(), Self::Err> {
        vals.push(());
        Ok(())
    }

    fn combine<'t>(
        &mut self,
        _st: &mut (),
        _kont: &'t Par,
        vals: &mut Vec<()>,
    ) -> Result<Outcome<(), Wide<'t>>, Self::Err>
    where
        Self: 't,
    {
        vals.pop().expect("value stack underflow");
        Ok(Outcome::Value(()))
    }

    fn arity(_kont: &&Par) -> usize { 1 }
}

// ---------------------------------------------------------------------------
// §A  The pins
// ---------------------------------------------------------------------------

/// ★★ `size_of::<Par>()` — the denominator of every byte argument about the clone
/// conversion, asserted for the first time.
#[test]
fn the_par_width_every_byte_argument_is_denominated_in_is_pinned() {
    let measured = std::mem::size_of::<Par>();
    // Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size
    // pin test): the message now gives both measurements. At 248 B the moves were
    // inline stores in `drive_with`. At 304 B they are `memcpy` calls.
    assert_eq!(
        measured, PAR_BYTES,
        "★ `size_of::<Par>()` is {measured} B, not {PAR_BYTES} B. This number is not a \
         detail: `models/codegen/schema.rs`'s clone-throughput section prices the \
         driven form's overhead as THREE EXTRA VALUE MOVES PER NODE. At 248 B the \
         deterministic confirmation was `drive_with`'s measured +93.6 write references \
         per node against `3 × 248 / 8 = 93`. At the merged 304-B `CloneVal` the moves \
         are `memcpy` calls instead, 2.5 per node at budget 0 (MEASURED 2026-10-20). If \
         `Par` changed width, BOTH the hypothesis and its confirmation have to be \
         recomputed. Recompute them, then move this pin."
    );
    // ★ `CloneVal` is what the value stack holds.
    //
    // Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size
    // pin test): `CloneVal` is now a three-variant enum over `Par`, `CostSignature`
    // and `CostStack`. Its widest variant is `CostSignature`: the `repr(C)` oneof
    // `cost_signature::Value` holds a whole `Par` after a 4-byte tag padded to one
    // word. rustc stores `CloneVal`'s discriminant in unused values of that tag. So
    // `CloneVal` is as wide as `CostSignature`, one word wider than `Par`, and a
    // value-stack push or pop moves 304 B. Before the merge `CloneVal` was a
    // single-variant enum over `Par`, exactly `Par`-sized.
    assert_eq!(
        std::mem::size_of::<CloneVal>(),
        // measured,
        measured + WORD,
        "`CloneVal` must be one word wider than `Par`: it is a three-variant enum whose \
         widest variant, `CostSignature`, is a `Par` behind a one-word tag, and rustc \
         stores `CloneVal`'s discriminant in unused values of that tag. That is what \
         makes 'a `vals` push moves 304 bytes' a true statement rather than an \
         approximation. It is {} B against `Par`'s {measured} B.",
        std::mem::size_of::<CloneVal>()
    );
    // Added by the merge of feature/cost-accounted-rho (plan step 1.9, type-size pin
    // test): the reason for the pin above, executed rather than quoted.
    assert_eq!(
        std::mem::size_of::<CostSignature>(),
        std::mem::size_of::<CloneVal>(),
        "`CostSignature` is {} B against `CloneVal`'s {} B. They must be EQUAL: \
         `CostSignature` is the widest variant of `CloneVal`, and `CloneVal`'s \
         discriminant goes in unused values of its tag. If they now differ, the \
         explanation of the `CloneVal` width in this file and in `models/codegen/schema.rs` \
         needs recomputing from the current layout.",
        std::mem::size_of::<CostSignature>(),
        std::mem::size_of::<CloneVal>()
    );
}

/// ★★★ The per-level pin: one work-stack cell, in words.
// Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size pin
// test): the cell is three words after the merge, so the name says three.
// fn one_work_stack_cell_is_two_words_and_tail_did_not_change_that() {
#[test]
fn one_work_stack_cell_is_three_words_and_tail_did_not_change_that() {
    let step = std::mem::size_of::<Step<'_, CloneTraversal>>();
    assert_eq!(
        step,
        STEP_WORDS * WORD,
        "★ `size_of::<Step<'_, CloneTraversal>>()` is {step} B, not {} B ({STEP_WORDS} \
         words). This is the ONLY type in the driver that multiplies by DEPTH: the work \
         stack holds one per pending obligation, and `rholang/tests/stack_depth_gate.rs` \
         drives the clone to depth 4,096, so one extra word here is +32 kB of live heap on \
         a path a deploy controls. \n\nIf a design needs a wider `Step` — destination-passing \
         descent needs exactly that — the widening is a REVIEWABLE COST and this assertion \
         is where it gets reviewed. Do not raise the budget to make a build pass.",
        STEP_WORDS * WORD
    );
    // Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size
    // pin test): the cut set has three members, so `CloneNode` has three variants
    // and carries a tag word next to the reference.
    assert_eq!(
        std::mem::size_of::<CloneNode<'_>>(),
        // WORD,
        2 * WORD,
        "`CloneNode` must be two words, a tag and a reference: the cut set \
         `{{Par, CostSignature, CostStack}}` has three members, so the enum has three \
         variants and rustc gives it a tag word. Before the merge of \
         feature/cost-accounted-rho the cut set had one member and `CloneNode` was one \
         word. It is {} B.",
        std::mem::size_of::<CloneNode<'_>>()
    );
    // ★★ `CloneKont` is budgeted at TWO words, not one, and the difference is a
    // MEASURED decision rather than slack.
    //
    // It was one word — a bare `&'t Par` — until the walk elimination landed: the
    // `Kont` now also carries `base`, the value-stack index its children start at,
    // which deletes the third `ExprInstance` dispatch walk per node. Measured, on
    // the production-weighted mix under `cachegrind`, per `Par` node:
    //
    //     Ir  2883.4 -> 2829.1   (-54.3, -1.88%)     Dw  698.4 -> 698.5  (+0.01%)
    //
    // ⚠ And the reason the budget is TWO rather than THREE is the fact this file's
    // `niche_packing` test exists to record: widening `CloneKont` from one word to
    // two leaves `size_of::<Step>()` at **16 B**, because rustc puts the
    // discriminant in the `&Par`'s null niche. So this widening is free PER LEVEL,
    // which is the only place width is expensive. A THIRD word would not be: it
    // would exhaust the niche's slack and grow `Step`, and the assertion above is
    // what would catch it.
    //
    // ★ This assertion was RED before it was green, and the RED is quoted rather
    // than described, because a width budget nobody has seen fail is not a budget:
    //
    //     assertion `left == right` failed: `CloneKont` must be one word, for the
    //     same reason as `CloneNode`. It is 16 B.
    //       left: 16
    //      right: 8
    //
    // Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size
    // pin test): the prediction above came true. The merge gave `CloneKont` a third
    // word, a tag for its three variants, and `Step` grew from 16 B to 24 B. The
    // `Step` assertion above caught it:
    //
    //     assertion `left == right` failed: ★ `size_of::<Step<'_, CloneTraversal>>()`
    //     is 24 B, not 16 B (2 words). This is the ONLY type in the driver that
    //     multiplies by DEPTH: ...
    //       left: 24
    //      right: 16
    //
    // The user accepted the extra word per level on 2026-10-20, after the
    // measurement in `models/codegen/schema.rs`. So the budget is now three words,
    // and a FOURTH word would grow `Step` again.
    assert!(
        // std::mem::size_of::<CloneKont<'_>>() <= 2 * WORD,
        std::mem::size_of::<CloneKont<'_>>() <= 3 * WORD,
        "`CloneKont` is {} B, over its three-word budget. One word is the `&'t` source \
         reference, the second is `base`, the value-stack index that deletes a whole \
         `ExprInstance` dispatch walk per node (-54.3 Ir/node, measured), and the third \
         is the tag for the three cut-set members. A FOURTH word is NOT free: `Step` is \
         as wide as `CloneKont`, and `Step` multiplies by DEPTH. If a `Kont` needs more \
         state, put it in `Traversal::State`, the discipline `bincode_decoder`'s \
         `ParFrame`/`ReceiveTail`/`NewFrame` already follow.",
        std::mem::size_of::<CloneKont<'_>>()
    );
}

/// ★ The RED, executed: a genuinely wider `Node` makes a genuinely wider `Step`.
///
/// Without this, `one_work_stack_cell_is_three_words_and_tail_did_not_change_that`
/// could be passing because `size_of` returns a constant, or because the author
/// wrote down whatever it printed. Here the mechanism is shown to *respond*.
/// (Changed by the merge of feature/cost-accounted-rho, plan step 1.9, type-size
/// pin test: that test was `one_work_stack_cell_is_two_words_and_tail_did_not_change_that`.)
#[test]
fn the_width_pin_responds_to_a_deliberately_widened_node() {
    let narrow = std::mem::size_of::<Step<'_, CloneTraversal>>();
    let wide = std::mem::size_of::<Step<'_, WideTraversal>>();
    assert!(
        wide > narrow,
        "★ THE WIDTH PIN IS INERT. A `Node` carrying 16 extra bytes produced a `Step` of \
         {wide} B against the narrow one's {narrow} B. If a widened node does not widen \
         `Step`, this file's pin cannot fail and is not evidence of anything — every \
         `size_of` assertion here would be satisfied by a driver whose layout nobody \
         controls."
    );
    // ★★ Calibrated, and the calibration is itself a finding. `Wide` carries TWO
    // extra words (`&Par, u64, u64` = 24 B) yet `Step` grows by only ONE: rustc
    // stores the discriminant in `Wide`'s non-null `&Par` niche instead of in a
    // word of its own, so the 8 bytes the narrow `Step` spent on a tag are
    // recovered. ⇒ **`Step` growth is not `Node` growth**, in either direction, and
    // a reviewer of a `Step`-widening design must read the measured number rather
    // than count the fields added. The first draft of this assertion expected
    // `narrow + 2 * WORD` and was wrong for exactly this reason.
    //
    // Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size
    // pin test): the merged narrow `Step` is 24 B, so `Wide` is now
    // `&Par, u64, u64, u64` = 32 B. The relation is the same as before: the widened
    // `Step` is exactly `Wide`, one word more than the narrow `Step`.
    assert_eq!(
        wide,
        std::mem::size_of::<Wide<'_>>(),
        "the widened `Step` is {wide} B against a {}-B `Wide` node. They must be EQUAL: \
         `Step`'s larger variant IS the node, and its discriminant goes in the node's \
         non-null-reference niche rather than in a word of its own. If they now differ the \
         niche is no longer being used, and every `Step`-width prediction in this file and \
         in `drive.rs` needs recomputing from the current layout.",
        std::mem::size_of::<Wide<'_>>()
    );
    // Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size
    // pin test): the message said "two extra words", but the assertion has always
    // required one.
    assert_eq!(
        wide,
        narrow + WORD,
        "the widened `Step` is {wide} B; one extra word over {narrow} B was expected. \
         The RED is calibrated as well as red: an unexpected size means the layout is \
         doing something (padding, niche reuse) that the narrow assertion's reasoning does \
         not account for, and that reasoning is what a reviewer of a `Step`-widening \
         design will rely on."
    );
}

/// ★★ Stage 1f, executed: `Outcome` is a RETURN VALUE, so it is deliberately
/// **not** budgeted — and the assertion says so by requiring it to be *large*.
///
/// This is the answer to the objection that adding an arm to `Outcome` spends the
/// encoder's `size_of::<Op>() <= 32 B` headroom. It does not, because `Outcome`
/// never occupies a per-level cell: it lives in a register pair or an `sret` slot
/// for the length of the `match` that consumes it. The clone instance's `Outcome`
/// is 256 B — an order of magnitude past any stack-cell budget — and that is
/// **fine**, which is the whole point. (Changed by the merge of
/// feature/cost-accounted-rho, plan step 1.9, type-size pin test: it is 312 B
/// after the merge, still `CloneVal` plus one tag word.)
#[test]
fn outcome_is_not_on_the_per_level_stack_and_is_deliberately_unbudgeted() {
    let outcome = std::mem::size_of::<Outcome<CloneVal, CloneNode<'_>>>();
    let step = std::mem::size_of::<Step<'_, CloneTraversal>>();
    assert!(
        outcome > step,
        "VACUOUS: `Outcome` ({outcome} B) is no larger than `Step` ({step} B), so this \
         test cannot distinguish 'unbudgeted because it is a return value' from \
         'small enough not to matter'. The claim being pinned is structural, not \
         numeric — recheck that `Step` still does not mention `Outcome`."
    );
    // The `Tail` arm's payload is a `Node`, which is strictly smaller than the
    // `Val` arm's — so `Tail` costs ZERO even in `Outcome`'s own layout.
    // Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size
    // pin test): the merged `CloneNode` is two words, so the message says two.
    assert!(
        std::mem::size_of::<CloneNode<'_>>() < std::mem::size_of::<CloneVal>(),
        "`Outcome::Tail(Node)` must not be the widest arm: its payload is two words while \
         `Outcome::Value(Val)` carries a whole `Par`. If that ever inverts, adding `Tail` \
         DID grow the return value and the claim 'Tail costs nothing' needs restating."
    );
    assert_eq!(
        outcome,
        std::mem::size_of::<CloneVal>() + WORD,
        "`Outcome<CloneVal, CloneNode>` is {outcome} B; a `Par`-sized payload plus one \
         discriminant word was expected. Not a budget — a statement that the layout is \
         understood, so that a future change to `Outcome` is a decision rather than a \
         surprise."
    );
}

/// ★★ The niche-packing fact that refuted half of a published attribution.
///
/// An earlier report charged a measured −2.8% to *"the wider `Kont` (8→16 B, so
/// `Step` 16→24 B) and the extra leaf branch"*, jointly. The first half does not
/// exist: a `Kont` of `(&Par, u64)` is 16 B, and the `Step` built over it is
/// **still 16 B**, because rustc stores the discriminant in the `&Par`'s null
/// niche. So a `Step` widening cannot be inferred from a `Kont` widening, in
/// either direction, and a reviewer must read the number rather than the diff.
#[test]
fn niche_packing() {
    /// A `Kont` the same shape as the 2026-07-29 padding control: a reference plus
    /// eight bytes of payload, 16 B in total.
    #[derive(Clone, Copy)]
    #[allow(dead_code)]
    struct PaddedKont<'t>(&'t Par, u64);

    struct PaddedKontTraversal;

    impl Traversal for PaddedKontTraversal {
        type Node<'t>
            = &'t Par
        where Self: 't;
        type Val = ();
        type Kont<'t>
            = PaddedKont<'t>
        where Self: 't;
        type State = ();
        type Err = std::convert::Infallible;

        fn descend<'t>(
            &mut self,
            _st: &mut (),
            _node: &'t Par,
            _work: &mut Vec<Step<'t, Self>>,
            vals: &mut Vec<()>,
        ) -> Result<(), Self::Err> {
            vals.push(());
            Ok(())
        }

        fn combine<'t>(
            &mut self,
            _st: &mut (),
            _kont: PaddedKont<'t>,
            vals: &mut Vec<()>,
        ) -> Result<Outcome<(), &'t Par>, Self::Err>
        where
            Self: 't,
        {
            vals.pop().expect("value stack underflow");
            Ok(Outcome::Value(()))
        }

        fn arity(_kont: &PaddedKont<'_>) -> usize { 1 }
    }

    assert_eq!(
        std::mem::size_of::<PaddedKont<'_>>(),
        2 * WORD,
        "the control is miscalibrated: a padded `Kont` must be 2 words for this test to be \
         about niche packing at all. It is {} B.",
        std::mem::size_of::<PaddedKont<'_>>()
    );
    let padded_step = std::mem::size_of::<Step<'_, PaddedKontTraversal>>();
    // Changed by the merge of feature/cost-accounted-rho (plan step 1.9, type-size
    // pin test): this control has a fixed shape, a one-word `Node` and a two-word
    // `Kont`, so its `Step` is two words. It used `STEP_WORDS` while the clone's
    // `Step` was also two words. The merge moved the clone's `Step` to three words,
    // so the control states its own width.
    assert_eq!(
        padded_step,
        // STEP_WORDS * WORD,
        2 * WORD,
        "★ `Step` over a 2-word `Kont` is {padded_step} B, not {} B. The point of this \
         test is that it IS still {} B — rustc packs the discriminant into the non-null \
         reference's niche, so widening `Kont` from 1 word to 2 costs NOTHING per level. \
         If this now differs, the 2026-07-29 measurement that refuted \
         '`Step` 16 → 24 B' no longer applies to the current compiler and the earlier \
         attribution deserves a fresh look.",
        // STEP_WORDS * WORD,
        // STEP_WORDS * WORD
        2 * WORD,
        2 * WORD
    );
}
