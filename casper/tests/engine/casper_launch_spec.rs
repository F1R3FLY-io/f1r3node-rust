use std::sync::Arc;
use std::time::Duration;

use casper::rust::engine::casper_launch::{CasperLaunch, CasperLaunchImpl};
use casper::rust::engine::engine_cell::EngineCell;
use comm::rust::errors::CommError;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::test_instances::TransportLayerStub;
use models::casper::ApprovedBlockRequestProto;
use models::routing::protocol::Message as ProtocolMessage;
use models::rust::casper::protocol::casper_message::ApprovedBlock;
use prost::bytes::Bytes;
use prost::Message;

use crate::engine::setup::{minimal_casper_conf, TestFixture};

fn stored_approved_block(fixture: &TestFixture) {
    fixture
        .block_store
        .put_approved_block(&ApprovedBlock {
            candidate: fixture.approved_block_candidate.clone(),
            sigs: vec![],
            floor_seed: None,
        })
        .expect("the approved block is stored");
}

fn launcher(
    fixture: &TestFixture,
    rp_conf: RPConf,
    engine_cell: Arc<EngineCell>,
) -> CasperLaunchImpl<TransportLayerStub> {
    CasperLaunchImpl::new(
        fixture.transport_layer.clone(),
        rp_conf,
        fixture.connections_cell.clone(),
        fixture.last_approved_block.clone(),
        fixture.event_publisher.clone(),
        engine_cell,
        fixture.block_store.clone(),
        fixture.block_dag_storage.clone(),
        fixture.deploy_storage.clone(),
        fixture.rejected_deploy_buffer.clone(),
        fixture.rspace_state_manager.clone(),
        fixture.runtime_manager.clone(),
        fixture.estimator.clone(),
        fixture.ingest.clone(),
        minimal_casper_conf(),
        true,
        true,
        casper::rust::heartbeat_signal::new_heartbeat_signal_ref(),
        false,
        false,
        Vec::new(),
        None,
    )
}

fn approved_block_requests(fixture: &TestFixture) -> Vec<PeerNode> {
    let expected = Bytes::from(
        ApprovedBlockRequestProto {
            identifier: "".to_string(),
            trim_state: true,
        }
        .encode_to_vec(),
    );
    fixture
        .transport_layer
        .get_all_requests()
        .into_iter()
        .filter(|request| {
            matches!(&request.msg.message, Some(ProtocolMessage::Packet(packet)) if packet.content == expected)
        })
        .map(|request| request.peer)
        .collect()
}

#[tokio::test]
async fn a_node_that_is_its_own_bootstrap_resumes_without_probing_itself() {
    let fixture = TestFixture::new().await;
    stored_approved_block(&fixture);
    let mut rp_conf = fixture.rp_conf_ask.clone();
    rp_conf.bootstrap = Some(PeerNode {
        id: rp_conf.local.id.clone(),
        endpoint: Endpoint::new("rnode.bootstrap".to_string(), 40400, 40404),
    });
    fixture.transport_layer.reset();
    fixture.transport_layer.set_responses(|_peer, _protocol| {
        Err(CommError::PeerUnavailable(
            "TCP connection failed".to_string(),
        ))
    });
    let engine_cell = Arc::new(EngineCell::init());
    let launch = launcher(&fixture, rp_conf, engine_cell.clone());

    tokio::time::pause();
    let outcome = tokio::time::timeout(Duration::from_secs(60), launch.launch()).await;

    assert!(
        matches!(outcome, Ok(Ok(()))),
        "launch did not resume within 60 s: {outcome:?}"
    );
    assert!(
        engine_cell.get().await.with_casper().is_some(),
        "the resumed node is Running"
    );
    assert!(
        approved_block_requests(&fixture).is_empty(),
        "the node sent an ApprovedBlockRequest to itself"
    );
}

#[tokio::test]
async fn a_node_with_another_bootstrap_still_probes_it_before_resuming() {
    let fixture = TestFixture::new().await;
    stored_approved_block(&fixture);
    let bootstrap = PeerNode {
        id: NodeIdentifier {
            key: Bytes::from_static(b"another-bootstrap"),
        },
        endpoint: Endpoint::new("rnode.bootstrap".to_string(), 40400, 40404),
    };
    let mut rp_conf = fixture.rp_conf_ask.clone();
    rp_conf.bootstrap = Some(bootstrap.clone());
    fixture.transport_layer.reset();
    fixture
        .transport_layer
        .set_responses(|_peer, _protocol| Ok(()));
    let engine_cell = Arc::new(EngineCell::init());
    let launch = launcher(&fixture, rp_conf, engine_cell.clone());

    tokio::time::pause();
    let outcome = tokio::time::timeout(Duration::from_secs(60), launch.launch()).await;

    assert!(matches!(outcome, Ok(Ok(()))), "launch failed: {outcome:?}");
    let probed = approved_block_requests(&fixture);
    assert!(
        !probed.is_empty() && probed.iter().all(|peer| *peer == bootstrap),
        "the horizon probe goes to the bootstrap: {probed:?}"
    );
    assert!(
        engine_cell.get().await.with_casper().is_none(),
        "the node waits in Initializing for the probe answer"
    );
}
