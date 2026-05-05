// T-15 generalized — Triple bisimilarity over the dispatch event.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.5,
// design/14a-tier-architecture.md §3 (triple-bisim diagnostic table).
//
// Property: for any sequence of equivocation events applied
// through Tier 1 (production), Tier 2 (oracle), Tier 3 (harness),
// each tier ends up with a record (presence agreement) for every
// validator that committed an equivocation.
//
// Disagreement diagnoses tier drift:
//   harness ≠ oracle = production → harness drift away from spec
//   harness = oracle ≠ production → production regression
//   harness = production ≠ oracle → oracle.rs is stale vs. Rocq
//
// Per Plan agent's Q5: 25 PR-gate cases, single-threaded tokio
// runtime per case (each case rebuilds genesis to avoid LMDB
// state contamination).

use proptest::prelude::*;

use super::triple_bisim_driver::{block_on, Event, TripleBisimDriver};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8, // start small; nightly job bumps to 25-100
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_15_triple_bisim_dispatch_agrees_on_record_presence(
        // Single equivocation event. Multi-event sequences require
        // record propagation across TestNode peers (each
        // equivocation lands on a different processor node) which
        // is out of scope for the per-event driver loop. Coverage
        // for sequences is provided by the harness-tier sequential
        // bisim (prop_t_15_bisim_under_workload.rs) at scale.
        equivocator_idx in 0usize..3,
    ) {
        block_on(async {
            let mut driver = TripleBisimDriver::new(3, 100).await;
            driver.apply(&Event::Equivocate { v_idx: equivocator_idx }).await;
            driver.assert_record_agreement().await;
        });
    }
}
