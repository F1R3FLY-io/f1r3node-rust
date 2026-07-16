// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// FV audit #6 — unbonded-window record pollution fork: post-fix determinism
// property (randomized interleaving).
//
// Maps to: docs/theory/slashing/design/12-failure-modes.md §12.2.1a.
// Rocq:  formal/rocq/slashing/theories/EquivocationDetector.v
//        (`unbonded_offender_oblivious`, `unbonded_stamp_noop`,
//         `unbonded_witness_order_independent`).
// TLA+:  formal/tlaplus/slashing/EquivocationDetector.tla
//        (`Inv_NoStampAgainstUnbonded`, `Inv_NeglectNotFromUnbondedPollution`).
//
// Property. For ANY schedule of {record creation, per-block bond toggles, block
// validations} over an HONEST offender (single self-chain, never equivocated):
//   (1) the offender's EquivocationRecord witness set
//       (`equivocation_detected_block_hashes`) stays EMPTY throughout, and
//   (2) no validated block is ever rejected `NeglectedEquivocation` on account
//       of that offender.
//
// Post-fix, the unbonded/stake-0 offender resolves to `EquivocationOblivious`,
// so the caller's stamping arm is unreachable and the witness set is never
// polluted — hence detectability is observation-order-independent and no honest
// block is falsely neglected. Pre-fix, an unbonded observation stamped the
// observer's block hash into the record, so assertion (1) would fail (the
// witness would become non-empty), reproducing the pollution source.

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
        cases: 48,
        .. ProptestConfig::default()
    })]

    // schedule: a sequence of (bond_offender, cite_tip) steps.
    //   bond_offender = true  → the validating block bonds the offender (stake > 0)
    //   bond_offender = false → the validating block leaves the offender UNBONDED
    //   cite_tip      = true  → the block cites the offender's honest tip b1
    //   cite_tip      = false → the block cites genesis instead
    #[test]
    fn unbonded_window_never_pollutes_or_falsely_neglects(
        schedule in prop::collection::vec((any::<bool>(), any::<bool>()), 1..=6)
    ) {
        run_async(async move {
            let fixture = DetectorFixture::new().await;

            // Offender V = validators[1], HONEST single chain b0 (seq 0) → b1 (seq 1).
            let offender = fixture.validators[1].clone();
            let observer = fixture.validators[2].clone();
            let b0 = block(10, offender.clone(), 0, vec![], fixture.validators.clone());
            let b1 = block(
                11,
                offender.clone(),
                1,
                vec![justification(offender.clone(), b0.block_hash.clone())],
                fixture.validators.clone(),
            );
            fixture.add_block(&b0);
            fixture.add_block(&b1);

            // Empty-witness record for V at base seq 0 (e.g. minted for an
            // UnauthorizedSlashDeploy): EquivocationRecord::new(V, 0, {}).
            fixture.add_record(1, 0, &[]);

            for (index, (bond_offender, cite_tip)) in schedule.iter().enumerate() {
                let cited = if *cite_tip {
                    b1.block_hash.clone()
                } else {
                    fixture.genesis.block_hash.clone()
                };
                // Per-block bond toggle: include or exclude the offender from the
                // validating block's bond map.
                let bonded = if *bond_offender {
                    vec![
                        fixture.validators[0].clone(),
                        offender.clone(),
                        observer.clone(),
                    ]
                } else {
                    vec![fixture.validators[0].clone(), observer.clone()]
                };
                let observer_block = block(
                    20u8.saturating_add(index as u8),
                    observer.clone(),
                    (index as i32) + 1,
                    vec![justification(offender.clone(), cited)],
                    bonded,
                );

                let verdict = fixture.check(&observer_block).await;

                // (2) An honest offender never yields NeglectedEquivocation.
                prop_assert!(
                    !matches!(
                        verdict,
                        Either::Left(BlockError::Invalid(InvalidBlock::NeglectedEquivocation))
                    ),
                    "honest offender must never trigger NeglectedEquivocation (step {})",
                    index
                );
                prop_assert_eq!(verdict, Either::Right(ValidBlock::Valid));
            }

            // (1) The offender's witness set stayed EMPTY across the whole
            // schedule — no unbonded observation ever stamped it.
            let records = fixture
                .dag_storage
                .equivocation_records()
                .expect("read equivocation records");
            for record in records {
                if record.equivocator == offender {
                    prop_assert!(
                        record.equivocation_detected_block_hashes.is_empty(),
                        "FV audit #6: unbonded-offender witness set must stay empty, got {:?}",
                        record.equivocation_detected_block_hashes
                    );
                }
            }
            Ok(())
        })?;
    }
}
