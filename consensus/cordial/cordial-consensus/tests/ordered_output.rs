use cordial_consensus::{Chain, ChainSpec, DurableBlocklace, StoreConfig, Validator};
use cordial_miners_core::NodeId;
use cordial_miners_core::blocklace::Blocklace;
use cordial_miners_core::consensus::{validated_received_insert, weighted_tau};
use k256::ecdsa::SigningKey;

#[test]
fn persisted_output_matches_native_tau_across_delivery_orders_and_restart() {
    let keys: Vec<_> = (1..=4)
        .map(|seed| SigningKey::from_slice(&[seed; 32]).unwrap())
        .collect();
    let chain = Chain::new(ChainSpec {
        network: "ordered-output".into(),
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
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let mut first =
        DurableBlocklace::open(first_dir.path(), chain.clone(), StoreConfig::default()).unwrap();
    let mut second =
        DurableBlocklace::open(second_dir.path(), chain.clone(), StoreConfig::default()).unwrap();
    let mut reference = Blocklace::new();
    let mut predecessors = Vec::new();
    let mut prefix = Vec::new();
    let validators = &chain.spec().validators;
    let leader = |wave| {
        Some(NodeId(
            validators[(wave % validators.len() as u64) as usize]
                .public_key
                .clone(),
        ))
    };
    for round in 0..12 {
        let blocks: Vec<_> = keys
            .iter()
            .map(|key| {
                chain
                    .build_block(
                        key,
                        predecessors.clone(),
                        if round == 0 { vec![] } else { vec![round] },
                    )
                    .unwrap()
            })
            .collect();
        for block in &blocks {
            first.admit(&chain.encode_block(block).unwrap()).unwrap();
            assert!(
                validated_received_insert(block.clone(), &mut reference, chain.weights())
                    .is_valid()
            );
        }
        for block in blocks.iter().rev() {
            second.admit(&chain.encode_block(block).unwrap()).unwrap();
        }
        first.advance_output().unwrap();
        second.advance_output().unwrap();
        let expected = weighted_tau(&reference, 3, chain.weights(), leader).unwrap();
        let current = first.ordered_output().unwrap();
        assert_eq!(current, expected);
        assert_eq!(current, second.ordered_output().unwrap());
        assert!(current.starts_with(&prefix));
        prefix = current;
        predecessors = blocks.into_iter().map(|b| b.identity).collect();
    }
    assert!(prefix.len() >= 25);
    drop(first);
    let mut recovered =
        DurableBlocklace::open(first_dir.path(), chain, StoreConfig::default()).unwrap();
    assert_eq!(recovered.ordered_output().unwrap(), prefix);
    assert_eq!(recovered.advance_output().unwrap(), 0);
}

#[test]
fn restart_recovers_the_gap_between_durable_admission_and_output_publication() {
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    let chain = Chain::new(ChainSpec {
        network: "output-gap".into(),
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
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    let one = chain
        .build_block(&key, vec![root.identity.clone()], vec![1])
        .unwrap();
    let two = chain
        .build_block(&key, vec![one.identity.clone()], vec![2])
        .unwrap();
    {
        let mut state =
            DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default())
                .unwrap();
        for block in [&root, &one, &two] {
            state.admit(&chain.encode_block(block).unwrap()).unwrap();
        }
        assert!(state.ordered_output().unwrap().is_empty());
    }
    let mut state =
        DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).unwrap();
    assert_eq!(state.advance_output().unwrap(), 1);
    assert_eq!(state.ordered_output().unwrap(), vec![root.identity]);
    assert_eq!(state.advance_output().unwrap(), 0);
}
