use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use rspace_plus_plus::rspace::candidate_order::deterministic_candidate_hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rho_path::{
    RhoPathQuery, RhoPathQueryAtom, encode_structural_sequence,
};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
enum Pattern {
    #[default]
    Wildcard,
    Exact(String),
    SequenceSecond {
        expected: String,
        second: Vec<u8>,
    },
}

impl StableHashSerialize for Pattern {}

impl ColdStoreDecode for Pattern {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

#[derive(Clone)]
struct CountingMatcher {
    pruning: bool,
    calls: Arc<AtomicUsize>,
    path_queries: Arc<AtomicUsize>,
}

impl Match<Pattern, String, String> for CountingMatcher {
    fn get(&self, pattern: &Pattern, datum: &String) -> Option<String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match pattern {
            Pattern::Wildcard => Some(datum.clone()),
            Pattern::Exact(expected) if expected == datum => Some(datum.clone()),
            Pattern::SequenceSecond { expected, .. } if expected == datum => Some(datum.clone()),
            Pattern::Exact(_) | Pattern::SequenceSecond { .. } => None,
        }
    }

    fn retained_pattern_path(&self, pattern: &Pattern) -> Option<Vec<u8>> {
        self.path_queries.fetch_add(1, Ordering::Relaxed);
        match pattern {
            Pattern::Wildcard => None,
            Pattern::Exact(value) => Some(value.as_bytes().to_vec()),
            Pattern::SequenceSecond { .. } => None,
        }
    }

    fn retained_datum_path(&self, datum: &String) -> Option<Vec<u8>> {
        self.path_queries.fetch_add(1, Ordering::Relaxed);
        Some(datum.as_bytes().to_vec())
    }

    fn rho_path_query(&self, pattern: &Pattern) -> Option<RhoPathQuery> {
        match pattern {
            Pattern::SequenceSecond { second, .. } => Some(RhoPathQuery::structural_sequence(
                [RhoPathQueryAtom::Wildcard, RhoPathQueryAtom::Exact(second.clone())],
                false,
            )),
            Pattern::Wildcard | Pattern::Exact(_) => None,
        }
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool { self.pruning }
}

fn sequence_value(first: &[u8], second: &[u8]) -> String {
    String::from_utf8(encode_structural_sequence([first, second]))
        .expect("ASCII structural sequence")
}

type TestSpace = RSpace<String, Pattern, String, String>;

async fn space(pruning: bool) -> (TestSpace, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.expect("in-memory stores");
    let calls = Arc::new(AtomicUsize::new(0));
    let path_queries = Arc::new(AtomicUsize::new(0));
    let matcher = CountingMatcher {
        pruning,
        calls: Arc::clone(&calls),
        path_queries: Arc::clone(&path_queries),
    };
    let space = RSpace::create(stores, Arc::new(Box::new(matcher))).expect("RSpace");
    (space, calls, path_queries)
}

fn last_canonical_value(channel: &str, values: &[String]) -> String {
    values
        .iter()
        .max_by_key(|value| {
            deterministic_candidate_hash(&Datum::create(
                &channel.to_string(),
                (*value).clone(),
                true,
            ))
        })
        .expect("nonempty corpus")
        .clone()
}

async fn populate(space: &TestSpace, channel: &str, values: &[String]) {
    for value in values {
        assert!(
            space
                .produce(channel.to_string(), value.clone(), true)
                .await
                .expect("produce")
                .is_none()
        );
    }
    drop(space.take_event_log().await);
}

async fn consume_exact(space: &TestSpace, channel: &str, target: &str) -> String {
    let (_, data) = space
        .consume(
            vec![channel.to_string()],
            vec![Pattern::Exact(target.to_string())],
            "continuation".to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("consume")
        .expect("resting datum must match");
    assert_eq!(data.len(), 1);
    data.into_iter().next().expect("one result").matched_datum
}

#[tokio::test]
async fn pathmap_prefix_prunes_before_decode_and_spatial_match() {
    let channel = "candidate-path-pruning";
    let values = (0..256)
        .map(|index| format!("record/{index:08x}"))
        .collect::<Vec<_>>();
    let target = last_canonical_value(channel, &values);

    let (control, control_calls, _) = space(false).await;
    populate(&control, channel, &values).await;
    control_calls.store(0, Ordering::Relaxed);
    assert_eq!(consume_exact(&control, channel, &target).await, target);

    let (indexed, indexed_calls, _) = space(true).await;
    populate(&indexed, channel, &values).await;
    indexed_calls.store(0, Ordering::Relaxed);
    assert_eq!(consume_exact(&indexed, channel, &target).await, target);

    assert_eq!(control_calls.load(Ordering::Relaxed), values.len());
    assert_eq!(indexed_calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn fresh_zero_byte_path_survives_the_same_prefix_contract() {
    let (space, calls, _) = space(true).await;
    let channel = "fresh-path".to_string();
    let target = "fresh\0datum".to_string();
    assert!(
        space
            .consume(
                vec![channel.clone()],
                vec![Pattern::Exact(target.clone())],
                "persistent-continuation".to_string(),
                true,
                BTreeSet::new(),
            )
            .await
            .expect("consume")
            .is_none()
    );
    calls.store(0, Ordering::Relaxed);
    let (_, data, _) = space
        .produce(channel, target.clone(), false)
        .await
        .expect("produce")
        .expect("fresh datum must match the waiting receive");
    assert_eq!(data[0].matched_datum, target);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn prefix_false_positives_preserve_the_spatial_oracle() {
    let (space, calls, _) = space(true).await;
    let channel = "prefix-false-positive";
    let values = vec!["ab-suffix".to_string(), "ab".to_string()];
    populate(&space, channel, &values).await;
    calls.store(0, Ordering::Relaxed);
    assert_eq!(consume_exact(&space, channel, "ab").await, "ab");
    assert!((1..=2).contains(&calls.load(Ordering::Relaxed)));
}

#[tokio::test]
async fn singleton_pool_uses_the_unprunable_direct_scan() {
    let (space, calls, path_queries) = space(true).await;
    let channel = "singleton-direct-scan";
    let target = "only-candidate";
    populate(&space, channel, &[target.to_string()]).await;
    calls.store(0, Ordering::Relaxed);
    path_queries.store(0, Ordering::Relaxed);

    assert_eq!(consume_exact(&space, channel, target).await, target);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        path_queries.load(Ordering::Relaxed),
        0,
        "a singleton pool cannot benefit from a retained-path query"
    );
}

#[tokio::test]
async fn pathmap_product_prunes_after_a_wildcard_frame_without_decoding_candidates() {
    let channel = "candidate-path-product";
    let records = (0..256)
        .map(|index| {
            let first = format!("record/{index:08x}");
            let second = format!("class/{index:08x}").into_bytes();
            (sequence_value(first.as_bytes(), &second), second)
        })
        .collect::<Vec<_>>();
    let values = records
        .iter()
        .map(|(value, _)| value.clone())
        .collect::<Vec<_>>();
    let target = last_canonical_value(channel, &values);
    let target_second = records
        .iter()
        .find(|(value, _)| value == &target)
        .expect("target belongs to corpus")
        .1
        .clone();

    let (control, control_calls, _) = space(false).await;
    populate(&control, channel, &values).await;
    control_calls.store(0, Ordering::Relaxed);
    let control_result = control
        .consume(
            vec![channel.to_string()],
            vec![Pattern::SequenceSecond {
                expected: target.clone(),
                second: target_second.clone(),
            }],
            "continuation".to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("control consume")
        .expect("control match");

    let (indexed, indexed_calls, _) = space(true).await;
    populate(&indexed, channel, &values).await;
    indexed_calls.store(0, Ordering::Relaxed);
    let indexed_result = indexed
        .consume(
            vec![channel.to_string()],
            vec![Pattern::SequenceSecond {
                expected: target,
                second: target_second,
            }],
            "continuation".to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("indexed consume")
        .expect("indexed match");

    assert_eq!(indexed_result, control_result);
    assert_eq!(control_calls.load(Ordering::Relaxed), values.len());
    assert_eq!(indexed_calls.load(Ordering::Relaxed), 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn indexed_and_full_scans_have_identical_results_traces_and_post_states(
        values in prop::collection::vec("[a-z0-9]{0,8}", 0..12),
        target in "[a-z0-9]{0,8}",
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Tokio runtime");
        runtime.block_on(async {
            let channel = "generated-differential";
            let (control, _, _) = space(false).await;
            let (indexed, _, _) = space(true).await;
            populate(&control, channel, &values).await;
            populate(&indexed, channel, &values).await;

            let control_result = control
                .consume(
                    vec![channel.to_string()],
                    vec![Pattern::Exact(target.clone())],
                    "continuation".to_string(),
                    false,
                    BTreeSet::new(),
                )
                .await
                .expect("control consume");
            let indexed_result = indexed
                .consume(
                    vec![channel.to_string()],
                    vec![Pattern::Exact(target)],
                    "continuation".to_string(),
                    false,
                    BTreeSet::new(),
                )
                .await
                .expect("indexed consume");

            prop_assert_eq!(control_result, indexed_result);
            prop_assert_eq!(control.to_map().await, indexed.to_map().await);
            prop_assert_eq!(control.take_event_log().await, indexed.take_event_log().await);
            Ok(())
        })?;
    }

    #[test]
    fn framed_product_queries_preserve_results_traces_and_post_states(
        pairs in prop::collection::vec(("[a-z0-9]{0,4}", "[a-z0-9]{0,4}"), 0..12),
        target in ("[a-z0-9]{0,4}", "[a-z0-9]{0,4}"),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Tokio runtime");
        runtime.block_on(async {
            let channel = "generated-product-differential";
            let values = pairs
                .iter()
                .map(|(first, second)| sequence_value(first.as_bytes(), second.as_bytes()))
                .collect::<Vec<_>>();
            let expected = sequence_value(target.0.as_bytes(), target.1.as_bytes());
            let pattern = Pattern::SequenceSecond {
                expected,
                second: target.1.into_bytes(),
            };
            let (control, _, _) = space(false).await;
            let (indexed, _, _) = space(true).await;
            populate(&control, channel, &values).await;
            populate(&indexed, channel, &values).await;

            let control_result = control
                .consume(
                    vec![channel.to_string()],
                    vec![pattern.clone()],
                    "continuation".to_string(),
                    false,
                    BTreeSet::new(),
                )
                .await
                .expect("control consume");
            let indexed_result = indexed
                .consume(
                    vec![channel.to_string()],
                    vec![pattern],
                    "continuation".to_string(),
                    false,
                    BTreeSet::new(),
                )
                .await
                .expect("indexed consume");

            prop_assert_eq!(control_result, indexed_result);
            prop_assert_eq!(control.to_map().await, indexed.to_map().await);
            prop_assert_eq!(control.take_event_log().await, indexed.take_event_log().await);
            Ok(())
        })?;
    }
}
