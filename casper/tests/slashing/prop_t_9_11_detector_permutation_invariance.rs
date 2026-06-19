// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Theorem T-9.11 (permutation invariance) — randomized.
//
// Maps to: docs/theory/slashing/slashing-specification.md §14 T-9.11.
// Rocq: formal/rocq/slashing/theories/EquivocationDetector.v
// theorem `detector_permutation_invariant`.
//
// Property: for any well-formed DAG, the detector's classification is
// invariant under permutation of the input block's justification list.
// Proptest randomizes orderings; the test compares the original vs.
// permuted classification. The UC-102 fixture covers a hand-picked
// neglect case; this property test sweeps the broader space.

use proptest::prelude::*;

use super::detector_totality_helpers::{assert_neglected, block, justification, DetectorFixture};

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
    fn t_9_11_detector_permutation_invariance(order in prop::sample::select(vec![
        vec![0usize, 1, 2],
        vec![0usize, 2, 1],
        vec![1usize, 0, 2],
        vec![1usize, 2, 0],
        vec![2usize, 0, 1],
        vec![2usize, 1, 0],
    ])) {
        run_async(async move {
            let fixture = DetectorFixture::new().await;
            fixture.add_record(0, 0, &[]);

            let child_a = block(
                10,
                fixture.validators[0].clone(),
                1,
                vec![],
                fixture.validators.clone(),
            );
            let child_b = block(
                11,
                fixture.validators[0].clone(),
                1,
                vec![],
                fixture.validators.clone(),
            );
            let missing_pointer = block(
                12,
                fixture.validators[3].clone(),
                1,
                vec![justification(
                    fixture.validators[3].clone(),
                    fixture.genesis.block_hash.clone(),
                )],
                fixture.validators.clone(),
            );
            fixture.add_block(&child_a);
            fixture.add_block(&child_b);
            fixture.add_block(&missing_pointer);

            let entries = vec![
                justification(fixture.validators[1].clone(), child_a.block_hash.clone()),
                justification(fixture.validators[2].clone(), child_b.block_hash.clone()),
                justification(fixture.validators[3].clone(), missing_pointer.block_hash.clone()),
            ];
            let current = block(
                20,
                fixture.validators[4].clone(),
                2,
                order.into_iter().map(|index| entries[index].clone()).collect(),
                fixture.validators.clone(),
            );

            assert_neglected(fixture.check(&current).await);
        });
    }
}
