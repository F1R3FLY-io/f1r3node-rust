//! `ResourceLogic` — the OSLF-generated linear-resource proof-checker interface
//! (spec §7.7), the Rust mirror of the Rocq `OSLF_Funding_Logic_Sound` capstone
//! (`formal/rocq/cost_accounted_rho/theories/GSLTOSLFCapstone.v`) and the
//! validator-contract proof-checker obligation (DR-12).
//!
//! §7.7: "the OSLF functor ... can be extended to generate a linear resource
//! logic whose judgments ARE funding proofs; the static analysis is a proof
//! search in this logic, and the validator is a proof checker." A validator —
//! the built-in one or a customer's custom validator (DR-12) — is therefore a
//! proof checker for the funding judgment `Σ_s ≥ Δ_s`: it computes a certified
//! finite upper bound `Δ_s` for a desugared deploy body and admits iff the
//! effective supply `Σ_s` funds that reservation. Unprovable demand is rejected.
//! The same interface exposes the finite located spatial/modal checker in
//! [`super::oslf`]. Exact evidence can prove graded use and post-state claims;
//! conservative evidence can prove sufficiency without inventing a transition.
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
//! of `ll_linear_no_contraction`, Remark 21 "≤1 competitor wins"). The native
//! formula laws are proved in `CAOSLFSpatialModal.v` and checked concurrently in
//! `OslfLocatedTyping.tla`.

use models::rhoapi::Par;

use super::authority::{DemandBound, ResourceMultiset, UnprovableDemand};
use super::delta_sigma::{self, DemandEntry};
use super::oslf::{self, CheckError, Formula, ResourceObservation};
use super::Sig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceDecomposition<K> {
    pub compound: K,
    pub left: K,
    pub right: K,
}

pub trait ResourceSignature: Clone {
    type Key: Clone + Ord + Eq;

    fn key(&self) -> Self::Key;

    fn split_join_decompositions(&self, out: &mut Vec<ResourceDecomposition<Self::Key>>);
}

pub trait GsltPresentation {
    type Program;
    type CanonicalProgram;
    type Signature: ResourceSignature;

    /// A presentation may be unable to project a neutral Cost(G) grade into
    /// a fundable canonical term. Never fabricate a canonical result in that
    /// case; the host boundary must propagate `UnprovableDemand`.
    fn canonicalize_for_funding(
        &self,
        program: &Self::Program,
    ) -> Result<Self::CanonicalProgram, UnprovableDemand>;
}

pub trait OslfResourceLogic<G: GsltPresentation> {
    fn demand(&self, canonical: &G::CanonicalProgram, deploy_sig: &G::Signature) -> DemandEntry;

    fn demand_bound(
        &self,
        _canonical: &G::CanonicalProgram,
        _deploy_sig: &G::Signature,
    ) -> DemandBound<<G::Signature as ResourceSignature>::Key> {
        // A generic GSLT demand estimate is not a certificate. Concrete
        // presentations must override this with a checked bound/proof (as
        // `DefaultResourceLogic` does for RhoGslt); otherwise fail closed.
        DemandBound::Unprovable(UnprovableDemand::UnsupportedSyntax)
    }

    fn verify_demand_bound(
        &self,
        canonical: &G::CanonicalProgram,
        deploy_sig: &G::Signature,
        bound: &DemandBound<<G::Signature as ResourceSignature>::Key>,
    ) -> Option<ResourceMultiset<<G::Signature as ResourceSignature>::Key>> {
        let DemandBound::Exact(claimed) = bound else {
            return None;
        };
        match self.demand_bound(canonical, deploy_sig) {
            DemandBound::Exact(expected) if claimed == &expected => Some(expected),
            _ => None,
        }
    }

    fn is_funded_bound(
        &self,
        canonical: &G::CanonicalProgram,
        deploy_sig: &G::Signature,
        analysis: &DemandBound<<G::Signature as ResourceSignature>::Key>,
        effective_supply: &ResourceMultiset<<G::Signature as ResourceSignature>::Key>,
    ) -> bool {
        self.verify_demand_bound(canonical, deploy_sig, analysis)
            .is_some_and(|reservation| effective_supply.dominates(&reservation))
    }

    fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64) -> bool;

    fn resource_observation(
        &self,
        canonical: &G::CanonicalProgram,
        deploy_sig: &G::Signature,
        authenticated_supply: &ResourceMultiset<<G::Signature as ResourceSignature>::Key>,
    ) -> Result<ResourceObservation<<G::Signature as ResourceSignature>::Key>, CheckError> {
        let candidate = self.demand_bound(canonical, deploy_sig);
        let exact = match &candidate {
            DemandBound::Exact(_) => true,
            DemandBound::FiniteUpperBound { .. } => false,
            DemandBound::Unprovable(reason) => return Err(CheckError::Unprovable(reason.clone())),
        };
        // The candidate is only a claim. Use the verifier's reservation, not
        // the claimed bytes, to construct an observation for formula checking.
        let verified = self
            .verify_demand_bound(canonical, deploy_sig, &candidate)
            .ok_or(CheckError::Unprovable(UnprovableDemand::UnsupportedSyntax))?;
        if exact {
            Ok(ResourceObservation::exact(
                authenticated_supply.clone(),
                verified,
            ))
        } else {
            Ok(ResourceObservation::upper_bound(
                authenticated_supply.clone(),
                verified,
            ))
        }
    }

    fn check_formula(
        &self,
        canonical: &G::CanonicalProgram,
        deploy_sig: &G::Signature,
        authenticated_supply: &ResourceMultiset<<G::Signature as ResourceSignature>::Key>,
        formula: &Formula<<G::Signature as ResourceSignature>::Key>,
    ) -> Result<(), CheckError> {
        oslf::check(
            &self.resource_observation(canonical, deploy_sig, authenticated_supply)?,
            formula,
        )
    }
}

/// The Rholang specialization kept for existing validator integrations.
pub trait ResourceLogic: OslfResourceLogic<RhoGslt> {}

impl<T> ResourceLogic for T where T: OslfResourceLogic<RhoGslt> {}

#[derive(Clone, Copy, Debug, Default)]
pub struct RhoGslt;

impl GsltPresentation for RhoGslt {
    type Program = Par;
    type CanonicalProgram = Par;
    type Signature = Sig;

    #[inline]
    fn canonicalize_for_funding(&self, program: &Par) -> Result<Par, UnprovableDemand> {
        Ok(delta_sigma::desugar_for_funding(program))
    }
}

impl ResourceSignature for Sig {
    type Key = delta_sigma::SigKey;

    #[inline]
    fn key(&self) -> Self::Key { self.lane_hash() }

    fn split_join_decompositions(&self, out: &mut Vec<ResourceDecomposition<Self::Key>>) {
        let mut work = vec![self];
        while let Some(sig) = work.pop() {
            if let Sig::And(left, right) = sig {
                out.push(ResourceDecomposition {
                    compound: sig.key(),
                    left: left.key(),
                    right: right.key(),
                });
                work.push(right);
                work.push(left);
            }
        }
    }
}

/// The built-in validator's proof checker: delegates to the verified pure
/// `delta_sigma` analyzer the live D2 acceptance gate uses, so the contract and
/// the gate cannot diverge.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultResourceLogic;

impl OslfResourceLogic<RhoGslt> for DefaultResourceLogic {
    #[inline]
    fn demand(&self, canonical: &Par, deploy_sig: &Sig) -> DemandEntry {
        delta_sigma::demand(canonical, deploy_sig)
    }

    fn demand_bound(&self, canonical: &Par, deploy_sig: &Sig) -> DemandBound<delta_sigma::SigKey> {
        delta_sigma::demand_bound(canonical, deploy_sig)
    }

    fn verify_demand_bound(
        &self,
        canonical: &Par,
        deploy_sig: &Sig,
        bound: &DemandBound<delta_sigma::SigKey>,
    ) -> Option<ResourceMultiset<delta_sigma::SigKey>> {
        let expected = delta_sigma::demand_bound(canonical, deploy_sig);
        let expected_bound = match &expected {
            DemandBound::Exact(reservation) => reservation,
            DemandBound::FiniteUpperBound { bound, .. } => bound,
            DemandBound::Unprovable(_) => return None,
        };
        let valid = match (bound, &expected) {
            (DemandBound::Exact(reservation), DemandBound::Exact(expected)) => {
                reservation == expected
            }
            (
                DemandBound::FiniteUpperBound { bound, proof },
                DemandBound::FiniteUpperBound {
                    bound: expected_bound,
                    proof: expected_proof,
                },
            ) => bound == expected_bound && proof == expected_proof,
            _ => false,
        };
        if valid {
            Some(expected_bound.clone())
        } else {
            None
        }
    }

    #[inline]
    fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64) -> bool {
        delta_sigma::is_funded(analysis, effective_supply_s)
    }

    fn resource_observation(
        &self,
        canonical: &Par,
        deploy_sig: &Sig,
        authenticated_supply: &ResourceMultiset<delta_sigma::SigKey>,
    ) -> Result<ResourceObservation<delta_sigma::SigKey>, CheckError> {
        oslf::rho_observation(canonical, deploy_sig, authenticated_supply)
    }
}

// ─── Multi-sig payment delegation (§"Multi-Signature Execution"; Join cost schema) ───
//
// A multi-sig (compound `s₁∘s₂`) deploy's settled demand `k` is drawn from the
// PARTIES RESPONSIBLE for payment: the combined pool `Σ⟦s₁∘s₂⟧` and/or the
// component pools `Σ⟦s₁⟧`, `Σ⟦s₂⟧`. The cost-accounted-rho **Conservation of
// Authority** proposition (Join cost schema) fixes the TOTAL each participant
// pays — "Grouping (along any axis) changes the bookkeeping; it never changes the
// total" — and proves the partition/grouping FREE, while the funding-slot model
// makes the payer OPEN ("Funding: anyone who deposits"). So WHICH pools absorb
// `k`, and in what order, is a free but consensus-deterministic policy. This
// trait delegates that choice; [`DefaultApportionment`] is the built-in
// combined-pool-first policy (the historical behavior, byte-identical).

/// One pool the apportionment may draw from, with its LIVE residual balance at
/// decision time (the cross-group settlement ledger value, ≥ 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolResidual<K> {
    pub key: K,
    pub residual: i64,
}

/// The funding shape of one admitted signature group (mirrors the runtime's
/// binary-at-top-level compound form; an n ≥ 3 left-assoc `∘`-fold still presents
/// as ONE [`GroupShape::Compound`] here — nested `And` nodes carry no demand and
/// never reach the policy).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupShape<K> {
    /// Single-signature group: its own pool only.
    Single { own: PoolResidual<K> },
    /// Top-level compound `s₁∘s₂`: the combined pool plus the component pair.
    Compound {
        combined: PoolResidual<K>,
        left: PoolResidual<K>,
        right: PoolResidual<K>,
    },
}

/// A single `(pool, amount)` draw the policy elects. The caller applies each to
/// the residual ledger and accumulates it into the per-pool settlement debit. A
/// matched component PAIR is expressed as TWO equal `PoolDraw`s (left, right):
/// debiting both components by the same amount consumes ONE unit of the group's
/// authority per unit (Conservation of Authority). `amount ≥ 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolDraw<K> {
    pub key: K,
    pub amount: i64,
}

/// Delegation of "which parties pay, and in what order" for a multi-sig
/// process's token payment.
///
/// CONTRACT (both obligations are consensus-load-bearing — the settlement debit
/// is consensus state, recomputed identically on replay):
///   * **(D) Deterministic & pure.** [`apportion`](Self::apportion) is a pure
///     function of `(shape, k)` — no I/O, no clock, no order-unstable iteration.
///     Identical inputs MUST yield byte-identical output on the play and replay
///     paths (the fork-safety bar).
///   * **(C) Conservation.** When the pools can fund `k`, the policy MUST settle
///     exactly `k` units of group authority: for `Compound`, `draw_combined +
///     draw_pair == k` whenever `combined.residual + min(left.residual,
///     right.residual) ≥ k` (the admission bound guarantees this for an admitted
///     group); for `Single`, the sole draw is `k` (the pre-#12 non-residual-capped
///     own-pool debit; the `checked_sub` backstop in `close_block_deploy` is the
///     hard underflow guard).
///   * **(NO-OVERDRAW).** The summed amount drawn from each DISTINCT key MUST be
///     `≤ that key's residual`, so the cross-group ledger never goes negative
///     (a component pair is two `PoolDraw`s, each bounded by its own residual).
///
/// Returning draws in a fixed order keeps the caller's ledger updates
/// order-deterministic.
pub trait ApportionmentPolicy<G: GsltPresentation> {
    fn apportion(
        &self,
        shape: GroupShape<<G::Signature as ResourceSignature>::Key>,
        k: i64,
    ) -> Vec<PoolDraw<<G::Signature as ResourceSignature>::Key>>;
}

/// The built-in policy: **combined pool first, then the component pair equally** —
/// byte-identical to the historical inline logic of `compute_settlement_debits`
/// (the consensus default).
///
/// **Greg P8 (2026-06-15) — BALANCED across all wallets.** Greg's directive ("the
/// tensor operator is commutative; the cost should be balanced across all wallets;
/// the token decomposes into the balanced cost per wallet") is REALIZED by this
/// policy, not violated by it: the component pair `(left, right)` is debited the
/// SAME amount (`draw_pair` each), so every cosigner WALLET pays an equal share —
/// the "balanced cost per wallet." Commutativity holds because the draw depends
/// only on `{combined, left, right}` residuals (a set), never on source order, so
/// `s₁∘s₂` and `s₂∘s₁` settle identically (the order-of-appearance draw the P8
/// question worried about predates this trait, commit 8032adb6). The
/// combined-pool-FIRST step is the ORTHOGONAL joint-funds policy — `Σ⟦s₁∘s₂⟧` is a
/// JOINTLY owned pool (a co-signed combined-cell token), so spending it first is
/// itself balanced (both parties co-own it), and the per-wallet split applies to
/// the remainder. No `BalancedApportionment` replacement is needed; this IS the
/// balanced policy.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultApportionment;

impl<G: GsltPresentation> ApportionmentPolicy<G> for DefaultApportionment {
    fn apportion(
        &self,
        shape: GroupShape<<G::Signature as ResourceSignature>::Key>,
        k: i64,
    ) -> Vec<PoolDraw<<G::Signature as ResourceSignature>::Key>> {
        let mut out = Vec::with_capacity(3);
        match shape {
            GroupShape::Single { own } => {
                // Pre-#12 single-pool path: debit `k` on the own pool, NOT
                // residual-capped (the `close_block_deploy` checked_sub is the
                // hard underflow guard).
                if k > 0 {
                    out.push(PoolDraw {
                        key: own.key,
                        amount: k,
                    });
                }
            }
            GroupShape::Compound {
                combined,
                left,
                right,
            } => {
                let draw_compound = k.min(combined.residual);
                let remaining = k - draw_compound;
                let draw_pair = remaining.min(left.residual).min(right.residual).max(0);
                if draw_compound > 0 {
                    out.push(PoolDraw {
                        key: combined.key,
                        amount: draw_compound,
                    });
                }
                if draw_pair > 0 {
                    out.push(PoolDraw {
                        key: left.key,
                        amount: draw_pair,
                    });
                    out.push(PoolDraw {
                        key: right.key,
                        amount: draw_pair,
                    });
                }
            }
        }
        out
    }
}

/// The FLAT-FEE apportionment (Cost-Accounted Rho Stage-D `FeeExtract`): charges a
/// flat `k` tokens per group (one per admitted deploy), drawn combined-pool-FIRST
/// then from a SINGLE component — NEVER the matched component PAIR.
///
/// DISTINCT from the COST policy ([`DefaultApportionment`]), whose compound case
/// debits BOTH components `k` each (Conservation of Authority: a COMM consumes one
/// unit of group authority from EVERY cosigner). The `FeeExtract` is ONE PHYSICAL
/// token per deploy (cost-accounted-rho.tex:3637 "one client token consumed as
/// fee"; design OD-3 "flat one token per admitted client deploy"; Rocq
/// `TokenConservation.fee_collect`'s flat single-pool `f`), so a COMPOUND deploy
/// owes 1, not 2. Reusing the cost policy for the fee over-charged multi-sig
/// deploys up to 2× (red-team F-1); this policy fixes that while staying
/// conserving (the carved total equals the credit to the proposer SystemVault).
///
/// The single component is the canonical-first (`left`, `SigKey`-ascending) —
/// deterministic play↔replay. The admission bound `effectiveΣ = combined +
/// min(left,right) ≥ cost + fee` leaves a post-cost residual `combined_res +
/// min(left_res,right_res) ≥ fee`, so `remaining = fee − combined_res ≤
/// min(left_res,right_res) ≤ left_res` — the flat draw cannot underflow on an
/// admitted group (the `min(left_res)` cap is a defensive no-op there, mirroring
/// [`DefaultApportionment`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct FlatFeeApportionment;

impl<G: GsltPresentation> ApportionmentPolicy<G> for FlatFeeApportionment {
    fn apportion(
        &self,
        shape: GroupShape<<G::Signature as ResourceSignature>::Key>,
        k: i64,
    ) -> Vec<PoolDraw<<G::Signature as ResourceSignature>::Key>> {
        let mut out = Vec::with_capacity(2);
        match shape {
            GroupShape::Single { own } => {
                if k > 0 {
                    out.push(PoolDraw {
                        key: own.key,
                        amount: k,
                    });
                }
            }
            GroupShape::Compound {
                combined,
                left,
                right: _,
            } => {
                let draw_compound = k.min(combined.residual).max(0);
                let remaining = (k - draw_compound).min(left.residual).max(0);
                if draw_compound > 0 {
                    out.push(PoolDraw {
                        key: combined.key,
                        amount: draw_compound,
                    });
                }
                if remaining > 0 {
                    // ONE component (canonical-first), NOT the matched pair — the
                    // FeeExtract is a flat single token per deploy, not a per-COMM
                    // unit of group authority. This is what makes a compound's fee
                    // == 1, not 2.
                    out.push(PoolDraw {
                        key: left.key,
                        amount: remaining,
                    });
                }
            }
        }
        out
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

    fn resolvable(bound: i64) -> DemandEntry {
        DemandEntry {
            certified_upper_bound: bound,
            unknown: false,
        }
    }

    fn unresolvable(bound: i64) -> DemandEntry {
        DemandEntry {
            certified_upper_bound: bound,
            unknown: true,
        }
    }

    /// Law (sound proof-checker): certified finite demand is funded exactly when
    /// `Σ ≥ Δ`; unprovable demand is rejected for every finite supply.
    fn law_sound<G, R>(rl: &R)
    where
        G: GsltPresentation,
        R: OslfResourceLogic<G>,
    {
        for &bound in &[0i64, 1, 5, 100] {
            for &supply in &[0i64, 1, 5, 100, 101] {
                let resolved = rl.is_funded(&resolvable(bound), supply);
                assert_eq!(
                    resolved,
                    i128::from(supply) >= i128::from(bound),
                    "resolvable funds judgment must be Σ ≥ Δ (bound={bound}, supply={supply})"
                );
                assert!(!rl.is_funded(&unresolvable(bound), supply));
            }
        }
    }

    /// Law (reject underfunded): a positive demand against zero supply is
    /// rejected — the Rust mirror of
    /// `strict_reject_when_underfunded`.
    fn law_reject_underfunded<G, R>(rl: &R)
    where
        G: GsltPresentation,
        R: OslfResourceLogic<G>,
    {
        assert!(!rl.is_funded(&resolvable(1), 0));
        assert!(!rl.is_funded(&resolvable(7), 0));
    }

    /// Law (no contraction / supply monotone): increasing the supply never turns
    /// a funded demand UNfunded — the operational image of
    /// `ll_linear_no_contraction` (tokens are consumed, never duplicated, so more
    /// supply only ever helps).
    fn law_supply_monotone<G, R>(rl: &R)
    where
        G: GsltPresentation,
        R: OslfResourceLogic<G>,
    {
        for &lower in &[0i64, 3, 50] {
            for supply in 0i64..60 {
                if rl.is_funded(&resolvable(lower), supply) {
                    assert!(
                        rl.is_funded(&resolvable(lower), supply + 1),
                        "is_funded must be monotone in supply (lower={lower}, supply={supply})"
                    );
                }
            }
        }
    }

    /// Law (decidable): the check always returns a verdict (a total function) —
    /// the decidable `funds` of Thm 20.
    fn law_decidable<G, R>(rl: &R)
    where
        G: GsltPresentation,
        R: OslfResourceLogic<G>,
    {
        let _verdict: bool = rl.is_funded(&resolvable(3), 5);
    }

    #[test]
    fn default_resource_logic_satisfies_oslf_laws() {
        let rl = DefaultResourceLogic;
        law_sound::<RhoGslt, _>(&rl);
        law_reject_underfunded::<RhoGslt, _>(&rl);
        law_supply_monotone::<RhoGslt, _>(&rl);
        law_decidable::<RhoGslt, _>(&rl);
    }

    /// The built-in `demand` delegate agrees with the free `delta_sigma::demand`
    /// — the contract uses the SAME analyzer the gate uses (no divergence).
    #[test]
    fn default_demand_delegates_to_delta_sigma() {
        let rl = DefaultResourceLogic;
        let par = Par::default();
        let sig = Sig::Ground(vec![1, 2, 3, 4]);
        assert_eq!(rl.demand(&par, &sig), delta_sigma::demand(&par, &sig));
    }

    #[test]
    fn finite_bound_cannot_be_relabelled_as_exact_evidence() {
        let logic = DefaultResourceLogic;
        let canonical = Par::default();
        let signature = Sig::Ground(b"bound-only".to_vec());
        let certified = logic.demand_bound(&canonical, &signature);
        let DemandBound::FiniteUpperBound { bound, .. } = &certified else {
            panic!("native structural demand must be a finite bound");
        };
        assert_eq!(
            logic.verify_demand_bound(&canonical, &signature, &certified),
            Some(bound.clone()),
        );
        assert!(matches!(
            logic
                .resource_observation(&canonical, &signature, &ResourceMultiset::default())
                .unwrap()
                .demand,
            oslf::DemandKnowledge::UpperBound(_),
        ));
        let forged = DemandBound::Exact(bound.clone());
        assert_eq!(
            logic.verify_demand_bound(&canonical, &signature, &forged),
            None,
        );
        assert!(!logic.is_funded_bound(
            &canonical,
            &signature,
            &forged,
            &ResourceMultiset::singleton(signature.lane_hash(), u64::MAX),
        ));
    }

    #[derive(Clone)]
    struct FakeSig(u8);

    impl ResourceSignature for FakeSig {
        type Key = u8;

        fn key(&self) -> Self::Key { self.0 }

        fn split_join_decompositions(&self, out: &mut Vec<ResourceDecomposition<Self::Key>>) {
            if self.0 > 1 {
                out.push(ResourceDecomposition {
                    compound: self.0,
                    left: self.0 - 1,
                    right: 1,
                });
            }
        }
    }

    struct FakeGslt;

    impl GsltPresentation for FakeGslt {
        type Program = u32;
        type CanonicalProgram = u64;
        type Signature = FakeSig;

        fn canonicalize_for_funding(
            &self,
            program: &Self::Program,
        ) -> Result<Self::CanonicalProgram, UnprovableDemand> {
            Ok(u64::from(*program) * 2)
        }
    }

    struct FakeLogic;

    impl OslfResourceLogic<FakeGslt> for FakeLogic {
        fn demand(
            &self,
            canonical: &<FakeGslt as GsltPresentation>::CanonicalProgram,
            deploy_sig: &<FakeGslt as GsltPresentation>::Signature,
        ) -> DemandEntry {
            DemandEntry {
                certified_upper_bound: (*canonical as i64) + i64::from(deploy_sig.key()),
                unknown: false,
            }
        }

        fn demand_bound(
            &self,
            canonical: &<FakeGslt as GsltPresentation>::CanonicalProgram,
            deploy_sig: &<FakeGslt as GsltPresentation>::Signature,
        ) -> DemandBound<u8> {
            DemandBound::Exact(ResourceMultiset::singleton(
                deploy_sig.key(),
                u64::try_from(self.demand(canonical, deploy_sig).certified_upper_bound).unwrap(),
            ))
        }

        fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64) -> bool {
            !analysis.unknown
                && i128::from(effective_supply_s) >= i128::from(analysis.certified_upper_bound)
        }
    }

    #[test]
    fn oslf_resource_logic_is_generic_over_gslt_presentations() {
        let gslt = FakeGslt;
        let logic = FakeLogic;
        let canonical = gslt.canonicalize_for_funding(&7).unwrap();
        let sig = FakeSig(3);
        let demand = logic.demand(&canonical, &sig);
        let mut decompositions = Vec::new();
        sig.split_join_decompositions(&mut decompositions);
        assert_eq!(canonical, 14);
        assert_eq!(demand.certified_upper_bound, 17);
        assert!(logic.is_funded(&demand, 17));
        assert!(!logic.is_funded(&demand, 16));
        logic
            .check_formula(
                &canonical,
                &sig,
                &ResourceMultiset::singleton(3, 17),
                &Formula::Located {
                    surface: 3,
                    body: Box::new(Formula::Sufficient { surface: 3 }),
                },
            )
            .unwrap();
        assert_eq!(decompositions, vec![ResourceDecomposition {
            compound: 3,
            left: 2,
            right: 1
        }]);
    }

    struct UncertifiedLogic;

    impl OslfResourceLogic<FakeGslt> for UncertifiedLogic {
        fn demand(&self, _: &u64, _: &FakeSig) -> DemandEntry {
            DemandEntry {
                certified_upper_bound: 1,
                unknown: false,
            }
        }

        fn is_funded(&self, _: &DemandEntry, _: i64) -> bool { true }
    }

    #[test]
    fn generic_demand_estimate_cannot_fabricate_a_finite_certificate() {
        let logic = UncertifiedLogic;
        let signature = FakeSig(3);
        let canonical = FakeGslt.canonicalize_for_funding(&7).unwrap();
        let analysis = logic.demand_bound(&canonical, &signature);
        assert!(matches!(
            &analysis,
            DemandBound::Unprovable(UnprovableDemand::UnsupportedSyntax)
        ));
        assert!(!logic.is_funded_bound(
            &canonical,
            &signature,
            &analysis,
            &ResourceMultiset::singleton(signature.key(), u64::MAX),
        ));
        assert!(matches!(
            logic.resource_observation(
                &canonical,
                &signature,
                &ResourceMultiset::singleton(signature.key(), u64::MAX),
            ),
            Err(CheckError::Unprovable(UnprovableDemand::UnsupportedSyntax))
        ));
    }

    struct ForgedFiniteLogic;

    impl OslfResourceLogic<FakeGslt> for ForgedFiniteLogic {
        fn demand(&self, _: &u64, _: &FakeSig) -> DemandEntry {
            DemandEntry {
                certified_upper_bound: 1,
                unknown: false,
            }
        }

        fn demand_bound(&self, _: &u64, deploy_sig: &FakeSig) -> DemandBound<u8> {
            DemandBound::FiniteUpperBound {
                bound: ResourceMultiset::singleton(deploy_sig.key(), 1),
                proof: b"unverified-claim".to_vec(),
            }
        }

        fn is_funded(&self, _: &DemandEntry, _: i64) -> bool { true }
    }

    #[test]
    fn generic_formula_check_rejects_unverified_finite_claim() {
        let logic = ForgedFiniteLogic;
        let canonical = FakeGslt.canonicalize_for_funding(&7).unwrap();
        let signature = FakeSig(3);
        let supply = ResourceMultiset::singleton(signature.key(), 1);
        let claim = logic.demand_bound(&canonical, &signature);
        assert!(matches!(&claim, DemandBound::FiniteUpperBound { .. }));
        assert!(logic
            .verify_demand_bound(&canonical, &signature, &claim)
            .is_none());
        assert_eq!(
            logic.check_formula(&canonical, &signature, &supply, &Formula::Sufficient {
                surface: signature.key(),
            },),
            Err(CheckError::Unprovable(UnprovableDemand::UnsupportedSyntax)),
        );
    }

    mod generic_demand_verification_properties {
        use proptest::prelude::*;

        use super::*;

        struct FiniteSequence;

        impl GsltPresentation for FiniteSequence {
            type Program = Vec<(u8, u8)>;
            type CanonicalProgram = Vec<(u8, u8)>;
            type Signature = FakeSig;

            fn canonicalize_for_funding(
                &self,
                program: &Self::Program,
            ) -> Result<Self::CanonicalProgram, UnprovableDemand> {
                Ok(program.clone())
            }
        }

        struct ExactSequenceLogic;

        impl OslfResourceLogic<FiniteSequence> for ExactSequenceLogic {
            fn demand(&self, canonical: &Vec<(u8, u8)>, _: &FakeSig) -> DemandEntry {
                DemandEntry {
                    certified_upper_bound: canonical
                        .iter()
                        .map(|(_, units)| i64::from(*units))
                        .sum(),
                    unknown: false,
                }
            }

            fn demand_bound(
                &self,
                canonical: &Vec<(u8, u8)>,
                signature: &FakeSig,
            ) -> DemandBound<u8> {
                let mut reservation = ResourceMultiset::default();
                for &(offset, units) in canonical {
                    if units > 0 {
                        let amount = reservation
                            .0
                            .entry(signature.key() ^ offset)
                            .or_insert(0u64);
                        *amount = amount
                            .checked_add(u64::from(units))
                            .expect("finite sequence demand fits in u64");
                    }
                }
                DemandBound::Exact(reservation)
            }

            fn is_funded(&self, analysis: &DemandEntry, supply: i64) -> bool {
                delta_sigma::is_funded(analysis, supply)
            }
        }

        fn execution_events(program: &[(u8, u8)], signature: u8) -> Vec<u8> {
            let mut events = Vec::new();
            for &(offset, count) in program {
                for _ in 0..count {
                    events.push(signature ^ offset);
                }
            }
            events
        }

        fn realized_demand(program: &[(u8, u8)], signature: u8) -> ResourceMultiset<u8> {
            let mut realized = ResourceMultiset::default();
            for surface in execution_events(program, signature) {
                *realized.0.entry(surface).or_insert(0) += 1;
            }
            realized
        }

        fn arbitrary_reservation() -> impl Strategy<Value = ResourceMultiset<u8>> {
            proptest::collection::btree_map(
                any::<u8>(),
                prop_oneof![Just(0u64), Just(u64::MAX), 0u64..128],
                0..8,
            )
            .prop_map(ResourceMultiset)
        }

        proptest! {
            #[test]
            fn generic_exact_claim_authentication_matches_finite_execution(
                program in proptest::collection::vec((0u8..4, 0u8..8), 0..20),
                signature in any::<u8>(),
                claimed in arbitrary_reservation(),
            ) {
                let canonical = FiniteSequence
                    .canonicalize_for_funding(&program)
                    .expect("finite sequences canonicalize");
                let actual = realized_demand(&program, signature);
                let signature = FakeSig(signature);
                let verified = ExactSequenceLogic.verify_demand_bound(
                    &canonical,
                    &signature,
                    &DemandBound::Exact(claimed.clone()),
                );
                prop_assert_eq!(verified.is_some(), claimed == actual);
                if let Some(reservation) = verified {
                    prop_assert_eq!(reservation, actual.clone());
                }
                let checked = DemandBound::Exact(actual.clone());
                prop_assert_eq!(
                    ExactSequenceLogic.verify_demand_bound(&canonical, &signature, &checked),
                    Some(actual.clone()),
                );
                prop_assert_eq!(
                    ExactSequenceLogic.resource_observation(&canonical, &signature, &actual),
                    Ok(ResourceObservation::exact(actual.clone(), actual)),
                );
            }

            #[test]
            fn generic_funding_tracks_finite_execution_per_surface(
                program in proptest::collection::vec((0u8..4, 0u8..8), 0..20),
                signature in any::<u8>(),
                supply in arbitrary_reservation(),
            ) {
                let actual = realized_demand(&program, signature);
                let signature = FakeSig(signature);
                let claim = DemandBound::Exact(actual.clone());
                prop_assert_eq!(
                    ExactSequenceLogic.is_funded_bound(&program, &signature, &claim, &supply),
                    actual.0.iter().all(|(surface, count)| supply.get(surface) >= *count),
                );
                prop_assert!(ExactSequenceLogic.is_funded_bound(
                    &program, &signature, &claim, &actual,
                ));
                let extra = actual
                    .checked_add(&ResourceMultiset::singleton(signature.key(), 1))
                    .expect("finite fixture supply has room for one extra unit");
                prop_assert!(ExactSequenceLogic.is_funded_bound(
                    &program, &signature, &claim, &extra,
                ));
                for (&surface, &count) in &actual.0 {
                    let mut deficient = extra.clone();
                    deficient.0.insert(surface, count - 1);
                    prop_assert!(!ExactSequenceLogic.is_funded_bound(
                        &program, &signature, &claim, &deficient,
                    ));
                }
            }

            #[test]
            fn generic_exact_claim_rechecks_program_and_signature(
                mut program in proptest::collection::vec((0u8..4, 0u8..8), 0..20),
                signature in any::<u8>(),
            ) {
                let actual = realized_demand(&program, signature);
                let claim = DemandBound::Exact(actual.clone());
                if !actual.0.is_empty() {
                    prop_assert_eq!(
                        ExactSequenceLogic.verify_demand_bound(
                            &program, &FakeSig(signature ^ 128), &claim,
                        ),
                        None,
                    );
                }
                program.push((0, 1));
                prop_assert_eq!(
                    ExactSequenceLogic.verify_demand_bound(&program, &FakeSig(signature), &claim),
                    None,
                );
            }

            #[test]
            fn generic_uncertified_provider_rejects_every_exact_claim(
                program in any::<u32>(),
                signature in any::<u8>(),
                claimed in arbitrary_reservation(),
            ) {
                let canonical = FakeGslt
                    .canonicalize_for_funding(&program)
                    .expect("fake programs canonicalize");
                let signature = FakeSig(signature);
                let claim = DemandBound::Exact(claimed.clone());
                prop_assert_eq!(
                    UncertifiedLogic.verify_demand_bound(&canonical, &signature, &claim),
                    None,
                );
                prop_assert!(!UncertifiedLogic.is_funded_bound(
                    &canonical, &signature, &claim, &claimed,
                ));
            }

            #[test]
            fn generic_finite_analysis_cannot_be_relabelled_exact(
                program in any::<u32>(),
                signature in any::<u8>(),
            ) {
                let canonical = FakeGslt
                    .canonicalize_for_funding(&program)
                    .expect("fake programs canonicalize");
                let signature = FakeSig(signature);
                let bound = ResourceMultiset::singleton(signature.key(), 1);
                prop_assert_eq!(
                    ForgedFiniteLogic.verify_demand_bound(
                        &canonical, &signature, &DemandBound::Exact(bound.clone()),
                    ),
                    None,
                );
                prop_assert!(!ForgedFiniteLogic.is_funded_bound(
                    &canonical, &signature, &DemandBound::Exact(bound.clone()), &bound,
                ));
            }
        }

        #[test]
        fn generic_unproved_zero_reservation_is_rejected() {
            let signature = FakeSig(3);
            let empty = ResourceMultiset::default();
            assert!(!UncertifiedLogic.is_funded_bound(
                &14,
                &signature,
                &DemandBound::Exact(empty.clone()),
                &empty,
            ));
        }

        #[test]
        fn native_finite_verifier_authenticates_proof_bytes() {
            let canonical = Par::default();
            let signature = Sig::Ground(b"proof-integrity".to_vec());
            let certified = DefaultResourceLogic.demand_bound(&canonical, &signature);
            let DemandBound::FiniteUpperBound { bound, proof } = certified else {
                panic!("native structural analysis provides a finite bound");
            };
            assert_eq!(
                DefaultResourceLogic.verify_demand_bound(
                    &canonical,
                    &signature,
                    &DemandBound::FiniteUpperBound {
                        bound: bound.clone(),
                        proof: proof.clone()
                    },
                ),
                Some(bound.clone()),
            );
            for forged_proof in [Vec::new(), [proof.as_slice(), &[255]].concat()] {
                assert_eq!(
                    DefaultResourceLogic.verify_demand_bound(
                        &canonical,
                        &signature,
                        &DemandBound::FiniteUpperBound {
                            bound: bound.clone(),
                            proof: forged_proof,
                        },
                    ),
                    None,
                );
            }
        }
    }

    #[test]
    fn rho_presentation_keeps_the_existing_canonical_fast_path() {
        let original = Par::default();
        assert_eq!(
            RhoGslt.canonicalize_for_funding(&original).unwrap(),
            delta_sigma::desugar_for_funding(&original),
        );
    }
}

#[cfg(test)]
#[path = "resource_logic/dependency_policy.rs"]
mod frontend_dependency_policy;

#[cfg(test)]
mod apportionment_conformance {
    //! The two `ApportionmentPolicy` contract laws — CONSERVATION (settles exactly
    //! `k` when fundable; Conservation of Authority) and NO-OVERDRAW (no pool drawn
    //! past its residual) — checked over a grid of `(k, Σ_compound, Σ_left,
    //! Σ_right)` for BOTH the built-in policy and a behaviorally-different one, so
    //! the laws are shown policy-independent (the partition is free, the total is
    //! not).
    use super::delta_sigma::SigKey;
    use super::*;

    const KC: SigKey = [0xC0; 32];
    const KL: SigKey = [0x1e; 32];
    const KR: SigKey = [0x12; 32];

    fn drawn(draws: &[PoolDraw<SigKey>], key: &SigKey) -> i64 {
        draws
            .iter()
            .filter(|d| &d.key == key)
            .map(|d| d.amount)
            .sum()
    }

    fn check_compound_laws<P: ApportionmentPolicy<RhoGslt>>(policy: &P) {
        for k in 0..=6i64 {
            for sc in 0..=6i64 {
                for sl in 0..=6i64 {
                    for sr in 0..=6i64 {
                        let shape = GroupShape::Compound {
                            combined: PoolResidual {
                                key: KC,
                                residual: sc,
                            },
                            left: PoolResidual {
                                key: KL,
                                residual: sl,
                            },
                            right: PoolResidual {
                                key: KR,
                                residual: sr,
                            },
                        };
                        let draws = policy.apportion(shape, k);
                        let dc = drawn(&draws, &KC);
                        let dl = drawn(&draws, &KL);
                        let dr = drawn(&draws, &KR);

                        // NO-OVERDRAW: each pool ≤ its residual.
                        assert!(
                            dc <= sc && dl <= sl && dr <= sr,
                            "overdraw: k={k} σ=({sc},{sl},{sr}) draws=({dc},{dl},{dr})"
                        );
                        // Matched pair: the components are debited equally (one unit
                        // of group authority per unit consumed).
                        assert_eq!(
                            dl, dr,
                            "unmatched pair: k={k} σ=({sc},{sl},{sr}) draws=({dc},{dl},{dr})"
                        );
                        // CONSERVATION: a compound draw counts once + the matched
                        // pair counts once. When the pools can fund k, settle k.
                        let settled = dc + dl; // dl == dr (matched)
                        if sc + sl.min(sr) >= k {
                            assert_eq!(
                                settled, k,
                                "not conserved: k={k} σ=({sc},{sl},{sr}) settled={settled}"
                            );
                        } else {
                            assert!(settled <= sc + sl.min(sr));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn default_apportionment_conserves_and_never_overdraws() {
        check_compound_laws(&DefaultApportionment);
    }

    #[test]
    fn built_in_single_pool_policies_cover_positive_zero_and_negative_costs() {
        let shape = || GroupShape::Single {
            own: PoolResidual {
                key: KL,
                residual: 0,
            },
        };
        for amount in [0, -1] {
            assert!(ApportionmentPolicy::<RhoGslt>::apportion(
                &DefaultApportionment,
                shape(),
                amount,
            )
            .is_empty());
            assert!(ApportionmentPolicy::<RhoGslt>::apportion(
                &FlatFeeApportionment,
                shape(),
                amount,
            )
            .is_empty());
        }
        for policy_draws in [
            ApportionmentPolicy::<RhoGslt>::apportion(&DefaultApportionment, shape(), 3),
            ApportionmentPolicy::<RhoGslt>::apportion(&FlatFeeApportionment, shape(), 3),
        ] {
            assert_eq!(policy_draws, vec![PoolDraw { key: KL, amount: 3 }]);
        }
    }

    /// Stage-D FEE policy (red-team F-1): a compound deploy's fee is FLAT — one
    /// physical token, drawn combined-first then a SINGLE component — NOT the
    /// matched-pair DOUBLING of the COST policy. Over the same grid: the right
    /// component is NEVER drawn (so the total physical draw is `k`, not `2k`),
    /// no pool overdraws, and conservation holds on every admitted shape
    /// (`sc + min(sl,sr) ≥ k ⟹ sc + sl ≥ k`, so combined+left cover `k` exactly).
    #[test]
    fn flat_fee_apportionment_is_flat_not_doubled() {
        for k in 0..=6i64 {
            for sc in 0..=6i64 {
                for sl in 0..=6i64 {
                    for sr in 0..=6i64 {
                        let shape = GroupShape::Compound {
                            combined: PoolResidual {
                                key: KC,
                                residual: sc,
                            },
                            left: PoolResidual {
                                key: KL,
                                residual: sl,
                            },
                            right: PoolResidual {
                                key: KR,
                                residual: sr,
                            },
                        };
                        let draws = ApportionmentPolicy::<RhoGslt>::apportion(
                            &FlatFeeApportionment,
                            shape,
                            k,
                        );
                        let dc = drawn(&draws, &KC);
                        let dl = drawn(&draws, &KL);
                        let dr = drawn(&draws, &KR);
                        assert!(
                            dc <= sc && dl <= sl && dr <= sr,
                            "overdraw: k={k} σ=({sc},{sl},{sr}) draws=({dc},{dl},{dr})"
                        );
                        // FLAT, NOT the matched pair: the right component is NEVER
                        // drawn ⇒ a compound's fee can never double.
                        assert_eq!(
                            dr, 0,
                            "flat fee drew the right component (doubling): k={k} σ=({sc},{sl},{sr})"
                        );
                        let settled = dc + dl + dr; // total PHYSICAL tokens == dc + dl
                        if sc + sl.min(sr) >= k {
                            assert_eq!(
                                settled, k,
                                "flat fee not conserved (or doubled): k={k} σ=({sc},{sl},{sr}) settled={settled}"
                            );
                        } else {
                            assert!(settled <= sc + sl);
                        }
                    }
                }
            }
        }
        // The exact F-1 scenario: combined pool drained, both components funded,
        // k=1. The COST policy doubles (left 1 + right 1 = 2 physical); the FLAT
        // FEE charges exactly 1 (left only).
        let f1 = GroupShape::Compound {
            combined: PoolResidual {
                key: KC,
                residual: 0,
            },
            left: PoolResidual {
                key: KL,
                residual: 5,
            },
            right: PoolResidual {
                key: KR,
                residual: 5,
            },
        };
        let cost = ApportionmentPolicy::<RhoGslt>::apportion(&DefaultApportionment, f1, 1);
        let fee = ApportionmentPolicy::<RhoGslt>::apportion(&FlatFeeApportionment, f1, 1);
        assert_eq!(
            drawn(&cost, &KL) + drawn(&cost, &KR),
            2,
            "cost should double the pair"
        );
        assert_eq!(
            drawn(&fee, &KL) + drawn(&fee, &KR),
            1,
            "flat fee must charge exactly 1"
        );
        assert_eq!(
            drawn(&fee, &KR),
            0,
            "flat fee must not touch the right component"
        );
    }

    /// A behaviorally-different policy (components-first) must satisfy the SAME
    /// contract — the laws are policy-independent (Conservation of Authority).
    struct ComponentsFirst;
    impl ApportionmentPolicy<RhoGslt> for ComponentsFirst {
        fn apportion(&self, shape: GroupShape<SigKey>, k: i64) -> Vec<PoolDraw<SigKey>> {
            match shape {
                GroupShape::Single { own } => {
                    if k > 0 {
                        vec![PoolDraw {
                            key: own.key,
                            amount: k,
                        }]
                    } else {
                        Vec::new()
                    }
                }
                GroupShape::Compound {
                    combined,
                    left,
                    right,
                } => {
                    let dp = k.min(left.residual).min(right.residual).max(0);
                    let dc = (k - dp).min(combined.residual).max(0);
                    let mut v = Vec::new();
                    if dp > 0 {
                        v.push(PoolDraw {
                            key: left.key,
                            amount: dp,
                        });
                        v.push(PoolDraw {
                            key: right.key,
                            amount: dp,
                        });
                    }
                    if dc > 0 {
                        v.push(PoolDraw {
                            key: combined.key,
                            amount: dc,
                        });
                    }
                    v
                }
            }
        }
    }

    #[test]
    fn alternative_policy_satisfies_the_same_contract() { check_compound_laws(&ComponentsFirst); }
}
