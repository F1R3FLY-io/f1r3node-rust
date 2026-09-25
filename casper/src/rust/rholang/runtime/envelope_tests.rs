use std::num::NonZeroUsize;
use std::sync::Arc;

use models::rhoapi::Expr;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeLimits};
use models::rust::phlo_controls::{PhloControlsLimits, PhloControlsV1};
use models::rust::phlo_intent::{PhloFundingIntentLimits, PhloFundingIntentV1};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::{FundedDeploy, FundedDeployLimits, OfferedFundedDeploy};
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension, NativePhloRegionLimits,
    NativePhloRules,
};
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::create_runtime_from_kv_store;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;

#[path = "envelope_tests/native_replay_failure.rs"]
mod native_replay_failure;

#[tokio::test]
async fn funded_envelope_replays_recorded_comm_trace() {
    for offered in [false, true] {
        for (members, selected) in [(1, 1), (3, 2), (9, 9)] {
            assert_funded_trace_replay(offered, 100_000, members, selected).await;
        }
    }
}

#[tokio::test]
async fn offered_native_envelope_replays_metered_trace_and_verified_funding_identity() {
    use models::rust::deploy_envelope::DeployEnvelopeRef;
    use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
    use rholang::rust::interpreter::accounting::native_phlo_rules::NativePhloExecutionContract;
    use rholang::rust::interpreter::accounting::phlo_controls::{
        check_phlo_controls, PhloScheduleBinding,
    };
    use rholang::rust::interpreter::accounting::phlo_execution::PhloFundingIntentBinding;
    use rholang::rust::interpreter::test_utils::resources::create_runtimes;

    for (members, selected) in [(1, 1), (3, 2), (3, 3), (9, 9)] {
        let descriptor = PhloScheduleV1 {
            protocol_version: 6,
            network: b"test",
            shard: b"root",
            settlement_asset: b"REV",
            settlement_unit: b"atomic-REV",
            decimal_scale: 8,
            classes: NativePhloDimension::ALL
                .into_iter()
                .zip([2, 3, 5, 7])
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
            "new id(`rho:system:deployId`), c, fresh in { c!(*id, *fresh) | for (@seen, @name <- c) { @0!(seen, name) } }",
            true, 1_000_000, members, selected, Some(descriptor.clone()), 1_000_000,
        );
        let DeployEnvelopeRef::OfferedFunded(envelope) = input.view() else {
            panic!("expected offered funding");
        };
        let limits = PhloFundingIntentLimits {
            wire: PhloWireLimits {
                total_bytes: 16_384,
                field_bytes: 8_192,
            },
            controls: PhloControlsLimits {
                wire: PhloWireLimits {
                    total_bytes: 16_384,
                    field_bytes: 8_192,
                },
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
            1_000_000,
        )
        .unwrap();
        let config = |controls| {
            accounting::NativeRuntimeConfig::new(
                NativePhloExecutionContract::new(controls, &binding).unwrap(),
                accounting::native_phlo_rules::NativeBudgetTraceLimits {
                    attempts: 100_000,
                    path_segments: 4096,
                    regions: NativePhloRegionLimits {
                        regions: 4096,
                        encoded_authority_bytes: 1_000_000,
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
            .evaluate_native_envelope(&input, config(controls))
            .await
            .unwrap();
        assert!(
            evaluation.errors.is_empty(),
            "cohort {members}/{selected}: {:?}, usage {:?}",
            evaluation.errors,
            evaluation.native_phlo_usage
        );
        assert!(evaluation.native_phlo_usage.unwrap() > 0);
        assert!(evaluation.native_phlo_usage.unwrap() <= controls.resource_bound());
        assert_eq!(evaluation.cost.value, 0);
        assert!(evaluation.byte_observations.has_complete_measurements());
        let measured = NativePhloRules::resolve(&descriptor).unwrap();
        let measured = measured
            .measure(&evaluation.byte_observations, 4096)
            .unwrap();
        assert_eq!(measured.total(NativePhloDimension::Compute), 1);
        for dimension in NativePhloDimension::ALL {
            assert!(measured.total(dimension) > 0);
        }
        let expected_usage: u64 = evaluation
            .byte_observations
            .rows
            .iter()
            .map(|row| {
                let raw = row.measurement.unwrap();
                let compute =
                    u64::from(row.kind == accounting::authority::AuthorityByteEventKind::Comm);
                (2 * compute
                    + 3 * raw.introduction_bytes
                    + 5 * raw.transfer_bytes
                    + 7 * raw.trace_bytes)
                    * selected as u64
                    * row.authority.regions.len() as u64
            })
            .sum();
        assert_eq!(evaluation.native_phlo_usage, Some(expected_usage));
        let checkpoint = play.runtime.create_checkpoint().await;
        replay.runtime.reset(&root).await.unwrap();
        replay.runtime.rig(checkpoint.log).await.unwrap();
        let replayed = replay
            .evaluate_native_envelope(&input, config(controls))
            .await
            .unwrap();
        assert!(replayed.errors.is_empty(), "{:?}", replayed.errors);
        replay.runtime.check_replay_data().await.unwrap();
        assert_eq!(evaluation.native_phlo_usage, replayed.native_phlo_usage);
        assert_eq!(evaluation.byte_observations, replayed.byte_observations);
        assert_eq!(
            checkpoint.root,
            replay.runtime.create_checkpoint().await.root
        );
        assert_eq!(play.runtime.cost.deploy_id(), input.identity().as_bytes());
        assert_eq!(
            play.runtime.cost.deploy_id(),
            replay.runtime.cost.deploy_id()
        );
        assert_eq!(
            play.runtime.cost.signature(),
            replay.runtime.cost.signature()
        );
        assert_eq!(
            play.runtime.cost.signature(),
            accounting::funding_sig(envelope)
        );
        if selected == 9 {
            assert!(expected_usage > 100_000);
            let limited_controls = check_phlo_controls(
                binding.schedule().environment,
                0,
                u64::MAX,
                view.controls(),
                binding.schedule(),
                100_000,
            )
            .unwrap();
            let mut limited = runtime().await;
            let rejected = limited
                .evaluate_native_envelope(&input, config(limited_controls))
                .await
                .unwrap();
            assert!(rejected
                .errors
                .contains(&InterpreterError::OutOfPhlogistonsError));
            assert!(rejected.native_phlo_usage.unwrap() <= 100_000);
            assert!(limited.get_data_par(&channel(0)).await.is_empty());
        }
    }
}

async fn assert_funded_trace_replay(
    offered: bool,
    exposure: u128,
    members: usize,
    selected: usize,
) {
    use rholang::rust::interpreter::test_utils::resources::create_runtimes;

    use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;

    let input = envelope_with_signers(
                "new id(`rho:system:deployId`), c, fresh in { c!(*id, *fresh) | for (@seen, @name <- c) { @0!(seen, name) } }",
                offered, exposure, members, selected,
            );
    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    let (play, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let mut play = RuntimeOps::new(play);
    let mut replay = ReplayRuntimeOps::new_from_runtime(replay);
    let (processed, evaluation, exhausted) = play
        .process_envelope_with_budget_and_authority_mode_and_host_work(
            input,
            Cost::create(100_000, "test"),
            None,
            DefaultCostAuthority::Funders,
            true,
            None,
        )
        .await
        .unwrap();
    assert!(!exhausted);
    assert!(evaluation.errors.is_empty(), "{:?}", evaluation.errors);
    assert!(!evaluation.authority_events.is_empty());
    replay.rig(&processed).await.unwrap();
    let (replayed, successful, _) = replay
        .run_user_deploy_with_host_work(
            &processed,
            &mut HashMap::new(),
            Some((
                Cost::create(100_000, "test"),
                evaluation.authority_realized.clone(),
            )),
            (selected == 2).then(|| {
                HostWorkBudget::new(HostWorkLimits::uniform(
                    models::rust::host_work::HostWorkLimit::new(1_000_000),
                ))
            }),
        )
        .await
        .unwrap();
    assert!(successful);
    replay
        .runtime_ops
        .runtime
        .check_replay_data()
        .await
        .unwrap();
    assert_eq!(evaluation.cost, replayed.cost);
    assert_eq!(evaluation.authority_realized, replayed.authority_realized);
    let counted = evaluation
        .byte_observations
        .checked_measurements(64)
        .unwrap();
    let replay_counted = replayed.byte_observations.checked_measurements(64).unwrap();
    assert_eq!(counted.totals(), replay_counted.totals());
    assert!(counted.totals().transfer_bytes > 0);
    assert!(counted.totals().introduction_bytes > 0);
    assert!(counted.totals().trace_bytes > 0);
    let descriptor = models::rust::phlo_schedule::PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip([
                b"compute".as_slice(),
                b"introduction",
                b"transfer",
                b"trace",
            ])
            .map(|(dimension, identity)| dimension.resource_class(identity, 1))
            .collect(),
        actual_price: 1,
        compatibility_rule: native_resource_compatibility_rule(),
    };
    let rules = NativePhloRules::resolve(&descriptor).unwrap();
    let terms = descriptor
        .encode(models::rust::phlo_schedule::PhloGenesisPolicy::LIMITS)
        .unwrap();
    let binding = rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding::new(
        &descriptor,
        models::rust::phlo_schedule::PhloGenesisPolicy::LIMITS,
    )
    .unwrap();
    let measured = rules.measure(&evaluation.byte_observations, 64).unwrap();
    let replay_measured = rules.measure(&replayed.byte_observations, 64).unwrap();
    assert_eq!(measured.total(NativePhloDimension::Compute), 1);
    for dimension in NativePhloDimension::ALL {
        assert_eq!(measured.total(dimension), replay_measured.total(dimension));
    }
    let canonical = |capture: &rholang::rust::interpreter::accounting::native_phlo_rules::NativePhloMeasurements<'_>| {
        let mut occurrences: Vec<_> = capture.occurrences()
            .map(|part| (part.observation().clone(), part.class(), part.quantity())).collect();
        occurrences.sort_by_key(|(row, class, quantity)| (row.event_id, row.kind.tag(), *class, *quantity));
        occurrences
    };
    assert_eq!(canonical(&measured), canonical(&replay_measured));
    let canonical_regions = |capture: &rholang::rust::interpreter::accounting::native_phlo_rules::NativePhloMeasurements<'_>| {
        let work = HostWorkBudget::new(HostWorkLimits::uniform(
            models::rust::host_work::HostWorkLimit::new(10_000_000),
        ));
        let demands = capture.region_demands(NativePhloRegionLimits {
            regions: 4096,
            encoded_authority_bytes: 1_000_000,
        }, &work).unwrap();
        let located = demands.locate_purses(
            rholang::rust::interpreter::accounting::native_phlo_rules::NativePhloPurseLimits {
                bindings: 4096,
                encoded_binding_bytes: 1_000_000,
            },
            &work,
        ).unwrap();
        let acquisition = located.prepare_acquisition_demand(
            &binding, &terms,
            rholang::rust::interpreter::accounting::native_phlo_rules::NativePhloAcquisitionLimits {
                schedule: models::rust::phlo_schedule::PhloGenesisPolicy::LIMITS, entries: 4096,
            }, &work,
        ).unwrap();
        use rholang::rust::interpreter::accounting::phlo_execution::{
            check_counted_phlo_execution, prepare_counted_phlo_discharge,
            PhloDischargeLimits, PhloExecutionLimits, PhloOutcome,
        };
        use rholang::rust::interpreter::accounting::phlo_controls::{
            check_phlo_controls, SignedPhloControls,
        };
        let execution_limits = PhloExecutionLimits {
            resource_entries: 16384, authority_nodes: 1_000_000, key_bytes: 10_000_000,
        };
        let discharge = prepare_counted_phlo_discharge(&[], acquisition.resources(), PhloDischargeLimits {
            execution: execution_limits,
            key: models::rust::phlo_obligation::PhloObligationKeyLimits {
                wire: models::rust::phlo_wire::PhloWireLimits { total_bytes: 1_000_000, field_bytes: 500_000 },
                authority_nodes: 100_000,
            }, aggregate_key_bytes: 10_000_000,
        }, &work).unwrap();
        let schedules = [binding.schedule()];
        let controls = check_phlo_controls(schedules[0].environment, 0, u64::MAX, SignedPhloControls {
            limit: 1_000_000, price_ceiling: 1, required_owner_ceilings: &[1], permitted_schedules: &schedules,
        }, schedules[0], 1_000_000).unwrap();
        acquisition.check_controls(controls).unwrap();
        let execution = check_counted_phlo_execution(controls, discharge.witness(), execution_limits).unwrap();
        let expected_usage: u64 = located.occurrences().map(|part| part.demand().measurement().quantity() * selected as u64).sum();
        assert_eq!(execution.usage(), expected_usage);
        assert_eq!(execution.retained_charge(PhloOutcome::Accepted(&[])), expected_usage + 1);
        assert_eq!(execution.prepaid_usage(), 0);
        let mut occurrences: Vec<_> = acquisition.occurrences().map(|(location, resource)| {
            let demand = location.demand();
            let part = demand.measurement();
            assert_eq!(resource.resource.authority, location.purse().authority());
            assert_eq!(resource.resource.location, location.purse().encoded_channel());
            assert_eq!(resource.resource.acquisition_terms, terms);
            assert_eq!(resource.quantity, part.quantity());
            (part.observation().clone(), demand.region().clone(), part.class(), part.quantity(),
                location.purse().authority().clone(), location.purse().encoded_channel().to_vec())
        }).collect();
        occurrences.sort_by_key(|(row, region, class, quantity, _, _)| (row.event_id, row.kind.tag(), region.instance_id.clone(), *class, *quantity));
        occurrences
    };
    let regions = canonical_regions(&measured);
    assert!(!regions.is_empty());
    assert_eq!(
        regions.len(),
        measured
            .occurrences()
            .map(|part| part.observation().authority.regions.len())
            .sum::<usize>()
    );
    assert_eq!(regions, canonical_regions(&replay_measured));
    let mut observed = evaluation.byte_observations.rows.clone();
    let mut replay_observed = replayed.byte_observations.rows.clone();
    observed.sort_by_key(|row| row.event_id);
    replay_observed.sort_by_key(|row| row.event_id);
    assert_eq!(observed, replay_observed);
    assert_eq!(
        play.runtime.cost.deploy_id(),
        replay.runtime_ops.runtime.cost.deploy_id()
    );
    assert_eq!(
        play.runtime.cost.signature(),
        replay.runtime_ops.runtime.cost.signature()
    );
    assert_eq!(
        play.runtime.create_checkpoint().await.root,
        replay.runtime_ops.runtime.create_checkpoint().await.root
    );
}

#[test]
fn generated_funded_cohorts_preserve_recorded_trace_replay() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};

    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut runner = TestRunner::new(Config {
        cases: 32,
        source_file: Some(file!()),
        ..Config::default()
    });
    runner
        .run(
            &(any::<bool>(), any::<u64>(), 1_usize..10, any::<u8>()),
            |(offered, exposure, members, selector)| {
                let selected = 1 + usize::from(selector) % members;
                executor.block_on(assert_funded_trace_replay(
                    offered,
                    exposure.into(),
                    members,
                    selected,
                ));
                Ok(())
            },
        )
        .unwrap();
}

#[tokio::test]
async fn funded_replay_keeps_genesis_and_ordinary_admission_closed() {
    use rholang::rust::interpreter::test_utils::resources::create_runtimes;

    use crate::rust::rholang::replay_runtime::{ReplayBlockKind, ReplayRuntimeOps};

    for offered in [false, true] {
        for kind in [ReplayBlockKind::Genesis, ReplayBlockKind::Ordinary] {
            let input = ProcessedDeploy::from_envelope(envelope("@0!(7)", offered, 100_000));
            let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
            let (_, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
            let mut replay = ReplayRuntimeOps::new_from_runtime(replay);
            let before = replay.runtime_ops.runtime.create_checkpoint().await.root;
            let error = replay.replay_deploy_e(kind, &input).await.unwrap_err();
            assert!(
                matches!(error, CasperError::InvalidCostSettlement(ref message)
                if message.contains(if kind == ReplayBlockKind::Genesis {
                    "body-only replay cannot execute a funded envelope"
                } else { "missing its authority certificate" }))
            );
            assert_eq!(
                before,
                replay.runtime_ops.runtime.create_checkpoint().await.root
            );
            assert!(replay
                .runtime_ops
                .get_data_par(&channel(0))
                .await
                .is_empty());
        }
    }
}

#[tokio::test]
async fn changed_funding_commitment_cannot_reuse_a_recorded_comm_trace() {
    use rholang::rust::interpreter::test_utils::resources::create_runtimes;

    use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;

    let term = "new c in { c!(7) | for (@value <- c) { @0!(value) } }";
    for offered in [false, true] {
        let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
        let (play, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
        let mut play = RuntimeOps::new(play);
        let mut replay = ReplayRuntimeOps::new_from_runtime(replay);
        let (original, evaluation, _) = play
            .process_envelope_with_budget_and_authority_mode_and_host_work(
                envelope(term, offered, 100_000),
                Cost::create(100_000, "test"),
                None,
                DefaultCostAuthority::Funders,
                true,
                None,
            )
            .await
            .unwrap();
        assert!(!original.is_failed);
        let mut changed = ProcessedDeploy::from_envelope(envelope(term, offered, 100_001));
        assert_ne!(original.typed_deploy_id(), changed.typed_deploy_id());
        changed.cost = original.cost.clone();
        changed.deploy_log = original.deploy_log;
        replay.rig(&changed).await.unwrap();
        let result = replay
            .run_user_deploy(
                &changed,
                &mut HashMap::new(),
                Some((Cost::create(100_000, "test"), evaluation.authority_realized)),
            )
            .await;
        if let Ok((_, successful, _)) = result {
            assert!(
                !successful
                    || replay
                        .runtime_ops
                        .runtime
                        .check_replay_data()
                        .await
                        .is_err()
            );
        }
    }
}

fn envelope(term: &str, offered: bool, exposure: u128) -> DeployEnvelope {
    envelope_with_signers(term, offered, exposure, 1, 1)
}

fn sign<A: std::fmt::Debug + serde::Serialize + crypto::rust::signatures::signed::ToMessage>(
    data: A,
    members: usize,
    selected: usize,
) -> Cosigned<A> {
    let mut keys: Vec<_> = (1..=members)
        .map(|index| {
            let key = PrivateKey::from_bytes(&[u8::try_from(index).unwrap(); 32]);
            (
                Cosigner {
                    pk: Secp256k1.to_public(&key),
                    sig: Bytes::new(),
                    sig_algorithm: Box::new(Secp256k1),
                },
                key,
            )
        })
        .collect();
    keys.sort_by(|left, right| left.0.pk.bytes.cmp(&right.0.pk.bytes));
    let mut signers: Vec<_> = keys.iter().map(|(signer, _)| signer.clone()).collect();
    let mut bitmap = vec![0; members.div_ceil(8)];
    for index in 0..selected {
        bitmap[index / 8] |= 1 << (index % 8);
    }
    let hash = Cosigned::envelope_signing_hash_for_presence(
        &data,
        &signers,
        selected as u32,
        &bitmap,
        "secp256k1",
    )
    .unwrap();
    for index in 0..selected {
        signers[index].sig = Secp256k1.sign(&hash, &keys[index].1.bytes).into();
    }
    Cosigned::from_envelope_signed_data_threshold(data, signers, selected as u32).unwrap()
}

fn envelope_with_signers(
    term: &str,
    offered: bool,
    exposure: u128,
    members: usize,
    selected: usize,
) -> DeployEnvelope {
    envelope_with_signers_and_schedule(term, offered, exposure, members, selected, None, 100_000)
}

fn envelope_with_signers_and_schedule(
    term: &str,
    offered: bool,
    exposure: u128,
    members: usize,
    selected: usize,
    schedule: Option<models::rust::phlo_schedule::PhloScheduleV1<'_>>,
    limit: u64,
) -> DeployEnvelope {
    let wire = PhloWireLimits {
        total_bytes: 16_384,
        field_bytes: 8_192,
    };
    let controls = PhloControlsLimits {
        wire,
        owners: 1,
        schedules: usize::from(schedule.is_some()),
        total_classes: schedule.as_ref().map_or(0, |value| value.classes.len()),
    };
    let funding = PhloFundingIntentLimits {
        wire,
        controls,
        sources: 0,
        resource_permissions: 0,
        authority_nodes: 0,
    };
    let payload = FundedDeployLimits {
        deploy_bytes: 32_768,
        signing: wire,
        funding,
    };
    let commitment = schedule.as_ref().map_or([1; 32], |value| {
        value
            .digest(controls.schedule(value.classes.len()))
            .unwrap()
    });
    let intent = PhloFundingIntentV1 {
        controls: PhloControlsV1 {
            limit,
            price_ceiling: 1,
            required_owner_ceilings: vec![1],
            permitted_schedules: schedule.into_iter().collect(),
        },
        schedule_commitment: commitment,
        total_exposure: exposure,
        sources: Vec::new(),
    }
    .encode(funding)
    .unwrap();
    let body = DeployData {
        term: term.to_string(),
        language: "rholang".to_string(),
        time_stamp: 17,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let proto = if offered {
        OfferedFundedDeploy::to_proto(&sign(
            OfferedFundedDeploy::new(body, intent, i64::try_from(limit).unwrap(), 1, payload)
                .unwrap(),
            members,
            selected,
        ))
        .unwrap()
    } else {
        FundedDeploy::to_proto(&sign(
            FundedDeploy::new(body, intent, payload).unwrap(),
            members,
            selected,
        ))
        .unwrap()
    };
    DeployEnvelope::from_proto(proto, DeployEnvelopeLimits {
        payload,
        members: NonZeroUsize::new(members).unwrap(),
    })
    .unwrap()
}

#[tokio::test]
async fn retained_runtime_matches_body_runtime_for_historical_formats() {
    let body = envelope("new fresh in { @0!(*fresh) }", true, 100_000)
        .body()
        .clone();
    let key = PrivateKey::from_bytes(&[9; 32]);
    let legacy = Cosigned::from_single_signer(
        Signed::create(body.clone(), Box::new(Secp256k1), key.clone()).unwrap(),
    )
    .unwrap();
    let bound = Cosigned::create_single_envelope(body, Box::new(Secp256k1), key).unwrap();
    for body in [legacy, bound] {
        let envelope = DeployEnvelope::from_body_envelope(body.clone()).unwrap();
        let mut prior = runtime().await;
        let old = prior
            .evaluate_cosigned_with_budget(&body, Cost::create(100_000, "test"))
            .await
            .unwrap();
        assert!(old.errors.is_empty(), "{:?}", old.errors);
        let mut current = runtime().await;
        let processed = process(&mut current, envelope).await.unwrap();
        assert_eq!(processed.is_failed, !old.errors.is_empty());
        assert_eq!(processed.cost, Cost::to_proto(old.cost));
        assert_eq!(
            processed.deploy_log,
            prior
                .runtime
                .take_event_log()
                .await
                .into_iter()
                .map(event_converter::to_casper_event)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            prior.runtime.cost.signature(),
            current.runtime.cost.signature()
        );
        assert_eq!(
            prior.runtime.cost.deploy_id(),
            current.runtime.cost.deploy_id()
        );
        assert_eq!(
            prior.get_data_par(&channel(0)).await,
            current.get_data_par(&channel(0)).await
        );
        assert_eq!(
            prior.runtime.create_checkpoint().await.root,
            current.runtime.create_checkpoint().await.root
        );
    }
}

#[tokio::test]
async fn funded_runtime_uses_all_selected_signers_and_excludes_placeholders() {
    for offered in [false, true] {
        for members in [1, 3, 9, 65] {
            for selected in [1, members] {
                let envelope = envelope_with_signers("@0!(7)", offered, 100_000, members, selected);
                let principals: Vec<_> = envelope
                    .signers()
                    .iter()
                    .filter(|signer| !signer.sig.is_empty())
                    .map(|signer| accounting::principal_ground_v61(&signer.pk.bytes))
                    .collect();
                assert_eq!(principals.len(), selected);
                let expected = if selected == 1 {
                    accounting::funding_sig_single(&principals[0])
                } else {
                    accounting::funding_sig_compound(
                        &principals.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                    )
                };
                let mut runtime = runtime().await;
                let processed = process(&mut runtime, envelope).await.unwrap();
                assert!(!processed.is_failed);
                assert_eq!(runtime.runtime.cost.signature(), expected);
            }
        }
    }
}

#[tokio::test]
async fn funded_runtime_failure_rolls_back_and_retains_measurements() {
    for offered in [false, true] {
        let mut runtime = runtime().await;
        let before = runtime.runtime.create_checkpoint().await.root;
        let failed = envelope("@0!(7) | @1!(8)", offered, 100_000);
        let (processed, evaluation, exhausted) = runtime
            .process_envelope_with_budget_and_authority_mode_and_host_work(
                failed.clone(),
                Cost::create(0, "test"),
                None,
                DefaultCostAuthority::Funders,
                true,
                None,
            )
            .await
            .unwrap();
        assert!(processed.is_failed);
        assert!(exhausted);
        assert!(!evaluation.errors.is_empty());
        assert_eq!(processed.envelope(), &failed);
        assert_eq!(before, runtime.runtime.create_checkpoint().await.root);
        assert!(runtime.get_data_par(&channel(0)).await.is_empty());
        let successful = envelope("@0!(7)", offered, 100_001);
        let (processed, evaluation, exhausted) = runtime
            .process_envelope_with_budget_and_authority_mode_and_host_work(
                successful,
                Cost::create(100_000, "test"),
                None,
                DefaultCostAuthority::Funders,
                true,
                None,
            )
            .await
            .unwrap();
        assert!(!processed.is_failed);
        assert!(!exhausted);
        assert!(evaluation.errors.is_empty());
        assert!(evaluation.byte_observations.has_complete_measurements());
        assert_eq!(processed.cost, Cost::to_proto(evaluation.cost));
        assert_eq!(runtime.get_data_par(&channel(0)).await.len(), 1);
    }
}

#[test]
fn funded_runtime_generated_round_trips_preserve_execution() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};

    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut runner = TestRunner::new(Config::with_cases(32));
    runner
        .run(
            &(any::<bool>(), 1_u64..1_000_000, 1_usize..10, any::<u8>()),
            |(offered, exposure, members, selector)| {
                let selected = 1 + usize::from(selector) % members;
                let envelope = envelope_with_signers(
                    "new id(`rho:system:deployId`), fresh in { @0!(*id, *fresh) }",
                    offered,
                    exposure.into(),
                    members,
                    selected,
                );
                let proto = envelope.to_proto().unwrap();
                executor.block_on(async {
                    let mut first = runtime().await;
                    let mut second = runtime().await;
                    let first_processed = process(&mut first, envelope.clone()).await.unwrap();
                    let restored = DeployEnvelope::from_proto(proto, DeployEnvelopeLimits {
                        payload: FundedDeployLimits {
                            deploy_bytes: 32_768,
                            signing: PhloWireLimits {
                                total_bytes: 16_384,
                                field_bytes: 8_192,
                            },
                            funding: PhloFundingIntentLimits {
                                wire: PhloWireLimits {
                                    total_bytes: 16_384,
                                    field_bytes: 8_192,
                                },
                                controls: PhloControlsLimits {
                                    wire: PhloWireLimits {
                                        total_bytes: 16_384,
                                        field_bytes: 8_192,
                                    },
                                    owners: 1,
                                    schedules: 0,
                                    total_classes: 0,
                                },
                                sources: 0,
                                resource_permissions: 0,
                                authority_nodes: 0,
                            },
                        },
                        members: NonZeroUsize::new(members).unwrap(),
                    })
                    .unwrap();
                    prop_assert_eq!(&restored, &envelope);
                    let second_processed = process(&mut second, restored).await.unwrap();
                    prop_assert!(!first_processed.is_failed);
                    prop_assert_eq!(first_processed, second_processed);
                    prop_assert_eq!(
                        first.runtime.create_checkpoint().await.root,
                        second.runtime.create_checkpoint().await.root
                    );
                    prop_assert_eq!(
                        first.runtime.cost.signature(),
                        second.runtime.cost.signature()
                    );
                    Ok(())
                })
            },
        )
        .unwrap();
}

#[tokio::test]
async fn funded_runtime_exhaustion_after_progress_reverts_partial_effects() {
    for offered in [false, true] {
        let input = envelope("@0!(7) | @1!(8) | @2!(9)", offered, 100_000);
        let mut probe = runtime().await;
        let successful = process(&mut probe, input.clone()).await.unwrap();
        let limit = i64::try_from(successful.cost.cost).unwrap() - 1;
        assert!(limit > 0);
        let mut runtime = runtime().await;
        let before = runtime.runtime.create_checkpoint().await.root;
        let (failed, measured, exhausted) = runtime
            .process_envelope_with_budget_and_authority_mode_and_host_work(
                input.clone(),
                Cost::create(limit, "test"),
                None,
                DefaultCostAuthority::Funders,
                true,
                None,
            )
            .await
            .unwrap();
        assert!(failed.is_failed);
        assert!(exhausted);
        assert!(
            !failed.deploy_log.is_empty(),
            "the fixture must execute a tuple-space operation before exhaustion"
        );
        assert!(measured.cost.value > 0);
        assert_eq!(failed.envelope(), &input);
        assert_eq!(before, runtime.runtime.create_checkpoint().await.root);
        for number in 0..3 {
            assert!(runtime.get_data_par(&channel(number)).await.is_empty());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_funded_runtimes_do_not_share_context() {
    let mut tasks = Vec::new();
    for exposure in 1..=8 {
        tasks.push(tokio::spawn(async move {
            let envelope = envelope(
                "new id(`rho:system:deployId`) in { @0!(*id) }",
                true,
                exposure,
            );
            let mut runtime = runtime().await;
            tokio::task::yield_now().await;
            let processed = process(&mut runtime, envelope.clone()).await.unwrap();
            tokio::task::yield_now().await;
            assert!(!processed.is_failed);
            assert_eq!(
                runtime.runtime.cost.deploy_id().as_slice(),
                envelope.identity().as_bytes()
            );
            let data = runtime.get_data_par(&channel(0)).await;
            assert!(matches!(&data[0].unforgeables[0].unf_instance,
                Some(UnfInstance::GDeployIdBody(id)) if id.sig == envelope.identity().as_bytes()));
            envelope.identity().as_bytes().to_vec()
        }));
    }
    let mut identities = std::collections::BTreeSet::new();
    for task in tasks {
        assert!(identities.insert(task.await.unwrap()));
    }
}

async fn runtime() -> RuntimeOps {
    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    RuntimeOps::new(
        create_runtime_from_kv_store(
            store,
            Arc::new(HashMap::new()),
            false,
            &mut Vec::new(),
            Arc::new(Box::new(Matcher)),
            ExternalServices::noop(),
        )
        .await,
    )
}

async fn process(
    runtime: &mut RuntimeOps,
    envelope: DeployEnvelope,
) -> Result<ProcessedDeploy, CasperError> {
    runtime
        .process_envelope_with_budget_and_authority_mode_and_host_work(
            envelope,
            Cost::create(100_000, "test"),
            None,
            DefaultCostAuthority::Funders,
            true,
            None,
        )
        .await
        .map(|(processed, evaluation, _)| {
            assert!(evaluation.errors.is_empty(), "{:?}", evaluation.errors);
            processed
        })
}

fn channel(number: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(number)),
    }])
}

#[tokio::test]
async fn funded_runtime_retains_signed_identity_and_changes_generated_names() {
    let term = "new id(`rho:system:deployId`), fresh in { @0!(*id) | @1!(*fresh) }";
    let mut outputs = Vec::new();
    for offered in [false, true] {
        for exposure in [100_000, 100_001] {
            let envelope = envelope(term, offered, exposure);
            let original = envelope.to_proto().unwrap();
            let mut runtime = runtime().await;
            let processed = process(&mut runtime, envelope.clone()).await.unwrap();
            assert!(!processed.is_failed);
            assert_eq!(processed.envelope().to_proto().unwrap(), original);
            assert_eq!(
                runtime.runtime.cost.deploy_id().as_slice(),
                envelope.identity().as_bytes()
            );
            let data = runtime.get_data_par(&channel(0)).await;
            assert_eq!(data.len(), 1);
            assert!(matches!(&data[0].unforgeables[0].unf_instance,
                Some(UnfInstance::GDeployIdBody(id)) if id.sig == envelope.identity().as_bytes()));
            let fresh = runtime.get_data_par(&channel(1)).await;
            assert_eq!(fresh.len(), 1);
            outputs.push(fresh[0].clone());
        }
    }
    for (index, first) in outputs.iter().enumerate() {
        for second in &outputs[index + 1..] {
            assert_ne!(first, second);
        }
    }
}
