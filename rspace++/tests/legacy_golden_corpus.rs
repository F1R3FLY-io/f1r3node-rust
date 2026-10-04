use std::sync::Arc;

use rspace_plus_plus::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce};
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(include_str!("../../test-vectors/legacy-golden-corpus-v1.json"))
        .expect("parse the legacy golden corpus JSON")
}

fn expected(section: &str, name: &str) -> Vec<u8> {
    hex::decode(
        corpus()[section][name]
            .as_str()
            .expect("the corpus entry is a hex string"),
    )
    .expect("decode the corpus hex value")
}

#[test]
fn legacy_golden_corpus_channel_hashes() {
    assert_eq!(hash(&"alpha").bytes(), expected("channel_hashes", "alpha"));
    assert_eq!(hash(&"beta").bytes(), expected("channel_hashes", "beta"));
    assert_eq!(
        hash_from_vec(&vec!["alpha", "beta"]).bytes(),
        expected("channel_hashes", "alpha_beta_multichannel")
    );
    assert_eq!(
        hash_from_vec(&vec!["beta", "alpha"]).bytes(),
        expected("channel_hashes", "alpha_beta_multichannel")
    );
}

#[test]
fn legacy_golden_corpus_event_identities() {
    let produce = Produce::create(&"alpha", &"datum", false);
    assert_eq!(produce.hash.bytes(), expected("event_log", "produce_alpha_datum_linear"));
    assert_eq!(produce.channel_hash.bytes(), expected("channel_hashes", "alpha"));
    let consume = Consume::create(&vec!["alpha", "beta"], &vec!["x"], &"body", false);
    assert_eq!(consume.hash.bytes(), expected("event_log", "consume_alpha_beta_x_body_linear"));
    assert_eq!(
        consume
            .channel_hashes
            .iter()
            .map(|hash| hash.bytes())
            .collect::<Vec<_>>(),
        vec![expected("channel_hashes", "beta"), expected("channel_hashes", "alpha")]
    );
}

#[test]
fn legacy_golden_corpus_empty_radix_root() {
    assert_eq!(
        RadixHistory::empty_root_node_hash().bytes(),
        expected("state_roots", "shared_empty_radix")
    );
}

struct StringMatch;

impl Match<String, String, String> for StringMatch {
    fn get(&self, pattern: &String, datum: &String) -> Option<String> {
        (pattern == datum).then(|| datum.clone())
    }
}

#[tokio::test]
async fn legacy_golden_corpus_nonempty_root_and_event() {
    let mut manager = InMemoryStoreManager::new();
    let store = manager
        .r_space_stores()
        .await
        .expect("create the in-memory RSpace stores");
    let space: RSpace<String, String, String, String> =
        RSpace::create(store, Arc::new(Box::new(StringMatch))).expect("create the RSpace");
    let empty = space
        .create_checkpoint()
        .await
        .expect("checkpoint the empty space");
    space
        .produce("alpha".to_string(), "datum".to_string(), false)
        .await
        .expect("produce the alpha datum");
    let checkpoint = space
        .create_checkpoint()
        .await
        .expect("checkpoint after the produce");
    assert_eq!(checkpoint.log.len(), 1);
    let Event::IoEvent(IOEvent::Produce(produce)) = &checkpoint.log[0] else {
        panic!("expected one produce event")
    };
    assert_eq!(produce.hash.bytes(), expected("event_log", "produce_alpha_datum_linear"));
    assert_eq!(empty.root.bytes(), expected("state_roots", "shared_empty_radix"));
    assert_eq!(checkpoint.root.bytes(), expected("state_roots", "current_nonempty_rspace"));
    assert_ne!(checkpoint.root.bytes(), expected("state_roots", "legacy_reified_nonempty_rspace"));
    assert_eq!(
        bincode::serialize(&IOEvent::Produce(produce.clone()))
            .expect("serialize the produce IO event"),
        expected("event_log", "current_produce_alpha_datum_linear_full_io_event_bytes")
    );
    assert_ne!(
        bincode::serialize(&IOEvent::Produce(produce.clone()))
            .expect("serialize the produce IO event"),
        expected("event_log", "legacy_reified_produce_alpha_datum_linear_full_io_event_bytes")
    );
}
