use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConfCell;
use comm::rust::transport::transport_layer::TransportLayer;
use consensus_runtime::{ConsensusHandle, RuntimeConfig};
use cordial_consensus::{
    Application, Chain, CommittedExecutor, CordialNode, CordialQueries, DurableBlocklace,
    NodeOptions, StoreConfig, Submission,
};
use cordial_rholang::{initialize_runtime, CommittedRholang};
use k256::ecdsa::SigningKey;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

mod api;
mod manifest;
mod network;
pub use manifest::{ChainConfig, DirectoryGuard};

use crate::rust::configuration::NodeConf;
use crate::rust::runtime::setup::PreparedNode;

pub struct Resources {
    pub runtime: RuntimeManager,
    _manager: Box<dyn KeyValueStoreManager>,
    _directory: DirectoryGuard,
}

pub struct PreparedApplication {
    pub handle: ConsensusHandle,
    pub queries: CordialQueries,
    resources: Arc<Resources>,
}

struct ManagedApplication {
    application: CommittedRholang,
    _resources: Arc<Resources>,
}

#[async_trait::async_trait]
impl CommittedExecutor for ManagedApplication {
    async fn execute_next(
        &self,
        state: &mut DurableBlocklace,
    ) -> Result<bool, cordial_consensus::Error> {
        CommittedExecutor::execute_next(&self.application, state).await
    }
}

impl Application for ManagedApplication {
    fn validate_submission(&self, payload: &[u8]) -> Result<Submission, cordial_consensus::Error> {
        self.application.validate_submission(payload)
    }
    fn proposal_payload(
        &self,
        submissions: &[Submission],
    ) -> Result<(Vec<u8>, usize), cordial_consensus::Error> {
        self.application.proposal_payload(submissions)
    }
}

pub async fn prepare<T: TransportLayer + Send + Sync + Clone + 'static>(
    connections: ConnectionsCell,
    configuration: RPConfCell,
    transport: Arc<T>,
    conf: NodeConf,
) -> eyre::Result<PreparedNode> {
    let options = conf
        .consensus
        .cordial
        .as_ref()
        .ok_or_else(|| eyre::eyre!("Cordial requires consensus.cordial.chain-file"))?;
    eyre::ensure!(
        (10..=60_000).contains(&options.tick_ms),
        "Cordial tick-ms must be between 10 and 60000"
    );
    eyre::ensure!(
        !conf.openai.enabled,
        "External execution services are disabled for the Cordial deterministic profile"
    );
    let config = ChainConfig::read(&options.chain_file)?;
    eyre::ensure!(
        config.chain.network == conf.protocol_server.network_id
            && config.chain.network == conf.protocol_client.network_id,
        "Cordial transport network does not match the chain"
    );
    let chain = Chain::new(config.chain.clone())?;
    let key = options
        .validator_key_file
        .as_ref()
        .map(|path| -> eyre::Result<SigningKey> {
            let file = std::fs::File::open(path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                eyre::ensure!(
                    file.metadata()?.permissions().mode() & 0o077 == 0,
                    "Cordial validator key file must be owner-only"
                );
            }
            let mut secret = String::new();
            file.take(257).read_to_string(&mut secret)?;
            eyre::ensure!(secret.len() <= 256, "Invalid Cordial validator key file");
            let bytes = hex::decode(secret.trim())
                .map_err(|_| eyre::eyre!("Invalid Cordial validator key encoding"))?;
            SigningKey::from_slice(&bytes).map_err(|_| eyre::eyre!("Invalid Cordial validator key"))
        })
        .transpose()?;
    if let Some(key) = &key {
        eyre::ensure!(
            config
                .chain
                .validators
                .iter()
                .any(|validator| validator.public_key
                    == key.verifying_key().to_sec1_bytes().as_ref()),
            "Cordial validator is outside the fixed committee"
        );
    }
    let directory = DirectoryGuard::open(&conf.storage.data_dir, &config)?;
    let (manager, runtime, root) = initialize_runtime(
        &conf.storage.data_dir.join("cordial-execution"),
        &config.chain,
        &config.genesis,
    )
    .await?;
    eyre::ensure!(
        root == config.chain.execution_genesis,
        "Cordial execution genesis does not match the pinned chain root"
    );
    let resources = Arc::new(Resources {
        runtime: runtime.clone(),
        _manager: manager,
        _directory: directory,
    });
    let executor = ManagedApplication {
        application: CommittedRholang::new(runtime, chain.clone())?,
        _resources: resources.clone(),
    };
    let (consensus, queries) = CordialNode::prepare(
        conf.storage.data_dir.join("cordial-consensus"),
        chain,
        StoreConfig::default(),
        RuntimeConfig::default(),
        NodeOptions {
            key,
            tick: Duration::from_millis(options.tick_ms),
            ..NodeOptions::default()
        },
        Arc::new(network::CordialTransport {
            transport,
            connections,
            configuration,
        }),
        Some(Box::new(executor)),
    )?;
    let handle = consensus.handle();
    Ok(PreparedNode {
        consensus,
        packet_handler: Arc::new(network::CordialPacketHandler(handle.clone())),
        application: crate::rust::runtime::setup::PreparedApplication::Cordial(Box::new(
            PreparedApplication {
                handle,
                queries,
                resources,
            },
        )),
    })
}
