use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::hot_store_action::{
    DeleteAction, DeleteContinuations, HotStoreAction, InsertAction, InsertContinuations,
    InsertData, InsertJoins,
};
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

use crate::history::history_repository_tests::{continuation, create_empty_repository, datum};

fn produce_on(channel: &String, seed: u8) -> Produce {
    Produce::new(hash(channel), Blake2b256Hash::from_bytes(vec![seed; 32]), false)
}

fn consume_on(channel: &String, seed: u8) -> Consume {
    Consume {
        channel_hashes: vec![hash(channel)],
        hash: Blake2b256Hash::from_bytes(vec![seed; 32]),
        persistent: false,
    }
}

#[tokio::test]
async fn state_change_from_empty_event_log_is_empty() {
    let repo = create_empty_repository();
    let reader_pre = repo.get_history_reader_struct(&repo.root()).unwrap();
    let reader_post = repo.get_history_reader_struct(&repo.root()).unwrap();

    let change = StateChange::new(reader_pre, reader_post, &EventLogIndex::empty()).unwrap();

    assert!(change.datums_changes.is_empty());
    assert!(change.cont_changes.is_empty());
    assert!(change.consume_channels_to_join_serialized_map.is_empty());
}

#[tokio::test]
async fn state_change_computes_datum_diff_between_pre_and_post_state() {
    let repo = create_empty_repository();
    let channel = "state-change-datum-channel".to_string();

    let pre = repo.checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertData(InsertData {
        channel: channel.clone(),
        data: vec![datum(1)],
    }))]);
    let pre_root = pre.root();
    let post = pre.checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertData(InsertData {
        channel: channel.clone(),
        data: vec![datum(2)],
    }))]);
    let post_root = post.root();

    let mut event_log_index = EventLogIndex::empty();
    event_log_index
        .produces_linear
        .0
        .insert(produce_on(&channel, 0xa1));

    let change = StateChange::new(
        post.get_history_reader_struct(&pre_root).unwrap(),
        post.get_history_reader_struct(&post_root).unwrap(),
        &event_log_index,
    )
    .unwrap();

    assert_eq!(change.datums_changes.len(), 1);
    let datum_change = change.datums_changes.get(&hash(&channel)).unwrap();
    assert_eq!(datum_change.added.len(), 1);
    assert_eq!(datum_change.removed.len(), 1);
    assert_ne!(datum_change.added, datum_change.removed);
    assert!(change.cont_changes.is_empty());
    assert!(change.consume_channels_to_join_serialized_map.is_empty());
}

#[tokio::test]
async fn state_change_drops_channels_whose_pre_and_post_values_are_identical() {
    let repo = create_empty_repository();
    let channel = "state-change-noop-channel".to_string();

    let pre = repo.checkpoint(vec![HotStoreAction::Insert(InsertAction::InsertData(InsertData {
        channel: channel.clone(),
        data: vec![datum(1)],
    }))]);
    let root = pre.root();

    let mut event_log_index = EventLogIndex::empty();
    event_log_index
        .produces_linear
        .0
        .insert(produce_on(&channel, 0xa2));

    let change = StateChange::new(
        pre.get_history_reader_struct(&root).unwrap(),
        pre.get_history_reader_struct(&root).unwrap(),
        &event_log_index,
    )
    .unwrap();

    assert!(
        change.datums_changes.is_empty(),
        "a touched channel with no net pre/post difference must be dropped"
    );
}

#[tokio::test]
async fn state_change_computes_cont_diff_and_join_for_removed_continuation() {
    let repo = create_empty_repository();
    let channel = "state-change-cont-channel".to_string();

    let pre = repo.checkpoint(vec![
        HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
            channels: vec![channel.clone()],
            continuations: vec![continuation(5)],
        })),
        HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
            channel: channel.clone(),
            joins: vec![vec![channel.clone()]],
        })),
    ]);
    let pre_root = pre.root();
    let post = pre.checkpoint(vec![HotStoreAction::Delete(DeleteAction::DeleteContinuations(
        DeleteContinuations {
            channels: vec![channel.clone()],
        },
    ))]);
    let post_root = post.root();

    let mut event_log_index = EventLogIndex::empty();
    event_log_index
        .consumes_linear_and_peeks
        .0
        .insert(consume_on(&channel, 0xb1));

    let change = StateChange::new(
        post.get_history_reader_struct(&pre_root).unwrap(),
        post.get_history_reader_struct(&post_root).unwrap(),
        &event_log_index,
    )
    .unwrap();

    assert!(change.datums_changes.is_empty());
    assert_eq!(change.cont_changes.len(), 1);
    let cont_change = change.cont_changes.get(&vec![hash(&channel)]).unwrap();
    assert!(cont_change.added.is_empty());
    assert_eq!(cont_change.removed.len(), 1);

    assert_eq!(change.consume_channels_to_join_serialized_map.len(), 1);
    let serialized_join = change
        .consume_channels_to_join_serialized_map
        .get(&vec![hash(&channel)])
        .unwrap();
    let join: Vec<String> = bincode::deserialize(&serialized_join).unwrap();
    assert_eq!(join, vec![channel]);
}
