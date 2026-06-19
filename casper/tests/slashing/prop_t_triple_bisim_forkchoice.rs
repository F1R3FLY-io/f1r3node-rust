// T-13c generalized — Triple bisimilarity on the fork-choice
// projection.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.5,
// design/14a-tier-architecture.md §3.
//
// Property: an equivocation followed by no slash should leave
// the fork-choice active set unchanged at every tier (the
// equivocator is still bonded; the slash transition requires a
// downstream SlashDeploy that this proptest does not exercise).
//
// This proptest pins the *negative* form of T-13c: equivocation
// alone is insufficient to remove a validator from fork-choice;
// the active set is only modified by a slash transition.

use proptest::prelude::*;

use super::observer::SlashingObserver;
use super::triple_bisim_driver::{block_on, Event, TripleBisimDriver};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 5,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_13c_triple_bisim_forkchoice_stable_under_equivocation(
        equivocator_idx in 0usize..3,
    ) {
        block_on(async {
            let mut driver = TripleBisimDriver::new(3, 100).await;
            driver.apply(&Event::Equivocate { v_idx: equivocator_idx }).await;

            let production = driver.production_snapshot().await;

            // All three tiers' fork-choice should still contain
            // every validator (no slash has fired).
            let h_fc = <_ as SlashingObserver>::fork_choice(&driver.harness);
            let o_fc = <_ as SlashingObserver>::fork_choice(&driver.oracle);
            let p_fc = <_ as SlashingObserver>::fork_choice(&production);

            assert_eq!(h_fc.len(), 3, "harness fork_choice should retain all 3 validators");
            assert_eq!(o_fc.len(), 3, "oracle fork_choice should retain all 3 validators");
            assert_eq!(p_fc.len(), 3, "production fork_choice should retain all 3 validators");
            assert_eq!(h_fc, o_fc, "harness↔oracle fork_choice agreement");
            assert_eq!(h_fc, p_fc, "harness↔production fork_choice agreement");
        });
    }
}
