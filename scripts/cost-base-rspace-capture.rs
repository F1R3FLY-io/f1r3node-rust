use std::sync::Arc;

use casper::rust::util::event_converter::to_casper_event;
use prost::Message;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

struct StringMatch;

impl Match<String, String, String> for StringMatch {
    fn get(&self, pattern: &String, datum: &String) -> Option<String> {
        (pattern == datum).then(|| datum.clone())
    }
}

#[tokio::test]
async fn capture_cost_base_event_and_state_root() {
    let mut manager = InMemoryStoreManager::new();
    let store = manager.r_space_stores().await.unwrap();
    let space: RSpace<String, String, String, String> =
        RSpace::create(store, Arc::new(Box::new(StringMatch))).unwrap();
    let empty = space.create_checkpoint().await.unwrap();
    space
        .produce("alpha".to_string(), "datum".to_string(), false)
        .await
        .unwrap();
    let checkpoint = space.create_checkpoint().await.unwrap();
    assert_eq!(checkpoint.log.len(), 1);
    let Event::IoEvent(IOEvent::Produce(produce)) = &checkpoint.log[0] else {
        panic!("expected one produce event");
    };
    println!("COST_BASE_EMPTY_ROOT={}", hex::encode(empty.root.bytes()));
    println!(
        "COST_BASE_NONEMPTY_ROOT={}",
        hex::encode(checkpoint.root.bytes())
    );
    println!(
        "COST_BASE_PRODUCE_HASH={}",
        hex::encode(produce.hash.bytes())
    );
    println!(
        "COST_BASE_EVENT_BYTES={}",
        hex::encode(bincode::serialize(&IOEvent::Produce(produce.clone())).unwrap())
    );
    println!(
        "COST_BASE_EVENT_PROTO_BYTES={}",
        hex::encode(
            to_casper_event(checkpoint.log[0].clone())
                .to_proto()
                .encode_to_vec()
        )
    );
}

#[tokio::test]
async fn capture_cost_base_nonempty_replay_root() {
    let mut manager = InMemoryStoreManager::new();
    let store = manager.r_space_stores().await.unwrap();
    let (space, replay): (RSpace<String, String, String, String>, _) =
        RSpace::create_with_replay(store, Arc::new(Box::new(StringMatch))).unwrap();
    let empty = space.create_checkpoint().await.unwrap();
    space
        .produce("alpha".to_string(), "datum".to_string(), false)
        .await
        .unwrap();
    let checkpoint = space.create_checkpoint().await.unwrap();
    replay
        .rig_and_reset(empty.root, checkpoint.log)
        .await
        .unwrap();
    replay
        .produce("alpha".to_string(), "datum".to_string(), false)
        .await
        .unwrap();
    let replay_checkpoint = replay.create_checkpoint().await.unwrap();
    assert_eq!(replay_checkpoint.root, checkpoint.root);
    println!(
        "COST_BASE_REPLAY_ROOT={}",
        hex::encode(replay_checkpoint.root.bytes())
    );
}
