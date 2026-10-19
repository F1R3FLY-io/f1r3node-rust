use casper::rust::blocks::block_processor::MAX_BLOCKS_IN_PROCESSING;
use casper::rust::engine::engine::Engine;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{BlockMessage, CasperMessage};

use crate::engine::setup::TestFixture;

fn random_signed_block(fixture: &TestFixture) -> BlockMessage {
    let block = get_random_block(
        None, None, None, None, None, None, None, None, None, None, None, None, None, None,
    );
    fixture.validator_id.sign_block(&block)
}

async fn enqueue_and_drop_unprocessed(fixture: &TestFixture, block: BlockMessage) {
    fixture
        .engine
        .handle(fixture.local.clone(), CasperMessage::BlockMessage(block))
        .await
        .unwrap();
    let item = fixture
        .block_processing_queue_rx
        .lock()
        .await
        .try_recv()
        .expect("the block must be queued");
    drop(item);
}

#[tokio::test]
async fn a_block_dropped_from_the_queue_can_be_queued_again() {
    let fixture = TestFixture::new().await;
    let block = random_signed_block(&fixture);

    enqueue_and_drop_unprocessed(&fixture, block.clone()).await;
    fixture
        .engine
        .handle(
            fixture.local.clone(),
            CasperMessage::BlockMessage(block.clone()),
        )
        .await
        .unwrap();

    let queued = fixture.block_processing_queue_rx.lock().await.try_recv();
    assert!(
        queued.is_ok_and(|(_, queued, _)| queued.block_hash == block.block_hash),
        "a block dropped from the queue without processing keeps its in-flight marker, \
         so the same block is skipped as already queued"
    );
}

#[tokio::test]
async fn blocks_dropped_from_the_queue_do_not_wedge_the_node_at_the_cap() {
    let fixture = TestFixture::new().await;
    for _ in 0..MAX_BLOCKS_IN_PROCESSING {
        let block = random_signed_block(&fixture);
        enqueue_and_drop_unprocessed(&fixture, block).await;
    }

    let next = random_signed_block(&fixture);
    fixture
        .engine
        .handle(
            fixture.local.clone(),
            CasperMessage::BlockMessage(next.clone()),
        )
        .await
        .unwrap();

    let queued = fixture.block_processing_queue_rx.lock().await.try_recv();
    assert!(
        queued.is_ok_and(|(_, block, _)| block.block_hash == next.block_hash),
        "after {MAX_BLOCKS_IN_PROCESSING} blocks dropped from the queue unprocessed, \
         the node drops every new block at the in-flight cap"
    );
}
