use cordial_consensus::{Chain, ChainSpec, Error, Validator};
use k256::ecdsa::SigningKey;

fn fixture() -> (SigningKey, ChainSpec) {
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    let spec = ChainSpec {
        network: "cordial-network-a".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
        wavelength: 3,
    };
    (key, spec)
}

#[test]
fn signed_native_object_is_bound_to_the_configured_chain() {
    let (key, spec) = fixture();
    let chain = Chain::new(spec.clone()).unwrap();
    let block = chain.build_block(&key, vec![], vec![]).unwrap();
    let packet = chain.encode_block(&block).unwrap();
    assert_eq!(chain.decode_block(&packet).unwrap(), block);
    let mut foreign_spec = spec;
    foreign_spec.network = "cordial-network-b".into();
    let foreign = Chain::new(foreign_spec).unwrap();
    assert!(matches!(
        foreign.decode_block(&packet),
        Err(Error::ChainMismatch)
    ));
}

#[test]
fn chain_identity_covers_shard_genesis_weights_and_wavelength() {
    let (key, spec) = fixture();
    let chain = Chain::new(spec.clone()).unwrap();
    let packet = chain
        .encode_block(&chain.build_block(&key, vec![], vec![]).unwrap())
        .unwrap();
    let mut variants = Vec::new();
    let mut shard = spec.clone();
    shard.shard = "different-shard".into();
    variants.push(shard);
    let mut genesis = spec.clone();
    genesis.execution_genesis[0] ^= 1;
    variants.push(genesis);
    let mut weight = spec.clone();
    weight.validators[0].weight = 2;
    variants.push(weight);
    let mut wavelength = spec;
    wavelength.wavelength = 5;
    variants.push(wavelength);
    for spec in variants {
        assert!(matches!(
            Chain::new(spec).unwrap().decode_block(&packet),
            Err(Error::ChainMismatch)
        ));
    }
}

#[test]
fn malformed_packets_and_unknown_validators_cannot_enter_the_chain() {
    let (key, spec) = fixture();
    let chain = Chain::new(spec).unwrap();
    let block = chain.build_block(&key, vec![], vec![]).unwrap();
    let bytes = chain.encode_block(&block).unwrap();
    let mut wrong_version = bytes.clone();
    wrong_version[..4].copy_from_slice(&1u32.to_le_bytes());
    assert!(chain.decode_block(&wrong_version).is_err());
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(chain.decode_block(&trailing).is_err());
    assert!(chain.decode_block(&bytes[..bytes.len() - 1]).is_err());
    assert!(
        chain
            .decode_block(&vec![0; cordial_consensus::MAX_PACKET_BYTES + 1])
            .is_err()
    );
    let mut tampered = bytes;
    *tampered.last_mut().unwrap() ^= 1;
    assert!(chain.decode_block(&tampered).is_err());
    let stranger = SigningKey::from_slice(&[2; 32]).unwrap();
    assert!(matches!(
        chain.build_block(&stranger, vec![], vec![]),
        Err(Error::UnknownValidator)
    ));
}

#[test]
fn configuration_rejects_duplicate_keys_aliases_and_zero_weights() {
    let (key, spec) = fixture();
    let mut duplicate = spec.clone();
    duplicate.validators.push(duplicate.validators[0].clone());
    assert!(Chain::new(duplicate).is_err());
    let mut alias = spec.clone();
    alias.validators[0].public_key = key
        .verifying_key()
        .to_encoded_point(false)
        .as_bytes()
        .to_vec();
    assert!(Chain::new(alias).is_err());
    let mut zero = spec;
    zero.validators[0].weight = 0;
    assert!(Chain::new(zero).is_err());
}
