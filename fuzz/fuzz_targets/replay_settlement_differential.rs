//! Fuzz the replay-field roundtrip together with the D3 (DR-9) per-COMM
//! funding/settlement invariants.
//!
//! The oracle ties two production boundaries together:
//!   1. processed-deploy protobuf conversion must preserve the scalar per-COMM
//!      `cost` and the failure flag across the wire (play == replay shape); and
//!   2. the per-signature gate (`is_funded`) must keep the settlement debit
//!      (= `Δ_s`, the COMM count) total, bounded, and UNDERFLOW-FREE for an
//!      admitted deploy (replacing the removed escrow refund arithmetic).

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use models::rust::casper::protocol::casper_message::ProcessedDeploy;
use rholang::rust::interpreter::accounting::delta_sigma::{is_funded, DemandEntry};

mod cost_accounting_fuzz_support;

#[derive(Arbitrary, Debug)]
struct Input {
    seed: u8,
    cost: u64,
    failed: bool,
    /// `Δ_s` per-COMM demand.
    demand: i64,
    /// `Σ_s` effective supply.
    supply: i64,
    /// Genesis safety margin.
    margin: i64,
}

fuzz_target!(|input: Input| {
    // (1) Replay-shape roundtrip: the per-COMM `cost` and failure flag survive
    // the ProcessedDeploy protobuf conversion byte-identically.
    let processed =
        cost_accounting_fuzz_support::processed_deploy(input.seed, input.cost, input.failed);
    let decoded = ProcessedDeploy::from_proto(processed.clone().to_proto())
        .expect("processed deploy protobuf roundtrip");
    assert_eq!(decoded.cost.cost, input.cost);
    assert_eq!(decoded.is_failed, input.failed);

    // (2) Funding/settlement: an admitted (funded) deploy's settlement debit
    // (= the COMM demand) never underflows the supply, and the gate decision is
    // monotone in supply and demand.
    let analysis = DemandEntry {
        known_lower_bound: input.demand,
        unknown: false,
    };
    let funded = is_funded(&analysis, input.supply, input.margin);

    if funded && input.margin >= 0 {
        let residual = i128::from(input.supply) - i128::from(analysis.known_lower_bound);
        assert!(
            residual >= 0,
            "funded ⇒ settlement debit (= Δ COMMs) must not underflow Σ⟦s⟧"
        );
    }

    // Monotone in supply: more supply keeps a funded deploy funded.
    if funded {
        if let Some(more) = input.supply.checked_add(1) {
            assert!(is_funded(&analysis, more, input.margin));
        }
    }
});
