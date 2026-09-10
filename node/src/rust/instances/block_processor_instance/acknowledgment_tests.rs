use casper::rust::blocks::block_processor::BlockProcessorDependencies;

use super::*;

pub(super) fn processor_with_shared_tracker(
    fixture: &Fixture,
) -> Arc<BlockProcessor<TransportLayerStub>> {
    Arc::new(BlockProcessor::new(
        BlockProcessorDependencies::new(
            fixture.casper.block_store.clone(),
            fixture.casper.block_dag_storage.clone(),
            fixture.casper.block_retriever.clone(),
            Arc::new(TransportLayerStub::new()),
            ConnectionsCell {
                peers: Arc::new(std::sync::Mutex::new(Connections::from_vec(Vec::new()))),
            },
            create_rp_conf_ask(
                PeerNode {
                    id: NodeIdentifier {
                        key: Bytes::from(vec![1; 32]),
                    },
                    endpoint: Endpoint {
                        host: "localhost".into(),
                        tcp_port: 40400,
                        udp_port: 40400,
                    },
                },
                None,
                None,
            ),
            None,
        )
        .unwrap(),
    ))
}

#[tokio::test]
async fn final_candidate_tracker_error_suppresses_proposal_without_another_visit() {
    let mut fixture = Fixture::new(1, 1024 * 1024).await;
    let candidate = block(1);
    fixture.add(&candidate);
    fixture
        .pending_policy
        .put(vec![(
            KeyValueTypedStoreImpl::<BlockHashSerde, Vec<u8>>::new(fixture.pending_policy.clone())
                .encode_key(&BlockHashSerde(candidate.block_hash.clone()))
                .unwrap(),
            vec![0xff],
        )])
        .unwrap();
    let result = fixture.step();
    assert!(result.is_err());
    assert_eq!(fixture.pass.pass.remaining(), 0);
    assert!(fixture.pass.complete());
    assert!(!fixture.pass.pass.proposal_ready());
    assert!(fixture.receiver.try_recv().is_err());
    assert!(fixture.sender.identities().is_empty());
    assert_eq!(fixture.sender.used_bytes(), 0);
}

#[tokio::test]
async fn receipt_publication_preserves_new_proposal_demand() {
    let mut fixture = Fixture::new(1, 1024 * 1024).await;
    let candidate = block(1);
    fixture.add(&candidate);
    let signal = fixture.sender.recovery().signal();
    assert_eq!(signal.take(), RecoveryWake::Idle);
    signal.request(true);
    signal.request(false);
    assert!(matches!(fixture.step().unwrap(), Step::Advanced));
    assert!(fixture.pass.complete());
    assert!(fixture.pass.pass.proposal_ready());
    assert_eq!(signal.take(), RecoveryWake::Work { proposal: true });
    assert_eq!(signal.take(), RecoveryWake::Idle);
    assert!(fixture
        .casper
        .block_retriever
        .is_received(candidate.block_hash.clone())
        .await
        .unwrap());
}

#[tokio::test]
async fn empty_selection_consumes_one_visit_and_leaves_no_candidate() {
    let mut fixture = Fixture::new(2, 1024 * 1024).await;
    assert!(matches!(fixture.step().unwrap(), Step::Advanced));
    assert_eq!(fixture.pass.pass.remaining(), 1);
    assert!(fixture.pass.pending.is_none());
    assert!(!fixture.pass.complete());
    assert!(matches!(fixture.step().unwrap(), Step::Advanced));
    assert!(fixture.pass.complete());
    assert!(fixture.pass.pass.proposal_ready());
    assert!(matches!(fixture.step().unwrap(), Step::Complete));
}

#[tokio::test]
async fn preselection_error_consumes_one_visit_and_records_failure() {
    let mut fixture = Fixture::new(2, 1024 * 1024).await;
    let result = fixture.pass.finish_attempt(
        Err(CasperError::RuntimeError(
            "injected context read failure".into(),
        )),
        2,
        false,
    );
    assert!(result.is_err());
    assert_eq!(fixture.pass.pass.remaining(), 1);
    assert!(fixture.pass.pending.is_none());
    assert!(matches!(fixture.step().unwrap(), Step::Advanced));
    assert!(fixture.pass.complete());
    assert!(!fixture.pass.pass.proposal_ready());
}
