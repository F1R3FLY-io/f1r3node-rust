//! Added by G3 (DR-135, S5): the owner checks of the play matcher (plan S5,
//! rules R5 and R6).
//!
//! A test observer stands for owner funding. It fails the item test of each
//! datum that it lists as unfunded, and it refuses the COMM of each tuple
//! that it lists as refused, as the recorder refuses a COMM whose owners
//! cannot pay. A pattern is a prefix, so a datum matches each pattern that
//! starts it.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use proptest::prelude::*;

use crate::rspace::errors::RSpaceError;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::r#match::Match;
use crate::rspace::operation_context::MatchAdmission;
use crate::rspace::rspace::RSpace;
use crate::rspace::rspace_interface::{ISpace, RSpaceAccountingObserver};
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use crate::rspace::space_matcher::SpaceMatcher;
use crate::rspace::trace::event::{COMM, Consume, Produce};

/// A datum matches each pattern that is a prefix of it.
struct Prefix;

impl Match<String, String, String> for Prefix {
    fn get(&self, pattern: &String, datum: &String) -> Option<String> {
        datum.starts_with(pattern.as_str()).then(|| datum.clone())
    }
}

/// The owner rules of a test: the data whose item test fails, the tuples
/// whose COMM is refused, and the tuples whose COMM was charged.
#[derive(Default)]
struct OwnerRules {
    active: bool,
    unfunded: Mutex<BTreeSet<String>>,
    refused: Mutex<BTreeSet<Vec<String>>>,
    charged: Mutex<Vec<Vec<String>>>,
}

impl RSpaceAccountingObserver<String, String, String, String> for OwnerRules {
    fn observe_produce(
        &self,
        _: &Produce,
        _: &String,
        _: &String,
        _: bool,
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn observe_consume(
        &self,
        _: &Consume,
        _: &[String],
        _: &[String],
        _: &String,
        _: bool,
        _: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn observe_comm(
        &self,
        _: &COMM,
        _: &String,
        _: bool,
        data: &[(&String, bool)],
    ) -> Result<(), RSpaceError> {
        let mut tuple: Vec<String> = data.iter().map(|(datum, _)| (*datum).clone()).collect();
        tuple.sort();
        if self.active &&
            self.refused
                .lock()
                .expect("refused tuples")
                .contains(&tuple)
        {
            return Err(RSpaceError::CommRefused);
        }
        self.charged.lock().expect("charged tuples").push(tuple);
        Ok(())
    }

    fn owner_checks_active(&self) -> bool { self.active }

    fn admit_item(
        &self,
        _: &String,
        _: bool,
        _: usize,
        datum: &String,
        _: bool,
    ) -> Result<bool, RSpaceError> {
        Ok(!self.unfunded.lock().expect("unfunded data").contains(datum))
    }
}

async fn space(rules: Arc<OwnerRules>) -> RSpace<String, String, String, String> {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.expect("in-memory stores");
    let space = RSpace::create(stores, Arc::new(Box::new(Prefix))).expect("an RSpace");
    space.set_accounting_observer(Some(rules));
    space
}

fn rules(active: bool, unfunded: &[&str], refused: &[&[&str]]) -> Arc<OwnerRules> {
    Arc::new(OwnerRules {
        active,
        unfunded: Mutex::new(unfunded.iter().map(|datum| datum.to_string()).collect()),
        refused: Mutex::new(
            refused
                .iter()
                .map(|tuple| tuple.iter().map(|datum| datum.to_string()).collect())
                .collect(),
        ),
        charged: Mutex::default(),
    })
}

async fn produce(space: &RSpace<String, String, String, String>, datum: &str) -> Option<String> {
    space
        .produce("c".to_string(), datum.to_string(), false)
        .await
        .expect("the produce runs")
        .map(|(_, data, _)| data[0].matched_datum.clone())
}

async fn consume(space: &RSpace<String, String, String, String>, body: &str) -> Option<String> {
    space
        .consume(
            vec!["c".to_string()],
            vec![String::new()],
            body.to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("the consume runs")
        .map(|(_, data)| data[0].matched_datum.clone())
}

async fn stored(space: &RSpace<String, String, String, String>) -> BTreeSet<String> {
    space
        .get_data(&"c".to_string())
        .await
        .into_iter()
        .map(|datum| datum.a)
        .collect()
}

async fn waiting(space: &RSpace<String, String, String, String>) -> Vec<String> {
    space
        .get_waiting_continuations(vec!["c".to_string()])
        .await
        .into_iter()
        .map(|waiting| waiting.continuation)
        .collect()
}

/// Fresh spaces for the random pick. Both data match, so a pick that never
/// takes one of them has probability `2^-64`.
const RUNS: usize = 64;

/// R5: a datum that fails its item test is a miss of the pattern, so the
/// consume takes a datum whose owners can pay. Without owner checks the
/// consume picks either datum, as on dev.
#[tokio::test]
async fn an_unfundable_datum_is_skipped_like_a_pattern_miss() {
    let mut without_checks = BTreeSet::new();
    for _ in 0..RUNS {
        let checked = space(rules(true, &["unfunded-a"], &[])).await;
        produce(&checked, "unfunded-a").await;
        produce(&checked, "funded-b").await;
        assert_eq!(consume(&checked, "body").await.as_deref(), Some("funded-b"));
        assert_eq!(stored(&checked).await, BTreeSet::from(["unfunded-a".to_string()]));
        let plain = space(rules(false, &["unfunded-a"], &[])).await;
        produce(&plain, "unfunded-a").await;
        produce(&plain, "funded-b").await;
        without_checks.insert(consume(&plain, "body").await.expect("a datum matches"));
    }
    assert_eq!(without_checks, BTreeSet::from(["funded-b".to_string(), "unfunded-a".to_string()]));
}

/// R5: a COMM whose owners cannot pay is refused. The produce then stores
/// its datum, and the consume waits, so both items stay stored.
#[tokio::test]
async fn a_refused_tuple_keeps_both_items_stored() {
    let rules = rules(true, &[], &[&["x"], &["y"]]);
    let refusing = space(Arc::clone(&rules)).await;
    assert_eq!(consume(&refusing, "first").await, None);
    assert_eq!(produce(&refusing, "x").await, None);
    assert_eq!(stored(&refusing).await, BTreeSet::from(["x".to_string()]));
    assert_eq!(waiting(&refusing).await, vec!["first".to_string()]);
    let refusing = space(Arc::clone(&rules)).await;
    assert_eq!(produce(&refusing, "y").await, None);
    assert_eq!(consume(&refusing, "second").await, None);
    assert_eq!(stored(&refusing).await, BTreeSet::from(["y".to_string()]));
    assert_eq!(waiting(&refusing).await, vec!["second".to_string()]);
    assert!(rules.charged.lock().expect("charged tuples").is_empty());
}

/// Stores the datum `old` and a continuation `body` on `c` with the owner
/// checks of `rules`: while `old` is unfunded, neither matches the other.
/// Then `old` is funded and `new` is not.
async fn own_datum_scenario(rules: Arc<OwnerRules>) -> RSpace<String, String, String, String> {
    rules
        .unfunded
        .lock()
        .expect("unfunded data")
        .insert("old".to_string());
    let space = space(Arc::clone(&rules)).await;
    assert_eq!(produce(&space, "old").await, None);
    assert_eq!(consume(&space, "body").await, None);
    let mut unfunded = rules.unfunded.lock().expect("unfunded data");
    unfunded.remove("old");
    unfunded.insert("new".to_string());
    drop(unfunded);
    space
}

/// T14 (R6): with owner checks, a produce never drops its own datum: the
/// tuple of the produce must hold the incoming datum. The incoming datum
/// fails its item test, so the produce stores it, and the stored datum and
/// the continuation stay as they are.
#[tokio::test]
async fn a_produce_never_drops_its_own_datum_with_owner_checks() {
    let checked = own_datum_scenario(Arc::new(OwnerRules {
        active: true,
        ..OwnerRules::default()
    }))
    .await;
    assert_eq!(produce(&checked, "new").await, None);
    assert_eq!(stored(&checked).await, BTreeSet::from(["new".to_string(), "old".to_string()]));
    assert_eq!(waiting(&checked).await, vec!["body".to_string()]);
}

/// T16: without owner funding, matching is that of dev. The observer is
/// installed, but its checks are off, so no item test and no tuple test run:
/// an unfunded datum matches, and a tuple of the refused list is charged.
#[tokio::test]
async fn legacy_runs_keep_dev_matching() {
    let rules = rules(false, &["unfunded-a"], &[&["unfunded-a"], &["x"]]);
    let plain = space(Arc::clone(&rules)).await;
    produce(&plain, "unfunded-a").await;
    assert_eq!(consume(&plain, "first").await.as_deref(), Some("unfunded-a"));
    assert_eq!(consume(&plain, "second").await, None);
    assert_eq!(produce(&plain, "x").await.as_deref(), Some("x"));
    assert!(stored(&plain).await.is_empty());
    assert!(waiting(&plain).await.is_empty());
    assert_eq!(rules.charged.lock().expect("charged tuples").as_slice(), &[
        vec!["unfunded-a".to_string()],
        vec!["x".to_string()]
    ]);
}

/// An admission that admits every item and every tuple and names no own
/// datum.
struct Open;

impl MatchAdmission<String, String, String, String> for Open {
    fn admit_item(&self, _: &String, _: bool, _: usize, _: &String, _: bool) -> bool { true }

    fn own_datum_channel(&self) -> Option<&String> { None }

    fn admit_tuple(
        &self,
        _: &WaitingContinuation<String, String>,
        _: &[crate::rspace::internal::ConsumeCandidate<String, String>],
    ) -> bool {
        true
    }
}

/// One match search: the continuation index and the matched (channel,
/// datum, index) triples, or none.
type Search = Option<(i32, Vec<(String, String, i32)>)>;

fn search(
    space: &RSpace<String, String, String, String>,
    channels: &[String],
    continuations: &[(Vec<String>, i32)],
    data: &HashMap<String, Vec<(String, i32)>>,
    admission: Option<&dyn MatchAdmission<String, String, String, String>>,
) -> Search {
    let matcher: Box<dyn Match<String, String, String>> = Box::new(Prefix);
    let candidates = continuations
        .iter()
        .map(|(patterns, index)| {
            (
                Arc::new(WaitingContinuation {
                    patterns: patterns.clone(),
                    continuation: format!("k{index}"),
                    persist: false,
                    peeks: BTreeSet::new(),
                    source: Consume::create(
                        &channels.to_vec(),
                        patterns,
                        &format!("k{index}"),
                        false,
                    ),
                }),
                *index,
            )
        })
        .collect();
    let indexed = data
        .iter()
        .map(|(channel, values)| {
            (
                channel.clone(),
                values
                    .iter()
                    .map(|(value, index)| {
                        (
                            Datum {
                                a: value.clone(),
                                persist: false,
                                source: Produce::create(channel, value, false),
                            },
                            *index,
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    space
        .extract_first_match(&matcher, channels.to_vec(), candidates, indexed, admission)
        .map(|candidate| {
            (
                candidate.continuation_index,
                candidate
                    .data_candidates
                    .into_iter()
                    .map(|data| (data.channel, data.datum.a, data.datum_index))
                    .collect(),
            )
        })
}

fn arb_scenario()
-> impl Strategy<Value = (Vec<String>, Vec<(Vec<String>, i32)>, HashMap<String, Vec<(String, i32)>>)>
{
    let channel = prop_oneof![Just("a".to_string()), Just("b".to_string())];
    let value = prop_oneof![
        Just("x".to_string()),
        Just("xy".to_string()),
        Just("y".to_string()),
        Just("yz".to_string())
    ];
    let pattern = prop_oneof![
        Just(String::new()),
        Just("x".to_string()),
        Just("y".to_string()),
        Just("xy".to_string())
    ];
    (prop::collection::vec(channel, 1..4), prop::collection::vec(value, 0..6))
        .prop_flat_map(move |(channels, values)| {
            let count = channels.len();
            let continuations =
                prop::collection::vec(prop::collection::vec(pattern.clone(), count..=count), 0..4);
            (Just(channels), continuations, Just(values))
        })
        .prop_map(|(channels, continuations, values)| {
            let mut data = HashMap::<String, Vec<(String, i32)>>::new();
            for channel in &channels {
                data.entry(channel.clone()).or_default();
            }
            for (index, value) in values.into_iter().enumerate() {
                let channel = channels[index % channels.len()].clone();
                data.entry(channel).or_default().push((value, index as i32));
            }
            let continuations = continuations
                .into_iter()
                .enumerate()
                .map(|(index, patterns)| (patterns, index as i32))
                .collect();
            (channels, continuations, data)
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The amended matcher with an admission that admits everything makes
    /// the choices of the dev matcher, for fixed orders of the continuations
    /// and the data (the inputs are already shuffled).
    #[test]
    fn the_amended_matcher_with_open_admission_equals_dev(
        (channels, continuations, data) in arb_scenario(),
    ) {
        let space = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime")
            .block_on(space(rules(false, &[], &[])));
        let dev = search(&space, &channels, &continuations, &data, None);
        let open = search(&space, &channels, &continuations, &data, Some(&Open));
        prop_assert_eq!(open, dev);
    }
}
