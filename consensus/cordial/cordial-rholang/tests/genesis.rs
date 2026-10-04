use cordial_consensus::{ChainSpec, Validator};
use cordial_rholang::{GenesisSettings, VaultAllocation, initialize_runtime};
use k256::ecdsa::SigningKey;

#[tokio::test]
async fn independent_execution_stores_derive_the_same_pinned_genesis() {
    let key = SigningKey::from_slice(&[3; 32]).unwrap();
    let spec = ChainSpec {
        network: "genesis-profile".into(),
        shard: "root".into(),
        execution_genesis: [0; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    };
    let genesis = GenesisSettings {
        timestamp: 1_700_000_000_000,
        vaults: vec![VaultAllocation {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            balance: 1_000_000_000,
        }],
        ..GenesisSettings::default()
    };
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (mut first_manager, first_vm, root) = initialize_runtime(first.path(), &spec, &genesis)
        .await
        .unwrap();
    assert_ne!(root, [0; 32]);
    let (mut second_manager, second_vm, other) = initialize_runtime(second.path(), &spec, &genesis)
        .await
        .unwrap();
    assert_eq!(root, other);
    drop(first_vm);
    drop(second_vm);
    first_manager.shutdown().await.unwrap();
    second_manager.shutdown().await.unwrap();
}
