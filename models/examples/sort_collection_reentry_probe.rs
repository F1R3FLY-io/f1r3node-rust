//! # The ladder probe for the sorter's collection re-entry
//!
//! ⚠ **Why this exists.** `Ordering::sort_pars` calls `ParSortMatcher::sort_match` on every
//! element and returns the *sorted terms*, and `combine_eset` reaches it three times for the
//! same elements (`eset_to_par_set` → `create_from_vec` → `sort_pars`; then its own
//! `sorted_pars.iter().map(sort_match)`; then `create_from_vec(element_terms)` → `sort_pars`).
//! Each of those descends into the element's own collections, so the re-entry is
//! **multiplicative** in nesting depth rather than additive.
//!
//! ★ That much is read from source. **The growth RATE is not**, and this probe is what settles
//! it. `3^d` is the naive reading, but it is only a reading: `sort_match` may be cheap on an
//! already-sorted term, and a one-element collection may short-circuit. The campaign's own rule
//! applies — *a number without a subject is not a number* — so nothing quotes an exponent until
//! this ladder has been fitted.
//!
//! ## Method
//!
//! Build `{{{…{0}…}}}` — a chain of `depth` nested `ESet`s, one element each — and call
//! `ParSortMatcher::sort_match` on it exactly once. Under `callgrind`, the **call count** of
//! `sort_match` is then an exact, deterministic function of `depth`, with no timer involved.
//!
//! ```text
//! valgrind --tool=callgrind --callgrind-out-file=cg.$d \
//!     target/release/examples/sort_collection_reentry_probe $d
//! callgrind_annotate --threshold=0 cg.$d | grep sort_match
//! ```
//!
//! ⚠ **One element per level is deliberate.** Width would confound the measurement: the
//! question is how many times a single element is re-scored as a function of how deep it sits,
//! and a wider collection multiplies that by the width at every level. The width axis is a
//! separate ladder.
//!
//! ★ **The invariant control** is `--control`: the same depth built from nested `EList`s
//! instead of `ESet`s. `EListBody` is *not* one of the three self-contained arms — its children
//! go through `expr_child_pars` like any other node — so its call count must grow **linearly**
//! in depth. If the control's slope moves with the subject's, the instrument is measuring the
//! harness rather than the arms, and the reading is void.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, ESet, Expr, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;

/// `{{{…{0}…}}}` — `depth` nested single-element `ESet`s. The SUBJECT.
fn nested_set(depth: usize) -> Par {
    let mut par = ground_zero();
    for _ in 0..depth {
        par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: vec![par],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }]);
    }
    par
}

/// `[[[…[0]…]]]` — the same shape in `EList`s. The invariant CONTROL: `EListBody` is not one
/// of the three self-contained arms, so its cost must be linear in depth.
fn nested_list(depth: usize) -> Par {
    let mut par = ground_zero();
    for _ in 0..depth {
        par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![par],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }]);
    }
    par
}

fn ground_zero() -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(0)),
    }])
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let control = args.iter().any(|a| a == "--control");
    let depth: usize = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .and_then(|a| a.parse().ok())
        .unwrap_or(4);

    let term = if control {
        nested_list(depth)
    } else {
        nested_set(depth)
    };

    // Exactly one entry into the sorter. Everything callgrind attributes to `sort_match`
    // beyond this single call is re-entry.
    let scored = ParSortMatcher::sort_match(&term);

    // ⚠ Consume the result so nothing here is optimised away. A probe whose subject is
    // dead-code-eliminated measures the empty program and reports it as a fast one.
    println!(
        "{} depth={} score_atoms={}",
        if control {
            "control/EList"
        } else {
            "subject/ESet"
        },
        depth,
        format!("{:?}", scored.score).len()
    );
}
