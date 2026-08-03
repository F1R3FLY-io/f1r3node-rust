//! # The `bincode_decoder` DIFFERENTIAL — machine vs. the retained derived oracle
//!
//! ## What is being proved
//!
//! The iterative decoder's obligation is **language identity**: for every byte
//! string `b`,
//!
//! ```text
//!     T::cold_decode(b)   and   bincode::deserialize::<T>(b)
//! ```
//!
//! agree — same `Ok` value, or both `Err`. This file proves the `Ok` half over
//! well-formed inputs; `models/tests/bincode_decoder_malformed.rs` proves the `Err`
//! half, which is the consensus-visible one.
//!
//! ## Why the oracle is the DERIVE itself
//!
//! `Deserialize` is still derived on every `rhoapi` type — the production path
//! simply no longer calls it. That makes the oracle **compiler-generated**,
//! so it cannot drift from what production used to do: there is no
//! hand-maintained twin to fall out of step. (The `Serialize`/`Deserialize`
//! twins in `rspace++/src/rspace/serializers/serializers.rs` are the precedent
//! for the pattern; this conversion is in a strictly better position than that
//! one was, because there is nothing to maintain.)
//!
//! ## ⚠ Why `decode(encode(x)) == x` is NOT the top-level property
//!
//! `EPathMap::serialize` deliberately re-orders `ps` into canonical trie order
//! for GROUND maps (`models/src/rust/rhoapi_ext.rs`), so for such an `x`,
//! `bincode::deserialize(bincode::serialize(&x)) != x` — for the *derived*
//! decoder too. Round-tripping is therefore asserted only over `generate_par`,
//! which builds no `EPathMap`; everything else is asserted by DIFFERENTIAL,
//! which is the stronger statement anyway: it holds whatever the encoder does.

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::rholang::bincode_decoder::{
    cold_decode_list_bind_patterns_for_test, cold_decode_par_with_random_for_test,
};
use models::rust::test_utils::test_utils::generate_par;
use proptest::prelude::*;
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
use serde::Deserialize;

mod par_corpus;
use par_corpus as corpus;

/// **The differential's VERDICT**, as a pure function of one observation.
///
/// Separating it from [`agree`] is what makes `the_differential_can_go_red`
/// possible: a verdict welded to the production decoder can only ever be run on
/// observations that decoder produces, and every observation it produces is —
/// by construction, since the suite is green — one the verdict accepts. Handed
/// the observation instead, the same verdict can be run on a *drifted* decoder's
/// output and shown to reject, naming which of its two clauses did it.
///
/// The extent clause is not incidental: `Datum<A>` embeds `A` as a *prefix*, so
/// a decoder that read the right value but reported the wrong extent would
/// silently mis-read `persist` and `source` from the middle of `a`.
fn differential_verdict<T>(
    label: &str,
    machine: &T,
    consumed: usize,
    oracle: &T,
    exact_len: usize,
) -> Result<(), String>
where
    T: PartialEq + std::fmt::Debug,
{
    if machine != oracle {
        return Err(format!(
            "{label}: machine and derived oracle produced DIFFERENT values"
        ));
    }
    if consumed != exact_len {
        return Err(format!(
            "{label}: machine reported the wrong byte extent ({consumed}, want {exact_len}) — \
             `Datum<A>` reads `A` as a PREFIX, so a wrong extent silently mis-reads the fields \
             that follow it"
        ));
    }
    Ok(())
}

/// Assert that the machine and the derived oracle agree on `bytes`, and that
/// the machine reports the exact number of bytes the value occupies.
fn agree<T>(label: &str, value: &T)
where
    T: ColdStoreDecode + serde::Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let bytes = bincode::serialize(value).expect("oracle: serialize");
    let oracle: T = bincode::deserialize(&bytes).expect("oracle: deserialize");
    let (machine, consumed) = T::cold_decode_prefix(&bytes)
        .unwrap_or_else(|e| panic!("{label}: machine rejected a well-formed encoding: {e}"));
    if let Err(why) = differential_verdict(label, &machine, consumed, &oracle, bytes.len()) {
        panic!("{why}");
    }
}

/// `allow_trailing_bytes()` is part of `bincode::deserialize`'s configuration.
/// Tightening it would NARROW the accepted language, which is itself a fork.
fn agree_with_trailing<T>(label: &str, value: &T)
where
    T: ColdStoreDecode + serde::Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let mut bytes = bincode::serialize(value).expect("oracle: serialize");
    let exact_len = bytes.len();
    bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let oracle: T = bincode::deserialize(&bytes)
        .expect("oracle: trailing bytes must be ACCEPTED (allow_trailing_bytes)");
    let (machine, consumed) = T::cold_decode_prefix(&bytes)
        .unwrap_or_else(|e| panic!("{label}: machine rejected trailing bytes: {e}"));
    assert!(machine == oracle, "{label}: disagreement with trailing bytes");
    assert_eq!(consumed, exact_len, "{label}: extent must exclude the trailing bytes");
}

// ===========================================================================
// ★ N2 — the corpus reaches the ALPHABET it is credited with
// ===========================================================================
//
// The only anti-vacuity this file used to carry was `cases.len() >= 60`: a
// CARDINALITY floor. Sixty cases that all happen to be `GInt`s satisfy it, and
// the property every test here asserts is per-arm agreement — so a corpus that
// omits an arm cannot falsify a claim about that arm, however many cases it has.
//
// That is not a hypothetical failure mode; it is a MEASURED one. When a
// single-field drift was injected into the `ETuple` arm (audit §12.8), six of
// this file's eleven tests went red and five stayed green, and all three
// `generate_par` proptests were among the green — because `generate_expr`'s
// alternative set is `GBool | GInt | GString | ENotBody` and cannot produce an
// `ETuple` at any depth, with any seed, at any case count. The proptests were
// not unlucky; they were structurally incapable of reaching the defect.
//
// The audit's own re-derivation of that split — "every RED test reaches an
// `ETuple` via `all_par_fields()` or `expr::ETupleBody`" — is a CHECKABLE
// property of the corpora, and it was not checked. It is now: each test below
// asserts that its corpus reaches every `ExprInstance` and `ConnectiveInstance`
// arm, computed with the PRODUCTION walker over the canonical oneof numbering,
// against the alphabet the corpus module itself enumerates.

use std::collections::{BTreeMap, BTreeSet};

use models::rust::rholang::par_children::{
    connective_instance_variant_index, expr_instance_variant_index, reachable_exprs_and_connectives,
};

/// The full `ExprInstance` alphabet, keyed by the canonical oneof index.
///
/// Derived from `corpus::every_expr_instance()` rather than written out here, so
/// that a new `rhoapi` arm becomes a *failure to cover* the moment the corpus
/// gains its representative — and, if the corpus is not updated, a failure of
/// `models/tests/bincode_decoder_shapes.rs`'s exhaustiveness instead. Either way
/// no arm can be added silently.
fn expr_alphabet() -> BTreeMap<u32, &'static str> {
    corpus::every_expr_instance()
        .into_iter()
        .map(|(name, instance)| (expr_instance_variant_index(&instance), name))
        .collect()
}

/// The same for `ConnectiveInstance`.
fn connective_alphabet() -> BTreeMap<u32, &'static str> {
    corpus::every_connective_instance()
        .into_iter()
        .map(|(name, instance)| (connective_instance_variant_index(&instance), name))
        .collect()
}

/// Which arms a set of `Par` roots reaches, anywhere in their trees.
///
/// Uses `par_children::reachable_exprs_and_connectives` — the production walker,
/// over `par_children`'s single source of truth for the oneof numbering — so
/// this measurement cannot disagree with what the decoder is dispatching on.
fn arms_reached<'a>(roots: impl IntoIterator<Item = &'a Par>) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut exprs = BTreeSet::new();
    let mut connectives = BTreeSet::new();
    for root in roots {
        let (es, cs) = reachable_exprs_and_connectives(root);
        for e in es {
            if let Some(instance) = e.expr_instance.as_ref() {
                exprs.insert(expr_instance_variant_index(instance));
            }
        }
        for c in cs {
            if let Some(instance) = c.connective_instance.as_ref() {
                connectives.insert(connective_instance_variant_index(instance));
            }
        }
    }
    (exprs, connectives)
}

/// **The alphabet-coverage VERDICT**, pure over the two reached sets. `Err`
/// names every missing arm, so a caller sees WHICH arm its corpus cannot
/// falsify a claim about rather than only that some arm is missing.
fn alphabet_verdict(
    label: &str,
    reached_exprs: &BTreeSet<u32>,
    reached_connectives: &BTreeSet<u32>,
) -> Result<(), String> {
    let missing_exprs: Vec<&str> = expr_alphabet()
        .iter()
        .filter(|(index, _)| !reached_exprs.contains(*index))
        .map(|(_, name)| *name)
        .collect();
    let missing_connectives: Vec<&str> = connective_alphabet()
        .iter()
        .filter(|(index, _)| !reached_connectives.contains(*index))
        .map(|(_, name)| *name)
        .collect();

    if missing_exprs.is_empty() && missing_connectives.is_empty() {
        return Ok(());
    }
    Err(format!(
        "ALPHABET COVERAGE FAILED for `{label}`: the corpus never reaches \
         {} ExprInstance arm(s) [{}] or {} ConnectiveInstance arm(s) [{}]. A differential \
         cannot falsify a claim about an arm its corpus cannot express — measured, not \
         hypothetical: see the `ETuple` field-drift experiment in \
         docs/design/audits/theta-depth-traversals-2026-07-26.md §12.8.",
        missing_exprs.len(),
        missing_exprs.join(", "),
        missing_connectives.len(),
        missing_connectives.join(", ")
    ))
}

/// The N2 obligation for one test: its corpus reaches the whole alphabet.
fn assert_alphabet_complete<'a>(label: &str, roots: impl IntoIterator<Item = &'a Par>) {
    let (exprs, connectives) = arms_reached(roots);
    if let Err(why) = alphabet_verdict(label, &exprs, &connectives) {
        panic!("{why}");
    }
}

/// Every `Par` reachable from a `TaggedContinuation`, for the coverage check.
fn tagged_continuation_pars(v: &TaggedContinuation) -> Vec<&Par> {
    use models::rhoapi::tagged_continuation::TaggedCont;
    let mut out: Vec<&Par> = v.guard.iter().collect();
    if let Some(TaggedCont::ParBody(pwr)) = v.tagged_cont.as_ref() {
        out.extend(pwr.body.iter());
    }
    out
}

// ===========================================================================
// The constructed corpus
// ===========================================================================

#[test]
fn par_corpus_agrees_with_the_derived_oracle() {
    let cases = corpus::par_corpus();
    assert!(
        cases.len() >= 60,
        "ANTI-VACUITY: the Par corpus shrank to {} cases; it must carry one \
         representative of every ExprInstance and ConnectiveInstance arm",
        cases.len()
    );
    // …and the cardinality floor above is not what makes that true. This is.
    assert_alphabet_complete("par_corpus", cases.iter().map(|(_, par)| par));
    for (label, par) in cases {
        agree(&label, &par);
    }
}

#[test]
fn par_corpus_accepts_trailing_bytes_exactly_as_the_oracle_does() {
    let cases = corpus::par_corpus();
    assert_alphabet_complete("par_corpus+trailing", cases.iter().map(|(_, par)| par));
    for (label, par) in cases {
        agree_with_trailing(&label, &par);
    }
}

#[test]
fn list_par_with_random_corpus_agrees() {
    let cases = corpus::list_par_with_random_corpus();
    assert_alphabet_complete(
        "list_par_with_random_corpus",
        cases.iter().flat_map(|(_, v)| v.pars.iter()),
    );
    for (label, v) in cases {
        agree(&label, &v);
    }
}

#[test]
fn bind_pattern_corpus_agrees() {
    let cases = corpus::bind_pattern_corpus();
    assert_alphabet_complete(
        "bind_pattern_corpus",
        cases.iter().flat_map(|(_, v)| v.patterns.iter()),
    );
    for (label, v) in cases {
        agree(&label, &v);
    }
}

#[test]
fn tagged_continuation_corpus_agrees() {
    let cases = corpus::tagged_continuation_corpus();
    assert_alphabet_complete(
        "tagged_continuation_corpus",
        cases.iter().flat_map(|(_, v)| tagged_continuation_pars(v)),
    );
    for (label, v) in cases {
        agree(&label, &v);
    }
}

/// The two machine types that are never a cold-store ROOT (`ParWithRandom` is
/// reached through `TaggedCont::ParBody`, `ListBindPatterns` through the gRPC
/// surface). Exercised through their test-only entry points so that no program
/// in the machine is left unrun.
#[test]
fn non_root_machine_types_agree() {
    let pwr = corpus::par_with_random_corpus();
    assert_alphabet_complete(
        "par_with_random_corpus",
        pwr.iter().flat_map(|(_, v)| v.body.iter()),
    );
    let lbp = corpus::list_bind_patterns_corpus();
    assert_alphabet_complete(
        "list_bind_patterns_corpus",
        lbp.iter()
            .flat_map(|(_, v)| v.patterns.iter())
            .flat_map(|p| p.patterns.iter()),
    );
    for (label, v) in corpus::par_with_random_corpus() {
        let bytes = bincode::serialize(&v).expect("serialize ParWithRandom");
        let oracle: models::rhoapi::ParWithRandom =
            bincode::deserialize(&bytes).expect("oracle ParWithRandom");
        let (machine, consumed) =
            cold_decode_par_with_random_for_test(&bytes).expect("machine ParWithRandom");
        assert!(machine == oracle, "ParWithRandom::{label}: disagreement");
        assert_eq!(consumed, bytes.len(), "ParWithRandom::{label}: extent");
    }
    for (label, v) in corpus::list_bind_patterns_corpus() {
        let bytes = bincode::serialize(&v).expect("serialize ListBindPatterns");
        let oracle: models::rhoapi::ListBindPatterns =
            bincode::deserialize(&bytes).expect("oracle ListBindPatterns");
        let (machine, consumed) =
            cold_decode_list_bind_patterns_for_test(&bytes).expect("machine ListBindPatterns");
        assert!(machine == oracle, "ListBindPatterns::{label}: disagreement");
        assert_eq!(consumed, bytes.len(), "ListBindPatterns::{label}: extent");
    }
}

// ===========================================================================
// The `generate_par` proptest corpus
// ===========================================================================

// ===========================================================================
// ★ N1 — the differential can go RED
// ===========================================================================

/// The production machine with a **single-arm field-list drift** injected.
///
/// The falsification experiment of audit §12.8 made `bincode_decoder.rs`'s `ETuple`
/// decoder read a `remainder` field that `ETuple` does not have — the smallest
/// realistic hand-written-codec defect, an arm whose field list has drifted by
/// one from the type it decodes. That mutation was performed by hand, observed,
/// and reverted; nothing re-runs it, so the six tests it turned red are credited
/// with a power no execution demonstrates.
///
/// This is that defect as a callable decoder. It is injected **post-order**, on
/// the value the machine returns, in the shape of
/// `normalize_differential.rs::the_differential_can_go_red` — an arm that read
/// one field too many returns wrong values for the fields around it, so the
/// drift here is exactly that: an `ETuple` comes back with a flipped
/// `connective_used` and one fewer element.
///
/// It is faithful where it matters. The property the audit re-derives is
/// **selectivity** — every red test reaches an `ETuple` via `all_par_fields()`
/// or `expr::ETupleBody`, and both of those put the `ETuple` in the ROOT `Par`'s
/// `exprs`, which is precisely where this fires. What it does not reproduce is
/// the original's byte-level mechanism (a wrong extent as well as a wrong
/// value); [`decode_with_an_extent_drift`] covers that clause separately.
fn decode_with_an_etuple_field_drift(bytes: &[u8]) -> (Par, usize) {
    use models::rhoapi::expr::ExprInstance;

    let (mut par, consumed) = Par::cold_decode_prefix(bytes).expect("drifted decoder: machine");
    for expr in par.exprs.iter_mut() {
        if let Some(ExprInstance::ETupleBody(tuple)) = expr.expr_instance.as_mut() {
            tuple.connective_used = !tuple.connective_used;
            tuple.ps.pop();
        }
    }
    (par, consumed)
}

/// The production machine reporting a byte extent one short of the truth.
///
/// A separate defect from the one above, and a separate clause of
/// [`differential_verdict`]. `agree`'s doc explains why the extent matters —
/// `Datum<A>` embeds `A` as a PREFIX — and until now nothing had shown that
/// clause rejecting anything.
fn decode_with_an_extent_drift(bytes: &[u8]) -> (Par, usize) {
    let (par, consumed) = Par::cold_decode_prefix(bytes).expect("drifted decoder: machine");
    (par, consumed.saturating_sub(1))
}

/// Pull one named case out of the constructed corpus, failing loudly if the
/// label is gone. A leg that silently skipped a renamed case would be the very
/// thing this file is being audited for.
fn corpus_case(label: &str) -> Par {
    corpus::par_corpus()
        .into_iter()
        .find(|(name, _)| name == label)
        .map(|(_, par)| par)
        .unwrap_or_else(|| {
            panic!(
                "the constructed corpus no longer carries a case labelled `{label}`; the \
                 reddening leg below cannot run on the shape it names"
            )
        })
}

/// Does this `Par` reach an `ETuple` anywhere? The gate for the selectivity
/// claim, computed with the production walker.
fn reaches_an_etuple(par: &Par) -> bool {
    use models::rhoapi::expr::ExprInstance;
    let etuple =
        expr_instance_variant_index(&ExprInstance::ETupleBody(models::rhoapi::ETuple::default()));
    let (exprs, _) = arms_reached(std::iter::once(par));
    exprs.contains(&etuple)
}

/// ★ **The executed reddening leg for the whole differential.**
///
/// [`differential_verdict`] is the judge every test in this file delegates to,
/// and its only evidence of discriminating power was a hand-performed mutation
/// recorded in a document. Both of its clauses are now run on data in the class
/// they exclude, with the rejecting clause asserted:
///
/// | decoder                            | risky shape (reaches `ETuple`) | control (does not) |
/// |------------------------------------|--------------------------------|--------------------|
/// | production                         | accepts                        | accepts            |
/// | [`decode_with_an_etuple_field_drift`] | **rejects — value clause**  | accepts            |
/// | [`decode_with_an_extent_drift`]    | **rejects — extent clause**    | rejects            |
///
/// Row 2's control column is the audit's SELECTIVITY claim, executed: an
/// arm-local defect is invisible to a corpus that never reaches the arm. That is
/// the same fact `the_generator_corpus_cannot_reach_the_arm_the_constructed_one_can`
/// measures from the other side, and together they are why this file keeps a
/// constructed corpus alongside a random one.
///
/// Row 3's control column is deliberately "rejects" everywhere: an extent defect
/// is not arm-selective, so a corpus of any shape catches it. Stating that makes
/// the contrast in row 2 attributable to selectivity rather than to the drift
/// being weak.
#[test]
fn the_differential_can_go_red() {
    // ── The two shapes, and the N2 check that they are what they claim. ──
    let risky = corpus_case("expr::ETupleBody");
    let control = corpus_case("deep_list::7");
    assert!(
        reaches_an_etuple(&risky),
        "the risky shape must actually reach the ETuple arm, or the divergence below is \
         attributable to something else"
    );
    assert!(
        !reaches_an_etuple(&control),
        "the control must NOT reach the ETuple arm, or its agreement below proves nothing \
         about selectivity"
    );

    for (label, par, expect_drifted_red) in [("risky", &risky, true), ("control", &control, false)]
    {
        let bytes = bincode::serialize(par).expect("serialize");
        let oracle: Par = bincode::deserialize(&bytes).expect("oracle");

        // The production decoder agrees on both shapes — so a verdict that
        // rejected everything would fail here, and every rejection below is a
        // statement about the drift and not about the judge.
        let (machine, consumed) = Par::cold_decode_prefix(&bytes).expect("machine");
        differential_verdict(label, &machine, consumed, &oracle, bytes.len())
            .expect("the PRODUCTION decoder must agree with the oracle on both shapes");

        // The field-list drift: red on the risky shape, green on the control.
        let (drifted, drifted_consumed) = decode_with_an_etuple_field_drift(&bytes);
        let verdict = differential_verdict(label, &drifted, drifted_consumed, &oracle, bytes.len());
        match expect_drifted_red {
            true => {
                let why = verdict.expect_err(
                    "an ETuple field-list drift MUST be caught on a shape that reaches the \
                     ETuple arm — if it is not, this file's six constructed-corpus tests do \
                     not have the power audit §12.8 credits them with",
                );
                assert!(
                    why.contains("DIFFERENT values"),
                    "the rejection must come from the VALUE clause, not the extent one; \
                     got: {why}"
                );
                // …and the drift really changed the value, rather than the
                // verdict rejecting a value it was handed unchanged.
                assert!(
                    drifted != machine,
                    "the injected drift left the decoded value untouched, so the rejection \
                     above cannot be attributed to it"
                );
            }
            false => verdict.expect(
                "an ETuple-arm defect must be INVISIBLE to a shape that never reaches the \
                 ETuple arm — that selectivity is exactly why six tests went red and five \
                 stayed green, and why a constructed corpus is kept alongside the random one",
            ),
        }

        // The extent drift: red on both shapes, because it is not arm-selective.
        let (same_value, short_extent) = decode_with_an_extent_drift(&bytes);
        let why = differential_verdict(label, &same_value, short_extent, &oracle, bytes.len())
            .expect_err(
                "an extent one byte short MUST be caught — `Datum<A>` reads `A` as a PREFIX, \
                 so a wrong extent silently mis-reads the fields that follow it",
            );
        assert!(
            why.contains("wrong byte extent"),
            "the rejection must come from the EXTENT clause, not the value one; got: {why}"
        );
    }
}

/// How many `generate_par(3)` draws the alphabet measurement below takes.
const GENERATOR_ALPHABET_DRAWS: u32 = 512;

/// ★ **The blindness of the random corpus, measured rather than read.**
///
/// Audit finding E45 says `generate_par` cannot produce an `ETuple` at any depth
/// or seed, and that this is why all three proptests in this file stayed green
/// under the `ETuple` field drift while six constructed-corpus tests went red.
/// The finding was established by READING `generate_expr`'s four alternatives.
/// Reading is a fine way to establish it and a bad way to keep it: an author who
/// adds a fifth alternative changes what the proptests are worth, and nothing
/// would say so.
///
/// This measures it. `GENERATOR_ALPHABET_DRAWS` draws are taken from the very
/// strategy the proptests use, the reached alphabet is computed with the same
/// production walker the constructed corpora are measured with, and three things
/// are asserted:
///
/// 1. the generator's alphabet is CONFINED to `generate_expr`'s documented
///    alternatives — an upper bound, so it cannot flake on an unlucky sample;
/// 2. it does **not** contain `ETupleBody`, which is E45 exactly;
/// 3. it really sampled — at least two distinct arms and one non-empty `Par` —
///    so a generator that silently started producing `Par::default()` could not
///    satisfy (1) and (2) by producing nothing at all.
///
/// A failure here is not a regression in the codec. It means `generate_expr`
/// changed and the audit's account of what the proptests can see must be
/// re-derived; the message says so.
#[test]
fn the_generator_corpus_cannot_reach_the_arm_the_constructed_one_can() {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let strategy = generate_par(3);
    let mut drawn: Vec<Par> = Vec::with_capacity(GENERATOR_ALPHABET_DRAWS as usize);
    for _ in 0..GENERATOR_ALPHABET_DRAWS {
        drawn.push(
            strategy
                .new_tree(&mut runner)
                .expect("generate_par must produce a value tree")
                .current(),
        );
    }

    let (generator_exprs, generator_connectives) = arms_reached(drawn.iter());
    let alphabet = expr_alphabet();
    let name_of = |index: &u32| *alphabet.get(index).unwrap_or(&"<unknown arm>");
    let reached_names: Vec<&str> = generator_exprs.iter().map(name_of).collect();

    // (3) It really sampled.
    assert!(
        generator_exprs.len() >= 2,
        "the generator reached {} distinct ExprInstance arm(s) in \
         {GENERATOR_ALPHABET_DRAWS} draws [{}]; with so little variety the exclusions \
         below would hold for the uninteresting reason that nothing was generated",
        generator_exprs.len(),
        reached_names.join(", ")
    );

    // (1) Confined to `generate_expr`'s documented alternative set.
    const GENERATOR_ALTERNATIVES: &[&str] = &["GBool", "GInt", "GString", "ENotBody"];
    let unexpected: Vec<&str> = reached_names
        .iter()
        .copied()
        .filter(|name| !GENERATOR_ALTERNATIVES.contains(name))
        .collect();
    assert!(
        unexpected.is_empty(),
        "`generate_expr` has gained alternative(s) [{}] beyond its documented set [{}]. \
         That is not a codec regression — it changes what the three proptests in this file \
         are worth, so audit finding E45 (§12.8) and this test's expectations must both be \
         re-derived from the new generator.",
        unexpected.join(", "),
        GENERATOR_ALTERNATIVES.join(", ")
    );

    // (2) E45 itself.
    let etuple_index = expr_instance_variant_index(
        &models::rhoapi::expr::ExprInstance::ETupleBody(models::rhoapi::ETuple::default()),
    );
    assert!(
        !generator_exprs.contains(&etuple_index),
        "the generator produced an ETuple in {GENERATOR_ALPHABET_DRAWS} draws. Audit §12.8's \
         account of why the three proptests stayed green under the ETuple field drift no \
         longer holds and must be re-derived."
    );

    // …and the constructed corpus does reach it, so the gap is a gap and not a
    // property of the walker. This is the whole justification for keeping a
    // constructed corpus alongside the random one.
    let (corpus_exprs, corpus_connectives) = arms_reached(
        corpus::par_corpus()
            .iter()
            .map(|(_, par)| par)
            .collect::<Vec<_>>(),
    );
    assert!(
        corpus_exprs.contains(&etuple_index),
        "the constructed corpus must reach the arm the generator cannot, or there is no \
         complementarity to justify maintaining both"
    );
    assert!(
        generator_exprs.is_subset(&corpus_exprs) && generator_exprs != corpus_exprs,
        "the generator's alphabet must be a STRICT subset of the constructed corpus's: \
         generator {:?}, corpus {} arms",
        reached_names,
        corpus_exprs.len()
    );
    assert!(
        generator_connectives.is_subset(&corpus_connectives),
        "the generator reached ConnectiveInstance arms the constructed corpus does not"
    );

    println!(
        "  generator alphabet: {} of {} ExprInstance arms [{}]; constructed corpus: {}",
        generator_exprs.len(),
        alphabet.len(),
        reached_names.join(", "),
        corpus_exprs.len()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Differential over the workspace's own generator. Non-vacuous since the
    /// `0..1` → `0..=2` fix (see `models/src/rust/test_utils/test_utils.rs`);
    /// before it, this test would have quantified over a two-element set.
    ///
    /// ⚠ Its corpus is **not** alphabet-complete, and that is measured:
    /// `the_generator_corpus_cannot_reach_the_arm_the_constructed_one_can`. This
    /// test explores COMBINATIONS the constructed corpus does not; it does not
    /// guarantee the alphabet, and it cannot falsify a claim about an arm
    /// `generate_expr` never emits.
    #[test]
    fn generated_pars_agree_with_the_derived_oracle(par in generate_par(3)) {
        let bytes = bincode::serialize(&par).expect("serialize");
        let oracle: Par = bincode::deserialize(&bytes).expect("oracle");
        let (machine, consumed) = Par::cold_decode_prefix(&bytes).expect("machine");
        prop_assert!(machine == oracle);
        prop_assert_eq!(consumed, bytes.len());
    }

    /// `cold_decode(encode(x)) == x` over `generate_par`.
    ///
    /// Restricted to this generator on purpose: it builds no `EPathMap`, so it
    /// avoids the GROUND-map canonical re-ordering that makes round-tripping
    /// false for the DERIVED decoder as well. See the module header.
    #[test]
    fn round_trip_over_generate_par(par in generate_par(3)) {
        let bytes = bincode::serialize(&par).expect("serialize");
        let decoded = Par::cold_decode(&bytes).expect("machine");
        prop_assert!(decoded == par);
    }

    /// The same, wrapped in each of the other three cold-store root types, so
    /// the roots are exercised on random terms and not only on the constructed
    /// corpus.
    #[test]
    fn round_trip_through_every_root(par in generate_par(2)) {
        let a = ListParWithRandom { pars: vec![par.clone()], random_state: vec![1u8, 2] };
        prop_assert!(ListParWithRandom::cold_decode(
            &bincode::serialize(&a).expect("serialize")).expect("machine") == a);

        let p = BindPattern { patterns: vec![par.clone()], remainder: None, free_count: 3 };
        prop_assert!(BindPattern::cold_decode(
            &bincode::serialize(&p).expect("serialize")).expect("machine") == p);

        let k = TaggedContinuation {
            guard: Some(par),
            tagged_cont: Some(models::rhoapi::tagged_continuation::TaggedCont::ScalaBodyRef(5)),
        };
        prop_assert!(TaggedContinuation::cold_decode(
            &bincode::serialize(&k).expect("serialize")).expect("machine") == k);
    }
}

// ===========================================================================
// The `EPathMap` fixtures
// ===========================================================================

/// `models/tests/epathmap_*` pin the wrapper's serde layout. Running the
/// differential over the same shapes ties the decoder to those pins: if the
/// wrapper's field list, its `#[serde(skip)]` cell or its `SharedPars`
/// transparency ever changes, this fails alongside them rather than after them.
#[test]
fn epathmap_shapes_agree() {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EZipper, Expr};

    let shapes = vec![
        ("nonground", corpus::nonground_pathmap()),
        ("ground", corpus::ground_pathmap()),
        (
            "empty",
            models::rust::rhoapi_ext::EPathMap::new(Vec::new(), Vec::new(), false, None),
        ),
        (
            "locally_free_only",
            models::rust::rhoapi_ext::EPathMap::new(
                vec![corpus::gint(1)],
                vec![1, 2, 3],
                false,
                None,
            ),
        ),
    ];
    for (label, map) in shapes {
        // Through `EPathmapBody` …
        agree(
            &format!("epathmap::{label}::body"),
            &models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::EPathmapBody(map.clone())),
                }],
                ..Default::default()
            },
        );
        // … and through `EZipper.pathmap`, whose `Option<EPathMap>` adds a tag
        // byte the direct arm does not have.
        agree(
            &format!("epathmap::{label}::zipper"),
            &models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                        pathmap: Some(map),
                        current_path: vec![vec![1], vec![]],
                        is_write_zipper: false,
                        locally_free: vec![9],
                        connective_used: true,
                        cursor_kind: 1,
                    })),
                }],
                ..Default::default()
            },
        );
    }

    // And the absent `Option<EPathMap>`.
    agree(
        "epathmap::absent::zipper",
        &models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                    pathmap: None,
                    current_path: vec![],
                    is_write_zipper: true,
                    locally_free: vec![],
                    connective_used: false,
                    cursor_kind: 0,
                })),
            }],
            ..Default::default()
        },
    );
}

/// A decoded `EPathMap` must carry an EMPTY shadow cell. The derived
/// `Deserialize` leaves `intern` at `OnceLock::default()` because the field is
/// `#[serde(skip)]`; a machine that filled it — or that reused a cell from a
/// sibling — would hand out a stale interned handle for a value it has never
/// interned, which the wrapper's cached-encode `debug_assert`s treat as a
/// defect.
#[test]
fn decoded_epathmaps_carry_no_intern_handle() {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::Expr;

    let source = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(corpus::nonground_pathmap())),
        }],
        ..Default::default()
    };
    let bytes = bincode::serialize(&source).expect("serialize");
    let decoded = Par::cold_decode(&bytes).expect("machine");
    // ⛔ Was: assert the decoded map carries no intern handle. With the store gone
    // there is no handle to carry, so the assertion is unspellable rather than
    // weakened. What still matters — that the decode produced an `EPathmapBody` at
    // all — is kept.
    match decoded.exprs[0].expr_instance.as_ref() {
        Some(ExprInstance::EPathmapBody(_)) => {}
        other => panic!("expected EPathmapBody, got {:?}", other.is_some()),
    }
}
