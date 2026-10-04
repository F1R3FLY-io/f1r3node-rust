use consensus_api::{ConsensusError, NetworkPacket, Peer, Phase};
use consensus_runtime::RuntimeConfig;
use cordial_consensus::{
    BLOCK_PACKET_KIND, Chain, ChainSpec, CordialIngressAdapter, DurableBlocklace, StoreConfig,
    Validator,
};
use k256::ecdsa::SigningKey;

fn fixture() -> (SigningKey, Chain) {
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    let chain = Chain::new(ChainSpec {
        network: "runtime-ingress".into(),
        shard: "root".into(),
        execution_genesis: [7; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: key.verifying_key().to_sec1_bytes().to_vec(),
            weight: 1,
        }],
    })
    .unwrap();
    (key, chain)
}

fn packet(payload: Vec<u8>) -> NetworkPacket {
    NetworkPacket {
        peer: Peer {
            id: vec![1],
            host: "localhost".into(),
            tcp_port: 40400,
            udp_port: 40404,
        },
        kind: BLOCK_PACKET_KIND.into(),
        payload,
    }
}

#[tokio::test]
async fn runtime_admits_native_packets_recovers_output_and_releases_store_on_shutdown() {
    let (key, chain) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("consensus");
    let prepared = CordialIngressAdapter::prepare(
        path.clone(),
        chain.clone(),
        StoreConfig::default(),
        RuntimeConfig::default(),
    )
    .unwrap();
    assert!(!path.exists());
    let runtime = prepared.start();
    let handle = runtime.handle();
    handle.wait_ready().await.unwrap();
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    let one = chain
        .build_block(&key, vec![root.identity.clone()], vec![1])
        .unwrap();
    let two = chain
        .build_block(&key, vec![one.identity.clone()], vec![2])
        .unwrap();
    for block in [&two, &one, &root, &root] {
        handle
            .handle_packet(packet(chain.encode_block(block).unwrap()))
            .await
            .unwrap();
    }
    assert_eq!(
        handle.propose(false).await,
        Err(ConsensusError::UnsupportedCapability("propose"))
    );
    assert_eq!(
        handle.submit(vec![]).await,
        Err(ConsensusError::UnsupportedCapability("submit"))
    );
    runtime.shutdown().await.unwrap();
    assert_eq!(handle.status().phase, Phase::Stopped);
    assert_eq!(
        handle.handle_packet(packet(vec![])).await,
        Err(ConsensusError::Stopped)
    );
    let state = DurableBlocklace::open(&path, chain, StoreConfig::default()).unwrap();
    assert_eq!(state.pending_count().unwrap(), 0);
    assert!(state.get(&two.identity).unwrap().is_some());
    assert_eq!(state.ordered_output().unwrap(), vec![root.identity]);
}

#[tokio::test]
async fn invalid_network_packets_do_not_fail_runtime_health() {
    let (key, chain) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let runtime = CordialIngressAdapter::prepare(
        directory.path().to_owned(),
        chain.clone(),
        StoreConfig::default(),
        RuntimeConfig::default(),
    )
    .unwrap()
    .start();
    let handle = runtime.handle();
    handle.wait_ready().await.unwrap();
    assert_eq!(
        handle
            .handle_packet(packet(vec![0; cordial_consensus::MAX_PACKET_BYTES + 1]))
            .await,
        Err(ConsensusError::InvalidInput(
            "payload limit exceeded".into()
        ))
    );
    assert!(matches!(
        handle.handle_packet(packet(vec![1, 2])).await,
        Err(ConsensusError::InvalidInput(_))
    ));
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    let mut wrong_kind = packet(chain.encode_block(&root).unwrap());
    wrong_kind.kind = "CasperMessage".into();
    assert!(matches!(
        handle.handle_packet(wrong_kind).await,
        Err(ConsensusError::InvalidInput(_))
    ));
    handle
        .handle_packet(packet(chain.encode_block(&root).unwrap()))
        .await
        .unwrap();
    assert_eq!(handle.status().phase, Phase::Ready);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn recovery_publishes_native_output_before_ready() {
    let (key, chain) = fixture();
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
    let runtime = CordialIngressAdapter::prepare(
        directory.path().to_owned(),
        chain.clone(),
        StoreConfig::default(),
        RuntimeConfig::default(),
    )
    .unwrap()
    .start();
    runtime.handle().wait_ready().await.unwrap();
    runtime.shutdown().await.unwrap();
    let state = DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).unwrap();
    assert_eq!(state.ordered_output().unwrap(), vec![root.identity]);
}

#[tokio::test]
async fn incompatible_store_fails_before_readiness() {
    let (_, chain) = fixture();
    let directory = tempfile::tempdir().unwrap();
    drop(DurableBlocklace::open(directory.path(), chain.clone(), StoreConfig::default()).unwrap());
    let mut spec = chain.spec().clone();
    spec.network = "other-network".into();
    let mut runtime = CordialIngressAdapter::prepare(
        directory.path().to_owned(),
        Chain::new(spec).unwrap(),
        StoreConfig::default(),
        RuntimeConfig::default(),
    )
    .unwrap()
    .start();
    let handle = runtime.handle();
    assert!(handle.wait_ready().await.is_err());
    assert!(runtime.wait().await.is_err());
    assert_eq!(handle.status().phase, Phase::Failed);
    assert!(DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).is_ok());
}

#[tokio::test]
async fn exhausted_pending_capacity_is_recoverable_when_dependencies_arrive() {
    let (key, chain) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let config = StoreConfig {
        max_pending_objects: 1,
        ..StoreConfig::default()
    };
    let runtime = CordialIngressAdapter::prepare(
        directory.path().to_owned(),
        chain.clone(),
        config,
        RuntimeConfig::default(),
    )
    .unwrap()
    .start();
    let handle = runtime.handle();
    handle.wait_ready().await.unwrap();
    let root = chain.build_block(&key, vec![], vec![]).unwrap();
    let first = chain
        .build_block(&key, vec![root.identity.clone()], vec![1])
        .unwrap();
    let second = chain
        .build_block(&key, vec![root.identity.clone()], vec![2])
        .unwrap();
    handle
        .handle_packet(packet(chain.encode_block(&first).unwrap()))
        .await
        .unwrap();
    assert_eq!(
        handle
            .handle_packet(packet(chain.encode_block(&second).unwrap()))
            .await,
        Err(ConsensusError::QueueFull)
    );
    assert_eq!(handle.status().phase, Phase::Ready);
    handle
        .handle_packet(packet(chain.encode_block(&root).unwrap()))
        .await
        .unwrap();
    handle
        .handle_packet(packet(chain.encode_block(&second).unwrap()))
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    let state = DurableBlocklace::open(directory.path(), chain, StoreConfig::default()).unwrap();
    assert!(state.get(&first.identity).unwrap().is_some());
    assert!(state.get(&second.identity).unwrap().is_some());
}

#[tokio::test]
async fn storage_failure_stops_the_runtime_without_losing_previous_admission() {
    let (key, chain) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let config = StoreConfig {
        map_size_bytes: 1024 * 1024,
        ..StoreConfig::default()
    };
    let mut runtime = CordialIngressAdapter::prepare(
        directory.path().to_owned(),
        chain.clone(),
        config.clone(),
        RuntimeConfig::default(),
    )
    .unwrap()
    .start();
    let handle = runtime.handle();
    handle.wait_ready().await.unwrap();
    let mut previous = chain.build_block(&key, vec![], vec![]).unwrap();
    handle
        .handle_packet(packet(chain.encode_block(&previous).unwrap()))
        .await
        .unwrap();
    let mut failure = None;
    for tag in 0..32 {
        let block = chain
            .build_block(
                &key,
                vec![previous.identity.clone()],
                vec![tag; cordial_consensus::MAX_PAYLOAD_BYTES - 100],
            )
            .unwrap();
        match handle
            .handle_packet(packet(chain.encode_block(&block).unwrap()))
            .await
        {
            Ok(()) => previous = block,
            Err(error @ ConsensusError::Protocol(_)) => {
                failure = Some(error);
                break;
            }
            other => panic!("unexpected packet result: {other:?}"),
        }
    }
    assert!(failure.is_some(), "the real LMDB map must fill");
    assert!(runtime.wait().await.is_err());
    assert_eq!(handle.status().phase, Phase::Failed);
    let state = DurableBlocklace::open(directory.path(), chain, config).unwrap();
    assert!(state.get(&previous.identity).unwrap().is_some());
}
