#![cfg(feature = "cbc-casper")]

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::errors::CommError;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::{RPConf, RPConfCell};
use comm::rust::transport::transport_layer::{Blob, TransportLayer};
use consensus_api::{Capabilities, ConsensusError, Phase};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::routing::Protocol;
use models::rust::casper::protocol::casper_message::DeployData;
use node::rust::configuration::NodeConf;
use node::rust::runtime::application::{ApplicationContext, ApplicationRoutes};
use node::rust::runtime::setup::setup_node_program;
use prost::Message;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use tower::ServiceExt;

#[derive(Clone)]
struct TestTransport;

#[async_trait::async_trait]
impl TransportLayer for TestTransport {
    async fn send(&self, _: &PeerNode, _: &Protocol) -> Result<(), CommError> { Ok(()) }
    async fn broadcast(&self, _: &[PeerNode], _: &Protocol) -> Result<(), CommError> { Ok(()) }
    async fn stream(&self, _: &PeerNode, _: &Blob) -> Result<(), CommError> { Ok(()) }
    async fn stream_mult(&self, _: &[PeerNode], _: &Blob) -> Result<(), CommError> { Ok(()) }
    async fn disconnect(&self, _: &PeerNode) -> Result<(), CommError> { Ok(()) }
    async fn get_channeled_peers(&self) -> Result<HashSet<PeerNode>, CommError> {
        Ok(HashSet::new())
    }
}

struct TestDiscovery;
#[async_trait::async_trait]
impl NodeDiscovery for TestDiscovery {
    async fn discover(&self) -> Result<(), CommError> { Ok(()) }
    fn peers(&self) -> Result<Vec<PeerNode>, CommError> { Ok(vec![]) }
    fn remove_peer(&self, _: &PeerNode) -> Result<(), CommError> { Ok(()) }
}

fn config(directory: &std::path::Path) -> (NodeConf, PrivateKey) {
    let mut conf: NodeConf = hocon::HoconLoader::new()
        .load_str(include_str!("../src/main/resources/defaults.conf"))
        .unwrap()
        .resolve()
        .unwrap();
    let (key, public) = Secp256k1.new_key_pair();
    let genesis = directory.join("genesis");
    std::fs::create_dir_all(&genesis).unwrap();
    let bonds = genesis.join("bonds.txt");
    let wallets = genesis.join("wallets.txt");
    std::fs::write(
        &bonds,
        format!("{} 100000000\n", hex::encode(&public.bytes)),
    )
    .unwrap();
    let address =
        rholang::rust::interpreter::util::vault_address::VaultAddress::from_public_key(&public)
            .unwrap()
            .to_base58();
    std::fs::write(&wallets, format!("{address},1000000000000\n")).unwrap();
    conf.storage.data_dir = directory.to_path_buf();
    conf.standalone = true;
    conf.autopropose = false;
    conf.dev_mode = false;
    conf.openai.enabled = false;
    conf.api_server.enable_reporting = false;
    conf.casper.validator_private_key = Some(hex::encode(&key.bytes));
    conf.casper.shard_name = "consensus-runtime-test".into();
    conf.casper.heartbeat_conf.enabled = false;
    conf.casper.finalization_rate = 1;
    conf.casper.casper_loop_interval = Duration::from_millis(100);
    conf.casper.genesis_ceremony.ceremony_master_mode = true;
    conf.casper.genesis_ceremony.genesis_validator_mode = false;
    conf.casper.genesis_ceremony.required_signatures = 0;
    conf.casper.genesis_ceremony.approve_duration = Duration::ZERO;
    conf.casper.genesis_ceremony.approve_interval = Duration::from_millis(10);
    conf.casper.genesis_block_data.genesis_data_dir = genesis.to_string_lossy().into();
    conf.casper.genesis_block_data.bonds_file = bonds.to_string_lossy().into();
    conf.casper.genesis_block_data.wallets_file = wallets.to_string_lossy().into();
    conf.casper.genesis_block_data.deploy_timestamp = Some(1_700_000_000_000);
    conf.casper.genesis_block_data.number_of_active_validators = 1;
    (conf, key)
}

struct PreparedTestNode {
    consensus: consensus_runtime::PreparedConsensus,
    packet_handler: Arc<dyn comm::rust::p2p::packet_handler::PacketHandler>,
    application: TestApplication,
}

struct TestApplication {
    public_http: axum::Router,
    admin_http: axum::Router,
    deploy:
        models::casper::v1::deploy_service_client::DeployServiceClient<tonic::transport::Channel>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    server: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}

impl TestApplication {
    async fn close(mut self) {
        let _ = self.shutdown.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(10), &mut self.server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    async fn deploy_http(
        &self,
        request: node::rust::consensus::casper::api::web_api::DeployRequest,
    ) -> String {
        let response = self
            .public_http
            .clone()
            .oneshot(
                axum::http::Request::post("/api/deploy")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&request).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn propose_http(&self) -> String {
        let response = self
            .admin_http
            .clone()
            .oneshot(
                axum::http::Request::post("/api/propose")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }
}

impl Drop for TestApplication {
    fn drop(&mut self) { self.server.abort(); }
}

async fn prepare(conf: NodeConf) -> eyre::Result<PreparedTestNode> {
    let local = PeerNode {
        id: NodeIdentifier {
            key: vec![1; 32].into(),
        },
        endpoint: Endpoint::new("127.0.0.1".into(), 40400, 40404),
    };
    let peer_conf = RPConfCell::new(RPConf::new(
        local,
        conf.protocol_server.network_id.clone(),
        None,
        Duration::from_secs(1),
        10,
        10,
    ));
    let connections = ConnectionsCell::new();
    let discovery = Arc::new(TestDiscovery);
    let events = F1r3flyEvents::new();
    let prepared = setup_node_program(
        connections.clone(),
        peer_conf.clone(),
        Arc::new(TestTransport),
        conf.clone(),
        events.clone(),
        discovery.clone(),
    )
    .await?;
    let ApplicationRoutes {
        external,
        public_http,
        admin_http,
        internal,
    } = prepared
        .application
        .routes(ApplicationContext {
            settings: conf.api_server,
            peer_conf,
            connections,
            discovery,
            events: events.consume(),
            startup_events: events.startup_buffer(),
        })
        .await?;
    drop(internal);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(external.serve_with_incoming_shutdown(
        tokio_stream::wrappers::TcpListenerStream::new(listener),
        async {
            let _ = stopped.await;
        },
    ));
    let deploy = models::casper::v1::deploy_service_client::DeployServiceClient::connect(format!(
        "http://{address}"
    ))
    .await?;
    Ok(PreparedTestNode {
        consensus: prepared.consensus,
        packet_handler: prepared.packet_handler,
        application: TestApplication {
            public_http,
            admin_http,
            deploy,
            shutdown: Some(shutdown),
            server,
        },
    })
}

fn copy_closed_store(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_closed_store(&entry.path(), &target);
        } else if entry.file_name() != "lock.mdb" {
            assert!(entry.file_type().unwrap().is_file());
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

async fn deploy_status(
    application: &TestApplication,
    signature: prost::bytes::Bytes,
) -> models::casper::DeployFinalizationStatusInfo {
    let response = application
        .deploy
        .clone()
        .deploy_finalization_status(tonic::Request::new(
            models::casper::DeployFinalizationStatusQuery {
                deploy_sig: signature,
            },
        ))
        .await
        .unwrap()
        .into_inner();
    match response.message {
        Some(models::casper::v1::deploy_finalization_status_response::Message::Status(status)) => {
            status
        }
        other => panic!("Deploy status failed: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn casper_runtime_deploy_propose_finalize_recover() {
    tokio::time::timeout(Duration::from_secs(180), async {
        let directory = tempfile::tempdir().unwrap();
        let (conf, key) = config(directory.path());
        let prepared = prepare(conf.clone()).await.unwrap();
        let handle = prepared.consensus.handle();
        assert_eq!(handle.status().phase, Phase::Prepared);
        assert_eq!(handle.propose(false).await, Err(ConsensusError::NotReady));
        let runtime = prepared.consensus.start();
        handle.wait_ready().await.unwrap();
        assert_eq!(handle.status().protocol.id, "cbc-casper");
        let genesis = handle.finalized().await.unwrap();
        runtime.shutdown().await.unwrap();
        prepared.application.close().await;
        drop(prepared.packet_handler);
        let follower_directory = tempfile::tempdir().unwrap();
        copy_closed_store(directory.path(), follower_directory.path());
        let prepared = prepare(conf.clone()).await.unwrap();
        let handle = prepared.consensus.handle();
        let runtime = prepared.consensus.start();
        handle.wait_ready().await.unwrap();
        assert_eq!(handle.finalized().await.unwrap(), genesis);
        let mut deploy_payload = vec![];
        let mut first_deploy_signature = prost::bytes::Bytes::new();
        let mut block_hashes = vec![];
        for index in 0..4 {
            let deploy = casper::rust::util::construct_deploy::source_deploy_now(
                format!("new x in {{ x!({index}) }}"), Some(key.clone()), Some(index), Some(conf.casper.shard_name.clone()),
            ).unwrap();
            let http_request = node::rust::consensus::casper::api::web_api::DeployRequest {
                data: deploy.data.clone(), deployer: hex::encode(&deploy.pk.bytes),
                signature: hex::encode(&deploy.sig), sig_algorithm: "secp256k1".into(),
            };
            let proto = DeployData::to_proto(deploy);
            if index == 0 { first_deploy_signature = proto.sig.clone(); }
            deploy_payload = proto.encode_to_vec();
            let response = match index {
                0 => {
                    let response = prepared.application.deploy.clone().do_deploy(tonic::Request::new(proto)).await.unwrap().into_inner();
                    match response.message {
                        Some(models::casper::v1::deploy_response::Message::Result(value)) => value,
                        other => panic!("gRPC deploy failed: {other:?}"),
                    }
                }
                1 => prepared.application.deploy_http(http_request).await,
                _ => handle.submit(deploy_payload.clone()).await.unwrap(),
            };
            assert!(response.starts_with("Success!"), "{response}");
            let response = if index == 0 {
                prepared.application.propose_http().await
            } else {
                handle.propose(false).await.unwrap()
            };
            assert!(response.contains("created and added"), "{response}");
            block_hashes.push(prost::bytes::Bytes::from(hex::decode(response.split_whitespace().nth(2).unwrap()).unwrap()));
        }
        let finalized = handle.finalized().await.unwrap();
        assert_ne!(finalized, genesis, "finality must advance beyond genesis");
        let status_before = deploy_status(&prepared.application, first_deploy_signature.clone()).await;
        assert_eq!(status_before.state, models::casper::DeployFinalizationStateProto::DeployStateFinalized as i32);
        assert!(matches!(handle.submit(deploy_payload).await, Err(ConsensusError::Rejected { code, .. }) if code == "casper.duplicate-deploy"));
        let manifest = std::fs::read(directory.path().join("consensus-manifest.json")).unwrap();
        runtime.shutdown().await.unwrap();
        assert_eq!(handle.status().phase, Phase::Stopped);
        prepared.application.close().await;
        drop(prepared.packet_handler);

        let native_blocks = {
            use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
            let mut manager = casper::rust::storage::rnode_key_value_store_manager::new_key_value_store_manager(directory.path().to_path_buf(), None);
            let store = block_storage::rust::key_value_block_store::KeyValueBlockStore::create_from_kvm(&mut manager).await.unwrap();
            let blocks: Vec<_> = block_hashes.iter().map(|hash| store.get(hash).unwrap().unwrap()).collect();
            manager.shutdown().await.unwrap();
            blocks
        };
        let mut follower_conf = conf.clone();
        follower_conf.storage.data_dir = follower_directory.path().to_path_buf();
        follower_conf.casper.validator_private_key = None;
        let follower = prepare(follower_conf).await.unwrap();
        let follower_handle = follower.consensus.handle();
        let follower_runtime = follower.consensus.start();
        follower_handle.wait_ready().await.unwrap();
        assert_eq!(follower_handle.finalized().await.unwrap(), genesis);
        let peer = PeerNode { id: NodeIdentifier { key: vec![2;32].into() }, endpoint: Endpoint::new("127.0.0.1".into(), 40410, 40414) };
        use models::rust::casper::protocol::packet_type_tag::ToPacket;
        let mut malformed = native_blocks[0].to_proto().mk_packet();
        malformed.content = vec![0xff].into();
        follower.packet_handler.handle_packet(&peer, &malformed).await.unwrap();
        let mut invalid = native_blocks[0].clone();
        invalid.header.timestamp += 1;
        invalid.block_hash = casper::rust::util::proto_util::hash_block(&invalid);
        follower.packet_handler.handle_packet(&peer, &invalid.to_proto().mk_packet()).await.unwrap();
        for block in native_blocks.iter().rev() {
            follower.packet_handler.handle_packet(&peer, &block.to_proto().mk_packet()).await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(40), async {
            while follower_handle.finalized().await.unwrap() != finalized {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }).await.expect("Follower must validate out-of-order blocks and advance finality");
        follower.packet_handler.handle_packet(&peer, &native_blocks[0].to_proto().mk_packet()).await.unwrap();
        assert_eq!(follower_handle.finalized().await.unwrap(), finalized);
        follower_runtime.shutdown().await.unwrap();
        follower.application.close().await;
        drop(follower.packet_handler);
        {
            use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
            let mut manager = casper::rust::storage::rnode_key_value_store_manager::new_key_value_store_manager(follower_directory.path().to_path_buf(), None);
            let store = block_storage::rust::key_value_block_store::KeyValueBlockStore::create_from_kvm(&mut manager).await.unwrap();
            let dag = block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
            let view = dag.get_representation().unwrap();
            assert!(!view.contains(&invalid.block_hash), "Invalid signature must not enter the native DAG");
            for block in &native_blocks {
                assert!(view.contains(&block.block_hash));
                assert_eq!(store.get(&block.block_hash).unwrap().unwrap().body.state.post_state_hash, block.body.state.post_state_hash);
            }
            manager.shutdown().await.unwrap();
        }

        let dag_before = std::fs::read(directory.path().join("dagstorage/data.mdb")).unwrap();
        let mut changed: node::rust::consensus::manifest::ChainManifest = serde_json::from_slice(&manifest).unwrap();
        changed.genesis = "00".repeat(32);
        std::fs::write(directory.path().join("consensus-manifest.json"), serde_json::to_vec(&changed).unwrap()).unwrap();
        let mismatch = prepare(conf.clone()).await.err().unwrap();
        assert!(mismatch.to_string().contains("genesis mismatch"), "{mismatch:#}");
        assert_eq!(std::fs::read(directory.path().join("dagstorage/data.mdb")).unwrap(), dag_before);
        std::fs::rename(directory.path().join("consensus-manifest.json"), directory.path().join("test-old-manifest.json")).unwrap();

        let recovered = prepare(conf.clone()).await.unwrap();
        let recovered_handle = recovered.consensus.handle();
        let runtime = recovered.consensus.start();
        recovered_handle.wait_ready().await.unwrap();
        assert_eq!(recovered_handle.finalized().await.unwrap(), finalized);
        assert_eq!(deploy_status(&recovered.application, first_deploy_signature).await, status_before);
        assert_eq!(std::fs::read(directory.path().join("consensus-manifest.json")).unwrap(), manifest);
        runtime.shutdown().await.unwrap();
        recovered.application.close().await;
        drop(recovered.packet_handler);

        let mut readonly = conf.clone();
        readonly.casper.validator_private_key = None;
        let readonly = prepare(readonly).await.unwrap();
        let handle = readonly.consensus.handle();
        assert!(!handle.status().protocol.capabilities.contains(Capabilities::PROPOSE));
        assert!(matches!(handle.propose(false).await, Err(ConsensusError::UnsupportedCapability(_))));
        let runtime = readonly.consensus.start();
        handle.wait_ready().await.unwrap();
        assert_eq!(handle.finalized().await.unwrap(), finalized);
        runtime.shutdown().await.unwrap();
    }).await.expect("Casper runtime acceptance test timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn casper_runtime_shutdown_while_waiting_for_peers() {
    let directory = tempfile::tempdir().unwrap();
    let (mut conf, _) = config(directory.path());
    conf.standalone = false;
    let prepared = prepare(conf).await.unwrap();
    let handle = prepared.consensus.handle();
    let runtime = prepared.consensus.start();
    tokio::time::sleep(Duration::from_millis(20)).await;
    tokio::time::timeout(Duration::from_secs(5), runtime.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(handle.status().phase, Phase::Stopped);
    assert!(!directory.path().join("consensus-manifest.json").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn casper_runtime_reports_failed_genesis_initialization() {
    let directory = tempfile::tempdir().unwrap();
    let (conf, _) = config(directory.path());
    std::fs::write(&conf.casper.genesis_block_data.bonds_file, "invalid bond").unwrap();
    let prepared = prepare(conf).await.unwrap();
    let handle = prepared.consensus.handle();
    let mut runtime = prepared.consensus.start();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), runtime.wait())
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(handle.status().phase, Phase::Failed);
    assert!(!directory.path().join("consensus-manifest.json").exists());
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn casper_adapter_owns_observer_preparation_and_shutdown() {
    use std::os::unix::fs::PermissionsExt;

    use tokio::io::AsyncReadExt;

    let directory = tempfile::tempdir().unwrap();
    let observer_directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(
        observer_directory.path(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let socket = observer_directory.path().join("observer.sock");
    let (mut conf, _) = config(directory.path());
    conf.standalone = false;
    conf.soak_observer = Some(node::rust::configuration::model::SoakObserverConfig {
        directory: observer_directory.path().to_path_buf(),
        source_revision: "a".repeat(40),
        approved_request_sha256: "b".repeat(64),
        peer_pid: std::process::id(),
        peer_start_ticks: node::rust::soak_observer::process_start_ticks(std::process::id())
            .unwrap(),
        session_timeout_ms: 500,
        max_sessions: 128,
    });
    let prepared = prepare(conf.clone()).await.unwrap();
    assert!(socket.exists());
    drop(prepared.consensus);
    prepared.application.close().await;
    drop(prepared.packet_handler);
    assert!(!socket.exists());

    let prepared = prepare(conf).await.unwrap();
    let handle = prepared.consensus.handle();
    let runtime = prepared.consensus.start();
    let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let hello_length = tokio::time::timeout(Duration::from_secs(5), stream.read_u32())
        .await
        .unwrap()
        .unwrap();
    assert!(hello_length > 0 && hello_length < 1024 * 1024);
    let mut hello = vec![0; hello_length as usize];
    stream.read_exact(&mut hello).await.unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&hello).is_ok());
    tokio::time::timeout(Duration::from_secs(5), runtime.shutdown())
        .await
        .unwrap()
        .unwrap();
    prepared.application.close().await;
    assert_eq!(handle.status().phase, Phase::Stopped);
    assert!(!socket.exists());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), stream.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn incompatible_manifest_is_rejected_before_store_creation() {
    let directory = tempfile::tempdir().unwrap();
    let (conf, _) = config(directory.path());
    let manifest = node::rust::consensus::manifest::ChainManifest {
        protocol: "cordial-miners".into(),
        protocol_version: 1,
        schema_version: 1,
        network: conf.protocol_server.network_id.clone(),
        shard: conf.casper.shard_name.clone(),
        genesis: "01".into(),
    };
    let bytes = serde_json::to_vec(&manifest).unwrap();
    std::fs::write(directory.path().join("consensus-manifest.json"), &bytes).unwrap();
    assert!(prepare(conf)
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("protocol mismatch"));
    assert!(!directory.path().join("blockstorage").exists());
    assert!(!directory.path().join("dagstorage").exists());
    assert_eq!(
        std::fs::read(directory.path().join("consensus-manifest.json")).unwrap(),
        bytes
    );
}
