//! End-to-end forced-collision proofs for canonical datum and continuation
//! selection.
//!
//! This file is compiled as a `data_index` unit-test child, not as a Cargo
//! integration crate. That placement keeps the collision injector private and
//! `cfg(test)`-only while exercising real play, history, COMM, and replay code.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::RSpaceDataIndex;
use crate::rspace::candidate_digest_test_hook;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::history::history_repository::HistoryRepositoryInstances;
use crate::rspace::hot_store::{HotStoreInstances, HotStoreState};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::r#match::Match;
use crate::rspace::replay_rspace::ReplayRSpace;
use crate::rspace::rho_path::{RhoPathEncode, RhoPathV1};
use crate::rspace::rspace::RSpace;
use crate::rspace::rspace_interface::ISpace;
use crate::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use crate::rspace::serializers::serializers::CandidateOrderingBytes;
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use crate::rspace::trace::event::{Consume, Event, IOEvent};

#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
struct CollisionAtom {
    identity_rank: u8,
    rmp_rank: u8,
}

impl CollisionAtom {
    const fn new(identity_rank: u8, rmp_rank: u8) -> Self {
        Self {
            identity_rank,
            rmp_rank,
        }
    }
}

impl StableHashSerialize for CollisionAtom {}

impl RhoPathEncode for CollisionAtom {
    fn rho_path_structure(&self) -> Vec<u8> { vec![self.rmp_rank] }
}

impl ColdStoreDecode for CollisionAtom {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

#[derive(Clone)]
struct WildcardMatch;

impl Match<String, CollisionAtom, String> for WildcardMatch {
    fn get(&self, _pattern: &String, datum: &CollisionAtom) -> Option<CollisionAtom> {
        Some(datum.clone())
    }
}

type Play = RSpace<String, String, CollisionAtom, String>;
type Replay = ReplayRSpace<String, String, CollisionAtom, String>;

async fn fixture() -> (Play, Replay) {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager
        .r_space_stores()
        .await
        .expect("in-memory RSpace stores");
    let history = Arc::new(
        HistoryRepositoryInstances::<String, String, CollisionAtom, String>::repository(
            stores.authority(),
        )
        .expect("history repository"),
    );
    let reader = history
        .get_history_reader(&history.root())
        .expect("history reader");
    let play_store = HotStoreInstances::create_from_hs_and_hr(
        HotStoreState::<String, String, CollisionAtom, String>::default(),
        reader.base(),
    );
    let replay_store = HotStoreInstances::create_from_hs_and_hr(
        HotStoreState::<String, String, CollisionAtom, String>::default(),
        reader.base(),
    );
    let matcher: Arc<Box<dyn Match<String, CollisionAtom, String>>> =
        Arc::new(Box::new(WildcardMatch));
    let play = RSpace::apply(history.clone(), play_store, matcher.clone());
    let replay = ReplayRSpace::apply(history, Arc::new(replay_store), matcher);
    (play, replay)
}

fn payloads(datums: &[Datum<CollisionAtom>]) -> Vec<CollisionAtom> {
    let mut values = datums
        .iter()
        .map(|datum| (*datum.a).clone())
        .collect::<Vec<_>>();
    values.sort_unstable();
    values
}

#[tokio::test(flavor = "current_thread")]
async fn forced_identity_digest_collision_preserves_comm_removal_reinsertion_and_replay() {
    let channel = "collision-channel".to_string();
    let pattern = "wildcard".to_string();
    let identity_first = CollisionAtom::new(0x00, 0xF0);
    let rmp_first = CollisionAtom::new(0x01, 0x10);

    let first_datum = Datum::create(&channel, identity_first.clone(), false);
    let second_datum = Datum::create(&channel, rmp_first.clone(), false);
    let first_path = RSpaceDataIndex::datum_path(&channel, &first_datum);
    let second_path = RSpaceDataIndex::datum_path(&channel, &second_datum);
    let first_view = RhoPathV1::parse_datum(&first_path).expect("first canonical datum path");
    let second_view = RhoPathV1::parse_datum(&second_path).expect("second canonical datum path");
    assert!(
        first_view.datum_identity() < second_view.datum_identity(),
        "identity bytes must order identity_first before rmp_first"
    );
    assert!(
        first_view.datum_structure() > second_view.datum_structure(),
        "derived RMP order must oppose canonical identity order"
    );

    let forced = candidate_digest_test_hook::force(Blake2b256Hash::from_bytes(vec![0xC3; 32]));
    let (play, replay) = fixture().await;
    let empty = play.create_checkpoint().await.expect("empty checkpoint");

    let _ = play
        .produce(channel.clone(), rmp_first.clone(), false)
        .await
        .expect("store RMP-first datum");
    for _ in 0..2 {
        let _ = play
            .produce(channel.clone(), identity_first.clone(), false)
            .await
            .expect("store identity-first multiplicity");
    }
    assert_eq!(
        forced.hits(),
        0,
        "an unmatched produce must not open or hash the resting datum catalog"
    );
    assert_eq!(play.get_store().data_count_at_path(&first_path).unwrap(), 2);
    assert_eq!(play.get_store().data_count_at_path(&second_path).unwrap(), 1);
    let play_repeated = play
        .consume(
            vec![channel.clone(), channel.clone()],
            vec![pattern.clone(), pattern.clone()],
            "remove-two".to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("play repeated-channel consume")
        .expect("two copies of the identity-first datum enable a COMM");
    assert_eq!(
        play_repeated
            .1
            .iter()
            .map(|result| result.matched_datum.clone())
            .collect::<Vec<_>>(),
        vec![identity_first.clone(), identity_first.clone()],
        "forced digest collision must fall through to canonical identity bytes"
    );
    assert_eq!(
        forced.hits(),
        2,
        "the initial play catalog must hash both distinct datum identities"
    );
    assert_eq!(
        payloads(&play.get_data(&channel).await),
        vec![rmp_first.clone()],
        "the multiplicity-two removal must consume both selected copies atomically"
    );
    assert_eq!(play.get_store().data_count_at_path(&first_path).unwrap(), 0);
    assert_eq!(play.get_store().data_count_at_path(&second_path).unwrap(), 1);

    for _ in 0..2 {
        let _ = play
            .produce(channel.clone(), identity_first.clone(), false)
            .await
            .expect("reinsert identity-first datum");
    }
    assert_eq!(play.get_store().data_count_at_path(&first_path).unwrap(), 2);
    assert_eq!(play.get_store().data_count_at_path(&second_path).unwrap(), 1);
    let play_single = play
        .consume(
            vec![channel.clone()],
            vec![pattern.clone()],
            "remove-one".to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("play single-channel consume")
        .expect("reinserted identity-first datum enables a COMM");
    assert_eq!(play_single.1[0].matched_datum, identity_first);
    assert_eq!(
        payloads(&play.get_data(&channel).await),
        vec![identity_first.clone(), rmp_first.clone()],
        "one copy of each identity must remain after reinsertion and removal"
    );
    assert_eq!(play.get_store().data_count_at_path(&first_path).unwrap(), 1);
    assert_eq!(play.get_store().data_count_at_path(&second_path).unwrap(), 1);

    let play_state = play.to_map().await;
    let play_point = play.create_checkpoint().await.expect("play checkpoint");
    assert_ne!(play_point.root, empty.root, "surviving datums require a non-empty root");
    let produce_events = play_point
        .log
        .iter()
        .filter(|event| matches!(event, Event::IoEvent(IOEvent::Produce(_))))
        .count();
    let consume_events = play_point
        .log
        .iter()
        .filter(|event| matches!(event, Event::IoEvent(IOEvent::Consume(_))))
        .count();
    let comms = play_point
        .log
        .iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => Some(comm),
            Event::IoEvent(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!((produce_events, consume_events, comms.len()), (5, 2, 2));
    let identity_source = Datum::create(&channel, identity_first.clone(), false).source;
    let repeated_comm = comms
        .iter()
        .find(|comm| comm.produces.len() == 2)
        .expect("multiplicity-two COMM");
    assert_eq!(
        repeated_comm.consume,
        Consume::create(
            &vec![channel.clone(), channel.clone()],
            &vec![pattern.clone(), pattern.clone()],
            &"remove-two".to_string(),
            false
        ),
    );
    assert!(
        repeated_comm
            .produces
            .iter()
            .all(|produce| produce == &identity_source)
    );
    assert_eq!(repeated_comm.times_repeated.get(&identity_source), Some(&2));
    let reinserted_comm = comms
        .iter()
        .find(|comm| comm.produces.len() == 1)
        .expect("post-reinsertion COMM");
    assert_eq!(
        reinserted_comm.consume,
        Consume::create(
            &vec![channel.clone()],
            &vec![pattern.clone()],
            &"remove-one".to_string(),
            false
        ),
    );
    assert_eq!(reinserted_comm.produces, vec![identity_source.clone()]);
    assert_eq!(reinserted_comm.times_repeated.get(&identity_source), Some(&4));

    replay
        .rig_and_reset(empty.root, play_point.log.clone())
        .await
        .expect("rig replay from the empty root");
    forced.reset_hits();
    let _ = replay
        .produce(channel.clone(), rmp_first.clone(), false)
        .await
        .expect("replay RMP-first produce");
    for _ in 0..2 {
        let _ = replay
            .produce(channel.clone(), identity_first.clone(), false)
            .await
            .expect("replay identity-first produce");
    }
    assert_eq!(
        forced.hits(),
        0,
        "replay unmatched produces must also avoid the resting datum catalog"
    );
    let replay_repeated = replay
        .consume(
            vec![channel.clone(), channel.clone()],
            vec![pattern.clone(), pattern.clone()],
            "remove-two".to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("replay repeated consume")
        .expect("recorded repeated COMM must fire");
    assert_eq!(replay_repeated, play_repeated);
    assert_eq!(forced.hits(), 2, "the initial replay catalog must hash both distinct identities");

    for _ in 0..2 {
        let _ = replay
            .produce(channel.clone(), identity_first.clone(), false)
            .await
            .expect("replay reinsertion");
    }
    let replay_single = replay
        .consume(
            vec![channel.clone()],
            vec![pattern],
            "remove-one".to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("replay single consume")
        .expect("recorded post-reinsertion COMM must fire");
    assert_eq!(replay_single, play_single);
    replay
        .check_replay_data()
        .await
        .expect("every recorded COMM was consumed");
    assert!(
        replay
            .replay_data
            .lock()
            .expect("replay data lock")
            .is_empty()
    );
    assert_eq!(replay.to_map().await, play_state);
    assert_eq!(payloads(&replay.get_data(&channel).await), vec![identity_first, rmp_first]);
    let replay_point = replay.create_checkpoint().await.expect("replay checkpoint");
    assert_eq!(replay_point.root, play_point.root, "identical final post-state root");
}

#[tokio::test(flavor = "current_thread")]
async fn forced_continuation_digest_collision_preserves_selection_removal_comm_and_replay() {
    let channel = "continuation-collision-channel".to_string();
    let channels = vec![channel.clone()];
    let pattern = "wildcard".to_string();
    let patterns = vec![pattern.clone()];
    let first = "z-cont".to_string();
    let second = "a-cont".to_string();
    let peeks = BTreeSet::new();
    let first_waiting =
        WaitingContinuation::create(&channels, &patterns, &first, false, peeks.clone());
    let second_waiting =
        WaitingContinuation::create(&channels, &patterns, &second, false, peeks.clone());
    assert!(
        second_waiting.candidate_ordering_bytes() < first_waiting.candidate_ordering_bytes(),
        "canonical identity order must oppose the actual vector store order"
    );

    let forced = candidate_digest_test_hook::force(Blake2b256Hash::from_bytes(vec![0xD7; 32]));
    let (play, replay) = fixture().await;
    let empty = play.create_checkpoint().await.expect("empty checkpoint");

    assert!(
        play.consume(channels.clone(), patterns.clone(), second.clone(), false, peeks.clone(),)
            .await
            .expect("store canonical-first continuation first")
            .is_none()
    );
    assert!(
        play.consume(channels.clone(), patterns.clone(), first.clone(), false, peeks.clone(),)
            .await
            .expect("store canonical-last continuation second")
            .is_none()
    );
    assert_eq!(forced.hits(), 0, "unmatched consumes must not order candidates");
    assert_eq!(
        play.get_waiting_continuations(channels.clone())
            .await
            .iter()
            .map(|waiting| waiting.continuation.as_ref().clone())
            .collect::<Vec<_>>(),
        vec![first.clone(), second.clone()],
        "newest-first store order must oppose canonical candidate ordering"
    );

    let datum = CollisionAtom::new(0x42, 0x24);
    let play_result = play
        .produce(channel.clone(), datum.clone(), false)
        .await
        .expect("play produce")
        .expect("one waiting continuation must fire");
    assert_eq!(play_result.0.continuation, second);
    assert_eq!(forced.hits(), 2, "the production ordering boundary must hash both identities once");
    let play_survivors = play.get_waiting_continuations(channels.clone()).await;
    assert_eq!(play_survivors.len(), 1);
    assert_eq!(play_survivors[0].continuation.as_ref(), &first);

    let play_state = play.to_map().await;
    let play_point = play.create_checkpoint().await.expect("play checkpoint");
    assert_ne!(
        play_point.root, empty.root,
        "the surviving continuation must make the post-state root non-empty"
    );
    let comms = play_point
        .log
        .iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => Some(comm),
            Event::IoEvent(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(comms.len(), 1);
    assert_eq!(
        comms[0].consume,
        Consume::create(&channels, &patterns, &second, false),
        "COMM identity must name the canonically selected continuation"
    );
    assert_eq!(comms[0].produces, vec![Datum::create(&channel, datum.clone(), false).source]);

    replay
        .rig_and_reset(empty.root, play_point.log.clone())
        .await
        .expect("rig replay from the empty root");
    forced.reset_hits();
    assert!(
        replay
            .consume(channels.clone(), patterns.clone(), second.clone(), false, peeks.clone(),)
            .await
            .expect("replay canonical-first consume")
            .is_none()
    );
    assert!(
        replay
            .consume(channels.clone(), patterns.clone(), first.clone(), false, peeks,)
            .await
            .expect("replay canonical-last consume")
            .is_none()
    );
    let replay_result = replay
        .produce(channel, datum, false)
        .await
        .expect("replay produce")
        .expect("recorded continuation must fire");
    assert_eq!(replay_result, play_result);
    assert_eq!(forced.hits(), 2);
    let replay_survivors = replay.get_waiting_continuations(channels).await;
    assert_eq!(replay_survivors.len(), 1);
    assert_eq!(replay_survivors[0].continuation.as_ref(), &first);
    replay
        .check_replay_data()
        .await
        .expect("every recorded COMM was consumed");
    assert!(
        replay
            .replay_data
            .lock()
            .expect("replay data lock")
            .is_empty()
    );
    assert_eq!(replay.to_map().await, play_state);
    let replay_point = replay.create_checkpoint().await.expect("replay checkpoint");
    assert_eq!(replay_point.root, play_point.root, "identical non-empty post-state root");
}
