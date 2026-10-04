use cordial_consensus::{
    Chain, ChainSpec, DurableBlocklace, ExecutionReceipt, StoreConfig, Validator,
};
use k256::ecdsa::SigningKey;

#[test]
fn accepted_submissions_survive_restart_and_execution_acknowledgment_removes_them_atomically() {
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    let chain = Chain::new(ChainSpec {
        network: "durable-submissions".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    })
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let submission = cordial_consensus::Submission {
        id: [8; 32],
        payload: vec![42],
    };
    let mut state =
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap();
    assert!(state.submit(submission.clone()).unwrap());
    assert!(!state.submit(submission.clone()).unwrap());
    drop(state);
    let mut state =
        DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).unwrap();
    assert_eq!(state.submissions(1).unwrap(), vec![submission.clone()]);
    for _ in 0..3 {
        state.propose(&key, vec![]).unwrap().unwrap();
    }
    state.advance_output().unwrap();
    let request = state.next_execution().unwrap().unwrap();
    state
        .acknowledge_execution(ExecutionReceipt {
            index: request.index,
            object: request.object,
            pre_state: request.pre_state,
            post_state: request.pre_state,
            result: vec![],
            deploy_ids: vec![submission.id],
        })
        .unwrap();
    assert!(state.submissions(1).unwrap().is_empty());
    assert!(!state.submit(submission).unwrap());
}

#[test]
fn journal_map_exhaustion_does_not_advance_execution_or_leave_partial_deploy_indexes() {
    let key = SigningKey::from_slice(&[3; 32]).unwrap();
    let chain = Chain::new(ChainSpec {
        network: "execution-map-full".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    })
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let config = StoreConfig {
        map_size_bytes: 1024 * 1024,
        ..StoreConfig::default()
    };
    let mut state =
        DurableBlocklace::open(directory.path(), chain.clone(), config.clone()).unwrap();
    let mut predecessors = vec![];
    for _ in 0..40 {
        let block = chain.build_block(&key, predecessors, vec![]).unwrap();
        state.admit(&chain.encode_block(&block).unwrap()).unwrap();
        predecessors = vec![block.identity];
    }
    state.advance_output().unwrap();
    let mut failed = None;
    while let Some(request) = state.next_execution().unwrap() {
        let id = [request.index as u8; 32];
        let receipt = ExecutionReceipt {
            index: request.index,
            object: request.object.clone(),
            pre_state: request.pre_state,
            post_state: [request.index as u8; 32],
            result: vec![0; cordial_consensus::MAX_RECEIPT_BYTES],
            deploy_ids: vec![id],
        };
        if let Err(error) = state.acknowledge_execution(receipt) {
            assert!(
                matches!(
                    error,
                    cordial_consensus::Error::Storage(heed::Error::Mdb(heed::MdbError::MapFull))
                ),
                "{error}"
            );
            assert_eq!(state.next_execution().unwrap(), Some(request.clone()));
            assert!(state.execution_receipt(request.index).unwrap().is_none());
            assert!(!state.has_executed_deploy(&id).unwrap());
            failed = Some((request, id));
            break;
        }
    }
    let (request, id) = failed.expect("the real LMDB map must fill");
    assert!(request.index > 0);
    drop(state);
    let recovered = DurableBlocklace::open(directory.path(), chain, config).unwrap();
    assert_eq!(recovered.next_execution().unwrap(), Some(request.clone()));
    assert!(
        recovered
            .execution_receipt(request.index - 1)
            .unwrap()
            .is_some()
    );
    assert!(
        recovered
            .execution_receipt(request.index)
            .unwrap()
            .is_none()
    );
    assert!(!recovered.has_executed_deploy(&id).unwrap());
}

#[test]
fn only_committed_output_executes_and_acknowledgment_survives_restart() {
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    let chain = Chain::new(ChainSpec {
        network: "execution-recovery".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    })
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut state =
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap();
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    state.admit(&chain.encode_block(&root).unwrap()).unwrap();
    assert!(state.next_execution().unwrap().is_none());
    let one = chain
        .build_block(&key, vec![root.identity.clone()], vec![1])
        .unwrap();
    let two = chain
        .build_block(&key, vec![one.identity.clone()], vec![2])
        .unwrap();
    for block in [&one, &two] {
        state.admit(&chain.encode_block(block).unwrap()).unwrap();
    }
    state.advance_output().unwrap();
    let request = state.next_execution().unwrap().unwrap();
    assert_eq!(request.index, 0);
    assert_eq!(request.object, root.identity);
    assert_eq!(request.pre_state, [7; 32]);
    assert!(request.payload.is_empty());
    drop(state);
    let mut state =
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap();
    assert_eq!(state.next_execution().unwrap(), Some(request.clone()));
    let receipt = ExecutionReceipt {
        index: request.index,
        object: request.object.clone(),
        pre_state: request.pre_state,
        post_state: [8; 32],
        result: vec![42],
        deploy_ids: vec![[9; 32]],
    };
    state.acknowledge_execution(receipt.clone()).unwrap();
    assert!(state.next_execution().unwrap().is_none());
    assert_eq!(state.execution_receipt(0).unwrap(), Some(receipt.clone()));
    assert!(state.has_executed_deploy(&[9; 32]).unwrap());
    drop(state);
    let mut recovered =
        DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).unwrap();
    assert!(recovered.next_execution().unwrap().is_none());
    assert_eq!(
        recovered.execution_receipt(0).unwrap(),
        Some(receipt.clone())
    );
    recovered.acknowledge_execution(receipt.clone()).unwrap();
    let mut conflict = receipt;
    conflict.post_state = [99; 32];
    assert!(recovered.acknowledge_execution(conflict).is_err());
    assert!(recovered.has_executed_deploy(&[9; 32]).unwrap());
}

#[test]
fn invalid_receipts_cannot_skip_output_or_reexecute_a_deploy() {
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    let chain = Chain::new(ChainSpec {
        network: "execution-validation".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    })
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut state =
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap();
    let mut predecessors = vec![];
    for round in 0..6 {
        let block = chain
            .build_block(
                &key,
                predecessors,
                if round == 0 { vec![] } else { vec![round] },
            )
            .unwrap();
        state.admit(&chain.encode_block(&block).unwrap()).unwrap();
        predecessors = vec![block.identity];
    }
    state.advance_output().unwrap();
    let request = state.next_execution().unwrap().unwrap();
    let receipt = ExecutionReceipt {
        index: request.index,
        object: request.object.clone(),
        pre_state: request.pre_state,
        post_state: [8; 32],
        result: vec![42],
        deploy_ids: vec![[9; 32]],
    };
    let mut invalid = Vec::new();
    let mut wrong_index = receipt.clone();
    wrong_index.index += 1;
    invalid.push(wrong_index);
    let mut wrong_object = receipt.clone();
    wrong_object.object.content_hash[0] ^= 1;
    invalid.push(wrong_object);
    let mut wrong_state = receipt.clone();
    wrong_state.pre_state = [99; 32];
    invalid.push(wrong_state);
    let mut duplicate_ids = receipt.clone();
    duplicate_ids.deploy_ids.push([9; 32]);
    invalid.push(duplicate_ids);
    let mut oversized = receipt.clone();
    oversized.result = vec![0; cordial_consensus::MAX_RECEIPT_BYTES + 1];
    invalid.push(oversized);
    for invalid in invalid {
        assert!(state.acknowledge_execution(invalid).is_err());
        assert_eq!(state.next_execution().unwrap(), Some(request.clone()));
        assert!(state.execution_receipt(0).unwrap().is_none());
        assert!(!state.has_executed_deploy(&[9; 32]).unwrap());
    }
    state.acknowledge_execution(receipt).unwrap();
    let next = state.next_execution().unwrap().unwrap();
    assert_eq!(next.index, 1);
    assert_eq!(next.pre_state, [8; 32]);
    let mut duplicate = ExecutionReceipt {
        index: next.index,
        object: next.object.clone(),
        pre_state: next.pre_state,
        post_state: [10; 32],
        result: vec![],
        deploy_ids: vec![[9; 32]],
    };
    assert!(state.acknowledge_execution(duplicate.clone()).is_err());
    assert_eq!(state.next_execution().unwrap(), Some(next));
    duplicate.deploy_ids = vec![[10; 32]];
    state.acknowledge_execution(duplicate).unwrap();
    drop(state);
    let recovered =
        DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).unwrap();
    assert_eq!(recovered.next_execution().unwrap().unwrap().index, 2);
    assert!(recovered.has_executed_deploy(&[9; 32]).unwrap());
    assert!(recovered.has_executed_deploy(&[10; 32]).unwrap());
}

fn tamper(
    source: &std::path::Path,
    change: impl FnOnce(
        &heed::Env,
        &mut heed::RwTxn,
        heed::Database<heed::types::Bytes, heed::types::Bytes>,
    ),
) -> tempfile::TempDir {
    let copy = tempfile::tempdir().unwrap();
    std::fs::copy(source.join("data.mdb"), copy.path().join("data.mdb")).unwrap();
    let env = unsafe {
        heed::EnvOpenOptions::new()
            .max_dbs(4)
            .map_size(64 * 1024 * 1024)
            .open(copy.path())
            .unwrap()
    };
    let mut txn = env.write_txn().unwrap();
    let meta: heed::Database<heed::types::Bytes, heed::types::Bytes> =
        env.open_database(&txn, Some("metadata")).unwrap().unwrap();
    change(&env, &mut txn, meta);
    txn.commit().unwrap();
    drop(env);
    copy
}

#[test]
fn tampered_execution_journals_are_rejected_on_recovery() {
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    let chain = Chain::new(ChainSpec {
        network: "execution-tamper".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    })
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    {
        let mut state =
            DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default())
                .unwrap();
        for _ in 0..12 {
            state.propose(&key, vec![]).unwrap();
        }
        state.advance_output().unwrap();
        let mut post = 7u8;
        for round in 0..2u8 {
            let request = state.next_execution().unwrap().expect("committed output");
            post += 1;
            state
                .acknowledge_execution(ExecutionReceipt {
                    index: request.index,
                    object: request.object,
                    pre_state: request.pre_state,
                    post_state: [post; 32],
                    result: vec![],
                    deploy_ids: vec![[round; 32]],
                })
                .unwrap();
        }
    }
    assert!(
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).is_ok()
    );
    let mut deploy_key = b"deploy/".to_vec();
    deploy_key.extend([0u8; 32]);
    let mut receipt_one = b"execution/".to_vec();
    receipt_one.extend(1u64.to_be_bytes());
    let mut orphan = b"execution/".to_vec();
    orphan.extend(9u64.to_be_bytes());
    let cases: Vec<(
        &str,
        Box<
            dyn FnOnce(
                &heed::Env,
                &mut heed::RwTxn,
                heed::Database<heed::types::Bytes, heed::types::Bytes>,
            ),
        >,
    )> = vec![
        (
            "missing deploy index",
            Box::new(move |_, txn, meta| {
                meta.delete(txn, &deploy_key).unwrap();
            }),
        ),
        (
            "orphan receipt",
            Box::new(move |_, txn, meta| {
                meta.put(txn, &orphan, b"x").unwrap();
            }),
        ),
        (
            "cursor beyond output",
            Box::new(|_, txn, meta| {
                meta.put(txn, b"execution-count", &u64::MAX.to_be_bytes())
                    .unwrap();
            }),
        ),
        (
            "broken state chain",
            Box::new(move |_, txn, meta| {
                use bincode::Options;
                let codec = bincode::DefaultOptions::new()
                    .with_fixint_encoding()
                    .reject_trailing_bytes();
                let mut receipt: ExecutionReceipt = codec
                    .deserialize(meta.get(txn, &receipt_one).unwrap().unwrap())
                    .unwrap();
                receipt.pre_state = [42; 32];
                let bytes = codec.serialize(&receipt).unwrap();
                meta.put(txn, &receipt_one, &bytes).unwrap();
            }),
        ),
    ];
    for (name, change) in cases {
        let copy = tamper(directory.path(), change);
        assert!(
            matches!(
                DurableBlocklace::open(copy.path(), chain.clone(), StoreConfig::default()),
                Err(cordial_consensus::Error::Corrupt(_))
            ),
            "{name} was adopted"
        );
    }
}
