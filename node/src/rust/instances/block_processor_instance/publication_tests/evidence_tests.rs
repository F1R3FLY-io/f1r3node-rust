use proptest::test_runner::{Config, TestRunner};

use super::*;

struct EvidenceFixture {
    processor: BlockProcessor<TransportLayerStub>,
    casper: Arc<dyn MultiParentCasper + Send + Sync>,
    other_context: Arc<dyn MultiParentCasper + Send + Sync>,
    candidate: BlockMessage,
    raw: Arc<InMemoryKeyValueStore>,
    blocks: KeyValueBlockStore,
    gate: Arc<PublicationGate>,
    buffer: CasperBufferKeyValueStorage,
    retriever: BlockRetriever<TransportLayerStub>,
}

impl EvidenceFixture {
    async fn new() -> Self {
        let mut manager = InMemoryStoreManager::new();
        let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
        let buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
            .await
            .unwrap();
        let (genesis, candidate) = publication_blocks();
        dag.insert(&genesis, InsertMode::ApprovedGenesis).unwrap();
        let transport = Arc::new(TransportLayerStub::new());
        let connections = ConnectionsCell {
            peers: Arc::new(std::sync::Mutex::new(Connections::from_vec(Vec::new()))),
        };
        let conf = create_rp_conf_ask(
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
        );
        let retriever = BlockRetriever::new(
            buffer.clone(),
            transport.clone(),
            connections.clone(),
            conf.clone(),
        );
        let raw = Arc::new(InMemoryKeyValueStore::new());
        let gate = Arc::new(PublicationGate {
            closed: AtomicBool::new(false),
            failed: tokio::sync::Notify::new(),
        });
        let blocks = KeyValueBlockStore::new(
            Arc::new(PublicationStore {
                inner: raw.clone(),
                fail_on_write: 0,
                writes: Arc::new(AtomicUsize::new(0)),
                failures: Arc::new(AtomicUsize::new(0)),
                gate: Some(gate.clone()),
                read_hook: None,
                write_hook: None,
            }),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        let processor = BlockProcessor::new(
            BlockProcessorDependencies::new(
                blocks.clone(),
                dag,
                retriever.clone(),
                transport,
                connections,
                conf,
                None,
            )
            .unwrap(),
        );
        let casper = Arc::new(TestCasperWithSnapshot::new(
            TestCasperWithSnapshot::create_empty_snapshot(),
            genesis.clone(),
        ));
        let other_context = Arc::new(TestCasperWithSnapshot::new(
            TestCasperWithSnapshot::create_empty_snapshot(),
            genesis,
        ));
        Self {
            processor,
            casper,
            other_context,
            candidate,
            raw,
            blocks,
            gate,
            buffer,
            retriever,
        }
    }

    fn interest(&self) -> casper::rust::blocks::block_processor::PublicationInterest {
        self.processor
            .capture_publication_interest(self.casper.clone(), &self.candidate)
            .unwrap()
            .unwrap()
    }

    async fn accept(&self) -> AcceptedBlockPublication {
        let (well_formed, evidence) = self
            .processor
            .check_and_store_for_publication(self.casper.clone(), &self.candidate, self.interest())
            .await
            .unwrap();
        assert!(well_formed);
        evidence.unwrap()
    }
}

#[test]
fn generated_publication_bindings_reject_wrong_identity_context_and_storage_failure() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let fixture = runtime.block_on(EvidenceFixture::new());
    let mut runner = TestRunner::new(Config {
        cases: 128,
        ..Config::default()
    });
    runner
        .run(
            &prop::collection::vec((any::<bool>(), any::<bool>(), any::<bool>()), 0..32),
            |history| {
                let exhaustive = [false, true].into_iter().flat_map(|wrong_hash| {
                    [false, true].into_iter().flat_map(move |wrong_context| {
                        [false, true]
                            .into_iter()
                            .map(move |failure| (wrong_hash, wrong_context, failure))
                    })
                });
                for (wrong_hash, wrong_context, failure) in exhaustive.chain(history) {
                    let interest = fixture.interest();
                    let mut candidate = fixture.candidate.clone();
                    if wrong_hash {
                        candidate.block_hash = Bytes::from(vec![99; 32]);
                    }
                    let casper = if wrong_context {
                        fixture.other_context.clone()
                    } else {
                        fixture.casper.clone()
                    };
                    let before = fixture.raw.to_map().unwrap();
                    fixture.gate.closed.store(failure, Ordering::SeqCst);
                    let result = runtime.block_on(
                        fixture
                            .processor
                            .check_and_store_for_publication(casper, &candidate, interest),
                    );
                    if wrong_hash || wrong_context || failure {
                        prop_assert!(result.is_err());
                        prop_assert_eq!(fixture.raw.to_map().unwrap(), before);
                    } else {
                        let (well_formed, evidence) = result.unwrap();
                        prop_assert!(well_formed && evidence.is_some());
                    }
                    prop_assert!(!fixture
                        .buffer
                        .contains_durable_row(&BlockHashSerde(fixture.candidate.block_hash.clone()))
                        .unwrap());
                }
                Ok(())
            },
        )
        .unwrap();
}

#[tokio::test]
async fn invalid_format_signature_and_content_never_create_publication_evidence() {
    let fixture = EvidenceFixture::new().await;
    for variant in 0..3 {
        let interest = fixture.interest();
        let mut candidate = fixture.candidate.clone();
        match variant {
            0 => candidate.sig_algorithm.clear(),
            1 => candidate.sig = Bytes::from(vec![0; 64]),
            _ => candidate.body.state.block_number += 1,
        }
        let (well_formed, evidence) = fixture
            .processor
            .check_and_store_for_publication(fixture.casper.clone(), &candidate, interest)
            .await
            .unwrap();
        assert_eq!(well_formed, variant == 2);
        assert!(evidence.is_none());
        if variant < 2 {
            assert!(fixture.raw.to_map().unwrap().is_empty());
        }
    }
}

#[tokio::test]
async fn accepted_publication_rejects_context_identity_and_stored_body_corruption() {
    let fixture = EvidenceFixture::new().await;
    let hash = &fixture.candidate.block_hash;
    assert!(matches!(
        fixture
            .processor
            .prepare_stored_publication(fixture.casper.clone(), hash)
            .unwrap(),
        StoredPublicationPreparation::Missing
    ));
    let evidence = fixture.accept().await;
    let original = fixture.raw.to_map().unwrap();
    for (casper, target) in [
        (fixture.other_context.clone(), hash.clone()),
        (fixture.casper.clone(), Bytes::from(vec![99; 32])),
    ] {
        assert!(fixture
            .processor
            .restore_accepted_publication(casper, &target, &evidence)
            .await
            .is_err());
        assert_eq!(fixture.raw.to_map().unwrap(), original);
        assert!(!fixture
            .buffer
            .contains_durable_row(&BlockHashSerde(hash.clone()))
            .unwrap());
    }
    for variant in 0..4 {
        fixture.raw.delete(vec![hash.to_vec()]).unwrap();
        if variant > 0 {
            let bytes = if variant == 1 {
                vec![0xff]
            } else {
                let mut invalid = fixture.candidate.clone();
                if variant == 2 {
                    invalid.block_hash = Bytes::from(vec![99; 32]);
                } else {
                    invalid.sig = Bytes::from(vec![0; 64]);
                }
                fixture
                    .blocks
                    .put_block_message_awaiting_certificate(&invalid)
                    .unwrap();
                fixture.raw.get(&vec![invalid.block_hash.to_vec()]).unwrap()[0]
                    .clone()
                    .unwrap()
            };
            fixture.raw.put(vec![(hash.to_vec(), bytes)]).unwrap();
        }
        assert!(fixture
            .processor
            .restore_accepted_publication(fixture.casper.clone(), hash, &evidence)
            .await
            .is_err());
        assert!(!fixture
            .buffer
            .contains_durable_row(&BlockHashSerde(hash.clone()))
            .unwrap());
        if variant > 0 {
            assert!(fixture
                .processor
                .prepare_stored_publication(fixture.casper.clone(), hash)
                .is_err());
        }
    }
}

#[tokio::test]
async fn accepted_publication_promotes_a_replacement_owner_without_recapture() {
    let fixture = EvidenceFixture::new().await;
    let hash = fixture.candidate.block_hash.clone();
    fixture
        .retriever
        .admit_hash(
            hash.clone(),
            None,
            AdmitHashReason::MissingDependencyRequested,
        )
        .await
        .unwrap();
    let evidence = fixture.accept().await;
    fixture.retriever.forget_hash_tracking(&hash).unwrap();
    fixture.retriever.record_received(hash.clone()).unwrap();
    assert!(!fixture
        .retriever
        .was_requested_as_dependency(&hash)
        .unwrap());
    fixture
        .processor
        .restore_accepted_publication(fixture.casper.clone(), &hash, &evidence)
        .await
        .unwrap();
    assert!(
        fixture
            .buffer
            .pending_request_policy(&BlockHashSerde(hash))
            .unwrap()
            .unwrap()
            .requested_as_dependency
    );
}
