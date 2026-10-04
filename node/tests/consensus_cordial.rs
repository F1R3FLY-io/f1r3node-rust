use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::errors::CommError;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::{RPConf, RPConfCell};
use comm::rust::transport::transport_layer::{Blob, TransportLayer};
use models::routing::Protocol;
use node::rust::configuration::model::{ConsensusConf, ConsensusProtocol};
use node::rust::configuration::NodeConf;
use node::rust::runtime::setup::{setup_node_program, PreparedApplication};
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

#[derive(Clone)]
struct NoPeers;

#[async_trait::async_trait]
impl TransportLayer for NoPeers {
    async fn send(&self, _: &PeerNode, _: &Protocol) -> Result<(), CommError> { Ok(()) }
    async fn broadcast(&self, _: &[PeerNode], _: &Protocol) -> Result<(), CommError> { Ok(()) }
    async fn stream(&self, _: &PeerNode, _: &Blob) -> Result<(), CommError> { Ok(()) }
    async fn stream_mult(&self, _: &[PeerNode], _: &Blob) -> Result<(), CommError> { Ok(()) }
    async fn disconnect(&self, _: &PeerNode) -> Result<(), CommError> { Ok(()) }
    async fn get_channeled_peers(&self) -> Result<HashSet<PeerNode>, CommError> {
        Ok(HashSet::new())
    }
}

#[async_trait::async_trait]
impl NodeDiscovery for NoPeers {
    async fn discover(&self) -> Result<(), CommError> { Ok(()) }
    fn peers(&self) -> Result<Vec<PeerNode>, CommError> { Ok(vec![]) }
    fn remove_peer(&self, _: &PeerNode) -> Result<(), CommError> { Ok(()) }
}

fn peer_configuration(conf: &NodeConf) -> RPConfCell {
    RPConfCell::new(RPConf::new(
        PeerNode {
            id: NodeIdentifier {
                key: vec![1; 20].into(),
            },
            endpoint: Endpoint::new("127.0.0.1".into(), 40400, 40404),
        },
        conf.protocol_server.network_id.clone(),
        None,
        Duration::from_secs(1),
        10,
        10,
    ))
}

async fn prepare(conf: NodeConf) -> eyre::Result<node::rust::runtime::setup::PreparedNode> {
    setup_node_program(
        ConnectionsCell::new(),
        peer_configuration(&conf),
        Arc::new(NoPeers),
        conf,
        F1r3flyEvents::new(),
        Arc::new(NoPeers),
    )
    .await
}

#[test]
fn cordial_selection_requires_its_own_configuration() {
    let conf: ConsensusConf = serde_json::from_str(
        r#"{"protocol":"cordial-miners","cordial":{"chain-file":"chain.json","tick-ms":1000}}"#,
    )
    .unwrap();
    assert_eq!(conf.protocol, ConsensusProtocol::CordialMiners);
    assert_eq!(
        conf.cordial.unwrap().chain_file.to_str(),
        Some("chain.json")
    );
    assert!(serde_json::from_str::<ConsensusConf>(r#"{"protocol":"unknown"}"#).is_err());
}

#[test]
fn cordial_manifest_rejects_changed_chain_and_second_owner() {
    use node::rust::consensus::cordial::{ChainConfig, DirectoryGuard};
    let directory = tempfile::tempdir().unwrap();
    let key = crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    let (_, public) = key.new_key_pair();
    let compressed = k256::ecdsa::VerifyingKey::from_sec1_bytes(&public.bytes)
        .unwrap()
        .to_sec1_bytes()
        .to_vec();
    let mut config = ChainConfig {
        chain: cordial_consensus::ChainSpec {
            network: "cordial-test".into(),
            shard: "root".into(),
            execution_genesis: [1; 32],
            wavelength: 3,
            validators: vec![cordial_consensus::Validator {
                public_key: compressed,
                weight: 1,
            }],
        },
        genesis: cordial_rholang::GenesisSettings::default(),
    };
    let guard = DirectoryGuard::open(directory.path(), &config).unwrap();
    assert!(DirectoryGuard::open(directory.path(), &config).is_err());
    drop(guard);
    config.chain.wavelength = 5;
    assert!(DirectoryGuard::open(directory.path(), &config).is_err());
    config.chain.wavelength = 3;
    config.genesis.timestamp += 1;
    assert!(DirectoryGuard::open(directory.path(), &config).is_err());
    assert!(node::rust::consensus::manifest::ManifestGuard::inspect(
        directory.path(),
        "cordial-test",
        "root"
    )
    .is_err());
    assert!(!directory.path().join("cordial-execution").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cordial_factory_serves_deploys_and_recovers_without_casper_stores() {
    use cordial_rholang::{initialize_runtime, GenesisSettings, VaultAllocation};
    use node::rust::consensus::cordial::ChainConfig;
    use prost::Message;
    use tower::ServiceExt;
    tokio::time::timeout(Duration::from_secs(120), async {
        let directory = tempfile::tempdir().unwrap();
        let seed = [3; 32];
        let key = k256::ecdsa::SigningKey::from_slice(&seed).unwrap();
        let public = key.verifying_key().to_sec1_bytes().to_vec();
        let mut config = ChainConfig {
            chain: cordial_consensus::ChainSpec {
                network: "cordial-factory-test".into(),
                shard: "root".into(),
                execution_genesis: [0; 32],
                wavelength: 3,
                validators: vec![cordial_consensus::Validator {
                    public_key: public.clone(),
                    weight: 1,
                }],
            },
            genesis: GenesisSettings {
                vaults: vec![VaultAllocation {
                    public_key: public,
                    balance: 1_000_000_000,
                }],
                ..GenesisSettings::default()
            },
        };
        let (mut manager, vm, root) = initialize_runtime(
            &directory.path().join("genesis-build"),
            &config.chain,
            &config.genesis,
        )
        .await
        .unwrap();
        drop(vm);
        manager.shutdown().await.unwrap();
        drop(manager);
        config.chain.execution_genesis = root;
        let chain_file = directory.path().join("chain.json");
        std::fs::write(&chain_file, serde_json::to_vec(&config).unwrap()).unwrap();
        let key_file = directory.path().join("validator.key");
        std::fs::write(&key_file, hex::encode(seed)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut conf: NodeConf = hocon::HoconLoader::new()
            .load_str(include_str!("../src/main/resources/defaults.conf"))
            .unwrap()
            .resolve()
            .unwrap();
        conf.storage.data_dir = directory.path().join("node");
        conf.protocol_server.network_id = config.chain.network.clone();
        conf.protocol_client.network_id = config.chain.network.clone();
        conf.openai.enabled = false;
        conf.consensus = ConsensusConf {
            protocol: ConsensusProtocol::CordialMiners,
            cordial: Some(node::rust::configuration::model::CordialConf {
                chain_file,
                validator_key_file: Some(key_file),
                tick_ms: 100,
            }),
        };
        let prepared = prepare(conf.clone()).await.unwrap();
        let queries = match &prepared.application {
            PreparedApplication::Cordial(app) => app.queries.clone(),
            _ => panic!("wrong adapter"),
        };
        let runtime = prepared.consensus.start();
        runtime.handle().wait_ready().await.unwrap();
        assert_eq!(runtime.handle().status().protocol.id, "cordial-miners");
        assert!(matches!(
            runtime.handle().finalized().await,
            Err(consensus_api::ConsensusError::UnsupportedCapability(_))
        ));
        let events = F1r3flyEvents::new();
        let routes = prepared
            .application
            .routes(
                &conf,
                peer_configuration(&conf),
                ConnectionsCell::new(),
                Arc::new(NoPeers),
                events.consume(),
                events.startup_buffer(),
            )
            .await
            .unwrap();
        let deploy = casper::rust::util::construct_deploy::source_deploy_now(
            "@42!(42)".into(),
            Some(crypto::rust::private_key::PrivateKey::from_bytes(&seed)),
            Some(0),
            Some("root".into()),
        )
        .unwrap();
        let payload = models::rust::casper::protocol::casper_message::DeployData::to_proto(deploy)
            .encode_to_vec();
        let response = routes
            .public_http
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/deploy")
                    .body(axum::body::Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let mut success = None;
        loop {
            let progress = queries.progress().await.unwrap();
            for index in 0..progress.executed {
                let receipt = queries.receipt(index).await.unwrap().unwrap();
                let summary: cordial_rholang::ExecutionSummary =
                    serde_json::from_slice(&receipt.result).unwrap();
                if summary
                    .deploys
                    .iter()
                    .any(|deploy| deploy.outcome == cordial_rholang::DeployOutcome::Succeeded)
                {
                    success = Some(receipt);
                    break;
                }
            }
            if success.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let success = success.unwrap();
        let response = routes
            .public_http
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/api/cordial/receipts/{}", success.index))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        for name in [
            "blockstorage",
            "dagstorage",
            "deploystorage",
            "casperbuffer",
        ] {
            assert!(!conf.storage.data_dir.join(name).exists());
        }
        runtime.shutdown().await.unwrap();
        drop(routes);
        drop(prepared.packet_handler);
        drop(queries);
        let recovered = prepare(conf.clone()).await.unwrap();
        let queries = match &recovered.application {
            PreparedApplication::Cordial(app) => app.queries.clone(),
            _ => panic!("wrong adapter"),
        };
        let runtime = recovered.consensus.start();
        runtime.handle().wait_ready().await.unwrap();
        assert_eq!(
            queries.receipt(success.index).await.unwrap().unwrap(),
            success
        );
        runtime.shutdown().await.unwrap();
        drop(recovered.application);
        drop(recovered.packet_handler);
        let mut changed = conf;
        changed.protocol_server.network_id = "other-chain".into();
        assert!(prepare(changed).await.is_err());
    })
    .await
    .expect("Cordial node assembly did not complete");
}
