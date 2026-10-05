use consensus_api::{ConsensusError, NetworkPacket, Peer};
use consensus_runtime::{ConsensusHandle, RuntimeConfig};
use cordial_consensus::{
    Chain, ChainSpec, CordialNode, NodeOptions, PeerNetwork, StoreConfig, Validator,
};
use k256::ecdsa::SigningKey;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

type NetworkNodes = BTreeMap<Vec<u8>, (Peer, ConsensusHandle)>;

#[derive(Clone)]
struct LocalNetwork {
    local: Peer,
    nodes: Arc<Mutex<NetworkNodes>>,
}

#[async_trait::async_trait]
impl PeerNetwork for LocalNetwork {
    fn peers(&self) -> Vec<Peer> {
        self.nodes
            .lock()
            .unwrap()
            .values()
            .filter(|(peer, _)| peer.id != self.local.id)
            .map(|(peer, _)| peer.clone())
            .collect()
    }
    async fn send(&self, peer: &Peer, kind: &str, payload: Vec<u8>) -> Result<(), ConsensusError> {
        let handle = self
            .nodes
            .lock()
            .unwrap()
            .get(&peer.id)
            .map(|(_, handle)| handle.clone())
            .ok_or(ConsensusError::Stopped)?;
        handle
            .handle_packet(NetworkPacket {
                peer: self.local.clone(),
                kind: kind.into(),
                payload,
            })
            .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_native_validators_progress_and_a_late_observer_recovers_the_committed_prefix() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let keys: Vec<_> = (1..=4)
            .map(|value| SigningKey::from_slice(&[value; 32]).unwrap())
            .collect();
        let chain = Chain::new(ChainSpec {
            network: "native-runtime-network".into(),
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
        let nodes = Arc::new(Mutex::new(BTreeMap::new()));
        let mut running = Vec::new();
        let mut queries = Vec::new();
        for (index, key) in keys.into_iter().enumerate() {
            let peer = Peer {
                id: vec![index as u8],
                host: "localhost".into(),
                tcp_port: 0,
                udp_port: 0,
            };
            let (prepared, query) = CordialNode::prepare(
                directory.path().join(index.to_string()),
                chain.clone(),
                StoreConfig::default(),
                RuntimeConfig::default(),
                NodeOptions {
                    key: Some(key),
                    tick: Duration::from_millis(50),
                    ..NodeOptions::default()
                },
                Arc::new(LocalNetwork {
                    local: peer.clone(),
                    nodes: nodes.clone(),
                }),
                None,
            )
            .unwrap();
            let runtime = prepared.start();
            nodes
                .lock()
                .unwrap()
                .insert(peer.id.clone(), (peer, runtime.handle()));
            runtime.handle().wait_ready().await.unwrap();
            running.push(runtime);
            queries.push(query);
        }
        loop {
            if queries[0].progress().await.unwrap().committed >= 12 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let expected = queries[0].output(0, 12).await.unwrap();
        let peer = Peer {
            id: vec![99],
            host: "localhost".into(),
            tcp_port: 0,
            udp_port: 0,
        };
        let (prepared, observer) = CordialNode::prepare(
            directory.path().join("observer"),
            chain,
            StoreConfig::default(),
            RuntimeConfig::default(),
            NodeOptions {
                tick: Duration::from_millis(50),
                ..NodeOptions::default()
            },
            Arc::new(LocalNetwork {
                local: peer.clone(),
                nodes: nodes.clone(),
            }),
            None,
        )
        .unwrap();
        let runtime = prepared.start();
        nodes
            .lock()
            .unwrap()
            .insert(peer.id.clone(), (peer, runtime.handle()));
        runtime.handle().wait_ready().await.unwrap();
        loop {
            if observer.progress().await.unwrap().committed >= 12 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(observer.output(0, 12).await.unwrap(), expected);
        for query in &queries {
            while query.progress().await.unwrap().committed < 12 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            assert_eq!(query.output(0, 12).await.unwrap(), expected);
        }
        runtime.shutdown().await.unwrap();
        for runtime in running {
            runtime.shutdown().await.unwrap();
        }
    })
    .await
    .expect("native network did not converge");
}
