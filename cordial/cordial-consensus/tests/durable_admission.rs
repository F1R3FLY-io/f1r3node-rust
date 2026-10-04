use cordial_consensus::{
    Admission, Chain, ChainSpec, DurableBlocklace, Error, StoreConfig, Validator,
};
use k256::ecdsa::SigningKey;

fn fixture() -> (SigningKey, ChainSpec) {
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    let spec = ChainSpec {
        network: "durable-admission".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    };
    (key, spec)
}

#[test]
fn empty_store_reopens_after_initialization() {
    let directory = tempfile::tempdir().unwrap();
    let (_, spec) = fixture();
    let chain = Chain::new(spec).unwrap();
    drop(DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap());
    assert!(DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).is_ok());
}

#[test]
fn deferred_children_survive_restart_and_enter_once_dependencies_arrive() {
    let directory = tempfile::tempdir().unwrap();
    let (key, spec) = fixture();
    let chain = Chain::new(spec).unwrap();
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    let child = chain
        .build_block(&key, vec![root.identity.clone()], vec![42])
        .unwrap();
    let packet = chain.encode_block(&child).unwrap();
    {
        let mut state =
            DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default())
                .unwrap();
        assert_eq!(
            state.admit(&packet).unwrap(),
            Admission::Deferred {
                object: child.identity.clone(),
                missing: vec![root.identity.clone()],
            }
        );
        assert!(state.get(&child.identity).unwrap().is_none());
        assert_eq!(state.pending_count().unwrap(), 1);
    }
    let mut recovered =
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap();
    assert_eq!(recovered.pending_count().unwrap(), 1);
    recovered
        .admit(&chain.encode_block(&root).unwrap())
        .unwrap();
    assert_eq!(recovered.get(&child.identity).unwrap(), Some(child.clone()));
    assert_eq!(recovered.pending_count().unwrap(), 0);
    assert_eq!(
        recovered.admit(&packet).unwrap(),
        Admission::Duplicate(child.identity)
    );
}

#[test]
fn admitted_native_objects_survive_restart_and_duplicates_are_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let (key, spec) = fixture();
    let chain = Chain::new(spec).unwrap();
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    let child = chain
        .build_block(&key, vec![root.identity.clone()], vec![42])
        .unwrap();
    let root_packet = chain.encode_block(&root).unwrap();
    let child_packet = chain.encode_block(&child).unwrap();
    {
        let mut state =
            DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default())
                .unwrap();
        assert_eq!(
            state.admit(&root_packet).unwrap(),
            Admission::Accepted(root.identity.clone())
        );
        assert_eq!(
            state.admit(&child_packet).unwrap(),
            Admission::Accepted(child.identity.clone())
        );
        assert_eq!(
            state.admit(&child_packet).unwrap(),
            Admission::Duplicate(child.identity.clone())
        );
    }
    let mut recovered =
        DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).unwrap();
    assert_eq!(recovered.get(&root.identity).unwrap(), Some(root));
    assert_eq!(recovered.get(&child.identity).unwrap(), Some(child.clone()));
    assert_eq!(
        recovered.admit(&child_packet).unwrap(),
        Admission::Duplicate(child.identity)
    );
}

#[test]
fn a_store_has_one_owner_and_cannot_reopen_under_another_chain() {
    let directory = tempfile::tempdir().unwrap();
    let (key, spec) = fixture();
    let chain = Chain::new(spec.clone()).unwrap();
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    let mut state =
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap();
    state.admit(&chain.encode_block(&root).unwrap()).unwrap();
    assert!(matches!(
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()),
        Err(Error::InUse)
    ));
    drop(state);
    let mut foreign = spec;
    foreign.network = "another-chain".into();
    assert!(matches!(
        DurableBlocklace::open(
            directory.path(),
            Chain::new(foreign).unwrap(),
            StoreConfig::default()
        ),
        Err(Error::ChainMismatch)
    ));
    let recovered =
        DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).unwrap();
    assert_eq!(recovered.get(&root.identity).unwrap(), Some(root));
}

#[test]
fn failed_durable_write_cannot_publish_a_live_admission() {
    let directory = tempfile::tempdir().unwrap();
    let (key, spec) = fixture();
    let chain = Chain::new(spec).unwrap();
    let config = StoreConfig {
        map_size_bytes: 1024 * 1024,
        ..StoreConfig::default()
    };
    let mut state =
        DurableBlocklace::open(directory.path(), chain.clone(), config.clone()).unwrap();
    let mut previous = chain.build_block(&key, vec![], vec![]).unwrap();
    state
        .admit(&chain.encode_block(&previous).unwrap())
        .unwrap();
    let mut failed = None;
    for tag in 0..32 {
        let block = chain
            .build_block(
                &key,
                vec![previous.identity.clone()],
                vec![tag; cordial_consensus::MAX_PAYLOAD_BYTES - 100],
            )
            .unwrap();
        match state.admit(&chain.encode_block(&block).unwrap()) {
            Ok(Admission::Accepted(_)) => previous = block,
            Err(Error::Storage(heed::Error::Mdb(heed::MdbError::MapFull))) => {
                assert!(state.get(&block.identity).unwrap().is_none());
                failed = Some(block.identity);
                break;
            }
            other => panic!("unexpected admission result: {other:?}"),
        }
    }
    let failed = failed.expect("the test must exhaust the real LMDB map");
    assert_eq!(
        state.get(&previous.identity).unwrap(),
        Some(previous.clone())
    );
    drop(state);
    let recovered = DurableBlocklace::open(directory.path(), chain, config).unwrap();
    assert!(recovered.get(&failed).unwrap().is_none());
    assert_eq!(recovered.get(&previous.identity).unwrap(), Some(previous));
}

#[test]
fn pending_limits_survive_duplicates_and_release_after_dependency_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let (key, spec) = fixture();
    let chain = Chain::new(spec).unwrap();
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    let first = chain
        .build_block(&key, vec![root.identity.clone()], vec![1])
        .unwrap();
    let second = chain
        .build_block(&key, vec![root.identity.clone()], vec![2])
        .unwrap();
    let config = StoreConfig {
        max_pending_objects: 1,
        ..StoreConfig::default()
    };
    let mut state = DurableBlocklace::open(directory.path(), chain.clone(), config).unwrap();
    let packet = chain.encode_block(&first).unwrap();
    assert!(matches!(
        state.admit(&packet).unwrap(),
        Admission::Deferred { .. }
    ));
    assert!(matches!(
        state.admit(&packet).unwrap(),
        Admission::Deferred { .. }
    ));
    assert_eq!(state.pending_count().unwrap(), 1);
    assert!(matches!(
        state.admit(&chain.encode_block(&second).unwrap()),
        Err(Error::Capacity)
    ));
    state.admit(&chain.encode_block(&root).unwrap()).unwrap();
    assert_eq!(state.pending_count().unwrap(), 0);
    assert_eq!(
        state.admit(&chain.encode_block(&second).unwrap()).unwrap(),
        Admission::Accepted(second.identity.clone())
    );
    assert!(state.get(&first.identity).unwrap().is_some());
    assert!(state.get(&second.identity).unwrap().is_some());
}

#[test]
fn pending_byte_limit_does_not_persist_an_oversized_deferred_object() {
    let directory = tempfile::tempdir().unwrap();
    let (key, spec) = fixture();
    let chain = Chain::new(spec).unwrap();
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    let child = chain
        .build_block(&key, vec![root.identity], vec![1])
        .unwrap();
    let config = StoreConfig {
        max_pending_bytes: 1,
        ..StoreConfig::default()
    };
    let mut state =
        DurableBlocklace::open(directory.path(), chain.clone(), config.clone()).unwrap();
    assert!(matches!(
        state.admit(&chain.encode_block(&child).unwrap()),
        Err(Error::Capacity)
    ));
    assert_eq!(state.pending_count().unwrap(), 0);
    drop(state);
    let state = DurableBlocklace::open(directory.path(), chain, config).unwrap();
    assert_eq!(state.pending_count().unwrap(), 0);
    assert!(state.get(&child.identity).unwrap().is_none());
}

#[test]
fn unidentified_or_truncated_stores_are_not_adopted() {
    let directory = tempfile::tempdir().unwrap();
    let (_, spec) = fixture();
    let chain = Chain::new(spec).unwrap();
    std::fs::write(directory.path().join("unrelated-data"), b"preserve").unwrap();
    assert!(
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).is_err()
    );
    assert_eq!(
        std::fs::read(directory.path().join("unrelated-data")).unwrap(),
        b"preserve"
    );
    let corrupt = tempfile::tempdir().unwrap();
    drop(DurableBlocklace::open(corrupt.path(), chain.clone(), StoreConfig::default()).unwrap());
    std::fs::OpenOptions::new()
        .write(true)
        .open(corrupt.path().join("data.mdb"))
        .unwrap()
        .set_len(0)
        .unwrap();
    assert!(DurableBlocklace::open(corrupt.path(), chain, StoreConfig::default()).is_err());
}

#[test]
fn equivocation_evidence_is_durable_and_both_branches_remain() {
    let directory = tempfile::tempdir().unwrap();
    let (key, spec) = fixture();
    let chain = Chain::new(spec).unwrap();
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    let left = chain
        .build_block(&key, vec![root.identity.clone()], vec![1])
        .unwrap();
    let right = chain
        .build_block(&key, vec![root.identity.clone()], vec![2])
        .unwrap();
    let mut expected = vec![left.identity.clone(), right.identity.clone()];
    expected.sort();
    {
        let mut state =
            DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default())
                .unwrap();
        assert!(state.equivocations(64).unwrap().is_empty());
        for block in [&root, &left, &right] {
            state.admit(&chain.encode_block(block).unwrap()).unwrap();
        }
        let records = state.equivocations(64).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].objects, expected);
        assert_eq!(records[0].creator, key.verifying_key().to_sec1_bytes().to_vec());
    }
    let recovered =
        DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).unwrap();
    assert_eq!(recovered.equivocations(64).unwrap()[0].objects, expected);
    assert!(recovered.get(&left.identity).unwrap().is_some());
    assert!(recovered.get(&right.identity).unwrap().is_some());
    assert!(recovered.equivocations(0).is_err());
}

#[test]
fn reverse_order_chain_is_released_through_dependency_index() {
    let directory = tempfile::tempdir().unwrap();
    let (key, spec) = fixture();
    let chain = Chain::new(spec).unwrap();
    let mut blocks = vec![chain.build_block(&key, vec![], vec![]).unwrap()];
    for value in 0..40u8 {
        let parent = blocks.last().unwrap().identity.clone();
        blocks.push(chain.build_block(&key, vec![parent], vec![value]).unwrap());
    }
    let mut state =
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap();
    for block in blocks.iter().skip(1).rev() {
        assert!(matches!(
            state.admit(&chain.encode_block(block).unwrap()).unwrap(),
            Admission::Deferred { .. }
        ));
    }
    assert_eq!(state.pending_count().unwrap(), 40);
    state.admit(&chain.encode_block(&blocks[0]).unwrap()).unwrap();
    assert_eq!(state.pending_count().unwrap(), 0);
    assert_eq!(state.admitted_count(), 41);
    drop(state);
    let recovered = DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).unwrap();
    assert_eq!(recovered.admitted_count(), 41);
    assert_eq!(recovered.pending_count().unwrap(), 0);
}
