use models::rust::deploy_envelope::DeployEnvelopeRef;
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use rholang::rust::interpreter::accounting::native_phlo_rules::NativePhloExecutionContract;
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding,
};
use rholang::rust::interpreter::accounting::phlo_execution::PhloFundingIntentBinding;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;

use super::*;

#[tokio::test]
async fn denied_native_comm_replays_the_same_economic_failure() {
    let descriptor = PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip([1, 0, 0, 0])
            .zip([
                b"compute".as_slice(),
                b"introduction",
                b"transfer",
                b"trace",
            ])
            .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
            .collect(),
        actual_price: 1,
        compatibility_rule: native_resource_compatibility_rule(),
    };
    let input = envelope_with_signers_and_schedule(
        "new c in { c!(7) | for (@x <- c) { @0!(x) } }",
        true,
        100_000,
        1,
        1,
        Some(descriptor.clone()),
        8,
    );
    let DeployEnvelopeRef::OfferedFunded(envelope) = input.view() else {
        panic!("expected offered envelope");
    };
    let wire = PhloWireLimits {
        total_bytes: 16_384,
        field_bytes: 8_192,
    };
    let limits = PhloFundingIntentLimits {
        wire,
        controls: PhloControlsLimits {
            wire,
            owners: 1,
            schedules: 1,
            total_classes: 4,
        },
        sources: 0,
        resource_permissions: 0,
        authority_nodes: 0,
    };
    let intent = PhloFundingIntentV1::decode(envelope.data.funding_intent(), limits).unwrap();
    let intent_binding = PhloFundingIntentBinding::new(&intent, limits).unwrap();
    let view = intent_binding.view().unwrap();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let controls = check_phlo_controls(
        binding.schedule().environment,
        0,
        u64::MAX,
        view.controls(),
        binding.schedule(),
        0,
    )
    .unwrap();
    let config = || {
        accounting::NativeRuntimeConfig::new(
            NativePhloExecutionContract::new(controls, &binding).unwrap(),
            accounting::native_phlo_rules::NativeBudgetTraceLimits {
                attempts: 100_000,
                path_segments: 4096,
                regions: NativePhloRegionLimits {
                    regions: 32,
                    encoded_authority_bytes: 1_048_576,
                },
            },
            HostWorkBudget::new(HostWorkLimits::uniform(
                models::rust::host_work::HostWorkLimit::new(100_000_000),
            )),
        )
    };
    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    let (play, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let mut play = RuntimeOps::new(play);
    let mut replay = RuntimeOps::new(replay);
    let root = play.runtime.get_root().await;
    let evaluation = play
        .evaluate_native_envelope(&input, config())
        .await
        .unwrap();
    assert!(
        evaluation
            .errors
            .contains(&InterpreterError::OutOfPhlogistonsError),
        "{:?}",
        evaluation.errors
    );
    assert_eq!(evaluation.native_phlo_usage, Some(0));
    assert!(play.get_data_par(&channel(0)).await.is_empty());
    let checkpoint = play.runtime.create_checkpoint().await;
    replay.runtime.reset(&root).await.unwrap();
    replay.runtime.rig(checkpoint.log).await.unwrap();
    let replayed = replay
        .evaluate_native_envelope(&input, config())
        .await
        .unwrap();
    assert_eq!(
        replayed.errors, evaluation.errors,
        "replay must reproduce the denied COMM, not silently omit its failure"
    );
    assert_eq!(replayed.native_phlo_usage, evaluation.native_phlo_usage);
    assert_eq!(replayed.byte_observations, evaluation.byte_observations);
    replay.runtime.check_replay_data().await.unwrap();
    assert!(replay.get_data_par(&channel(0)).await.is_empty());
}
