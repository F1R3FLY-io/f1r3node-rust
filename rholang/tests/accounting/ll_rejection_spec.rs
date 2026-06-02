//! Rejection tests for the FORBIDDEN identities of linear logic.
//!
//! LL is defined as much by what it rejects as by what it accepts.
//! Without these tests, an implementation that silently accepts
//! contraction (`σ ⊢ σ ⊗ σ`), weakening (`σ ⊢ 1`), or commutes
//! Plus with Tensor at the variant level would still pass every
//! positive identity test in `ll_algebra_spec.rs`.
//!
//! References:
//! - Girard 1987 §III.2 ("Linearity: rejection of structural rules")
//! - cost-accounted-rho paper §3.7 ("must reject contraction")
//! - TypedCurrency `typed_value.tex` lines 307–363
//! - Plan §4.3 (`/home/dylon/.claude/plans/multi-sig-support-is-modeled-sparkling-minsky.md`)

use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;
use rholang::rust::interpreter::accounting::{RuntimeBudget, Sig, SignatureChannel};

use super::test_support::{any_sig, channel_eq, fixed_atoms};

use models::rhoapi::Par;

/// True iff `sig`'s channel reflection is non-trivial (i.e., NOT
/// `SignatureChannel { par: Par::default() }`). Many connectives
/// like `Bang(Plus(Unit, Unit))` recursively reflect to Unit, so
/// the proper LL-rejection filter must check channel-Unit, not
/// enum-Unit.
fn channel_is_non_unit(sig: &Sig) -> bool {
    let channel = SignatureChannel::from_sig(sig);
    channel.par != Par::default()
}

// ---------------------------------------------------------------------
// Enum-level variant distinctness — Plus/With/And/Lolly are NOT collapsed
// ---------------------------------------------------------------------

/// Verifies the four binary connectives (And/Plus/With/Lolly) remain
/// distinguishable at the Sig-enum level even when they share the same
/// sub-Sigs. The runtime dispatch and verifier layer rely on this:
/// even though `from_sig` collapses all four to identical channels at
/// the substrate, the enum variant carries the operational distinction.
#[test]
fn enum_distinguishes_and_plus_with_lolly_with_same_sub_sigs() {
    let [a, b, _, _] = fixed_atoms();
    let and_ab = Sig::And(Box::new(a.clone()), Box::new(b.clone()));
    let plus_ab = Sig::Plus(Box::new(a.clone()), Box::new(b.clone()));
    let with_ab = Sig::With(Box::new(a.clone()), Box::new(b.clone()));
    let lolly_ab = Sig::Lolly(Box::new(a), Box::new(b));
    // All four are pairwise distinct at the enum level.
    assert_ne!(and_ab, plus_ab);
    assert_ne!(and_ab, with_ab);
    assert_ne!(and_ab, lolly_ab);
    assert_ne!(plus_ab, with_ab);
    assert_ne!(plus_ab, lolly_ab);
    assert_ne!(with_ab, lolly_ab);
}

/// Bang and WhyNot are also distinct at the enum level even though
/// their channel reflections happen to coincide with the inner Sig.
#[test]
fn enum_distinguishes_bang_from_whynot_from_inner() {
    let [a, _, _, _] = fixed_atoms();
    let bang = Sig::Bang(Box::new(a.clone()));
    let whynot = Sig::WhyNot(Box::new(a.clone()));
    assert_ne!(bang, whynot);
    assert_ne!(bang, a);
    assert_ne!(whynot, a);
}

// ---------------------------------------------------------------------
// Domain separation: legacy vs compound deploy-signature paths
// ---------------------------------------------------------------------

/// `set_deploy_signature` (legacy) and `set_deploy_signatures` (Phase 2+
/// compound) MUST produce different `deploy_id` values even for the
/// same single signature input, because they use distinct domain
/// separators (`DEPLOY_SIGNATURE_DOMAIN` vs
/// `COMPOUND_DEPLOY_SIGNATURE_DOMAIN`). If they collided, multi-sig
/// deploys could replay legacy deploy_ids (deploy_id collision attack).
#[test]
fn legacy_vs_compound_set_deploy_signature_produces_distinct_deploy_ids() {
    let sig_bytes: Vec<u8> = (0..64).collect();

    let legacy = RuntimeBudget::new(rholang::rust::interpreter::accounting::costs::Cost::create(
        1000,
        "test budget",
    ));
    legacy.set_deploy_signature(&sig_bytes);
    let legacy_id = legacy.deploy_id();

    let compound = RuntimeBudget::new(rholang::rust::interpreter::accounting::costs::Cost::create(
        1000,
        "test budget",
    ));
    compound.set_deploy_signatures(&[&sig_bytes]);
    let compound_id = compound.deploy_id();

    assert_ne!(
        legacy_id, compound_id,
        "legacy and compound paths must produce distinct deploy_ids; \
         identical IDs would allow deploy_id replay across paths"
    );
}

/// Two distinct multi-sig deploys with the same wire signatures BUT
/// in different order must produce different `deploy_id`s. The
/// compound path's `set_deploy_signatures` deliberately encodes
/// signature ORDER into the deploy_id (the canonical sort happens at
/// the envelope layer, NOT at the deploy_id computation), so a
/// wire-reorder attack cannot reuse a deploy_id.
#[test]
fn compound_deploy_id_depends_on_signature_order() {
    let sig_a: Vec<u8> = vec![0x11; 32];
    let sig_b: Vec<u8> = vec![0x22; 32];

    let ab = RuntimeBudget::new(rholang::rust::interpreter::accounting::costs::Cost::create(
        1000,
        "test budget",
    ));
    ab.set_deploy_signatures(&[&sig_a, &sig_b]);
    let ab_id = ab.deploy_id();

    let ba = RuntimeBudget::new(rholang::rust::interpreter::accounting::costs::Cost::create(
        1000,
        "test budget",
    ));
    ba.set_deploy_signatures(&[&sig_b, &sig_a]);
    let ba_id = ba.deploy_id();

    assert_ne!(ab_id, ba_id);
}

// ---------------------------------------------------------------------
// Anti-contraction: σ ⊬ σ ⊗ σ (no duplication without `!`)
// ---------------------------------------------------------------------

/// Submitting the same wire signature twice yields a multi-sig deploy
/// (`set_deploy_signatures(&[s, s])`) whose deploy_id DIFFERS from
/// the single-presentation case (`set_deploy_signatures(&[s])`). The
/// substrate does not silently coalesce duplicated signatures.
#[test]
fn anti_contraction_duplicating_signature_yields_distinct_deploy_id() {
    let sig: Vec<u8> = vec![0xCC; 48];

    let once = RuntimeBudget::new(rholang::rust::interpreter::accounting::costs::Cost::create(
        1000,
        "test budget",
    ));
    once.set_deploy_signatures(&[&sig]);
    let once_id = once.deploy_id();

    let twice = RuntimeBudget::new(rholang::rust::interpreter::accounting::costs::Cost::create(
        1000,
        "test budget",
    ));
    twice.set_deploy_signatures(&[&sig, &sig]);
    let twice_id = twice.deploy_id();

    assert_ne!(
        once_id, twice_id,
        "presenting the same wire signature twice must NOT silently \
         collapse to a single presentation — that would violate LL \
         linearity (no contraction on non-`!` atoms)"
    );
}

// ---------------------------------------------------------------------
// Anti-Plus-Tensor / Anti-With-Tensor at the enum level
// ---------------------------------------------------------------------

/// `Sig::Plus(σ, τ)` and `Sig::And(σ, τ)` MUST be distinguishable at
/// the enum level even though their `SignatureChannel::from_sig`
/// reflections collapse to identical channels. The substrate-channel
/// collapse is intentional (verifier dispatches on the enum variant);
/// the rejection here is at the type-level: a future refactor that
/// merges these variants would break operational semantics.
#[test]
fn anti_plus_tensor_at_enum_layer() {
    let [a, b, _, _] = fixed_atoms();
    let plus_ab = Sig::Plus(Box::new(a.clone()), Box::new(b.clone()));
    let and_ab = Sig::And(Box::new(a), Box::new(b));
    assert_ne!(plus_ab, and_ab);
}

#[test]
fn anti_with_tensor_at_enum_layer() {
    let [a, b, _, _] = fixed_atoms();
    let with_ab = Sig::With(Box::new(a.clone()), Box::new(b.clone()));
    let and_ab = Sig::And(Box::new(a), Box::new(b));
    assert_ne!(with_ab, and_ab);
}

// ---------------------------------------------------------------------
// Threshold edge cases that MUST be rejected at the runtime layer
// ---------------------------------------------------------------------

/// Threshold(0, members) is malformed (quorum < 1). The proto-decoder
/// in `from_proto_cosigned_with_sig_algebra` (Phase 3 task #17)
/// catches this at the wire boundary; this test verifies the Sig
/// substrate also panics-or-errors on the malformed structure when
/// reflected. Reflection itself is total (no panic) — the rejection
/// lives at the verifier dispatch layer; for the substrate we simply
/// document via this test that the structure is constructible but
/// will be rejected downstream.
#[test]
fn threshold_with_more_than_members_size_constructible_but_invalid() {
    // The Sig enum permits the malformed value (substrate is total);
    // verifier dispatch in models::DeployData::from_proto_cosigned_
    // with_sig_algebra rejects threshold > members.len(). See
    // casper_message.rs:from_proto_cosigned_sig_algebra tests for the
    // wire-level rejection. This test just documents that the
    // substrate is permissive (no panic on reflection).
    let sig = Sig::Threshold {
        threshold: 5,
        members: vec![Sig::Ground(vec![0x01]), Sig::Ground(vec![0x02])],
    };
    let _channel = rholang::rust::interpreter::accounting::SignatureChannel::from_sig(&sig);
    // No panic = pass. The structural invariant is enforced upstream.
}

// ---------------------------------------------------------------------
// Property-based: random Sig values never accidentally satisfy
// forbidden identities
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// `σ ⊗ σ ≢ σ` for arbitrary non-Unit σ (anti-contraction at the
    /// channel level). The double-σ-tensor produces TWO copies of σ's
    /// atom set; the single σ produces ONE — distinct channels.
    /// Filter: σ must channel-reflect to a non-Unit `Par` — Bang/
    /// WhyNot wrappers around Unit-equivalent sub-channels collapse
    /// to Unit themselves, which would defeat the anti-contraction
    /// claim spuriously.
    #[test]
    fn anti_contraction_non_unit_sigma_self_tensor_distinct(
        s in any_sig().prop_filter("non-trivial channel", channel_is_non_unit),
    ) {
        let doubled = Sig::And(Box::new(s.clone()), Box::new(s.clone()));
        prop_assert!(
            !channel_eq(&doubled, &s),
            "σ ⊗ σ must NOT collapse to σ for non-trivial σ"
        );
    }

    /// `σ ⊗ τ ⊗ ρ ≢ σ` (anti-weakening at the channel level) for
    /// non-trivial τ. The atom set of the LHS strictly contains τ
    /// which the RHS lacks.
    #[test]
    fn anti_weakening_extra_atom_must_be_observable(
        s in any_sig().prop_filter("non-trivial channel", channel_is_non_unit),
        t in any_sig().prop_filter("non-trivial channel", channel_is_non_unit),
    ) {
        let with_extra = Sig::And(Box::new(s.clone()), Box::new(t));
        prop_assert!(
            !channel_eq(&with_extra, &s),
            "presenting extra atom τ must NOT silently dissolve to σ alone"
        );
    }
}
