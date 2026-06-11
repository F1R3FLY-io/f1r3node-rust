//! Local mirror of f1r3node's OSLF/GSLT **funding port**
//! (`accounting::resource_logic::OslfResourceLogic`) — Anti-Corruption Layer +
//! Dependency Inversion, with **no hard dependency** on the native crate. The
//! shim's signature algebra ([`Sig`]) is checked against the *same* conformance
//! laws the native resource logic must satisfy, so the funding/signature port
//! is shared across the swap (the lowering is retired; this algebra is reused).
//!
//! The judgment is the decidable `funds Σ Δ := Δ ≤ Σ`: a supply multiset
//! (tokens held, keyed by [`Sig::key`]) funds a demand multiset (tokens needed,
//! one per signed term) iff it dominates it per signature. A *compound* demand
//! `s1*…*sn` may be met either by a combined-cell token (its own key) or by its
//! component tokens ([`Sig::split_join_decompositions`]) — the R2/R3 duality.

use std::collections::BTreeMap;

use super::ir::{ResourceSignature, Sig};

/// A multiset of signatures keyed by content hash (token counts).
pub type Multiset = BTreeMap<[u8; 32], u64>;

/// The funding judgment, mirroring native `OslfResourceLogic`.
pub trait ResourceLogic {
    /// `funds Σ Δ := Δ ≤ Σ` — decidable, total.
    fn is_funded(&self, supply: &Multiset, demand: &[Sig]) -> bool;
}

/// The Rholang instance of the funding judgment.
pub struct RhoResourceLogic;

impl ResourceLogic for RhoResourceLogic {
    fn is_funded(&self, supply: &Multiset, demand: &[Sig]) -> bool {
        // Greedily consume one token per demanded signature from a working copy
        // of the supply; a compound falls back to consuming its components.
        let mut pool = supply.clone();
        demand.iter().all(|sig| consume(&mut pool, sig))
    }
}

/// Consume one token funding `sig` from `pool` (mutating it). Prefers a token
/// on `sig` itself (combined-cell / atom); a compound falls back to consuming
/// one token per component (split form). Returns whether funding succeeded.
fn consume(pool: &mut Multiset, sig: &Sig) -> bool {
    let key = sig.key();
    if let Some(count) = pool.get_mut(&key) {
        if *count > 0 {
            *count -= 1;
            return true;
        }
    }
    match sig.split_join_decompositions().as_slice() {
        [] => false,
        components => components.iter().all(|component| consume(pool, component)),
    }
}

/// Build a supply multiset from the tokens a purse stack holds.
pub fn supply_of(tokens: &[Sig]) -> Multiset {
    let mut supply = Multiset::new();
    for sig in tokens {
        *supply.entry(sig.key()).or_insert(0) += 1;
    }
    supply
}

// ───────────────────────────── conformance laws ─────────────────────────────
//
// The OSLF contract any resource logic must satisfy. Reusable so the same laws
// can be run against the native impl and a future MeTTaIL adapter.

/// `law_sound`: an exactly-matching supply funds its demand.
pub fn law_sound(rl: &impl ResourceLogic, demand: &[Sig]) -> bool {
    rl.is_funded(&supply_of(demand), demand)
}

/// `law_reject_underfunded`: dropping any one token from an exact supply makes
/// a non-empty demand unfundable (no fuel is conjured).
pub fn law_reject_underfunded(rl: &impl ResourceLogic, demand: &[Sig]) -> bool {
    if demand.is_empty() {
        return true;
    }
    let mut supply = supply_of(demand);
    // Remove one token of the first demanded (atomized) signature.
    let key = atomic_key(&demand[0]);
    match supply.get_mut(&key) {
        Some(count) if *count > 0 => *count -= 1,
        _ => return true, // already absent (compound-only); vacuously holds
    }
    !rl.is_funded(&supply, demand)
}

/// `law_supply_monotone` (no contraction): extra supply never revokes funding.
pub fn law_supply_monotone(
    rl: &impl ResourceLogic,
    supply: &Multiset,
    demand: &[Sig],
    extra: &Sig,
) -> bool {
    if !rl.is_funded(supply, demand) {
        return true; // premise false
    }
    let mut bigger = supply.clone();
    *bigger.entry(extra.key()).or_insert(0) += 1;
    rl.is_funded(&bigger, demand)
}

/// `law_decidable`: the judgment terminates and is a pure function (idempotent).
pub fn law_decidable(rl: &impl ResourceLogic, supply: &Multiset, demand: &[Sig]) -> bool {
    rl.is_funded(supply, demand) == rl.is_funded(supply, demand)
}

/// The key of the first atom reachable from `sig` (for the underfunding probe).
fn atomic_key(sig: &Sig) -> [u8; 32] {
    match sig.split_join_decompositions().first() {
        Some(component) => atomic_key(component),
        None => sig.key(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ground(name: &[u8]) -> Sig { Sig::Ground(name.to_vec()) }

    fn samples() -> Vec<Vec<Sig>> {
        let (a, b, c) = (ground(b"a"), ground(b"b"), ground(b"c"));
        vec![
            vec![],
            vec![a.clone()],
            vec![a.clone(), a.clone()],
            vec![a.clone(), b.clone()],
            vec![Sig::compound(vec![a.clone(), b.clone()])],
            vec![Sig::compound(vec![a.clone(), b.clone()]), c.clone()],
            vec![Sig::Quote(b"#P".to_vec()), a],
        ]
    }

    #[test]
    fn oslf_laws_hold_over_the_shim_algebra() {
        let rl = RhoResourceLogic;
        for demand in samples() {
            assert!(law_sound(&rl, &demand), "law_sound: {demand:?}");
            assert!(
                law_reject_underfunded(&rl, &demand),
                "law_reject_underfunded: {demand:?}"
            );
            assert!(
                law_decidable(&rl, &supply_of(&demand), &demand),
                "law_decidable: {demand:?}"
            );
            for extra in [ground(b"a"), ground(b"z")] {
                assert!(
                    law_supply_monotone(&rl, &supply_of(&demand), &demand, &extra),
                    "law_supply_monotone: {demand:?} + {extra:?}"
                );
            }
        }
    }

    #[test]
    fn compound_demand_funded_by_components_or_combined() {
        let rl = RhoResourceLogic;
        let (a, b) = (ground(b"a"), ground(b"b"));
        let compound = Sig::compound(vec![a.clone(), b.clone()]);
        // Funded by component tokens (R2/R4 split form).
        assert!(rl.is_funded(&supply_of(&[a.clone(), b.clone()]), &[compound.clone()]));
        // Funded by a combined-cell token (R3/R5).
        assert!(rl.is_funded(&supply_of(&[compound.clone()]), &[compound.clone()]));
        // Not funded by only one component.
        assert!(!rl.is_funded(&supply_of(&[a]), &[compound]));
    }
}
