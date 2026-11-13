// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Theorem T-9.11 (totality) — randomized.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §14 T-9.11.
// Rocq: formal/rocq/slashing/theories/EquivocationDetector.v
// theorem `detector_total`.
//
// Property: for any well-formed input DAG, `check(...)` returns Ok. It
// never returns Err, never panics, and with at most one child of the held
// equivocation visible it judges the block Valid. Proptest randomizes the
// justification list and the missing-pointer pattern. This is the
// *property-test* companion to the UC-101..UC-108 fixed regression suite.

use casper::rust::block_status::ValidBlock;
use proptest::prelude::*;
use rspace_plus_plus::rspace::history::Either;

use super::detector_totality_helpers::{block, hash, justification, DetectorFixture};

fn run_async<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(future)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 48,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_11_detector_is_total_for_missing_views(
        missing_count in 0usize..4,
        include_one_child in any::<bool>(),
    ) {
        run_async(async move {
            let fixture = DetectorFixture::new().await;
            fixture.add_equivocation(0, 0);

            let child = block(
                10,
                fixture.validators[0].clone(),
                1,
                vec![],
                fixture.validators.clone(),
            );
            if include_one_child {
                fixture.add_block(&child);
            }

            let mut justifications = Vec::new();
            for index in 0..missing_count {
                justifications.push(justification(
                    fixture.validators[index + 1].clone(),
                    hash(100 + index as u8),
                ));
            }
            if include_one_child {
                justifications.push(justification(
                    fixture.validators[5].clone(),
                    child.block_hash.clone(),
                ));
            }
            let current = block(
                20,
                fixture.validators[7].clone(),
                2,
                justifications,
                fixture.validators.clone(),
            );

            prop_assert_eq!(fixture.check(&current).await, Either::Right(ValidBlock::Valid));
            Ok(())
        })?;
    }
}
