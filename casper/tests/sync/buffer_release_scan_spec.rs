//! The release scan finds the buffered blocks whose dependencies are all
//! validated. It runs after every processed block, so its duration is a
//! direct cost on the processing path and the soak must be able to read it.

use casper::rust::casper::Casper;
use casper::rust::metrics_constants::CASPER_BUFFER_RELEASE_SCAN_TIME_METRIC;
use metrics_util::debugging::{DebugValue, DebuggingRecorder};

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn the_release_scan_records_its_duration() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let nodes = TestNode::create_network(genesis, 1, None, None, None, None)
        .await
        .unwrap();

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::with_local_recorder(&recorder, || {
        nodes[0].casper.get_dependency_free_from_buffer().unwrap();
    });

    let samples: usize = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, _, _, _)| key.key().name() == CASPER_BUFFER_RELEASE_SCAN_TIME_METRIC)
        .map(|(_, _, _, value)| match value {
            DebugValue::Histogram(samples) => samples.len(),
            _ => 0,
        })
        .sum();
    assert_eq!(samples, 1, "one release scan records one duration sample");
}
