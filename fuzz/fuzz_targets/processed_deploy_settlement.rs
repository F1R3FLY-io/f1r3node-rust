//! Fuzz the D3 (DR-9) per-signature FUNDING + SETTLEMENT invariants.
//!
//! The singular-phlo escrow refund model is REMOVED. D3's settlement is the
//! per-COMM token count debited ONCE from the per-signature supply pool Σ⟦s⟧:
//! the block-assembly gate (`delta_sigma::is_funded`) admits a deploy iff its
//! EFFECTIVE supply meets the demand `Δ_s` (the COMM count) plus the genesis
//! safety margin, and the settlement write `post = pre − Δ_s` must never
//! underflow for an admitted deploy.
//!
//! Fuzzed invariants:
//!   * NO-UNDERFLOW: if `is_funded(Δ, Σ, margin)` then `Σ − Δ ≥ margin ≥ 0`, so
//!     the settlement debit (= Δ, the COMM count) leaves a non-negative pool.
//!   * MONOTONICITY: raising the supply can only keep a funded deploy funded;
//!     raising the demand can only keep an unfunded deploy unfunded.
//!   * REJECT-DIRECTION: a deploy with `Σ < Δ + margin` is NOT funded.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rholang::rust::interpreter::accounting::delta_sigma::{is_funded, DemandEntry};

#[derive(Arbitrary, Debug)]
struct Input {
    /// `Δ_s` known lower bound (the per-COMM demand). Bounded to a sane range
    /// so the i128 funding comparison stays in-domain while still exercising
    /// the boundary arithmetic.
    demand: i64,
    /// Whether the demand is an over-approximation (Thm 20 `unknown` flag).
    unknown: bool,
    /// `Σ_s` effective supply (a balance).
    supply: i64,
    /// The genesis safety margin (`min_phlo_price`). Non-negative in practice;
    /// fuzzed across i64 to defend the comparison.
    margin: i64,
}

fuzz_target!(|input: Input| {
    let analysis = DemandEntry {
        known_lower_bound: input.demand,
        unknown: input.unknown,
    };
    let margin = input.margin;
    let supply = input.supply;

    let funded = is_funded(&analysis, supply, margin);

    // NO-UNDERFLOW: a funded deploy with a non-negative margin leaves a
    // non-negative residual after the settlement debit (= the COMM demand).
    // Computed in i128 to mirror the gate and avoid wrap.
    if funded && margin >= 0 {
        let residual = i128::from(supply) - i128::from(analysis.known_lower_bound);
        assert!(
            residual >= i128::from(margin),
            "funded ⇒ Σ − Δ ({residual}) ≥ margin ({margin}); settlement underflowed"
        );
        assert!(residual >= 0, "funded ⇒ settlement debit never underflows the pool");
    }

    // REJECT-DIRECTION: Σ strictly below Δ + margin must NOT be funded.
    let required = i128::from(analysis.known_lower_bound) + i128::from(margin);
    if i128::from(supply) < required {
        assert!(!funded, "Σ < Δ + margin must be rejected by the gate");
    }

    // MONOTONICITY in supply: more supply cannot un-fund a funded deploy.
    if funded {
        if let Some(more) = supply.checked_add(1) {
            assert!(
                is_funded(&analysis, more, margin),
                "raising the supply must keep a funded deploy funded"
            );
        }
    }

    // MONOTONICITY in demand: more demand cannot fund an unfunded deploy.
    if !funded {
        if let Some(more_demand) = analysis.known_lower_bound.checked_add(1) {
            let harder = DemandEntry {
                known_lower_bound: more_demand,
                unknown: analysis.unknown,
            };
            assert!(
                !is_funded(&harder, supply, margin),
                "raising the demand must keep an unfunded deploy unfunded"
            );
        }
    }
});
