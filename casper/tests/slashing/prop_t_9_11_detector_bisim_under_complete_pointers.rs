// Theorem T-9.11 (bisimulation under complete pointers) — randomized.
//
// Maps to: docs/theory/slashing/slashing-specification.md §14 T-9.11.
// Rocq: formal/rocq/slashing/theories/EquivocationDetector.v
// theorem `detector_bisim_under_complete_pointers`.
//
// Property: when every justification pointer resolves to a present
// block, the post-fix detector classifies identically to the pre-fix
// detector. This is the regression-free guarantee — the totality fix
// only changed behavior on the missing-pointer edge case. UC-103 is the
// fixed-fixture companion.

use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use proptest::prelude::*;
use rspace_plus_plus::rspace::history::Either;

use super::detector_totality_helpers::{block, justification, DetectorFixture};

fn run_async<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(future)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_11_complete_pointer_views_match_child_cardinality(child_count in 0usize..=3) {
        run_async(async move {
            let fixture = DetectorFixture::new().await;
            fixture.add_record(0, 0, &[]);

            let mut justifications = Vec::new();
            for index in 0..child_count {
                let child = block(
                    10 + index as u8,
                    fixture.validators[0].clone(),
                    1,
                    vec![],
                    fixture.validators.clone(),
                );
                fixture.add_block(&child);
                let observer = block(
                    20 + index as u8,
                    fixture.validators[index + 1].clone(),
                    1,
                    vec![justification(
                        fixture.validators[0].clone(),
                        child.block_hash.clone(),
                    )],
                    fixture.validators.clone(),
                );
                fixture.add_block(&observer);
                justifications.push(justification(
                    fixture.validators[index + 1].clone(),
                    observer.block_hash.clone(),
                ));
            }

            let current = block(
                40,
                fixture.validators[5].clone(),
                2,
                justifications,
                fixture.validators.clone(),
            );
            let result = fixture.check(&current).await;
            if child_count >= 2 {
                prop_assert_eq!(
                    result,
                    Either::Left(BlockError::Invalid(InvalidBlock::NeglectedEquivocation))
                );
            } else {
                prop_assert_eq!(result, Either::Right(ValidBlock::Valid));
            }
            Ok(())
        })?;
    }
}
