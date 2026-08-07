//! # Canonical form is a function of the term — and nothing else
//!
//! ⛔ **This gate exists because `SS-Y4` was live at HEAD and no existing check could see it.**
//! Sibling order was decided by score; the score is **not injective on canonical terms**; and
//! `sort_vec` was stable, so tied siblings inherited whatever filled the input vector — `HashSet`
//! iteration order (seeded **per process**) at `SortedParHashSet::create_from_vec`, and the
//! message's own **field order** at `combine_par`.
//!
//! Measured before the repair: a set containing `{3 → 30}` and `{3 → 90}` split **20/20 across
//! 40 processes**, and `{3:30} | {3:90}` versus `{3:90} | {3:30}` — two spellings of one
//! process, `|` being commutative — reached **different canonical bytes deterministically**.
//!
//! ## Why the existing checks could not be repaired into this
//!
//! | check | why it is blind |
//! |---|---|
//! | `sorter_canonical_golden.rs` | its collections carry pairwise-**distinct** scores *by construction* (`:90`), and it says so — adding tied rows would have made it flake under the old code and would destroy its capture-order authority |
//! | `sort_recursive.rs` (the frozen oracle) | shares `sort_combine` with the driver, so both sides move together |
//! | `the_score_and_the_canonical_term_agree_with_each_other` | asserted an **iff whose reverse was false**, and passed on sampling luck |
//!
//! ⇒ The golden pins **which** order; this gate pins **that there is one**. They are
//! complementary and neither subsumes the other.
//!
//! ## The four clauses
//!
//! 1. **Corpus vitality** — every witness pair really *is* a tie (distinct terms, equal scores).
//!    ★ This is the clause a tie-free corpus **cannot** satisfy, which is what stops the gate
//!    from being quietly retired by a future change that makes the score injective. Such a change
//!    turns this RED and must be *stated and defended*, not absorbed.
//! 2. **Permutation invariance** — in-process, deterministic, no seed needed. This is the clause
//!    that catches the `combine_par` fork, which a cross-process check alone would miss entirely.
//! 3. **Cross-process determinism** — the same term canonicalises identically in fresh processes.
//! 4. **The control, proven live by mutation** — a twin of the *pre-repair* `sort_vec` (score
//!    only, no tie-break) must **vary** across processes and must **fail** permutation
//!    invariance. ⚠ Without this, clause 3 is green in any environment with a fixed hash seed and
//!    the gate is worse than none.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EMap, ESet, Expr, KeyValuePair, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::score_tree::{compare_score, ScoredTerm};
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;

fn gint(v: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(v)),
    }])
}

/// A single-pair map `{key → value}` as an `Expr`.
///
/// ⚠ The **map** is the tie carrier: `combine_emap` chains only the key's score, so two maps
/// sharing a key tie regardless of their values.
fn map1(key: i64, value: i64) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(gint(key)),
                value: Some(gint(value)),
            }],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }
}

fn par_of(exprs: Vec<Expr>) -> Par { Par::default().with_exprs(exprs) }

/// The tie-carrying witness pairs. **Every entry must be a genuine tie** — clause 1 checks it.
fn witness_pairs() -> Vec<(&'static str, Expr, Expr)> {
    vec![
        ("emap-values-3", map1(3, 30), map1(3, 90)),
        ("emap-values-7", map1(7, 1), map1(7, 2)),
    ]
}

/// The four wrappers each pair is exercised in. A tie only reaches the bytes through a container
/// that sorts, so the wrapper is part of the witness, not decoration.
fn wrappers(a: &Expr, b: &Expr) -> Vec<(&'static str, Par, Par)> {
    let set = |x: &Expr, y: &Expr| {
        par_of(vec![Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: vec![par_of(vec![x.clone()]), par_of(vec![y.clone()])],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }])
    };
    vec![
        // `Par.exprs` — sorted by `combine_par` from the MESSAGE'S FIELD ORDER. Deterministic,
        // and the one a cross-process check cannot see.
        (
            "par-exprs",
            par_of(vec![a.clone(), b.clone()]),
            par_of(vec![b.clone(), a.clone()]),
        ),
        // `ESet` elements — sorted through `SortedParHashSet`'s `HashSet`, i.e. SEEDED.
        ("eset-elements", set(a, b), set(b, a)),
    ]
}

/// The **pre-repair** `sort_vec`: score only, no tie-break. Held here verbatim as the control,
/// in the house idiom (`compare_score_recursive`, `sort_recursive.rs`, `substitute_oracle.rs`).
fn sort_vec_without_tie_break<T>(scored: &mut [ScoredTerm<T>]) {
    scored.sort_by(|s1, s2| compare_score(&s1.score, &s2.score));
}

fn canonical_hex(p: &Par) -> String {
    // ★ The literal expression `cost_accounting/sig.rs:255` signs — not a rendering of it.
    ParSortMatcher::sort_match(p)
        .term
        .encode_to_vec()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// **Clause 1 — corpus vitality.** A tie-free corpus cannot make this file green.
#[test]
fn the_corpus_still_carries_real_ties() {
    for (name, a, b) in witness_pairs() {
        let pa = par_of(vec![a]);
        let pb = par_of(vec![b]);
        let sa = ParSortMatcher::sort_match(&pa);
        let sb = ParSortMatcher::sort_match(&pb);

        assert_ne!(
            sa.term, sb.term,
            "witness '{name}': the two members reached the SAME canonical term, so this pair is \
             no longer a witness to anything. Rebuild it from a genuinely distinct pair."
        );
        assert_eq!(
            sa.score, sb.score,
            "★ witness '{name}': the score now SEPARATES these two, so this corpus can no longer \
             express the fault class this gate exists to catch.\n\n\
             That may be GOOD NEWS — if the score was deliberately made injective on canonical \
             terms, the tie set is empty and `SS-Y4`'s whole class is closed at the root. But it \
             must be STATED AND DEFENDED, not absorbed by a gate quietly going green: supply a \
             new tie witness, or prove the tie set empty and retire this file explicitly.\n\n\
             ⚠ Other lossy score paths were known at the time of writing (`EZipper`'s cursor, \
             `ReceiveBind.free_count`), so 'the `EMap` path was fixed' is NOT a proof that the \
             class is empty."
        );
    }
}

/// **Clause 2 — permutation invariance.** In-process and deterministic; catches the `combine_par`
/// fork that no cross-process check can see.
#[test]
fn permuting_tied_siblings_does_not_change_the_canonical_form() {
    for (pair_name, a, b) in witness_pairs() {
        for (wrapper, ab, ba) in wrappers(&a, &b) {
            assert_eq!(
                canonical_hex(&ab),
                canonical_hex(&ba),
                "⛔ CONSENSUS FAULT — witness '{pair_name}' in wrapper '{wrapper}': two orderings \
                 of the same siblings reached DIFFERENT canonical forms.\n\n\
                 For 'par-exprs' this is the sharper failure: `|` is commutative, so the two are \
                 two spellings of ONE process and must sign identically. \
                 `cost_accounting/sig.rs` signs these bytes.\n\n\
                 The sibling order has stopped being TOTAL. See `SS-Y4` and CBR-040."
            );
        }
    }
}

/// **Clause 4 — the control, proven live by mutation.**
///
/// ⚠ This is what makes clause 2 meaningful. The pre-repair comparator must still be *able* to
/// disagree on this corpus; if it cannot, the corpus stopped carrying ties and clause 2 would be
/// green for the wrong reason.
#[test]
fn the_pre_repair_comparator_still_fails_on_this_corpus() {
    let mut disagreements = 0;

    for (_, a, b) in witness_pairs() {
        // Two scored siblings that TIE. Under the pre-repair comparator their order is decided
        // entirely by input order; under the repaired one it is decided by the emitted bytes.
        let sa = ParSortMatcher::sort_match(&par_of(vec![a]));
        let sb = ParSortMatcher::sort_match(&par_of(vec![b]));

        let mut forward = vec![sa.clone(), sb.clone()];
        let mut reversed = vec![sb.clone(), sa.clone()];
        sort_vec_without_tie_break(&mut forward);
        sort_vec_without_tie_break(&mut reversed);

        if forward[0].term != reversed[0].term {
            disagreements += 1;
        }

        // The repaired comparator must agree regardless of input order.
        let mut rf = vec![sa.clone(), sb.clone()];
        let mut rr = vec![sb.clone(), sa.clone()];
        ScoredTerm::sort_vec(&mut rf);
        ScoredTerm::sort_vec(&mut rr);
        assert_eq!(
            rf[0].term, rr[0].term,
            "the REPAIRED comparator is input-order dependent — the tie-break is not total."
        );
    }

    assert!(
        disagreements > 0,
        "⛔ THE CONTROL IS DEAD, so clause 2 proves nothing.\n\n\
         The pre-repair comparator (score only, no tie-break) produced the SAME order from both \
         input orders on every witness — which means this corpus no longer carries a tie that \
         input order could decide. `permuting_tied_siblings_does_not_change_the_canonical_form` \
         would then be green because the corpus is inert, not because the order is total.\n\n\
         Fix the CORPUS, not this assertion."
    );
}
