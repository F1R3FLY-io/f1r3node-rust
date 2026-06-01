//! `ResourceLogic` — the OSLF-generated linear-resource proof-checker interface
//! (spec §7.7), the Rust mirror of the Rocq `OSLF_Funding_Logic_Sound` capstone
//! (`formal/rocq/cost_accounted_rho/theories/GSLTOSLFCapstone.v`) and the
//! validator-contract proof-checker obligation (DR-12).
//!
//! §7.7: "the OSLF functor ... can be extended to generate a linear resource
//! logic whose judgments ARE funding proofs; the static analysis is a proof
//! search in this logic, and the validator is a proof checker." A validator —
//! the built-in one or a customer's custom validator (DR-12) — is therefore a
//! proof checker for the funding judgment `Σ_s ≥ Δ_s`: it computes the demand
//! `Δ_s` of a (desugared) deploy body and admits iff the effective supply `Σ_s`
//! funds that demand (plus the shard's safety margin; Thm 20 over-approximation).
//!
//! This trait is the contract surface for that obligation. The built-in
//! [`DefaultResourceLogic`] delegates to the already-verified pure analyzer
//! ([`super::delta_sigma::demand`] / [`super::delta_sigma::is_funded`]) — so the
//! contract and the live D2 acceptance gate share ONE implementation and cannot
//! diverge. A custom validator implements [`ResourceLogic`] to supply its own
//! proof checker; the `resource_logic_conformance` test module checks any
//! implementation against the OSLF linear-logic laws the Rocq capstone proves:
//! the judgment is the resource inequality, it is decidable, a funded demand is
//! accepted and an underfunded one rejected (soundness), and supply is monotone
//! (no contraction — more supply never un-funds a demand, the operational image
//! of `ll_linear_no_contraction`, Remark 21 "≤1 competitor wins").

use models::rhoapi::Par;

use super::delta_sigma::{demand, is_funded, DemandEntry};
use super::Sig;

/// The linear-resource proof-checker interface (spec §7.7; the DR-12 validator
/// proof-checker obligation). Implementors decide deploy admission as a proof
/// search in the OSLF-generated linear resource logic.
pub trait ResourceLogic {
    /// `Δ_s` — the per-signature demand of a fully-desugared deploy body
    /// `desugared` under its envelope signature `deploy_sig`
    /// (cost-accounted-rho Def 17).
    fn demand(&self, desugared: &Par, deploy_sig: &Sig) -> DemandEntry;

    /// The funding judgment / proof check (Def 19, Thm 20): the demand is funded
    /// iff the effective supply `effective_supply_s` (`Σ_s`) meets or exceeds the
    /// demand's known lower bound plus the shard `margin`. This is the decidable
    /// `funds Σ Δ := Δ ≤ Σ` of the OSLF resource logic.
    fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64, margin: i64) -> bool;
}

/// The built-in validator's proof checker: delegates to the verified pure
/// `delta_sigma` analyzer the live D2 acceptance gate uses, so the contract and
/// the gate cannot diverge.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultResourceLogic;

impl ResourceLogic for DefaultResourceLogic {
    #[inline]
    fn demand(&self, desugared: &Par, deploy_sig: &Sig) -> DemandEntry {
        demand(desugared, deploy_sig)
    }

    #[inline]
    fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64, margin: i64) -> bool {
        is_funded(analysis, effective_supply_s, margin)
    }
}

#[cfg(test)]
mod resource_logic_conformance {
    //! Conformance harness: the OSLF linear-logic laws (the Rocq
    //! `OSLF_Funding_Logic_Sound` capstone) that EVERY `ResourceLogic`
    //! implementation must satisfy. Run here against the built-in
    //! [`DefaultResourceLogic`]; a custom validator's impl (DR-12) is checked by
    //! re-running these laws with its type substituted for `R`.
    use super::*;

    fn resolvable(lower: i64) -> DemandEntry {
        DemandEntry {
            known_lower_bound: lower,
            unknown: false,
        }
    }

    /// Law (sound proof-checker, both directions): funded iff `Σ ≥ Δ + margin`.
    fn law_sound<R: ResourceLogic>(rl: &R) {
        for &lower in &[0i64, 1, 5, 100] {
            for &supply in &[0i64, 1, 5, 100, 101] {
                for &margin in &[0i64, 1, 10] {
                    let d = resolvable(lower);
                    let funded = rl.is_funded(&d, supply, margin);
                    assert_eq!(
                        funded,
                        i128::from(supply) >= i128::from(lower) + i128::from(margin),
                        "funds judgment must be Σ ≥ Δ + margin (lower={lower}, supply={supply}, margin={margin})"
                    );
                }
            }
        }
    }

    /// Law (reject underfunded): a positive demand against zero supply (an absent
    /// pool) at zero margin is rejected — the Rust mirror of
    /// `strict_reject_when_underfunded`.
    fn law_reject_underfunded<R: ResourceLogic>(rl: &R) {
        assert!(!rl.is_funded(&resolvable(1), 0, 0));
        assert!(!rl.is_funded(&resolvable(7), 0, 0));
    }

    /// Law (no contraction / supply monotone): increasing the supply never turns
    /// a funded demand UNfunded — the operational image of
    /// `ll_linear_no_contraction` (tokens are consumed, never duplicated, so more
    /// supply only ever helps).
    fn law_supply_monotone<R: ResourceLogic>(rl: &R) {
        for &lower in &[0i64, 3, 50] {
            for &margin in &[0i64, 2] {
                for supply in 0i64..60 {
                    if rl.is_funded(&resolvable(lower), supply, margin) {
                        assert!(
                            rl.is_funded(&resolvable(lower), supply + 1, margin),
                            "is_funded must be monotone in supply (lower={lower}, supply={supply}, margin={margin})"
                        );
                    }
                }
            }
        }
    }

    /// Law (decidable): the check always returns a verdict (a total function) —
    /// the decidable `funds` of Thm 20.
    fn law_decidable<R: ResourceLogic>(rl: &R) {
        let _verdict: bool = rl.is_funded(&resolvable(3), 5, 0);
    }

    #[test]
    fn default_resource_logic_satisfies_oslf_laws() {
        let rl = DefaultResourceLogic;
        law_sound(&rl);
        law_reject_underfunded(&rl);
        law_supply_monotone(&rl);
        law_decidable(&rl);
    }

    /// The built-in `demand` delegate agrees with the free `delta_sigma::demand`
    /// — the contract uses the SAME analyzer the gate uses (no divergence).
    #[test]
    fn default_demand_delegates_to_delta_sigma() {
        let rl = DefaultResourceLogic;
        let par = Par::default();
        let sig = Sig::Ground(vec![1, 2, 3, 4]);
        assert_eq!(rl.demand(&par, &sig), demand(&par, &sig));
    }
}
