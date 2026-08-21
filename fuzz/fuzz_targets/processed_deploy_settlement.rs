//! Fuzz the D3 (DR-9) per-signature FUNDING + SETTLEMENT invariants.
//!
//! The singular-phlo escrow refund model is REMOVED. D3's settlement is the
//! per-COMM token count debited ONCE from the per-signature supply pool Σ⟦s⟧:
//! the block-assembly gate (`delta_sigma::is_funded`) admits a deploy iff its
//! EFFECTIVE supply meets a certified finite upper bound `Δ_s`. Unprovable
//! demand is rejected, and realized settlement must never exceed reservation.
//!
//! Fuzzed invariants:
//!   * NO-UNDERFLOW: if `is_funded(Δ, Σ)` then `Σ − Δ ≥ 0`.
//!   * MONOTONICITY: raising the supply can only keep a funded deploy funded;
//!     raising the demand can only keep an unfunded deploy unfunded.
//!   * REJECT-DIRECTION: `Σ < Δ` and every unprovable demand are rejected.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rholang::rust::interpreter::accounting::delta_sigma::{is_funded, DemandEntry};

#[derive(Arbitrary, Debug)]
struct Input {
    /// `Δ_s` certified upper bound. Bounded to a sane range
    /// so the i128 funding comparison stays in-domain while still exercising
    /// the boundary arithmetic.
    demand: i64,
    /// Whether the demand lacks a finite proof (Thm 20 `unknown` flag).
    unknown: bool,
    /// `Σ_s` effective supply (a balance).
    supply: i64,
}

fuzz_target!(|input: Input| {
    let analysis = DemandEntry {
        certified_upper_bound: input.demand,
        unknown: input.unknown,
    };
    let supply = input.supply;

    let funded = is_funded(&analysis, supply);

    // NO-UNDERFLOW: a funded deploy leaves a
    // non-negative residual after the settlement debit (= the COMM demand).
    // Computed in i128 to mirror the gate and avoid wrap.
    if funded {
        let residual = i128::from(supply) - i128::from(analysis.certified_upper_bound);
        assert!(residual >= 0, "funded ⇒ settlement debit never underflows the pool");
        assert!(!analysis.unknown);
    }

    if analysis.unknown || i128::from(supply) < i128::from(analysis.certified_upper_bound) {
        assert!(!funded, "Σ below the regime threshold must be rejected by the gate");
    }

    // MONOTONICITY in supply: more supply cannot un-fund a funded deploy.
    if funded {
        if let Some(more) = supply.checked_add(1) {
            assert!(
                is_funded(&analysis, more),
                "raising the supply must keep a funded deploy funded"
            );
        }
    }

    // MONOTONICITY in demand: more demand cannot fund an unfunded deploy.
    if !funded {
        if let Some(more_demand) = analysis.certified_upper_bound.checked_add(1) {
            let harder = DemandEntry {
                certified_upper_bound: more_demand,
                unknown: analysis.unknown,
            };
            assert!(
                !is_funded(&harder, supply),
                "raising the demand must keep an unfunded deploy unfunded"
            );
        }
    }
});
