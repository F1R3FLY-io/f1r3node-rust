//! # F1 — is the score/term correspondence an IFF, or only one direction?
//!
//! `models/tests/scored_term_sort_test.rs:339`
//! (`the_score_and_the_canonical_term_agree_with_each_other`) asserts
//! `sx.term == sy.term  ⟺  sx.score == sy.score`, over **generated** corpora.
//!
//! ⚠ `combine_emap` chains only `sorted_key.score` (`sort_combine.rs:1448`) and discards the
//! value's, and nothing else in an `EMap`'s score tree depends on the values. ⇒ `{3 → 30}` and
//! `{3 → 90}` should be **distinct canonical terms with identical score trees** — a witness
//! that the `⟸` direction is FALSE.
//!
//! ## Why this is not a curiosity
//!
//! `SortedParHashSet::create_from_vec` collects into a `HashSet<Par>` and then iterates **that**
//! into the vector it sorts (`sorted_par_hash_set.rs:22-24`). `HashSet` iteration order depends
//! on `RandomState`, which is seeded per process. `ScoredTerm::sort_vec` uses a **stable** sort,
//! so elements whose scores tie keep their input order — i.e. the **hash iteration order**. And
//! `par_set_type_mapper.rs:19` emits `ps: par_set.ps.sorted_pars` straight into the bytes that
//! `cost_accounting/sig.rs` signs.
//!
//! ⇒ If two distinct elements can tie on score, the canonical form of a set containing both is
//! **process-dependent**. This probe answers whether they can.
//!
//! ★ Run it more than once: the tie-order line is the one that may differ between runs, and a
//! single run cannot show that.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EMap, ESet, Expr, KeyValuePair, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;

fn gint(v: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(v)),
    }])
}

/// A single-pair map `{key → value}`.
fn map1(key: i64, value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(gint(key)),
                value: Some(gint(value)),
            }],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn set_of(ps: Vec<Par>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::ESetBody(ESet {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn main() {
    let a = map1(3, 30);
    let b = map1(3, 90);

    let sa = ParSortMatcher::sort_match(&a);
    let sb = ParSortMatcher::sort_match(&b);

    let terms_differ = sa.term != sb.term;
    let scores_equal = format!("{:?}", sa.score) == format!("{:?}", sb.score);

    println!("── F1: the score/term correspondence ──");
    println!("  terms differ  : {terms_differ}");
    println!("  scores EQUAL  : {scores_equal}");
    println!("  score(a)      : {:?}", sa.score);
    println!("  score(b)      : {:?}", sb.score);

    if terms_differ && scores_equal {
        println!("\n  ⛔ THE IFF IS FALSE. Two DISTINCT canonical terms carry IDENTICAL scores.");
        println!("     `the_score_and_the_canonical_term_agree_with_each_other` over-claims;");
        println!("     it passes on sampling luck, the same shape report §5.7.3 documents for");
        println!("     the three tests it replaced.");
    } else {
        println!("\n  the iff survives this witness.");
    }

    // ── The consequence: is a SET containing both process-dependent? ──
    //
    // Both orders are built and printed. If the two elements tie on score, the stable sort
    // preserves `HashSet` iteration order, so this line is the one that may differ BETWEEN RUNS.
    let both = set_of(vec![a.clone(), b.clone()]);
    let sorted_both = ParSortMatcher::sort_match(&both);
    println!(
        "\n  canonical bytes of {{ {{3→30}}, {{3→90}} }} : {}",
        hex(&sorted_both.term.encode_to_vec())
    );
    permutation_check();
    println!("  ★ run this repeatedly — if the line above is not STABLE across processes,");
    println!("    canonical form is seed-dependent on the path cost_accounting/sig.rs signs.");
}

/// ★★★ The DETERMINISTIC falsifier — no `HashSet`, no seed, one process.
///
/// `combine_par` sorts `Par.exprs` with `ScoredTerm::sort_vec`, fed from the message's own
/// field order. `|` is commutative, so `{3:30} | {3:90}` and `{3:90} | {3:30}` are two
/// spellings of the SAME process and must reach the same canonical form.
fn permutation_check() {
    let a = map1(3, 30);
    let b = map1(3, 90);

    let ab = Par::default().with_exprs(vec![
        a.exprs.first().expect("a has an expr").clone(),
        b.exprs.first().expect("b has an expr").clone(),
    ]);
    let ba = Par::default().with_exprs(vec![
        b.exprs.first().expect("b has an expr").clone(),
        a.exprs.first().expect("a has an expr").clone(),
    ]);

    let sab = ParSortMatcher::sort_match(&ab).term.encode_to_vec();
    let sba = ParSortMatcher::sort_match(&ba).term.encode_to_vec();

    println!("\n── the DETERMINISTIC falsifier: {{3:30}} | {{3:90}} vs {{3:90}} | {{3:30}} ──");
    println!("  canonical(ab) : {}", hex(&sab));
    println!("  canonical(ba) : {}", hex(&sba));
    if sab == sba {
        println!("  ✓ permutation-invariant — `|` collapses as it must.");
    } else {
        println!("\n  ⛔⛔ NOT PERMUTATION-INVARIANT. Two spellings of the SAME process sign");
        println!("      DIFFERENTLY. No HashSet, no seed, one process — these are bytes that");
        println!("      ARE defined today, and they are wrong.");
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
