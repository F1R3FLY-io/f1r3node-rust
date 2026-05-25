//! Property-based tests for the intuitionistic linear logic with
//! exponentials (ILLE) identities satisfied by the `Sig` algebra.
//!
//! Each identity has BOTH a hand-picked sanity test and a `proptest!`
//! block that randomizes the participating sub-Sigs. The proptest
//! invariant is always `channel_eq(lhs, rhs)`, where `channel_eq` post-
//! applies `ParSortMatcher::sort_match` canonicalization via
//! `SignatureChannel::from_sig` (see `test_support.rs`).
//!
//! References:
//! - Girard 1987 "Linear logic" (canonical)
//! - Wadler "There's no substitute for linear logic"
//! - Benton-Bierman-de Paiva-Hyland 1992 "Term assignment for intuitionistic linear logic"
//! - Bierman 1995, Mellies 2009 (exponential and coherence laws)
//! - `formal/rocq/cost_accounted_rho/theories/LLIdentities.v` (Rocq mirror)
//! - Plan §3.7 / §4.2 (`/home/dylon/.claude/plans/multi-sig-support-is-modeled-sparkling-minsky.md`)

use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;
use rholang::rust::interpreter::accounting::Sig;

use super::test_support::{any_sig, any_sig_bounded, channel_eq, fixed_atoms};

// ---------------------------------------------------------------------
// Multiplicative laws (Tensor ⊗)
// ---------------------------------------------------------------------

#[test]
fn tensor_commutative_sanity() {
    let [a, b, _, _] = fixed_atoms();
    let lhs = Sig::And(Box::new(a.clone()), Box::new(b.clone()));
    let rhs = Sig::And(Box::new(b), Box::new(a));
    assert!(channel_eq(&lhs, &rhs));
}

#[test]
fn tensor_associative_sanity() {
    let [a, b, c, _] = fixed_atoms();
    let lhs = Sig::And(
        Box::new(Sig::And(Box::new(a.clone()), Box::new(b.clone()))),
        Box::new(c.clone()),
    );
    let rhs = Sig::And(
        Box::new(a),
        Box::new(Sig::And(Box::new(b), Box::new(c))),
    );
    assert!(channel_eq(&lhs, &rhs));
}

#[test]
fn tensor_left_unit_sanity() {
    let [a, _, _, _] = fixed_atoms();
    let lhs = Sig::And(Box::new(Sig::Unit), Box::new(a.clone()));
    assert!(channel_eq(&lhs, &a));
}

#[test]
fn tensor_right_unit_sanity() {
    let [a, _, _, _] = fixed_atoms();
    let lhs = Sig::And(Box::new(a.clone()), Box::new(Sig::Unit));
    assert!(channel_eq(&lhs, &a));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn tensor_commutative_property(s in any_sig(), t in any_sig()) {
        let lhs = Sig::And(Box::new(s.clone()), Box::new(t.clone()));
        let rhs = Sig::And(Box::new(t), Box::new(s));
        prop_assert!(channel_eq(&lhs, &rhs));
    }

    #[test]
    fn tensor_associative_property(
        s in any_sig_bounded(3, 16),
        t in any_sig_bounded(3, 16),
        r in any_sig_bounded(3, 16),
    ) {
        let lhs = Sig::And(
            Box::new(Sig::And(Box::new(s.clone()), Box::new(t.clone()))),
            Box::new(r.clone()),
        );
        let rhs = Sig::And(
            Box::new(s),
            Box::new(Sig::And(Box::new(t), Box::new(r))),
        );
        prop_assert!(channel_eq(&lhs, &rhs));
    }

    #[test]
    fn tensor_left_unit_property(s in any_sig()) {
        let lhs = Sig::And(Box::new(Sig::Unit), Box::new(s.clone()));
        prop_assert!(channel_eq(&lhs, &s));
    }

    #[test]
    fn tensor_right_unit_property(s in any_sig()) {
        let lhs = Sig::And(Box::new(s.clone()), Box::new(Sig::Unit));
        prop_assert!(channel_eq(&lhs, &s));
    }
}

// ---------------------------------------------------------------------
// Additive laws (Plus ⊕)
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// At the channel-reflection layer, `Plus` is structurally
    /// identical to `Tensor` — branch witness is wire-only metadata.
    /// Commutativity therefore holds at the channel level (substrate
    /// distinction is enforced by the verifier, not the channel shape).
    #[test]
    fn plus_commutative_property(s in any_sig(), t in any_sig()) {
        let lhs = Sig::Plus(Box::new(s.clone()), Box::new(t.clone()));
        let rhs = Sig::Plus(Box::new(t), Box::new(s));
        prop_assert!(channel_eq(&lhs, &rhs));
    }

    #[test]
    fn plus_associative_property(
        s in any_sig_bounded(3, 16),
        t in any_sig_bounded(3, 16),
        r in any_sig_bounded(3, 16),
    ) {
        let lhs = Sig::Plus(
            Box::new(Sig::Plus(Box::new(s.clone()), Box::new(t.clone()))),
            Box::new(r.clone()),
        );
        let rhs = Sig::Plus(
            Box::new(s),
            Box::new(Sig::Plus(Box::new(t), Box::new(r))),
        );
        prop_assert!(channel_eq(&lhs, &rhs));
    }
}

// ---------------------------------------------------------------------
// Additive laws (With &)
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn with_commutative_property(s in any_sig(), t in any_sig()) {
        let lhs = Sig::With(Box::new(s.clone()), Box::new(t.clone()));
        let rhs = Sig::With(Box::new(t), Box::new(s));
        prop_assert!(channel_eq(&lhs, &rhs));
    }

    #[test]
    fn with_associative_property(
        s in any_sig_bounded(3, 16),
        t in any_sig_bounded(3, 16),
        r in any_sig_bounded(3, 16),
    ) {
        let lhs = Sig::With(
            Box::new(Sig::With(Box::new(s.clone()), Box::new(t.clone()))),
            Box::new(r.clone()),
        );
        let rhs = Sig::With(
            Box::new(s),
            Box::new(Sig::With(Box::new(t), Box::new(r))),
        );
        prop_assert!(channel_eq(&lhs, &rhs));
    }
}

// ---------------------------------------------------------------------
// Exponential laws (Bang !, WhyNot ?)
// ---------------------------------------------------------------------

#[test]
fn bang_idempotent_sanity() {
    let [a, _, _, _] = fixed_atoms();
    let inner = Sig::Bang(Box::new(a.clone()));
    let outer = Sig::Bang(Box::new(inner.clone()));
    assert!(channel_eq(&outer, &inner));
}

#[test]
fn whynot_idempotent_sanity() {
    let [a, _, _, _] = fixed_atoms();
    let inner = Sig::WhyNot(Box::new(a.clone()));
    let outer = Sig::WhyNot(Box::new(inner.clone()));
    assert!(channel_eq(&outer, &inner));
}

#[test]
fn bang_unit_sanity() {
    let lhs = Sig::Bang(Box::new(Sig::Unit));
    let rhs = Sig::Unit;
    assert!(channel_eq(&lhs, &rhs));
}

#[test]
fn whynot_unit_sanity() {
    let lhs = Sig::WhyNot(Box::new(Sig::Unit));
    let rhs = Sig::Unit;
    assert!(channel_eq(&lhs, &rhs));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn bang_idempotent_property(s in any_sig()) {
        let inner = Sig::Bang(Box::new(s));
        let outer = Sig::Bang(Box::new(inner.clone()));
        prop_assert!(channel_eq(&outer, &inner));
    }

    #[test]
    fn whynot_idempotent_property(s in any_sig()) {
        let inner = Sig::WhyNot(Box::new(s));
        let outer = Sig::WhyNot(Box::new(inner.clone()));
        prop_assert!(channel_eq(&outer, &inner));
    }

    /// Bang/WhyNot are channel-level identity wrappers (the
    /// replication semantics live at the verifier layer / capabilities
    /// registry, NOT in `SignatureChannel::from_sig`). Hence
    /// `!σ ≡_chan σ` at the channel reflection.
    #[test]
    fn bang_dereliction_at_channel_level(s in any_sig()) {
        let bang = Sig::Bang(Box::new(s.clone()));
        prop_assert!(channel_eq(&bang, &s));
    }

    #[test]
    fn whynot_dereliction_at_channel_level(s in any_sig()) {
        let whynot = Sig::WhyNot(Box::new(s.clone()));
        prop_assert!(channel_eq(&whynot, &s));
    }

    /// Bang monoidal law `!(σ ⊗ τ) ≡_chan !σ ⊗ !τ`. Holds at the channel
    /// level because Bang is a channel-identity wrapper (see above) AND
    /// Tensor is permutation-invariant via sort_match.
    #[test]
    fn bang_monoidal_property(s in any_sig(), t in any_sig()) {
        let lhs = Sig::Bang(Box::new(Sig::And(Box::new(s.clone()), Box::new(t.clone()))));
        let rhs = Sig::And(
            Box::new(Sig::Bang(Box::new(s))),
            Box::new(Sig::Bang(Box::new(t))),
        );
        prop_assert!(channel_eq(&lhs, &rhs));
    }
}

// ---------------------------------------------------------------------
// Linear implication (Lolly ⊸)
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Currying / closed-monoidal adjunction:
    ///   `(σ ⊗ τ) ⊸ ρ ≡_chan σ ⊸ (τ ⊸ ρ)`
    /// Holds at the channel level because all three connectives reduce
    /// to `concatenate_pars + sort_match` over the three sub-channels.
    #[test]
    fn lolly_curry_property(
        s in any_sig_bounded(3, 12),
        t in any_sig_bounded(3, 12),
        r in any_sig_bounded(3, 12),
    ) {
        let lhs = Sig::Lolly(
            Box::new(Sig::And(Box::new(s.clone()), Box::new(t.clone()))),
            Box::new(r.clone()),
        );
        let rhs = Sig::Lolly(
            Box::new(s),
            Box::new(Sig::Lolly(Box::new(t), Box::new(r))),
        );
        prop_assert!(channel_eq(&lhs, &rhs));
    }

    /// Linear modus ponens at the channel level: `σ ⊗ (σ ⊸ τ) ≡_chan
    /// σ ⊗ σ ⊗ τ` — the substrate composes `σ ⊸ τ` as the channel
    /// union of σ and τ, and Tensor concatenates parallel channels.
    /// (The runtime modus-ponens rule consumes σ to produce τ; that's
    /// modeled in the Rocq Bisimulation.v reduction lemma, not at the
    /// channel reflection.)
    #[test]
    fn lolly_modus_ponens_channel_composition(s in any_sig(), t in any_sig()) {
        let lhs = Sig::And(
            Box::new(s.clone()),
            Box::new(Sig::Lolly(Box::new(s.clone()), Box::new(t.clone()))),
        );
        let rhs = Sig::And(
            Box::new(s.clone()),
            Box::new(Sig::And(Box::new(s), Box::new(t))),
        );
        prop_assert!(channel_eq(&lhs, &rhs));
    }
}

// ---------------------------------------------------------------------
// Threshold (substrate primitive, permutation-invariant in members)
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn threshold_members_permutation_invariant_property(
        k in 1u32..=4,
        ms in proptest::collection::vec(any_sig_bounded(3, 12), 1..=4),
    ) {
        let kk = k.min(ms.len() as u32);
        let mut reversed = ms.clone();
        reversed.reverse();
        let lhs = Sig::Threshold { threshold: kk, members: ms };
        let rhs = Sig::Threshold { threshold: kk, members: reversed };
        prop_assert!(channel_eq(&lhs, &rhs));
    }

    #[test]
    fn threshold_single_member_collapses_to_member(s in any_sig()) {
        let one = Sig::Threshold { threshold: 1, members: vec![s.clone()] };
        prop_assert!(channel_eq(&one, &s));
    }
}

// ---------------------------------------------------------------------
// Coherence (Mac Lane pentagon + triangle)
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Pentagon: `((s ⊗ t) ⊗ r) ⊗ u ≡ s ⊗ (t ⊗ (r ⊗ u))` via the
    /// associator. Since `ParSortMatcher::sort_match` is fully
    /// canonical (associativity-free), both sides collapse to the
    /// same flat-sorted channel.
    #[test]
    fn tensor_associator_pentagon_property(
        s in any_sig_bounded(3, 10),
        t in any_sig_bounded(3, 10),
        r in any_sig_bounded(3, 10),
        u in any_sig_bounded(3, 10),
    ) {
        let lhs = Sig::And(
            Box::new(Sig::And(
                Box::new(Sig::And(Box::new(s.clone()), Box::new(t.clone()))),
                Box::new(r.clone()),
            )),
            Box::new(u.clone()),
        );
        let rhs = Sig::And(
            Box::new(s),
            Box::new(Sig::And(
                Box::new(t),
                Box::new(Sig::And(Box::new(r), Box::new(u))),
            )),
        );
        prop_assert!(channel_eq(&lhs, &rhs));
    }

    /// Triangle: `(s ⊗ 1) ⊗ t ≡ s ⊗ (1 ⊗ t)`.
    #[test]
    fn tensor_unitor_triangle_property(
        s in any_sig_bounded(3, 12),
        t in any_sig_bounded(3, 12),
    ) {
        let lhs = Sig::And(
            Box::new(Sig::And(Box::new(s.clone()), Box::new(Sig::Unit))),
            Box::new(t.clone()),
        );
        let rhs = Sig::And(
            Box::new(s),
            Box::new(Sig::And(Box::new(Sig::Unit), Box::new(t))),
        );
        prop_assert!(channel_eq(&lhs, &rhs));
    }
}

// ---------------------------------------------------------------------
// Distributivity is FORBIDDEN by linear logic (anti-distributivity)
// ---------------------------------------------------------------------

/// Distributivity `σ ⊗ (τ ⊕ ρ) ≡ (σ ⊗ τ) ⊕ (σ ⊗ ρ)` is FALSE in linear
/// logic — the RHS duplicates σ. Linearity forbids unbounded
/// duplication of non-`!` resources. The Rocq theorem
/// `tensor_over_plus_subset_lhs_in_rhs` at
/// `formal/rocq/cost_accounted_rho/theories/LLIdentities.v:242`
/// proves only one direction (LHS atoms ⊆ RHS atoms).
///
/// This test EXHIBITS the asymmetry: for σ non-trivial, the RHS has
/// two copies of σ's channel while the LHS has one — confirming the
/// implementation respects linearity rather than silently flattening.
#[test]
fn anti_distributivity_tensor_over_plus_witnessed_by_atom_duplication() {
    let [s, t, r, _] = fixed_atoms();
    let lhs = Sig::And(
        Box::new(s.clone()),
        Box::new(Sig::Plus(Box::new(t.clone()), Box::new(r.clone()))),
    );
    let rhs = Sig::Plus(
        Box::new(Sig::And(Box::new(s.clone()), Box::new(t))),
        Box::new(Sig::And(Box::new(s), Box::new(r))),
    );
    assert!(
        !channel_eq(&lhs, &rhs),
        "Distributivity must NOT hold at the channel-multiset level — \
         RHS duplicates σ, violating LL linearity"
    );
}

/// The Rocq-proved one-directional containment (LHS atoms appear in
/// RHS atoms after duplication) is structurally guaranteed by the
/// substrate: every connective reduces to `concatenate_pars + sort_match`,
/// so LHS sub-channels textually appear in the RHS expansion.
/// This test checks the (weaker but provable) symmetric claim:
/// when σ = Unit, the asymmetry vanishes and both sides coincide.
#[test]
fn tensor_over_plus_distributive_degenerate_unit_witness() {
    let [t, r, _, _] = fixed_atoms();
    let lhs = Sig::And(
        Box::new(Sig::Unit),
        Box::new(Sig::Plus(Box::new(t.clone()), Box::new(r.clone()))),
    );
    let rhs = Sig::Plus(
        Box::new(Sig::And(Box::new(Sig::Unit), Box::new(t))),
        Box::new(Sig::And(Box::new(Sig::Unit), Box::new(r))),
    );
    assert!(channel_eq(&lhs, &rhs));
}
