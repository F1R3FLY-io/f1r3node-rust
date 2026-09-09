use super::*;

enum Interruption {
    None,
    ReceiptError,
    ByteCapacity,
}

async fn check_startup_publication(interruption: Interruption) {
    let candidate = block(1);
    let hash = candidate.block_hash.clone();
    let bytes = candidate.to_proto().encoded_len();
    let mut fixture = Fixture::new(1, bytes).await;
    fixture
        .casper
        .block_store
        .put_block_message(&candidate)
        .unwrap();
    fixture
        .casper
        .casper_buffer_storage
        .put_pendant(BlockHashSerde(hash.clone()))
        .unwrap();
    let mut ticket = fixture.pass.origin.request_startup(false).unwrap();
    ticket
        .capture_buffer_snapshot(&fixture.casper.casper_buffer_storage)
        .await
        .unwrap();
    let mut pass = StartupPass {
        active: fixture
            .sender
            .recovery()
            .startup()
            .activate()
            .unwrap()
            .unwrap(),
        pending: None,
    };
    for _ in 0..2 {
        assert!(matches!(
            pass.step(&fixture.sender.downgrade(), &fixture.processor)
                .unwrap(),
            Step::Advanced
        ));
        assert!(fixture.receiver.try_recv().is_err());
        assert!(pass.pending.is_none());
    }
    assert!(!fixture
        .casper
        .block_retriever
        .is_received(hash.clone())
        .await
        .unwrap());
    assert!(matches!(
        fixture.casper.prepare_startup_candidate(&hash).unwrap(),
        RetryCandidate::Ready(_)
    ));

    match interruption {
        Interruption::None => {}
        Interruption::ReceiptError => {
            let key = KeyValueTypedStoreImpl::<BlockHashSerde, Vec<u8>>::new(
                fixture.pending_policy.clone(),
            )
            .encode_key(&BlockHashSerde(hash.clone()))
            .unwrap();
            fixture.pending_policy.put(vec![(key, vec![0xff])]).unwrap();
            assert!(!fixture
                .processor
                .is_validation_failure_quarantined(&hash)
                .unwrap());
            let error = pass
                .step(&fixture.sender.downgrade(), &fixture.processor)
                .err()
                .expect("corrupt policy must fail receipt publication");
            assert_eq!(pass.pending.as_ref(), Some(&hash));
            assert!(fixture.receiver.try_recv().is_err());
            assert!(fixture.sender.identities().is_empty());
            assert_eq!(fixture.sender.used_bytes(), 0);
            assert_eq!(fixture.sender.capacity(), 3);
            assert!(pass.active.fail(error).unwrap());
            drop(pass);
            assert!(tokio::time::timeout(
                Duration::from_secs(1),
                ticket.finish(None::<fn() -> std::future::Ready<Result<(), CasperError>>>),
            )
            .await
            .unwrap()
            .is_err());
            assert!(fixture
                .casper
                .casper_buffer_storage
                .is_pendant(&BlockHashSerde(hash)));
            return;
        }
        Interruption::ByteCapacity => {
            let mut occupying = candidate.clone();
            occupying.block_hash = Bytes::from(vec![2; 32]);
            fixture
                .sender
                .try_enqueue(fixture.casper.clone(), occupying)
                .unwrap();
            let held = fixture.receiver.try_recv().unwrap();
            for _ in 0..3 {
                assert!(matches!(
                    pass.step(&fixture.sender.downgrade(), &fixture.processor)
                        .unwrap(),
                    Step::Parked
                ));
                assert_eq!(pass.pending.as_ref(), Some(&hash));
                assert!(fixture.receiver.try_recv().is_err());
                assert!(!fixture.sender.identities().contains(&hash));
                assert_eq!(fixture.sender.used_bytes(), bytes);
                assert!(!fixture
                    .casper
                    .block_retriever
                    .is_received(hash.clone())
                    .await
                    .unwrap());
            }
            drop(held);
            assert_eq!(fixture.sender.used_bytes(), 0);
        }
    }

    assert!(matches!(
        pass.step(&fixture.sender.downgrade(), &fixture.processor)
            .unwrap(),
        Step::Advanced
    ));
    assert!(pass.pending.is_none());
    assert!(fixture
        .casper
        .block_retriever
        .is_received(hash.clone())
        .await
        .unwrap());
    let admitted = fixture.receiver.try_recv().unwrap();
    assert_eq!(admitted.block, candidate);
    assert!(fixture.sender.identities().contains(&hash));
    assert_eq!(fixture.sender.used_bytes(), bytes);
    drop(admitted);
    assert!(fixture.sender.identities().is_empty());
    assert_eq!(fixture.sender.used_bytes(), 0);
    assert!(matches!(
        pass.step(&fixture.sender.downgrade(), &fixture.processor)
            .unwrap(),
        Step::Complete
    ));
    assert!(fixture.receiver.try_recv().is_err());
    tokio::time::timeout(
        Duration::from_secs(1),
        ticket.finish(None::<fn() -> std::future::Ready<Result<(), CasperError>>>),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn startup_ready_candidate_records_receipt_before_dequeue() {
    check_startup_publication(Interruption::None).await;
}

#[tokio::test]
async fn startup_receipt_error_preserves_candidate_and_rolls_back_reservations() {
    check_startup_publication(Interruption::ReceiptError).await;
}

#[tokio::test]
async fn startup_byte_rejection_preserves_candidate_without_recording_receipt() {
    check_startup_publication(Interruption::ByteCapacity).await;
}
