// Three-tier agreement on the records component.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.5,
// design/14a-tier-architecture.md §3.
//
// Property: each equivocation event produces a record at every
// tier (production, oracle, harness). This pins per-validator record
// presence across the tiers and fails fast on tracker-shape drift.

use proptest::prelude::*;

use super::observer::SlashingObserver;
use super::triple_bisim_driver::{block_on, Event, TripleBisimDriver};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 5,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_13b_triple_bisim_records(
        equivocator_idx in 0usize..3,
    ) {
        block_on(async {
            let mut driver = TripleBisimDriver::new(3, 100).await;
            driver.apply(&Event::Equivocate { v_idx: equivocator_idx }).await;

            // After exactly one equivocation by v_idx, all three
            // tiers should report SOME record for that validator.
            let label = format!("v{}", equivocator_idx);
            let production = driver.production_snapshot().await;

            let h_any = (0..=5)
                .any(|b| <_ as SlashingObserver>::has_record(&driver.harness, &label, b));
            let o_any = (0..=5)
                .any(|b| <_ as SlashingObserver>::has_record(&driver.oracle, &label, b));
            let p_any = (0..=10)
                .any(|b| <_ as SlashingObserver>::has_record(&production, &label, b));
            assert!(h_any, "harness must record equivocator {}", label);
            assert!(o_any, "oracle must record equivocator {}", label);
            assert!(p_any, "production must record equivocator {} (post-fix #1+#3)", label);
        });
    }
}
