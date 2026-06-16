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

use super::delta_sigma::{self, DemandEntry};
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

    fn canonicalize_for_funding(&self, program: &Self::Program) -> Self::CanonicalProgram;
}

pub trait OslfResourceLogic<G: GsltPresentation> {
    fn demand(&self, canonical: &G::CanonicalProgram, deploy_sig: &G::Signature) -> DemandEntry;

    /// The funding judgment / proof check (Def 19, Thm 20): the demand is funded
    /// iff the effective supply `effective_supply_s` (`Σ_s`) meets or exceeds the
    /// demand's known lower bound plus the shard `margin`. This is the decidable
    /// `funds Σ Δ := Δ ≤ Σ` of the OSLF resource logic.
    fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64, margin: i64) -> bool;
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
    fn canonicalize_for_funding(&self, program: &Par) -> Par {
        delta_sigma::desugar_for_funding(program)
    }
}

impl ResourceSignature for Sig {
    type Key = delta_sigma::SigKey;

    #[inline]
    fn key(&self) -> Self::Key { self.lane_hash() }

    fn split_join_decompositions(&self, out: &mut Vec<ResourceDecomposition<Self::Key>>) {
        if let Sig::And(left, right) = self {
            out.push(ResourceDecomposition {
                compound: self.key(),
                left: left.key(),
                right: right.key(),
            });
            left.split_join_decompositions(out);
            right.split_join_decompositions(out);
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

    #[inline]
    fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64, margin: i64) -> bool {
        delta_sigma::is_funded(analysis, effective_supply_s, margin)
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
                    out.push(PoolDraw { key: own.key, amount: k });
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
/// deploys up to 2× (red-team F-1); this policy fixes that while staying conserving
/// (the carved total still equals the sum of client debits == `F_v` credit).
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
                    out.push(PoolDraw { key: own.key, amount: k });
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

    fn resolvable(lower: i64) -> DemandEntry {
        DemandEntry {
            known_lower_bound: lower,
            unknown: false,
        }
    }

    fn unresolvable(lower: i64) -> DemandEntry {
        DemandEntry {
            known_lower_bound: lower,
            unknown: true,
        }
    }

    /// Law (sound proof-checker, BOTH regimes — F-B): for RESOLVABLE demand the
    /// funds judgment is EXACTLY Def 19 `Σ ≥ Δ` (the economic margin is inert — it
    /// is not folded into the resolvable-demand correctness gate); for
    /// OVER-APPROXIMATED (`unknown`) demand the Thm 20 safety margin applies,
    /// `Σ ≥ Δ + margin`.
    fn law_sound<G, R>(rl: &R)
    where
        G: GsltPresentation,
        R: OslfResourceLogic<G>,
    {
        for &lower in &[0i64, 1, 5, 100] {
            for &supply in &[0i64, 1, 5, 100, 101] {
                for &margin in &[0i64, 1, 10] {
                    // Resolvable: Def 19 `Σ ≥ Δ` — margin NOT applied.
                    let resolved = rl.is_funded(&resolvable(lower), supply, margin);
                    assert_eq!(
                        resolved,
                        i128::from(supply) >= i128::from(lower),
                        "resolvable funds judgment must be Σ ≥ Δ (lower={lower}, supply={supply}, margin={margin})"
                    );
                    // Over-approximated: Thm 20 `Σ ≥ Δ + margin`.
                    let over = rl.is_funded(&unresolvable(lower), supply, margin);
                    assert_eq!(
                        over,
                        i128::from(supply) >= i128::from(lower) + i128::from(margin),
                        "unknown funds judgment must be Σ ≥ Δ + margin (lower={lower}, supply={supply}, margin={margin})"
                    );
                }
            }
        }
    }

    /// Law (reject underfunded): a positive demand against zero supply (an absent
    /// pool) at zero margin is rejected — the Rust mirror of
    /// `strict_reject_when_underfunded`.
    fn law_reject_underfunded<G, R>(rl: &R)
    where
        G: GsltPresentation,
        R: OslfResourceLogic<G>,
    {
        assert!(!rl.is_funded(&resolvable(1), 0, 0));
        assert!(!rl.is_funded(&resolvable(7), 0, 0));
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
    fn law_decidable<G, R>(rl: &R)
    where
        G: GsltPresentation,
        R: OslfResourceLogic<G>,
    {
        let _verdict: bool = rl.is_funded(&resolvable(3), 5, 0);
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

        fn canonicalize_for_funding(&self, program: &Self::Program) -> Self::CanonicalProgram {
            u64::from(*program) * 2
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
                known_lower_bound: (*canonical as i64) + i64::from(deploy_sig.key()),
                unknown: false,
            }
        }

        fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64, margin: i64) -> bool {
            // Two-regime (F-B): the margin applies only to over-approximated demand.
            let applied_margin = if analysis.unknown { margin } else { 0 };
            i128::from(effective_supply_s)
                >= i128::from(analysis.known_lower_bound) + i128::from(applied_margin)
        }
    }

    #[test]
    fn oslf_resource_logic_is_generic_over_gslt_presentations() {
        let gslt = FakeGslt;
        let logic = FakeLogic;
        let canonical = gslt.canonicalize_for_funding(&7);
        let sig = FakeSig(3);
        let demand = logic.demand(&canonical, &sig);
        let mut decompositions = Vec::new();
        sig.split_join_decompositions(&mut decompositions);
        assert_eq!(canonical, 14);
        assert_eq!(demand.known_lower_bound, 17);
        assert!(logic.is_funded(&demand, 17, 0));
        assert!(!logic.is_funded(&demand, 16, 0));
        assert_eq!(decompositions, vec![ResourceDecomposition {
            compound: 3,
            left: 2,
            right: 1
        }]);
    }

    #[test]
    fn mettail_rust_is_not_a_cargo_dependency() {
        use std::path::{Path, PathBuf};

        fn collect_manifests(dir: &Path, out: &mut Vec<PathBuf>) {
            let entries = std::fs::read_dir(dir).expect("read workspace directory");
            for entry in entries {
                let entry = entry.expect("read directory entry");
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if path.is_dir() {
                    if name == ".git" || name == "target" {
                        continue;
                    }
                    collect_manifests(&path, out);
                } else if name == "Cargo.toml" {
                    out.push(path);
                }
            }
        }

        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        let mut manifests = Vec::new();
        collect_manifests(workspace, &mut manifests);

        for manifest in manifests {
            let content = std::fs::read_to_string(&manifest).expect("read Cargo.toml");
            assert!(
                !content.contains("mettail-rust") && !content.contains("mettail_rust"),
                "MeTTaIL must remain an adapter over GSLT/OSLF, not a workspace Cargo dependency: {}",
                manifest.display()
            );
        }
    }
}

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
        draws.iter().filter(|d| &d.key == key).map(|d| d.amount).sum()
    }

    fn check_compound_laws<P: ApportionmentPolicy<RhoGslt>>(policy: &P) {
        for k in 0..=6i64 {
            for sc in 0..=6i64 {
                for sl in 0..=6i64 {
                    for sr in 0..=6i64 {
                        let shape = GroupShape::Compound {
                            combined: PoolResidual { key: KC, residual: sc },
                            left: PoolResidual { key: KL, residual: sl },
                            right: PoolResidual { key: KR, residual: sr },
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
                            combined: PoolResidual { key: KC, residual: sc },
                            left: PoolResidual { key: KL, residual: sl },
                            right: PoolResidual { key: KR, residual: sr },
                        };
                        let draws =
                            ApportionmentPolicy::<RhoGslt>::apportion(&FlatFeeApportionment, shape, k);
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
            combined: PoolResidual { key: KC, residual: 0 },
            left: PoolResidual { key: KL, residual: 5 },
            right: PoolResidual { key: KR, residual: 5 },
        };
        let cost = ApportionmentPolicy::<RhoGslt>::apportion(&DefaultApportionment, f1, 1);
        let fee = ApportionmentPolicy::<RhoGslt>::apportion(&FlatFeeApportionment, f1, 1);
        assert_eq!(drawn(&cost, &KL) + drawn(&cost, &KR), 2, "cost should double the pair");
        assert_eq!(drawn(&fee, &KL) + drawn(&fee, &KR), 1, "flat fee must charge exactly 1");
        assert_eq!(drawn(&fee, &KR), 0, "flat fee must not touch the right component");
    }

    /// A behaviorally-different policy (components-first) must satisfy the SAME
    /// contract — the laws are policy-independent (Conservation of Authority).
    struct ComponentsFirst;
    impl ApportionmentPolicy<RhoGslt> for ComponentsFirst {
        fn apportion(&self, shape: GroupShape<SigKey>, k: i64) -> Vec<PoolDraw<SigKey>> {
            match shape {
                GroupShape::Single { own } => {
                    if k > 0 {
                        vec![PoolDraw { key: own.key, amount: k }]
                    } else {
                        Vec::new()
                    }
                }
                GroupShape::Compound { combined, left, right } => {
                    let dp = k.min(left.residual).min(right.residual).max(0);
                    let dc = (k - dp).min(combined.residual).max(0);
                    let mut v = Vec::new();
                    if dp > 0 {
                        v.push(PoolDraw { key: left.key, amount: dp });
                        v.push(PoolDraw { key: right.key, amount: dp });
                    }
                    if dc > 0 {
                        v.push(PoolDraw { key: combined.key, amount: dc });
                    }
                    v
                }
            }
        }
    }

    #[test]
    fn alternative_policy_satisfies_the_same_contract() {
        check_compound_laws(&ComponentsFirst);
    }
}
