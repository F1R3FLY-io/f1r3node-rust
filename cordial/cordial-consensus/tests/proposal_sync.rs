use cordial_consensus::{Chain, ChainSpec, DurableBlocklace, StoreConfig, Validator};
use k256::ecdsa::SigningKey;

#[test]
fn bounded_history_pages_allow_an_empty_peer_to_recover_native_output() {
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    let chain = Chain::new(ChainSpec {
        network: "paged-sync".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    })
    .unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut source =
        DurableBlocklace::open(first.path(), chain.clone(), StoreConfig::default()).unwrap();
    for _ in 0..12 {
        source.propose(&key, vec![]).unwrap().unwrap();
    }
    source.advance_output().unwrap();
    let mut target = DurableBlocklace::open(second.path(), chain, StoreConfig::default()).unwrap();
    let mut cursor = 0;
    loop {
        let page = source.history_page(cursor, 3).unwrap();
        assert!(page.packets.len() <= 3);
        for packet in &page.packets {
            target.admit(packet).unwrap();
        }
        cursor = page.next;
        if cursor == page.total {
            break;
        }
    }
    assert_eq!(cursor, 12);
    target.advance_output().unwrap();
    assert_eq!(
        source.ordered_output().unwrap(),
        target.ordered_output().unwrap()
    );
    assert!(!target.ordered_output().unwrap().is_empty());
    assert!(source.history_page(0, 0).is_err());
    assert!(source.history_page(13, 3).is_err());
    assert!(source.history_page(0, 33).is_err());
}

#[test]
fn native_proposal_waits_for_quorum_and_continues_its_persisted_history_after_restart() {
    let keys: Vec<_> = (1..=4)
        .map(|value| SigningKey::from_slice(&[value; 32]).unwrap())
        .collect();
    let chain = Chain::new(ChainSpec {
        network: "proposal-recovery".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: keys
            .iter()
            .map(|key| Validator {
                public_key: key.verifying_key().to_sec1_bytes().to_vec(),
                weight: 1,
            })
            .collect(),
    })
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut state =
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap();
    let root = state.propose(&keys[0], vec![]).unwrap().unwrap();
    assert!(root.content.predecessors.is_empty());
    assert!(state.propose(&keys[0], vec![42]).unwrap().is_none());
    for key in &keys[1..] {
        let block = chain.build_block(key, vec![], vec![]).unwrap();
        state.admit(&chain.encode_block(&block).unwrap()).unwrap();
    }
    let one = state.propose(&keys[0], vec![42]).unwrap().unwrap();
    assert!(one.content.predecessors.contains(&root.identity));
    assert_eq!(chain.application_data(&one).unwrap(), vec![42]);
    assert!(state.propose(&keys[0], vec![]).unwrap().is_none());
    let second = state.propose(&keys[1], vec![]).unwrap().unwrap();
    assert!(second.content.predecessors.contains(&root.identity));
    let third = state.propose(&keys[2], vec![]).unwrap().unwrap();
    drop(state);
    let mut recovered =
        DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap();
    let two = recovered.propose(&keys[0], vec![43]).unwrap().unwrap();
    assert!(two.content.predecessors.contains(&one.identity));
    assert!(two.content.predecessors.contains(&second.identity));
    assert!(two.content.predecessors.contains(&third.identity));
    assert!(recovered.get(&one.identity).unwrap().is_some());
}
