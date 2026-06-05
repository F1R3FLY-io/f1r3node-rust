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
    fn law_sound<G, R>(rl: &R)
    where
        G: GsltPresentation,
        R: OslfResourceLogic<G>,
    {
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
            i128::from(effective_supply_s)
                >= i128::from(analysis.known_lower_bound) + i128::from(margin)
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
