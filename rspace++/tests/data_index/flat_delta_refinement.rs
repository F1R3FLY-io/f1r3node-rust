use std::collections::BTreeMap;
use std::sync::Arc;

use proptest::prelude::*;

use super::{RSpaceDataIndex, RSpaceDataStore};
use crate::rspace::bag::{BagDelta, Multiplicity};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::internal::{ConsumeCandidate, Datum, DatumAddress, stored_datum_removal_delta};
use crate::rspace::rho_path::RhoPathV1;
use crate::rspace::trace::event::Produce;

type LogicalKey = (usize, usize);
type Oracle = BTreeMap<LogicalKey, i128>;

struct Fixture {
    channels: Vec<String>,
    data: Vec<Datum<String>>,
    paths: BTreeMap<LogicalKey, Vec<u8>>,
}

impl Fixture {
    fn new() -> Self {
        let channels = ["a", "a\0", "ab", "ab\0"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let data = ["v", "v\0", "vv"]
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let label = u8::try_from(index).unwrap();
                Datum {
                    a: Arc::new(value.to_owned()),
                    persist: false,
                    source: Produce::new(
                        Blake2b256Hash::new(&[label]),
                        Blake2b256Hash::new(&[label, 17]),
                        false,
                    ),
                }
            })
            .collect::<Vec<_>>();
        let mut paths = BTreeMap::new();
        for (channel_index, channel) in channels.iter().enumerate() {
            for (datum_index, datum) in data.iter().enumerate() {
                paths.insert(
                    (channel_index, datum_index),
                    RSpaceDataIndex::datum_path(channel, datum),
                );
            }
        }
        Self {
            channels,
            data,
            paths,
        }
    }

    fn store(&self) -> RSpaceDataStore<String, String> {
        let mut store = RSpaceDataStore::new();
        for channel in &self.channels {
            store
                .set_channel(
                    channel.clone(),
                    self.data
                        .iter()
                        .flat_map(|datum| std::iter::repeat_n(datum.clone(), 4)),
                )
                .unwrap();
            store.channel_pool(channel).unwrap().unwrap();
        }
        store
    }

    fn agrees(&self, delta: &BagDelta, oracle: &Oracle) -> bool {
        let mut positive = 0;
        let mut negative = 0;
        for (key, path) in &self.paths {
            let net = oracle.get(key).copied().unwrap_or(0);
            let addition = u64::try_from(net.max(0)).unwrap();
            let removal = u64::try_from((-net).max(0)).unwrap();
            if delta.additions().count(path) != addition || delta.removals().count(path) != removal
            {
                return false;
            }
            positive += usize::from(addition != 0);
            negative += usize::from(removal != 0);
        }
        delta.additions().distinct_len() == positive && delta.removals().distinct_len() == negative
    }

    fn assert_application(&self, delta: &BagDelta, oracle: &Oracle) {
        let mut store = self.store();
        let original = store.index().clone();
        let original_anchor = store.index().outer_anchor_identity();
        let original_pending = store.pending_delta().clone();
        let held_pools = self
            .channels
            .iter()
            .map(|channel| store.channel_pool(channel).unwrap().unwrap())
            .collect::<Vec<_>>();
        let applicable = self.paths.keys().all(|key| {
            let after = 4 + oracle.get(key).copied().unwrap_or(0);
            (0..=i128::from(u64::MAX)).contains(&after)
        });
        let result = store.apply_delta(delta);
        assert_eq!(result.is_ok(), applicable, "TLRS-DELTA trace applicability");
        if applicable {
            let mut total = 0u128;
            let mut distinct = 0;
            for (key, path) in &self.paths {
                let expected = u64::try_from(4 + oracle.get(key).copied().unwrap_or(0)).unwrap();
                assert_eq!(store.index().count_path(path), expected, "grouped count at {key:?}");
                total += u128::from(expected);
                distinct += usize::from(expected != 0);
            }
            assert_eq!(store.index().exact_cardinality, total);
            assert_eq!(store.distinct_len(), distinct);
            assert_eq!(store.loaded_channel_count(), self.channels.len());
        } else {
            assert_eq!(store.index(), &original);
            assert_eq!(store.index().outer_anchor_identity(), original_anchor);
            assert_eq!(store.pending_delta(), &original_pending);
        }
        for (channel_index, pool) in held_pools.iter().enumerate() {
            assert_eq!(pool.len(), self.data.len());
            for position in 0..pool.len() {
                let (datum, address) = pool.candidate(position).unwrap();
                let DatumAddress::Stored(absolute) = address else {
                    panic!("a held stored catalog cannot become Fresh");
                };
                let view = RhoPathV1::parse_datum(&absolute).unwrap();
                assert_eq!(
                    pool.stored_availability()
                        .count(&absolute[view.channel_prefix_len()..]),
                    4
                );
                assert_eq!(
                    absolute.as_ref(),
                    RSpaceDataIndex::datum_path(&self.channels[channel_index], datum)
                );
            }
        }
    }
}

fn record(delta: &mut BagDelta, path: &[u8], addition: bool, amount: u64) {
    let amount = Multiplicity::new(amount).unwrap();
    if addition {
        delta.record_addition(path, amount).unwrap();
    } else {
        delta.record_removal(path, amount).unwrap();
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn flat_delta_operation_traces_refine_independent_logical_counts(
        operations in proptest::collection::vec((0usize..4, 0usize..3, any::<bool>(), 1u64..17), 0..65),
    ) {
        let fixture = Fixture::new();
        let mut delta = BagDelta::new();
        let mut oracle = Oracle::new();
        for (channel, datum, addition, amount) in operations {
            let key = (channel, datum);
            record(&mut delta, &fixture.paths[&key], addition, amount);
            let change = if addition { i128::from(amount) } else { -i128::from(amount) };
            *oracle.entry(key).or_default() += change;
            prop_assert!(fixture.agrees(&delta, &oracle), "TLRS-DELTA normalization and off-key laws");
        }
        fixture.assert_application(&delta, &oracle);
        for key in fixture.paths.keys() {
            let net = oracle.get(key).copied().unwrap_or(0);
            if net < 0 {
                record(&mut delta, &fixture.paths[key], true, u64::try_from(-net).unwrap());
                oracle.insert(*key, 0);
                prop_assert!(fixture.agrees(&delta, &oracle), "compensating trace retains separate normalized supports");
            }
        }
        fixture.assert_application(&delta, &oracle);
    }
}

#[test]
fn flat_delta_application_preserves_full_u64_counts_without_expansion() {
    let fixture = Fixture::new();
    let amount = u64::MAX - 4;
    let oracle = Oracle::from([((0, 0), i128::from(amount)), ((1, 1), i128::from(amount))]);
    let mut delta = BagDelta::new();
    for key in oracle.keys() {
        record(&mut delta, &fixture.paths[key], true, amount);
    }
    assert!(fixture.agrees(&delta, &oracle));
    fixture.assert_application(&delta, &oracle);
}

#[test]
fn flat_delta_judge_rejects_channel_polarity_cancellation_and_omission_mutants() {
    let fixture = Fixture::new();
    let oracle = Oracle::from([((0, 0), 2), ((1, 1), 4), ((2, 2), -1)]);
    let mut baseline = BagDelta::new();
    for (key, add, amount) in
        [((0, 0), true, 5), ((0, 0), false, 3), ((1, 1), true, 4), ((2, 2), false, 1)]
    {
        record(&mut baseline, &fixture.paths[&key], add, amount);
    }
    assert!(fixture.agrees(&baseline, &oracle));
    fixture.assert_application(&baseline, &oracle);
    assert!(!fixture.agrees(&baseline.inverse(), &oracle), "polarity swap");
    for (label, moved, first, omit) in [
        ("wrong channel", (1, 0), 2, false),
        ("skipped cancellation", (0, 0), 5, false),
        ("doubled cancellation", (0, 0), -1, false),
        ("omitted partition", (0, 0), 2, true),
    ] {
        let mut mutant = BagDelta::new();
        record(
            &mut mutant,
            &fixture.paths[&moved],
            first > 0,
            u64::try_from(i64::abs(first)).unwrap(),
        );
        if !omit {
            record(&mut mutant, &fixture.paths[&(1, 1)], true, 4);
        }
        record(&mut mutant, &fixture.paths[&(2, 2)], false, 1);
        assert!(!fixture.agrees(&mutant, &oracle), "{label}");
    }
    let mut wrong_offset = baseline.clone();
    record(&mut wrong_offset, &fixture.paths[&(0, 0)][1..], true, 1);
    assert!(!fixture.agrees(&wrong_offset, &oracle), "shifted absolute key");
}

#[test]
fn flat_delta_record_overflow_preserves_both_supports() {
    let fixture = Fixture::new();
    for addition in [false, true] {
        let mut delta = BagDelta::new();
        record(&mut delta, &fixture.paths[&(0, 0)], addition, u64::MAX);
        record(&mut delta, &fixture.paths[&(1, 1)], !addition, 7);
        let before = delta.clone();
        let result = if addition {
            delta.record_addition(&fixture.paths[&(0, 0)], Multiplicity::ONE)
        } else {
            delta.record_removal(&fixture.paths[&(0, 0)], Multiplicity::ONE)
        };
        assert!(result.is_err());
        assert_eq!(delta, before, "record overflow must preserve both sides");
        record(&mut delta, &fixture.paths[&(0, 0)], !addition, u64::MAX);
        assert_eq!(delta.additions().count(&fixture.paths[&(0, 0)]), 0);
        assert_eq!(delta.removals().count(&fixture.paths[&(0, 0)]), 0);
    }
}

#[test]
fn valid_prefix_then_malformed_delta_preserves_published_authority() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    let before = store.index().clone();
    let anchor = store.index().outer_anchor_identity();
    let pending = store.pending_delta().clone();
    let mut delta = BagDelta::new();
    record(&mut delta, &fixture.paths[&(0, 0)], true, 1);
    record(&mut delta, &[255], true, 1);
    let mut visited = Vec::new();
    delta
        .additions()
        .visit_prefix(b"", |path, _| visited.push(path.to_vec()));
    assert_eq!(visited, vec![fixture.paths[&(0, 0)].clone(), vec![255]]);
    assert!(store.apply_delta(&delta).is_err());
    assert_eq!(store.index(), &before);
    assert_eq!(store.index().outer_anchor_identity(), anchor);
    assert_eq!(store.pending_delta(), &pending);
}

#[test]
fn fresh_catalog_shift_preserves_absolute_stored_removal_addresses() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let channel = &fixture.channels[0];
    let mut pool = store.channel_pool(channel).unwrap().unwrap();
    let original = (0..pool.len())
        .map(|position| pool.candidate(position).unwrap().1)
        .collect::<Vec<_>>();
    assert!(pool.prepend_fresh(fixture.data[0].clone()));
    assert_eq!(pool.candidate(0).unwrap().1, DatumAddress::Fresh);
    for (position, expected) in original.iter().enumerate() {
        assert_eq!(&pool.candidate(position + 1).unwrap().1, expected);
        assert!(pool.reserve(position + 1));
        assert_eq!(&pool.candidate(position + 1).unwrap().1, expected);
    }
    let (selected, address) = pool.candidate(1).unwrap();
    let candidates = vec![
        ConsumeCandidate {
            channel: channel.clone(),
            datum: selected.clone(),
            removed_datum: Arc::clone(&selected.a),
            address: address.clone(),
        },
        ConsumeCandidate {
            channel: channel.clone(),
            datum: fixture.data[0].clone(),
            removed_datum: Arc::clone(&fixture.data[0].a),
            address: DatumAddress::Fresh,
        },
    ];
    let delta = stored_datum_removal_delta(&candidates).unwrap();
    let DatumAddress::Stored(absolute) = address else {
        panic!("stored address expected");
    };
    assert_eq!(delta.removals().count(&absolute), 1);
    assert_eq!(delta.removals().distinct_len(), 1);
    let mut ordinal_mutant = BagDelta::new();
    record(&mut ordinal_mutant, &1u64.to_be_bytes(), false, 1);
    assert_ne!(ordinal_mutant.removals().count(&absolute), 1);
}
