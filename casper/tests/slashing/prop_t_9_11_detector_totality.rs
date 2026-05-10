use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
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
        include_detected in any::<bool>(),
    ) {
        run_async(async move {
            let fixture = DetectorFixture::new().await;
            let detected_hash = hash(40);
            let detected = if include_detected { vec![detected_hash.clone()] } else { vec![] };
            fixture.add_record(0, 0, &detected);

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
            if include_detected {
                justifications.push(justification(fixture.validators[6].clone(), detected_hash));
            }

            let current = block(
                20,
                fixture.validators[7].clone(),
                2,
                justifications,
                fixture.validators.clone(),
            );

            let result = fixture.check(&current).await;
            if include_detected {
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
