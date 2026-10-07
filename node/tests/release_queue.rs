use node::rust::instances::release_queue::ReleaseQueue;

#[tokio::test]
async fn a_released_block_is_processed_before_later_gossip_and_gossip_still_progresses() {
    let queue = ReleaseQueue::new(3, 16);

    queue.push_gossip("gossip-1").unwrap();
    for released in [
        "released-1",
        "released-2",
        "released-3",
        "released-4",
        "released-5",
    ] {
        queue.push_released(released);
    }
    queue.push_gossip("gossip-2").unwrap();

    let mut order = Vec::new();
    for _ in 0..7 {
        order.push(queue.pop().await.unwrap());
    }

    assert_eq!(
        order,
        vec![
            "released-1",
            "released-2",
            "released-3",
            "gossip-1",
            "released-4",
            "released-5",
            "gossip-2",
        ],
        "released blocks go first, and after three released blocks in a row one waiting gossip block is taken"
    );
}

#[test]
fn a_released_block_records_its_wait_from_release_to_processing() {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};
    use node::rust::instances::release_queue::RELEASE_QUEUE_WAIT_METRIC;

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let queue = ReleaseQueue::new(3, 16);
    queue.push_released("released-1");
    queue.push_gossip("gossip-1").unwrap();

    metrics::with_local_recorder(&recorder, || {
        futures::executor::block_on(async {
            queue.pop().await.unwrap();
            queue.pop().await.unwrap();
        })
    });

    let samples: usize = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, _, _, _)| key.key().name() == RELEASE_QUEUE_WAIT_METRIC)
        .map(|(_, _, _, value)| match value {
            DebugValue::Histogram(samples) => samples.len(),
            _ => 0,
        })
        .sum();
    assert_eq!(
        samples, 1,
        "only the released block records a release-to-processing wait"
    );
}
