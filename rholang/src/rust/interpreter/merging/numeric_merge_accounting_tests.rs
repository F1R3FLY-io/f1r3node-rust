use std::collections::{BTreeMap, BTreeSet};

use models::rhoapi::{CostAuthority, CostSignature, CostStack};
use proptest::prelude::*;
use rspace_plus_plus::rspace::trace::event::{Consume, COMM};

use super::*;
use crate::rust::interpreter::accounting::authority::{
    allocate_authority_events, allocate_quantitative_events, authority_demand,
    compound_cost_signatures, cost_region, sig_to_cost_signature, AuthorityByteEvent,
    AuthorityByteEventKind, AuthorityError, AuthorityEvent, ResourceMultiset,
};
use crate::rust::interpreter::accounting::byte_accounting::comm_charge;
use crate::rust::interpreter::accounting::Sig;

fn signature(unit: bool) -> CostSignature {
    sig_to_cost_signature(&if unit {
        Sig::Unit
    } else {
        Sig::Ground(vec![19; 32])
    })
    .unwrap()
}

fn datum(
    value: i64,
    authority: Option<CostAuthority>,
    stack: Option<CostStack>,
) -> Datum<ListParWithRandom> {
    let channel = Blake2b256Hash::new(b"numeric-merge-accounting-regression");
    let entropy = bincode::serialize(&(value, &authority, &stack)).unwrap();
    let data = ListParWithRandom {
        pars: vec![RhoNumber::create_par(value)],
        random_state: Blake2b512Random::create_from_bytes(&entropy).to_bytes(),
        cost_authority: authority,
        cost_stack: stack,
    };
    Datum {
        source: Produce {
            hash: stable_hash_provider::hash_produce(channel.bytes(), &data, false),
            channel_hash: channel,
            persistent: false,
            is_deterministic: true,
            output_value: vec![],
            failed: false,
        },
        a: data,
        persist: false,
    }
}

fn accounted_datum(unit: bool, stack: bool) -> Datum<ListParWithRandom> {
    let signature = signature(unit);
    datum(
        15,
        Some(CostAuthority {
            regions: vec![cost_region(&signature, b"surviving-writer", 0).unwrap()],
        }),
        stack.then(|| CostStack {
            cells: vec![signature],
        }),
    )
}

fn merge_single(
    original: &Datum<ListParWithRandom>,
    merge_type: MergeType,
) -> Datum<ListParWithRandom> {
    let base = datum(10, None, None);
    let changes = ChannelChange {
        added: vec![serializers::encode_datum(original)],
        removed: vec![serializers::encode_datum(&base)],
    };
    merge_changes(vec![base], changes, 5, merge_type).unwrap()
}

fn merge_changes(
    base: Vec<Datum<ListParWithRandom>>,
    changes: ChannelChange<Vec<u8>>,
    diff: i64,
    merge_type: MergeType,
) -> Result<Datum<ListParWithRandom>, HistoryError> {
    let action = RholangMergingLogic::calculate_number_channel_merge(
        &datum(0, None, None).source.channel_hash,
        diff,
        merge_type,
        &changes,
        |_| Ok(base.clone()),
    )?;
    match action {
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(action)) => {
            assert_eq!(action.data.len(), 1);
            Ok(serializers::decode_datum(&action.data[0]))
        }
        other => panic!("unexpected numeric merge action: {other:?}"),
    }
}

fn merge_outputs(
    base: Datum<ListParWithRandom>,
    outputs: &[Datum<ListParWithRandom>],
    diff: i64,
    merge_type: MergeType,
) -> Result<Datum<ListParWithRandom>, HistoryError> {
    let changes = ChannelChange {
        added: outputs.iter().map(serializers::encode_datum).collect(),
        removed: vec![serializers::encode_datum(&base)],
    };
    merge_changes(vec![base], changes, diff, merge_type)
}

#[test]
fn numeric_merge_retains_distinct_regions_for_the_same_signature() {
    for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
        let left = accounted_datum(false, false);
        let mut authority = left.a.cost_authority.clone().unwrap();
        authority.regions[0].instance_id = vec![23; 32];
        let right = datum(15, Some(authority), None);
        let merged = merge_outputs(datum(10, None, None), &[left, right], 5, merge_type).unwrap();
        let authority = merged.a.cost_authority.unwrap();
        assert_eq!(authority.regions.len(), 2);
        assert_eq!(
            authority_demand(&authority)
                .unwrap()
                .0
                .values()
                .sum::<u64>(),
            2
        );
    }
}

#[test]
fn numeric_merge_rejects_conflicting_region_bindings() {
    let left = accounted_datum(false, false);
    let mut right = left.clone();
    right.a.cost_authority.as_mut().unwrap().regions[0].signature = Some(signature(true));
    for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
        assert!(matches!(
            merge_outputs(
                datum(10, None, None),
                &[left.clone(), right.clone()],
                5,
                merge_type
            ),
            Err(HistoryError::MergeError(_))
        ));
    }
}

#[test]
fn numeric_merge_rejects_numeric_stack_hybrids() {
    for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
        assert!(matches!(
            merge_outputs(
                datum(10, None, None),
                &[accounted_datum(false, true)],
                5,
                merge_type
            ),
            Err(HistoryError::MergeError(_))
        ));
    }
}

#[test]
fn numeric_merge_retains_explicit_empty_authority() {
    for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
        let empty = datum(15, Some(CostAuthority::default()), None);
        let bare = datum(15, None, None);
        let singleton = merge_single(&empty, merge_type);
        assert_eq!(singleton.a, empty.a);
        let merged = merge_outputs(datum(10, None, None), &[empty, bare], 5, merge_type).unwrap();
        assert_eq!(merged.a.cost_authority, Some(CostAuthority::default()));
    }
}

#[test]
fn numeric_merge_empty_additions_return_checked_error() {
    assert!(matches!(
        merge_changes(
            vec![datum(10, None, None)],
            ChannelChange::empty(),
            0,
            MergeType::IntegerAdd
        ),
        Err(HistoryError::MergeError(_))
    ));
}

#[test]
fn numeric_merge_distinct_datums_with_one_random_state_return_checked_error() {
    let left = accounted_datum(false, false);
    let mut right = accounted_datum(true, false);
    right.a.random_state = left.a.random_state.clone();
    right.source.hash =
        stable_hash_provider::hash_produce(right.source.channel_hash.bytes(), &right.a, false);
    assert!(matches!(
        merge_outputs(
            datum(10, None, None),
            &[left, right],
            5,
            MergeType::IntegerAdd
        ),
        Err(HistoryError::MergeError(_))
    ));
}

#[test]
fn numeric_merge_malformed_encoding_returns_checked_error() {
    assert!(matches!(
        merge_changes(
            vec![],
            ChannelChange {
                added: vec![vec![255]],
                removed: vec![]
            },
            0,
            MergeType::IntegerAdd
        ),
        Err(HistoryError::MergeError(_))
    ));
}

#[test]
fn numeric_merge_zero_difference_does_not_restore_consumed_authority() {
    for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
        let original = accounted_datum(false, false);
        let replacement = datum(15, Some(CostAuthority::default()), None);
        let merged =
            merge_outputs(original, std::slice::from_ref(&replacement), 0, merge_type).unwrap();
        assert_eq!(merged.a, replacement.a);
    }
}

#[test]
fn numeric_merge_netted_intermediate_authority_does_not_return() {
    let base = datum(10, None, None);
    let intermediate = accounted_datum(false, false);
    let final_output = datum(15, Some(CostAuthority::default()), None);
    let changes = ChannelChange {
        added: vec![
            serializers::encode_datum(&intermediate),
            serializers::encode_datum(&final_output),
        ],
        removed: vec![
            serializers::encode_datum(&base),
            serializers::encode_datum(&intermediate),
        ],
    }
    .normalized();
    let merged = merge_changes(vec![base], changes, 5, MergeType::IntegerAdd).unwrap();
    assert_eq!(merged.a, final_output.a);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn numeric_merge_preserves_compound_quote_and_name_metadata(
        descriptions in prop::collection::vec(
            (0_u8..4, prop::collection::vec(any::<u8>(), 1..257)), 1..17,
        ),
        bitmask in any::<bool>(),
    ) {
        use models::rhoapi::cost_signature::Value;
        let mut outputs = Vec::new();
        let mut combined = signature(true);
        for (index, (kind, payload)) in descriptions.iter().enumerate() {
            let value = match kind {
                0 => Value::Ground(payload.clone()),
                1 => Value::Unit(true),
                2 => Value::Quote(crate::rust::interpreter::rho_type::RhoString::create_par(hex::encode(payload))),
                _ => Value::Name(crate::rust::interpreter::rho_type::RhoString::create_par(hex::encode(payload))),
            };
            combined = compound_cost_signatures(&combined, &CostSignature { value: Some(value) }).unwrap();
            outputs.push(datum(1, Some(CostAuthority {
                regions: vec![cost_region(&combined, b"numeric-structured-metadata", index as u32).unwrap()],
            }), None));
        }
        let merge_type = if bitmask { MergeType::BitmaskOr } else { MergeType::IntegerAdd };
        let expected = CostAuthority {
            regions: outputs.iter().map(|output| {
                let region = &output.a.cost_authority.as_ref().unwrap().regions[0];
                (region.instance_id.clone(), region.clone())
            }).collect::<BTreeMap<_, _>>().into_values().collect(),
        };
        let merged = merge_outputs(datum(1, None, None), &outputs, 0, merge_type).unwrap();
        prop_assert_eq!(merged.a.cost_authority.as_ref(), Some(&expected));
        let input_bytes: usize = outputs.iter().map(|output| {
            bincode::serialize(output.a.cost_authority.as_ref().unwrap()).unwrap().len()
        }).sum();
        prop_assert!(bincode::serialize(&expected).unwrap().len() <= input_bytes);
        let mut reversed = outputs;
        reversed.reverse();
        let reordered = merge_outputs(datum(1, None, None), &reversed, 0, merge_type).unwrap();
        prop_assert_eq!(reordered.a, merged.a);
    }

    #[test]
    fn numeric_merge_replacement_history_does_not_grow_live_authority(
        widths in prop::collection::vec(1_usize..17, 1..33),
        duplicates in 1_usize..5,
        bitmask in any::<bool>(),
    ) {
        let merge_type = if bitmask { MergeType::BitmaskOr } else { MergeType::IntegerAdd };
        let mut current = datum(1, None, None);
        for (round, width) in widths.into_iter().enumerate() {
            let outputs: Vec<_> = (0..width).map(|index| datum(1, Some(CostAuthority {
                regions: vec![cost_region(
                    &signature(index % 2 == 0),
                    b"numeric-replacement-history",
                    (round * 16 + index) as u32,
                ).unwrap()],
            }), None)).collect();
            let expected = merge_authorities(outputs.iter().map(|output| {
                output.a.cost_authority.as_ref().unwrap()
            })).unwrap();
            let carriers: Vec<_> = (0..duplicates).flat_map(|_| outputs.iter().cloned()).collect();
            current = merge_outputs(current, &carriers, 0, merge_type).unwrap();
            let authority = current.a.cost_authority.as_ref().unwrap();
            prop_assert_eq!(authority, &expected);
            prop_assert_eq!(authority.regions.len(), width);
            let input_bytes: usize = outputs.iter().map(|output| {
                bincode::serialize(output.a.cost_authority.as_ref().unwrap()).unwrap().len()
            }).sum();
            prop_assert!(bincode::serialize(authority).unwrap().len() <= input_bytes);
        }
    }

    #[test]
    fn numeric_merge_funding_conserves_each_wallet_and_rejects_one_unit_short(
        wallet_ids in prop::collection::vec(1_u8..17, 1..17),
        byte_amount in 1_u64..4097,
        bitmask in any::<bool>(),
    ) {
        let outputs: Vec<_> = wallet_ids.iter().enumerate().map(|(index, wallet)| {
            let signature = sig_to_cost_signature(&Sig::Ground(vec![*wallet; 32])).unwrap();
            datum(index as i64 + 1, Some(CostAuthority {
                regions: vec![cost_region(&signature, b"numeric-funding-property", index as u32).unwrap()],
            }), None)
        }).collect();
        let merge_type = if bitmask { MergeType::BitmaskOr } else { MergeType::IntegerAdd };
        let merged = merge_outputs(datum(0, None, None), &outputs, 1, merge_type).unwrap();
        let authority = merged.a.cost_authority.unwrap();
        let mut required = ResourceMultiset::default();
        for output in &outputs {
            required = required.checked_add(&authority_demand(output.a.cost_authority.as_ref().unwrap()).unwrap()).unwrap();
        }
        prop_assert_eq!(authority_demand(&authority).unwrap(), required.clone());
        let byte_required = ResourceMultiset(required.0.iter().map(|(key, units)| (*key, units * byte_amount)).collect());
        let capacity = required.checked_add(&byte_required).unwrap();
        let before = capacity.clone();
        let events = [AuthorityEvent { event_id: [0xe1; 32], authority: authority.clone(), debit: required.clone() }];
        let bytes = [AuthorityByteEvent { event_id: [0xe2; 32], kind: AuthorityByteEventKind::Comm, authority, amount: byte_amount }];
        let compute_draw = allocate_authority_events(&events, &capacity).unwrap();
        prop_assert_eq!(&compute_draw, &required);
        let after_compute = capacity.checked_sub(&compute_draw).unwrap();
        let byte_draw = allocate_quantitative_events(&bytes, &after_compute).unwrap();
        prop_assert_eq!(&byte_draw, &byte_required);
        prop_assert!(after_compute.checked_sub(&byte_draw).unwrap().0.is_empty());
        for key in capacity.0.keys() {
            let mut short = capacity.clone();
            *short.0.get_mut(key).unwrap() -= 1;
            let remaining = short.checked_sub(&compute_draw).unwrap();
            prop_assert_eq!(allocate_quantitative_events(&bytes, &remaining), Err(AuthorityError::InsufficientAuthority));
        }
        prop_assert_eq!(capacity, before);
    }

    #[test]
    fn numeric_merge_authority_matches_distinct_retained_identities(
        writers in prop::collection::vec(prop::collection::btree_set(0_u8..32, 0..12), 1..17),
        bitmask in any::<bool>(),
    ) {
        let merge_type = if bitmask { MergeType::BitmaskOr } else { MergeType::IntegerAdd };
        let outputs: Vec<_> = writers.iter().map(|regions| datum(15, Some(CostAuthority {
            regions: regions.iter().map(|id| models::rhoapi::CostRegion {
                instance_id: vec![*id; 32],
                signature: Some(signature(id % 2 == 0)),
            }).collect(),
        }), None)).collect();
        let expected: BTreeSet<_> = writers.iter().flat_map(|regions| regions.iter().copied()).collect();
        let base = datum(10, None, None);
        let merged = merge_outputs(base.clone(), &outputs, 5, merge_type).unwrap();
        let actual = merged.a.cost_authority.as_ref().unwrap();
        prop_assert_eq!(actual.regions.len(), expected.len());
        prop_assert_eq!(actual.regions.iter().map(|r| r.instance_id[0]).collect::<Vec<_>>(), expected.iter().copied().collect::<Vec<_>>());
        prop_assert_eq!(authority_demand(actual).unwrap().0.values().sum::<u64>(), expected.iter().filter(|id| **id % 2 != 0).count() as u64);
        let input_bytes: usize = outputs.iter().map(|output| {
            bincode::serialize(output.a.cost_authority.as_ref().unwrap()).unwrap().len()
        }).sum();
        prop_assert!(bincode::serialize(actual).unwrap().len() <= input_bytes);
        if outputs.len() > 1 {
            let split = outputs.len() / 2;
            let left = merge_outputs(base.clone(), &outputs[..split], 5, merge_type).unwrap();
            let right = merge_outputs(base.clone(), &outputs[split..], 5, merge_type).unwrap();
            let grouped = merge_outputs(base.clone(), &[left, right], 5, merge_type).unwrap();
            prop_assert_eq!(grouped.a.cost_authority.as_ref(), Some(actual));
        }
        let mut reordered = outputs.clone();
        reordered.reverse();
        let reverse = merge_outputs(base.clone(), &reordered, 5, merge_type).unwrap();
        prop_assert_eq!(&merged.a, &reverse.a);
        reordered.extend(outputs);
        let duplicate = merge_outputs(base, &reordered, 5, merge_type).unwrap();
        prop_assert_eq!(&merged.a, &duplicate.a);
        prop_assert_eq!(merged.source.hash, duplicate.source.hash);
    }
}

#[test]
fn numeric_merge_wide_authority_preserves_live_region_bound() {
    for width in [256, 1024] {
        let outputs: Vec<_> = (0..width)
            .map(|index| {
                datum(
                    1,
                    Some(CostAuthority {
                        regions: vec![cost_region(
                            &signature(index % 2 == 0),
                            b"numeric-wide-authority",
                            index as u32,
                        )
                        .unwrap()],
                    }),
                    None,
                )
            })
            .collect();
        let input_bytes: usize = outputs
            .iter()
            .map(|output| {
                bincode::serialize(output.a.cost_authority.as_ref().unwrap())
                    .unwrap()
                    .len()
            })
            .sum();
        for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
            let merged = merge_outputs(datum(1, None, None), &outputs, 0, merge_type).unwrap();
            let authority = merged.a.cost_authority.unwrap();
            assert_eq!(authority.regions.len(), width);
            assert!(bincode::serialize(&authority).unwrap().len() <= input_bytes);
            assert_eq!(
                authority_demand(&authority)
                    .unwrap()
                    .0
                    .values()
                    .sum::<u64>(),
                (width / 2) as u64
            );
        }
    }
}

#[test]
fn numeric_merge_single_writer_retains_unit_authority() {
    for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
        let original = accounted_datum(true, false);
        let merged = merge_single(&original, merge_type);
        assert_eq!(merged.a, original.a);
        assert_eq!(merged.source.hash, original.source.hash);
    }
}

#[test]
fn numeric_merge_single_writer_retains_funding_demand() {
    for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
        let original = accounted_datum(false, false);
        let merged = merge_single(&original, merge_type);
        let before = authority_demand(original.a.cost_authority.as_ref().unwrap()).unwrap();
        let after = authority_demand(&merged.a.cost_authority.unwrap_or_default()).unwrap();
        assert_ne!(before, Default::default());
        assert_eq!(after, before);
    }
}

#[test]
fn numeric_merge_single_writer_retains_following_comm_byte_charge() {
    for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
        for unit in [true, false] {
            let original = accounted_datum(unit, false);
            let merged = merge_single(&original, merge_type);
            let comm = COMM {
                consume: Consume::create(
                    &vec!["channel"],
                    &vec!["pattern"],
                    &"continuation",
                    false,
                ),
                produces: vec![original.source.clone()],
                peeks: BTreeSet::new(),
                times_repeated: BTreeMap::new(),
            };
            let before = comm_charge(&comm, &[(&original.a, false)]).unwrap();
            let after = comm_charge(&comm, &[(&merged.a, false)]).unwrap();
            assert_eq!(after, before);
        }
    }
}

#[test]
fn numeric_merge_single_writer_retains_unaccounted_datum() {
    for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
        let original = datum(15, None, None);
        let merged = merge_single(&original, merge_type);
        assert_eq!(merged.a, original.a);
        assert_eq!(merged.source.hash, original.source.hash);
    }
}

#[tokio::test]
async fn numeric_merge_following_native_comm_and_replay_preserve_accounting() {
    use models::rust::utils::new_gstring_par;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::rho_runtime::RhoRuntime;
    use crate::rust::interpreter::test_utils::resources::create_runtimes;

    for merge_type in [MergeType::IntegerAdd, MergeType::BitmaskOr] {
        for (output_unit, consumer_unit) in [(true, true), (true, false), (false, false)] {
            let original = accounted_datum(output_unit, false);
            let merged = merge_single(&original, merge_type);
            let mut results = Vec::new();
            for value in [original.a, merged.a] {
                let mut manager = InMemoryStoreManager::new();
                let stores = manager.r_space_stores().await.unwrap();
                let (mut play, mut replay, _) =
                    create_runtimes(stores, false, &mut Vec::new()).await;
                let channel =
                    new_gstring_par("numeric-following-comm".to_string(), Vec::new(), false);
                assert!(play
                    .reducer
                    .space
                    .produce(channel, value, false)
                    .await
                    .unwrap()
                    .is_none());
                let pre = play.create_checkpoint().await;
                let consumer = if consumer_unit {
                    Sig::Unit
                } else {
                    Sig::Ground(vec![21; 32])
                };
                play.cost
                    .set_deploy_signature_funded(b"numeric-following-consumer", consumer.clone());
                let source =
                    r#"for(@value <- @"numeric-following-comm") { @"numeric-result"!(value) }"#;
                let seed = Blake2b512Random::create_from_bytes(b"numeric-following-comm-seed");
                let executed = play
                    .evaluate(
                        source,
                        Cost::create(1_000_000, "numeric follow-on"),
                        Default::default(),
                        seed.clone(),
                    )
                    .await
                    .unwrap();
                assert!(executed.errors.is_empty(), "{:?}", executed.errors);
                assert_eq!(executed.authority_events.len(), 1);
                let checkpoint = play.create_checkpoint().await;
                replay.reset(&pre.root).await.unwrap();
                replay.rig(checkpoint.log.clone()).await.unwrap();
                replay
                    .cost
                    .set_deploy_signature_funded(b"numeric-following-consumer", consumer);
                let replayed = replay
                    .evaluate(
                        source,
                        Cost::create(1_000_000, "numeric follow-on"),
                        Default::default(),
                        seed,
                    )
                    .await
                    .unwrap();
                assert!(replayed.errors.is_empty(), "{:?}", replayed.errors);
                replay.check_replay_data().await.unwrap();
                assert_eq!(replayed.authority_events, executed.authority_events);
                assert_eq!(
                    replayed.authority_byte_events,
                    executed.authority_byte_events
                );
                assert_eq!(replayed.cost, executed.cost);
                assert_eq!(replay.create_checkpoint().await.root, checkpoint.root);
                results.push((
                    executed.authority_events,
                    executed.authority_byte_events,
                    executed.cost,
                    checkpoint.root,
                ));
            }
            assert_eq!(results[0], results[1]);
        }
    }
}
