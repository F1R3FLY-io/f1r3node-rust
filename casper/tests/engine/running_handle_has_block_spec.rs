// See casper/src/test/scala/coop/rchain/casper/engine/RunningHandleHasBlockSpec.scala
//
// The engine hands a HasBlock for a hash it does not know to block ingest as
// an announcement; whom to ask and when is the fetch scheduler's decision
// (fetch_scheduler tests). A hash the node already holds is ignored.

use casper::rust::blocks::ingest::block_ingest::Stores;
use casper::rust::blocks::ingest::dependency_tracker::{BodyAt, HeldMeta};
use casper::rust::blocks::ingest::ingest_state::{IngestInput, IngestState};
use casper::rust::engine::engine::Engine;
use models::rust::casper::protocol::casper_message::{CasperMessage, HasBlock};
use prost::bytes::Bytes;

use crate::engine::setup::{peer_node, TestFixture};

async fn announcements(fixture: &TestFixture) -> Vec<IngestInput> {
    let mut ports = fixture.ingest_ports.lock().await;
    let mut inputs = Vec::new();
    while let Ok(input) = ports.inputs.try_recv() {
        inputs.push(input);
    }
    inputs
}

#[tokio::test]
async fn a_has_block_for_an_unknown_hash_is_announced_to_block_ingest() {
    let fixture = TestFixture::new().await;
    let sender = peer_node("some_peer", 40400);
    let hash = Bytes::from_static(b"hash");

    fixture
        .engine
        .handle(
            sender.clone(),
            CasperMessage::HasBlock(HasBlock { hash: hash.clone() }),
        )
        .await
        .unwrap();

    assert_eq!(announcements(&fixture).await, vec![
        IngestInput::Announced { hash, peer: sender }
    ]);
    assert_eq!(
        fixture.transport_layer.request_count(),
        0,
        "the engine sends no request itself"
    );
}

#[tokio::test]
async fn a_has_block_for_a_held_block_is_ignored() {
    let fixture = TestFixture::new().await;
    let hash = Bytes::from_static(b"hash");
    let mut state = IngestState::new(fixture.ingest.view().clone());
    let stores = Stores {
        dag: &fixture.block_dag_storage,
        block_store: &fixture.block_store,
    };
    state.handle(
        IngestInput::Received(HeldMeta {
            hash: hash.clone(),
            sender: Bytes::from_static(b"sender"),
            height: 1,
            deps: vec![fixture.genesis.block_hash.clone()],
            body: BodyAt::Disk,
            size: 0,
            trusted: true,
            bonded_at_anchor: true,
            solicited: false,
            pushed_by: None,
        }),
        fixture.ingest.view().now_ms(),
        &stores,
    );
    assert!(fixture.ingest.view().is_held(&hash));

    fixture
        .engine
        .handle(
            peer_node("some_peer", 40400),
            CasperMessage::HasBlock(HasBlock { hash }),
        )
        .await
        .unwrap();

    assert!(announcements(&fixture).await.is_empty());
    assert_eq!(fixture.transport_layer.request_count(), 0);
}
