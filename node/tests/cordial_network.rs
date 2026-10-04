use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use cordial_consensus::{Chain, ChainSpec, Validator};
use cordial_rholang::{initialize_runtime, GenesisSettings, VaultAllocation};
use crypto::rust::util::certificate_helper::{CertificateHelper, CertificatePrinter};
use k256::ecdsa::SigningKey;
use models::casper::v1::deploy_service_client::DeployServiceClient;
use models::casper::v1::propose_service_client::ProposeServiceClient;
use models::casper::ProposeQuery;
use node::rust::consensus::cordial::ChainConfig;
use prost::Message;
use serde_json::{json, Value};

struct Process {
    child: Option<Child>,
    config: PathBuf,
    directory: PathBuf,
    http: u16,
    grpc: u16,
    admin_grpc: u16,
    ports: Vec<u16>,
    peer: comm::rust::peer_node::PeerNode,
}

impl Process {
    fn start(&mut self) {
        assert!(self.child.is_none());
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.directory.join("network-test.log"))
            .unwrap();
        self.child = Some(
            Command::new(env!("CARGO_BIN_EXE_node"))
                .args(["run", "--config-file"])
                .arg(&self.config)
                .arg("--data-dir")
                .arg(&self.directory)
                .arg(format!("--protocol-port={}", self.ports[0]))
                .arg(format!("--discovery-port={}", self.ports[1]))
                .arg(format!("--api-port-grpc-external={}", self.ports[2]))
                .arg(format!("--api-port-grpc-internal={}", self.ports[3]))
                .arg(format!("--api-port-http={}", self.ports[4]))
                .arg(format!("--api-port-admin-http={}", self.ports[5]))
                .env("TOKIO_WORKER_THREADS", "4")
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log))
                .spawn()
                .unwrap(),
        );
    }

    fn signal(&self, signal: &str) {
        let status = Command::new("kill")
            .arg(signal)
            .arg(self.child.as_ref().unwrap().id().to_string())
            .status()
            .unwrap();
        assert!(status.success());
    }

    async fn stop(&mut self) {
        self.signal("-TERM");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(40);
        loop {
            if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                assert!(
                    status.success(),
                    "node shutdown failed: {status}, logs: {}",
                    self.directory.display()
                );
                self.child = None;
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "node shutdown timeout"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn crash(&mut self) {
        let mut child = self.child.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
    }

    fn url(&self, path: &str) -> String { format!("http://127.0.0.1:{}{path}", self.http) }
}

impl Drop for Process {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn owner_file(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

async fn get(client: &reqwest::Client, node: &Process, path: &str) -> Value {
    client
        .get(node.url(path))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn ready(client: &reqwest::Client, node: &mut Process) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = node.child.as_mut().unwrap().try_wait().unwrap() {
            panic!("node exited {status}, logs: {}", node.directory.display());
        }
        if let Ok(response) = client.get(node.url("/api/status")).send().await {
            if let Ok(status) = response.json::<Value>().await {
                if status["ready"] == true {
                    return;
                }
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "node readiness timeout, logs: {}",
            node.directory.display()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn executed(client: &reqwest::Client, node: &Process, id: &str) -> (u64, Value) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let mut cursor = 0;
    loop {
        let status = get(client, node, "/api/status").await;
        let end = status["progress"]["executed"].as_u64().unwrap();
        while cursor < end {
            let receipt = get(client, node, &format!("/api/cordial/receipts/{cursor}")).await;
            if receipt["summary"]["deploys"]
                .as_array()
                .unwrap()
                .iter()
                .any(|deploy| {
                    deploy["id"].as_array().is_some_and(|bytes| {
                        hex::encode(
                            bytes
                                .iter()
                                .map(|byte| byte.as_u64().unwrap() as u8)
                                .collect::<Vec<_>>(),
                        ) == id
                    }) && deploy["outcome"] == "Succeeded"
                })
            {
                return (cursor, receipt);
            }
            cursor += 1;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "deploy did not execute, logs: {}",
            node.directory.display()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn wait_index(client: &reqwest::Client, node: &Process, index: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        let status = get(client, node, "/api/status").await;
        if status["progress"]["executed"].as_u64().unwrap() > index {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "history recovery timed out, logs: {}",
            node.directory.display()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cordial_real_nodes_tls_deploy_execution_late_join_restart_and_partition() {
    let memory = std::fs::read_to_string("/proc/meminfo").unwrap();
    let available: u64 = memory
        .lines()
        .find(|line| line.starts_with("MemAvailable:"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        available >= 2 * 1024 * 1024,
        "Cordial network acceptance requires at least 2 GiB available RAM"
    );
    let directory = tempfile::Builder::new()
        .prefix("cordial-network-")
        .tempdir()
        .unwrap()
        .keep();
    eprintln!("Cordial network evidence: {}", directory.display());
    tokio::time::timeout(Duration::from_secs(420), async {
        let seeds: Vec<_> = (1..=4).map(|value| [value; 32]).collect();
        let keys: Vec<_> = seeds.iter().map(|seed| SigningKey::from_slice(seed).unwrap()).collect();
        let chain = Chain::new(ChainSpec {
            network: format!("cordial-network-{}", uuid::Uuid::new_v4()), shard: "root".into(), execution_genesis: [0; 32], wavelength: 3,
            validators: keys.iter().map(|key| Validator { public_key: key.verifying_key().to_sec1_bytes().to_vec(), weight: 1 }).collect(),
        }).unwrap();
        let mut config = ChainConfig { chain: chain.spec().clone(), genesis: GenesisSettings { vaults: vec![VaultAllocation { public_key: keys[0].verifying_key().to_sec1_bytes().to_vec(), balance: 1_000_000_000 }], ..GenesisSettings::default() } };
        let (mut manager, vm, root) = initialize_runtime(&directory.join("genesis-build"), &config.chain, &config.genesis).await.unwrap();
        drop(vm); manager.shutdown().await.unwrap(); drop(manager);
        config.chain.execution_genesis = root;
        let chain_file = directory.join("chain.json");
        std::fs::write(&chain_file, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let reservations: Vec<_> = (0..30).map(|_| std::net::TcpListener::bind("127.0.0.1:0").unwrap()).collect();
        let ports: Vec<_> = reservations.iter().map(|socket| socket.local_addr().unwrap().port()).collect();
        let mut nodes = Vec::<Process>::new();
        for index in 0..5 {
            let path = directory.join(format!("node-{index}"));
            std::fs::create_dir(&path).unwrap();
            let (secret, public) = CertificateHelper::generate_key_pair();
            let certificate = CertificatePrinter::print_certificate(&CertificateHelper::generate_certificate(&secret, &public).unwrap());
            let private = CertificatePrinter::print_private_key_from_secret(&secret).unwrap();
            std::fs::write(path.join("node.certificate.pem"), certificate).unwrap();
            owner_file(&path.join("node.key.pem"), private);
            let peer = comm::rust::peer_node::PeerNode::new(comm::rust::peer_node::NodeIdentifier { key: CertificateHelper::public_address(&public).unwrap().into() }, "127.0.0.1".into(), ports[index * 6], ports[index * 6 + 1]);
            let mut cordial = json!({"chain-file": chain_file, "tick-ms": 1000});
            if index < 4 {
                let key_path = path.join("validator.key");
                owner_file(&key_path, hex::encode(seeds[index]));
                cordial["validator-key-file"] = json!(key_path);
            }
            let settings = json!({
                "standalone": index == 0,
                "consensus": {"protocol": "cordial-miners", "cordial": cordial},
                "storage": {"data-dir": path},
                "protocol-server": {"network-id": config.chain.network, "host": "127.0.0.1", "port": ports[index*6], "no-upnp": true, "allow-private-addresses": true},
                "protocol-client": {"network-id": config.chain.network, "bootstrap": if index == 0 { String::new() } else { nodes[0].peer.to_address() }, "network-timeout": "1 second"},
                "peers-discovery": {"port": ports[index*6+1], "lookup-interval": "1 second", "cleanup-interval": "2 seconds", "init-wait-loop-interval": "100ms"},
                "api-server": {"host": "127.0.0.1", "port-grpc-external": ports[index*6+2], "port-grpc-internal": ports[index*6+3], "port-http": ports[index*6+4], "port-admin-http": ports[index*6+5], "enable-reporting": false},
                "tls": {"certificate-path": path.join("node.certificate.pem"), "key-path": path.join("node.key.pem")},
                "openai": {"enabled": false}, "logging": {"filter": "info,tonic=warn,hyper=warn,h2=warn", "sink": "stdout"}
            });
            let configuration = directory.join(format!("node-{index}.json"));
            std::fs::write(&configuration, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();
            nodes.push(Process { child: None, config: configuration, directory: path, http: ports[index*6+4], grpc: ports[index*6+2], admin_grpc: ports[index*6+3], ports: ports[index*6..index*6+6].to_vec(), peer });
        }
        drop(reservations);
        let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
        for node in &mut nodes[..4] { node.start(); ready(&client, node).await; }
        let deploy = casper::rust::util::construct_deploy::source_deploy_now("@42!(42)".into(), Some(crypto::rust::private_key::PrivateKey::from_bytes(&seeds[0])), Some(0), Some("root".into())).unwrap();
        let proto = models::rust::casper::protocol::casper_message::DeployData::to_proto(deploy);
        let mut grpc = DeployServiceClient::connect(format!("http://127.0.0.1:{}", nodes[0].grpc)).await.unwrap();
        let response = grpc.do_deploy(proto.clone()).await.unwrap().into_inner();
        let id = match response.message { Some(models::casper::v1::deploy_response::Message::Result(id)) => id, other => panic!("deploy failed: {other:?}") };
        let duplicate: Value = client.post(nodes[0].url("/api/deploy")).body(proto.encode_to_vec()).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        assert_eq!(duplicate["deploy_id"], id);
        let mut proposer = ProposeServiceClient::connect(format!("http://127.0.0.1:{}", nodes[0].admin_grpc)).await.unwrap();
        let proposal = proposer.propose(ProposeQuery { is_async: false }).await;
        assert!(proposal.is_ok() || proposal.unwrap_err().code() == tonic::Code::FailedPrecondition);
        assert_eq!(grpc.last_finalized_block(models::casper::LastFinalizedBlockQuery {}).await.unwrap_err().code(), tonic::Code::Unimplemented);
        let mut batch = vec![];
        for value in 0..3 {
            let deploy = casper::rust::util::construct_deploy::source_deploy_now(format!("@\"batch\"!({value})"), Some(crypto::rust::private_key::PrivateKey::from_bytes(&seeds[0])), Some(0), Some("root".into())).unwrap();
            let response: Value = client.post(nodes[0].url("/api/deploy")).body(models::rust::casper::protocol::casper_message::DeployData::to_proto(deploy).encode_to_vec()).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
            batch.push(response["deploy_id"].as_str().unwrap().to_string());
        }
        let (index, expected) = executed(&client, &nodes[0], &id).await;
        for batch_id in &batch {
            let (batch_index, batch_receipt) = executed(&client, &nodes[0], batch_id).await;
            for node in &nodes[1..4] {
                wait_index(&client, node, batch_index).await;
                assert_eq!(get(&client, node, &format!("/api/cordial/receipts/{batch_index}")).await, batch_receipt);
            }
        }
        for node in &nodes[..4] {
            wait_index(&client, node, index).await;
            assert_eq!(get(&client, node, &format!("/api/cordial/receipts/{index}")).await, expected);
            let channel = rholang::rust::interpreter::compiler::compiler::Compiler::source_to_adt("42").unwrap();
            let data: Value = client.post(node.url("/api/cordial/data")).json(&json!({"channel": channel, "index": index})).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
            assert_eq!(data["data"], serde_json::to_value(vec![channel]).unwrap());
        }
        nodes[4].start(); ready(&client, &mut nodes[4]).await;
        wait_index(&client, &nodes[4], index).await;
        assert_eq!(get(&client, &nodes[4], &format!("/api/cordial/receipts/{index}")).await, expected);
        let observer_submit = client.post(nodes[4].url("/api/deploy")).body(proto.encode_to_vec()).send().await.unwrap();
        assert_eq!(observer_submit.status(), reqwest::StatusCode::NOT_IMPLEMENTED);
        wait_index(&client, &nodes[0], 11).await;
        let prefix = get(&client, &nodes[0], "/api/cordial/output?start=0&limit=12").await;
        {
            use comm::rust::transport::{grpc_transport_client::GrpcTransportClient, transport_layer::TransportLayer};
            use comm::rust::rp::rp_conf::RPConf;
            let transport = GrpcTransportClient::new(config.chain.network.clone(), std::fs::read_to_string(nodes[0].directory.join("node.certificate.pem")).unwrap(), std::fs::read_to_string(nodes[0].directory.join("node.key.pem")).unwrap(), 1024 * 1024, 1024 * 1024, 32, std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())), Duration::from_secs(3)).unwrap();
            let rp = RPConf::new(nodes[0].peer.clone(), config.chain.network.clone(), None, Duration::from_secs(3), 10, 10);
            let id = prefix["records"][0]["id"].as_str().unwrap();
            let packet = get(&client, &nodes[0], &format!("/api/cordial/objects/{id}")).await;
            let valid = hex::decode(packet["packet"].as_str().unwrap()).unwrap();
            for _ in 0..2 { transport.send_packet_to_peer(&rp, &nodes[1].peer, models::routing::Packet { type_id: cordial_consensus::BLOCK_PACKET_KIND.into(), content: valid.clone().into() }).await.unwrap(); }
            let mut foreign = config.chain.clone(); foreign.shard = "other-shard".into();
            let foreign = Chain::new(foreign).unwrap();
            let block = foreign.build_block(&keys[0], vec![], vec![]).unwrap();
            let foreign_packet = foreign.encode_block(&block).unwrap();
            let _ = transport.send_packet_to_peer(&rp, &nodes[1].peer, models::routing::Packet { type_id: cordial_consensus::BLOCK_PACKET_KIND.into(), content: foreign_packet.into() }).await;
            let _ = transport.send_packet_to_peer(&rp, &nodes[1].peer, models::routing::Packet { type_id: cordial_consensus::BLOCK_PACKET_KIND.into(), content: vec![255; 32].into() }).await;
            use bincode::Options;
            let foreign_id = hex::encode(bincode::DefaultOptions::new().with_fixint_encoding().serialize(&block.identity).unwrap());
            tokio::time::sleep(Duration::from_millis(500)).await;
            assert_eq!(client.get(nodes[1].url(&format!("/api/cordial/objects/{foreign_id}"))).send().await.unwrap().status(), reqwest::StatusCode::NOT_FOUND);
            assert_eq!(get(&client, &nodes[1], &format!("/api/cordial/objects/{id}")).await, packet);
            transport.disconnect(&nodes[1].peer).await.unwrap();
        }
        nodes[0].stop().await;
        nodes[0].start(); ready(&client, &mut nodes[0]).await;
        assert_eq!(get(&client, &nodes[0], &format!("/api/cordial/receipts/{index}")).await, expected);
        nodes[1].crash();
        nodes[1].start(); ready(&client, &mut nodes[1]).await;
        assert_eq!(get(&client, &nodes[1], &format!("/api/cordial/receipts/{index}")).await, expected);
        nodes[2].signal("-STOP"); nodes[3].signal("-STOP");
        tokio::time::sleep(Duration::from_secs(3)).await;
        nodes[2].signal("-CONT"); nodes[3].signal("-CONT");
        let target = get(&client, &nodes[0], "/api/status").await["progress"]["executed"].as_u64().unwrap() + 4;
        for node in &nodes {
            wait_index(&client, node, target).await;
            assert_eq!(get(&client, node, "/api/cordial/output?start=0&limit=12").await, prefix);
            assert_eq!(get(&client, node, "/api/cordial/equivocations").await["equivocations"], json!([]));
            for name in ["blockstorage", "dagstorage", "deploystorage", "casperbuffer"] { assert!(!node.directory.join(name).exists()); }
        }
        let malformed = client.post(nodes[0].url("/api/deploy")).body(vec![255; 32]).send().await.unwrap();
        assert_eq!(malformed.status(), reqwest::StatusCode::BAD_REQUEST);
        for node in &mut nodes { node.stop().await; }
        std::fs::write(directory.join("result.json"), serde_json::to_vec_pretty(&json!({"result":"passed", "validators":4, "late_observers":1, "transport":"real TLS", "process":"node binary", "deploy_id": id, "receipt_index": index, "receipt": expected, "verified":["grpc-deploy", "multi-deploy-batches", "crash-restart", "equivocation-query", "http-duplicate", "native-commit", "execution-root-agreement", "committed-data-query", "late-join", "restart", "pause-resume-recovery", "malformed-input", "duplicate-native-packets", "cross-chain-rejection", "unsupported-capabilities", "graceful-shutdown"]})).unwrap()).unwrap();
    }).await.expect("Cordial network acceptance exceeded its deadline");
}
