//! Web API implementation for F1r3fly node

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use casper::rust::api::block_api::{
    BlockAPI, DeployNotFoundError, InvalidHashError, InvalidPublicKeyError,
};
use casper::rust::api::block_report_api::BlockReportAPI;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::ProposeFunction;
use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::rp::connect::ConnectionsCell;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
#[cfg(feature = "schnorr_secp256k1_experimental")]
use crypto::rust::signatures::{
    frost_secp256k1::FrostSecp256k1, schnorr_secp256k1::SchnorrSecp256k1,
};
use eyre::{eyre, Result};
use hex;
use models::casper::LightBlockInfo;
use models::rust::casper::protocol::casper_message::DeployData;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use tracing::warn;
use utoipa::ToSchema;

use crate::rust::api::serde_types::block_info::BlockInfoSerde;
use crate::rust::api::serde_types::deploy_info::TransferInfoSerde;
use crate::rust::api::serde_types::light_block_info::LightBlockInfoSerde;
use crate::rust::web::block_info_enricher::extract_transfers_from_report;
use crate::rust::web::version_info::get_version_info_str;

const FIND_DEPLOY_RETRY_INTERVAL_MS: u64 = 50;
const FIND_DEPLOY_MAX_ATTEMPTS: u16 = 1;

fn find_deploy_retry_interval_ms() -> u64 { FIND_DEPLOY_RETRY_INTERVAL_MS }

fn find_deploy_max_attempts() -> u16 { FIND_DEPLOY_MAX_ATTEMPTS }

/// Web API trait defining the interface for HTTP endpoints
#[async_trait::async_trait]
pub trait WebApi {
    /// Get API status information
    async fn status(&self) -> Result<ApiStatus>;

    /// Prepare deploy request
    async fn prepare_deploy(&self, request: Option<PrepareRequest>) -> Result<PrepareResponse>;

    /// Deploy a contract
    async fn deploy(&self, request: DeployRequest) -> Result<String>;

    /// Get data at a par (parallel expression)
    async fn get_data_at_par(
        &self,
        request: DataAtNameByBlockHashRequest,
    ) -> Result<RhoDataResponse>;

    /// Get the last finalized block
    async fn last_finalized_block(&self, view: ViewMode) -> Result<BlockInfoSerde>;

    /// Get a specific block by hash
    async fn get_block(&self, hash: String, view: ViewMode) -> Result<BlockInfoSerde>;

    /// Get blocks with specified depth
    async fn get_blocks(&self, depth: i32, view: ViewMode) -> Result<Vec<BlockInfoSerde>>;

    /// Find a deploy by ID with the specified view.
    async fn find_deploy(&self, deploy_id: String, view: ViewMode) -> Result<DeployResponse>;

    /// Perform exploratory deploy against the requested block or the LFB post-state.
    async fn exploratory_deploy(
        &self,
        term: String,
        block_hash: Option<String>,
        use_pre_state_hash: bool,
    ) -> Result<RhoDataResponse>;

    /// Get blocks by height range
    async fn get_blocks_by_heights(
        &self,
        start_block_number: i64,
        end_block_number: i64,
        view: ViewMode,
    ) -> Result<Vec<BlockInfoSerde>>;

    /// Check if a block is finalized
    async fn is_finalized(&self, hash: String) -> Result<bool>;

    /// Query the finalization status of a deploy by its signature (hex-encoded).
    async fn deploy_finalization_status(
        &self,
        deploy_sig_hex: String,
    ) -> Result<DeployFinalizationStatusJson>;

    /// Bulk snapshot of pending deploys (deploy_storage + rejected-recovery
    /// buffer), optionally filtered by deployer public key (hex-encoded).
    async fn get_pending_deploys(&self, deployer: Option<String>) -> Result<PendingDeploysJson>;

    /// Get balance for an address via exploratory deploy against SystemVault.
    /// Queries against `block_hash` if provided, otherwise LFB.
    async fn get_balance(
        &self,
        address: String,
        block_hash: Option<String>,
    ) -> Result<BalanceResponse>;

    /// Look up a registry URI via exploratory deploy.
    /// Queries against `block_hash` if provided, otherwise LFB.
    async fn get_registry(
        &self,
        uri: String,
        block_hash: Option<String>,
    ) -> Result<RegistryResponse>;

    /// Get active validator set via exploratory deploy against PoS contract.
    /// Queries against `block_hash` if provided, otherwise LFB.
    async fn get_validators(&self, block_hash: Option<String>) -> Result<ValidatorsResponse>;

    /// Get epoch info via exploratory deploy against PoS contract.
    /// Queries against `block_hash` if provided, otherwise LFB.
    async fn get_epoch(&self, block_hash: Option<String>) -> Result<EpochResponse>;

    /// Estimate phlogiston cost of Rholang code via exploratory deploy
    async fn estimate_cost(
        &self,
        term: String,
        block_hash: Option<String>,
        deployer: Option<String>,
    ) -> Result<EstimateCostResponse>;

    /// Get current epoch rewards from PoS contract
    async fn get_epoch_rewards(&self, block_hash: Option<String>) -> Result<EpochRewardsResponse>;

    /// Get status of a specific validator (bond, active/quarantined)
    async fn get_validator(
        &self,
        pubkey: String,
        block_hash: Option<String>,
    ) -> Result<ValidatorStatusResponse>;

    /// Check if a public key is bonded
    async fn get_bond_status(&self, pubkey: String) -> Result<BondStatusResponse>;
}

/// JSON-serializable view of a deploy-finalization-status response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeployFinalizationStatusJson {
    /// One of "Finalized", "Failed", "Pending", "Expired".
    pub state: String,
    pub rejection_count: u32,
    /// Hex-encoded block hash. Absent (`null`) when the deploy has never
    /// been included in any block.
    pub latest_block_hash: Option<String>,
}

/// JSON-serializable view of a single pending deploy.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PendingDeployJson {
    #[serde(rename = "deployId")]
    pub deploy_id: String,
    pub term: String,
    pub timestamp: i64,
    #[serde(rename = "validAfterBlockNumber")]
    pub valid_after_block_number: i64,
    #[serde(rename = "shardId")]
    pub shard_id: String,
    /// Hex-encoded deployer public key.
    pub deployer: String,
    /// Hex-encoded deploy signature.
    pub sig: String,
    #[serde(rename = "sigAlgorithm")]
    pub sig_algorithm: String,
    #[serde(rename = "expirationTimestamp")]
    pub expiration_timestamp: Option<i64>,
    /// `true` when the deploy is in the rejected-recovery buffer
    /// (recovering after a merge conflict); `false` when it is fresh in
    /// deploy_storage (not yet proposed).
    #[serde(rename = "isRejected")]
    pub is_rejected: bool,
}

/// JSON-serializable view of a pending-deploys response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PendingDeploysJson {
    pub deploys: Vec<PendingDeployJson>,
    /// Total count of pending deploys that matched the query before cap
    /// truncation was applied. Compare with `deploys.len()` to detect
    /// truncation.
    #[serde(rename = "totalAvailable")]
    pub total_available: u32,
}

fn deploy_state_json_label(
    state: casper::rust::api::deploy_finalization_status::DeployFinalizationState,
) -> &'static str {
    use casper::rust::api::deploy_finalization_status::DeployFinalizationState as S;
    match state {
        S::Finalized => "Finalized",
        S::Failed => "Failed",
        S::Pending => "Pending",
        S::Expired => "Expired",
    }
}

/// Web API implementation
pub struct WebApiImpl {
    api_max_blocks_limit: i32,
    dev_mode: bool,
    network_id: String,
    shard_id: String,
    min_phlo_price: i64,
    native_token_name: String,
    native_token_symbol: String,
    native_token_decimals: u32,
    is_node_read_only: bool,
    engine_cell: Arc<EngineCell>,
    block_report_api: BlockReportAPI,
    transfer_unforgeable: models::rhoapi::Par,
    rp_conf_cell: comm::rust::rp::rp_conf::RPConfCell,
    connections_cell: ConnectionsCell,
    node_discovery: Arc<dyn NodeDiscovery + Send + Sync>,
    trigger_propose_f: Option<Arc<ProposeFunction>>,
    epoch_length: i32,
    quarantine_length: i32,
    is_ready: Arc<AtomicBool>,
}

impl WebApiImpl {
    pub fn new(
        api_max_blocks_limit: i32,
        dev_mode: bool,
        network_id: String,
        shard_id: String,
        min_phlo_price: i64,
        native_token_name: String,
        native_token_symbol: String,
        native_token_decimals: u32,
        is_node_read_only: bool,
        block_report_api: BlockReportAPI,
        transfer_unforgeable: models::rhoapi::Par,
        engine_cell: Arc<EngineCell>,
        rp_conf_cell: comm::rust::rp::rp_conf::RPConfCell,
        connections_cell: ConnectionsCell,
        node_discovery: Arc<dyn NodeDiscovery + Send + Sync>,
        trigger_propose_f: Option<Arc<ProposeFunction>>,
        epoch_length: i32,
        quarantine_length: i32,
        is_ready: Arc<AtomicBool>,
    ) -> Self {
        Self {
            api_max_blocks_limit,
            dev_mode,
            network_id,
            shard_id,
            min_phlo_price,
            native_token_name,
            native_token_symbol,
            native_token_decimals,
            is_node_read_only,
            engine_cell,
            block_report_api,
            transfer_unforgeable,
            rp_conf_cell,
            connections_cell,
            node_discovery,
            trigger_propose_f,
            epoch_length,
            quarantine_length,
            is_ready,
        }
    }
    /// Resolve a block hash to query against. If provided, use it; otherwise use LFB.
    /// Returns (block_hash, block_number).
    async fn resolve_block(&self, block_hash: Option<String>) -> Result<(String, i64)> {
        match block_hash {
            Some(hash) => {
                let block = BlockAPI::get_block(&self.engine_cell, &hash).await?;
                let bi = block
                    .block_info
                    .as_ref()
                    .ok_or_else(|| eyre!("Block {} returned without block_info", hash))?;
                Ok((hash, bi.block_number))
            }
            None => {
                let lfb = BlockAPI::last_finalized_block(&self.engine_cell).await?;
                let bi = lfb
                    .block_info
                    .as_ref()
                    .ok_or_else(|| eyre!("Last finalized block returned without block_info"))?;
                Ok((bi.block_hash.clone(), bi.block_number))
            }
        }
    }

    /// Enrich a BlockInfoSerde with transfer data from the block report.
    ///
    /// A report gives each deploy `Some(transfers)`, where an empty vector means
    /// the deploy moved nothing. Anything else — a validator node, a reporter busy
    /// with another replay, pruned pre-state, or a store failure — gives `None`,
    /// which omits the field rather than claiming the deploy had no transfers.
    async fn enrich_transfers(&self, serde: &mut BlockInfoSerde, block_hash_hex: String) {
        let deploys = match serde.deploys.as_mut() {
            Some(deploys) => deploys,
            None => return,
        };

        let block_hash_bytes: prost::bytes::Bytes = match hex::decode(&block_hash_hex) {
            Ok(bytes) => bytes.into(),
            Err(_) => {
                for deploy in deploys {
                    deploy.transfers = None;
                }
                return;
            }
        };
        // Serves the cached report when there is one and replays only when the
        // reporter is idle: block_report refuses rather than queues, so a read
        // arriving during catch-up returns without waiting instead of adding to
        // the load. A replay that does happen also caches, so ordinary reads
        // repopulate what pre-caching missed.
        match self
            .block_report_api
            .block_report(block_hash_bytes, false)
            .await
        {
            Ok(report) => {
                let transfers_by_deploy =
                    extract_transfers_from_report(&report, &self.transfer_unforgeable);
                for deploy in deploys {
                    let deploy_id = if deploy.deploy_id.is_empty() {
                        &deploy.sig
                    } else {
                        &deploy.deploy_id
                    };
                    deploy.transfers = Some(
                        transfers_by_deploy
                            .get(deploy_id)
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .map(TransferInfoSerde::from)
                            .collect(),
                    );
                }
            }
            Err(_) => {
                for deploy in deploys {
                    deploy.transfers = None;
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl WebApi for WebApiImpl {
    async fn status(&self) -> Result<ApiStatus> {
        const STATUS_SLOW_THRESHOLD: Duration = Duration::from_millis(500);
        let total_start = Instant::now();

        let rp_conf_start = Instant::now();
        let rp_conf = self.rp_conf_cell.read()?;
        let rp_conf_elapsed = rp_conf_start.elapsed();

        let connections_start = Instant::now();
        let address = rp_conf.local.to_address();
        let connections = self.connections_cell.read()?;
        let connections_elapsed = connections_start.elapsed();

        let discovery_start = Instant::now();
        let discovered_nodes = self.node_discovery.peers()?;
        let discovery_elapsed = discovery_start.elapsed();

        let peers = connections.len() as i32;
        let nodes = discovered_nodes.len() as i32;

        // Create a set of connected peer IDs for quick lookup
        let connected_ids: std::collections::HashSet<_> =
            connections.iter().map(|p| p.id.key.clone()).collect();

        // Convert PeerNode to PeerInfoData with connection status
        let peer_list: Vec<PeerInfoData> = discovered_nodes
            .iter()
            .map(|node| PeerInfoData {
                address: node.to_address(),
                node_id: node.id.to_string(),
                host: node.endpoint.host.clone(),
                protocol_port: node.endpoint.tcp_port as i32,
                discovery_port: node.endpoint.udp_port as i32,
                is_connected: connected_ids.contains(&node.id.key),
            })
            .collect();

        // Timed below, not above: this is the one step that waits on `global_lock`,
        // so it is the step a slow-status warning most needs to name.
        let lfb_start = Instant::now();
        let lfb_number = match BlockAPI::last_finalized_block(&self.engine_cell).await {
            Ok(block_info) => block_info
                .block_info
                .as_ref()
                .map(|bi| bi.block_number)
                .unwrap_or(-1),
            Err(_) => -1,
        };
        let lfb_elapsed = lfb_start.elapsed();

        let total_elapsed = total_start.elapsed();
        if total_elapsed >= STATUS_SLOW_THRESHOLD {
            warn!(
                ?total_elapsed,
                ?rp_conf_elapsed,
                ?connections_elapsed,
                ?discovery_elapsed,
                ?lfb_elapsed,
                peers,
                nodes,
                "Web API status assembly is slow"
            );
        }

        let is_validator = self.trigger_propose_f.is_some();
        let is_ready = self.is_ready.load(Ordering::Relaxed);
        let current_epoch = if self.epoch_length > 0 && lfb_number >= 0 {
            lfb_number / self.epoch_length as i64
        } else {
            0
        };

        Ok(ApiStatus {
            version: VersionInfo {
                api: "1".to_string(),
                node: get_version_info_str(),
            },
            address,
            network_id: self.network_id.clone(),
            shard_id: self.shard_id.clone(),
            peers,
            nodes,
            min_phlo_price: self.min_phlo_price,
            peer_list,
            native_token_name: self.native_token_name.clone(),
            native_token_symbol: self.native_token_symbol.clone(),
            native_token_decimals: self.native_token_decimals,
            last_finalized_block_number: lfb_number,
            is_validator,
            is_read_only: self.is_node_read_only,
            is_ready,
            current_epoch,
            epoch_length: self.epoch_length,
        })
    }

    async fn prepare_deploy(&self, request: Option<PrepareRequest>) -> Result<PrepareResponse> {
        let seq_number = BlockAPI::get_latest_message(&self.engine_cell)
            .await
            .map(|message| message.sequence_number)
            .unwrap_or(-1);

        let names = prepare_deploy_names(request.as_ref())?;

        Ok(PrepareResponse { names, seq_number })
    }

    async fn deploy(&self, request: DeployRequest) -> Result<String> {
        let cosigned_deploy = to_cosigned_deploy(&request)?;
        BlockAPI::deploy_cosigned(
            &self.engine_cell,
            cosigned_deploy,
            &self.trigger_propose_f,
            self.min_phlo_price,
            self.is_node_read_only,
            &self.shard_id,
        )
        .await
    }

    async fn get_data_at_par(
        &self,
        request: DataAtNameByBlockHashRequest,
    ) -> Result<RhoDataResponse> {
        let (pars, block) = BlockAPI::get_data_at_par(
            &self.engine_cell,
            &to_par(request.name)?,
            request.block_hash,
            request.use_pre_state_hash,
        )
        .await?;

        Ok(to_rho_data_response(pars, block, 0))
    }

    async fn last_finalized_block(&self, view: ViewMode) -> Result<BlockInfoSerde> {
        let block_info = BlockAPI::last_finalized_block(&self.engine_cell).await?;
        let mut serde = BlockInfoSerde::from(block_info);
        if view == ViewMode::Full {
            let block_hash = serde.block_info.block_hash.clone();
            self.enrich_transfers(&mut serde, block_hash).await;
        } else {
            serde.deploys = None;
        }
        Ok(serde)
    }

    async fn get_block(&self, hash: String, view: ViewMode) -> Result<BlockInfoSerde> {
        let block_info = BlockAPI::get_block(&self.engine_cell, &hash).await?;
        let mut serde = BlockInfoSerde::from(block_info);
        if view == ViewMode::Full {
            let block_hash = serde.block_info.block_hash.clone();
            self.enrich_transfers(&mut serde, block_hash).await;
        } else {
            serde.deploys = None;
        }
        Ok(serde)
    }

    async fn get_blocks(&self, depth: i32, view: ViewMode) -> Result<Vec<BlockInfoSerde>> {
        if view == ViewMode::Full {
            let blocks =
                BlockAPI::get_blocks_full(&self.engine_cell, depth, self.api_max_blocks_limit)
                    .await?;
            Ok(blocks.into_iter().map(BlockInfoSerde::from).collect())
        } else {
            let blocks =
                BlockAPI::get_blocks(&self.engine_cell, depth, self.api_max_blocks_limit).await?;
            Ok(blocks
                .into_iter()
                .map(|block| BlockInfoSerde::from_light(LightBlockInfoSerde::from(block)))
                .collect())
        }
    }

    async fn find_deploy(&self, deploy_id: String, view: ViewMode) -> Result<DeployResponse> {
        let deploy_id_bytes = hex::decode(&deploy_id).map_err(|_| {
            eyre::Report::new(InvalidHashError(format!(
                "'{}' is not a valid hex deploy ID",
                deploy_id
            )))
        })?;
        let deploy_lookup_id =
            BlockAPI::deploy_lookup_id(&self.engine_cell, &deploy_id_bytes).await?;

        let retry_interval_ms = find_deploy_retry_interval_ms();
        let max_attempts = find_deploy_max_attempts();

        // Retry loop: deploy may not be visible in DAG immediately after submission
        let light_block: LightBlockInfo = {
            let mut attempt: u16 = 1;
            loop {
                match BlockAPI::find_deploy(&self.engine_cell, &deploy_lookup_id).await {
                    Ok(block) => break block,
                    Err(err) => {
                        let not_found = err.downcast_ref::<DeployNotFoundError>().is_some();

                        if !not_found || attempt >= max_attempts {
                            return Err(err);
                        }

                        tracing::debug!(
                            ?attempt,
                            ?max_attempts,
                            ?retry_interval_ms,
                            ?deploy_id,
                            "Waiting for deploy to become visible in block DAG"
                        );
                        sleep(Duration::from_millis(retry_interval_ms)).await;
                        attempt += 1;
                    }
                }
            }
        };
        let known_block_hash = hex::decode(&light_block.block_hash)
            .map(prost::bytes::Bytes::from)
            .map_err(|e| eyre!("Invalid block hash returned by findDeploy: {}", e))?;
        let light_block = LightBlockInfoSerde::from(light_block);

        // Always fetch full block to get deploy execution details
        let block_info = self
            .get_block(light_block.block_hash.clone(), ViewMode::Full)
            .await?;

        let deploys = block_info
            .deploys
            .as_ref()
            .ok_or_else(|| eyre!("Block {} returned without deploys", light_block.block_hash))?;

        let deploy = deploys
            .iter()
            .find(|candidate| candidate.deploy_id == deploy_id)
            .ok_or_else(|| {
                eyre!(
                    "Deploy {} found in block {} but not in deploy list",
                    deploy_id,
                    light_block.block_hash
                )
            })?;

        let is_full = view == ViewMode::Full;
        let finalization = BlockAPI::deploy_finalization_status_with_known_block(
            &self.engine_cell,
            &deploy_lookup_id,
            Some(&known_block_hash),
        )
        .await?;

        Ok(DeployResponse {
            deploy_id,
            block_hash: light_block.block_hash,
            block_number: light_block.block_number,
            timestamp: light_block.timestamp,
            cost: deploy.cost,
            errored: deploy.errored,
            is_finalized: light_block.is_finalized,
            finalization_state: deploy_state_json_label(finalization.state).to_string(),
            rejection_count: finalization.rejection_count,
            deployer: if is_full {
                Some(deploy.deployer.clone())
            } else {
                None
            },
            term: if is_full {
                Some(deploy.term.clone())
            } else {
                None
            },
            system_deploy_error: if is_full {
                Some(deploy.system_deploy_error.clone())
            } else {
                None
            },
            sig_algorithm: if is_full {
                Some(deploy.sig_algorithm.clone())
            } else {
                None
            },
            valid_after_block_number: if is_full {
                Some(deploy.valid_after_block_number)
            } else {
                None
            },
            transfers: if is_full {
                deploy.transfers.clone()
            } else {
                None
            },
        })
    }

    async fn exploratory_deploy(
        &self,
        term: String,
        block_hash: Option<String>,
        use_pre_state_hash: bool,
    ) -> Result<RhoDataResponse> {
        let (pars, block, cost) = BlockAPI::exploratory_deploy(
            &self.engine_cell,
            term,
            block_hash,
            use_pre_state_hash,
            self.dev_mode,
            None,
        )
        .await?;

        Ok(to_rho_data_response(pars, block, cost))
    }

    async fn get_blocks_by_heights(
        &self,
        start_block_number: i64,
        end_block_number: i64,
        view: ViewMode,
    ) -> Result<Vec<BlockInfoSerde>> {
        if view == ViewMode::Full {
            let blocks = BlockAPI::get_blocks_by_heights_full(
                &self.engine_cell,
                start_block_number,
                end_block_number,
                self.api_max_blocks_limit,
            )
            .await?;
            Ok(blocks.into_iter().map(BlockInfoSerde::from).collect())
        } else {
            let blocks = BlockAPI::get_blocks_by_heights(
                &self.engine_cell,
                start_block_number,
                end_block_number,
                self.api_max_blocks_limit,
            )
            .await?;
            Ok(blocks
                .into_iter()
                .map(|block| BlockInfoSerde::from_light(LightBlockInfoSerde::from(block)))
                .collect())
        }
    }

    async fn is_finalized(&self, hash: String) -> Result<bool> {
        BlockAPI::is_finalized(&self.engine_cell, &hash).await
    }

    async fn deploy_finalization_status(
        &self,
        deploy_sig_hex: String,
    ) -> Result<DeployFinalizationStatusJson> {
        let sig = hex::decode(deploy_sig_hex.trim_start_matches("0x")).map_err(|_| {
            eyre::Report::new(InvalidHashError(format!(
                "'{}' is not a valid hex deploy signature",
                deploy_sig_hex
            )))
        })?;
        let deploy_id = BlockAPI::deploy_lookup_id(&self.engine_cell, &sig).await?;
        let status = BlockAPI::deploy_finalization_status(&self.engine_cell, &deploy_id).await?;
        Ok(DeployFinalizationStatusJson {
            state: deploy_state_json_label(status.state).to_string(),
            rejection_count: status.rejection_count,
            latest_block_hash: status.latest_block_hash.map(|h| hex::encode(&h)),
        })
    }

    async fn get_pending_deploys(&self, deployer: Option<String>) -> Result<PendingDeploysJson> {
        let deployer_bytes = match deployer.as_deref() {
            None => None,
            Some(s) if s.trim_start_matches("0x").is_empty() => None,
            Some(s) => {
                let hex_str = s.trim_start_matches("0x");
                Some(hex::decode(hex_str).map_err(|_| {
                    eyre::Report::new(InvalidPublicKeyError(format!(
                        "'{}' is not a valid hex deployer public key",
                        s
                    )))
                })?)
            }
        };

        let snapshot =
            BlockAPI::list_pending_deploys(&self.engine_cell, deployer_bytes.as_deref()).await?;

        let deploys = snapshot
            .deploys
            .into_iter()
            .map(|(envelope, is_rejected)| {
                let dd = envelope.data();
                let signed = envelope.primary();
                let deploy_id = if envelope.is_envelope_bound() {
                    envelope
                        .envelope_commitment()
                        .expect("validated protocol-v6 envelope")
                } else {
                    signed.sig.clone()
                };
                PendingDeployJson {
                    deploy_id: hex::encode(deploy_id),
                    term: dd.term.clone(),
                    timestamp: dd.time_stamp,
                    valid_after_block_number: dd.valid_after_block_number,
                    shard_id: dd.shard_id.clone(),
                    deployer: hex::encode(&signed.pk.bytes),
                    sig: hex::encode(&signed.sig),
                    sig_algorithm: signed.sig_algorithm.name(),
                    expiration_timestamp: dd.expiration_timestamp,
                    is_rejected,
                }
            })
            .collect();

        Ok(PendingDeploysJson {
            deploys,
            total_available: snapshot.total_available,
        })
    }

    async fn get_balance(
        &self,
        address: String,
        block_hash: Option<String>,
    ) -> Result<BalanceResponse> {
        let term = format!(
            r#"new return, rl(`rho:registry:lookup`), systemVaultCh, vaultCh, balanceCh in {{
  rl!(`rho:vault:system`, *systemVaultCh) |
  for (@(_, SystemVault) <- systemVaultCh) {{
    @SystemVault!("findOrCreate", "{address}", *vaultCh) |
    for (@either <- vaultCh) {{
      match either {{
        (true, vault) => {{
          @vault!("balance", *balanceCh) |
          for (@balance <- balanceCh) {{
            return!(balance)
          }}
        }}
        (false, errorMsg) => {{
          return!(errorMsg)
        }}
      }}
    }}
  }}
}}"#
        );

        let (resolved_hash, block_number) = self.resolve_block(block_hash).await?;

        let (pars, _block, _cost) = BlockAPI::exploratory_deploy(
            &self.engine_cell,
            term,
            Some(resolved_hash.clone()),
            false,
            self.dev_mode,
            None,
        )
        .await?;

        let exprs: Vec<RhoExpr> = pars.into_iter().filter_map(expr_from_par_proto).collect();
        let balance = match exprs.first() {
            Some(RhoExpr::ExprInt { data }) => *data,
            _ => return Err(eyre!("Unexpected balance result for address {}", address)),
        };

        Ok(BalanceResponse {
            address,
            balance,
            block_number,
            block_hash: resolved_hash,
        })
    }

    async fn get_registry(
        &self,
        uri: String,
        block_hash: Option<String>,
    ) -> Result<RegistryResponse> {
        let term = format!(
            r#"new return, rl(`rho:registry:lookup`), ch in {{
  rl!(`{uri}`, *ch) |
  for (@val <- ch) {{
    match val {{
      (true, data) => {{ return!(data) }}
      (false, _)   => {{ return!("not found") }}
    }}
  }}
}}"#
        );

        let (resolved_hash, block_number) = self.resolve_block(block_hash).await?;

        let (pars, _block, _cost) = BlockAPI::exploratory_deploy(
            &self.engine_cell,
            term,
            Some(resolved_hash.clone()),
            false,
            self.dev_mode,
            None,
        )
        .await?;

        let data: Vec<RhoExpr> = pars.into_iter().filter_map(expr_from_par_proto).collect();

        Ok(RegistryResponse {
            uri,
            data,
            block_number,
            block_hash: resolved_hash,
        })
    }

    async fn get_validators(&self, block_hash: Option<String>) -> Result<ValidatorsResponse> {
        let term = r#"new return, rl(`rho:registry:lookup`), poSCh in {
  rl!(`rho:system:pos`, *poSCh) |
  for(@(_, PoS) <- poSCh) {
    @PoS!("getBonds", *return)
  }
}"#
        .to_string();

        let (resolved_hash, block_number) = self.resolve_block(block_hash).await?;

        let (pars, _block, _cost) = BlockAPI::exploratory_deploy(
            &self.engine_cell,
            term,
            Some(resolved_hash.clone()),
            false,
            self.dev_mode,
            None,
        )
        .await?;

        let exprs: Vec<RhoExpr> = pars.into_iter().filter_map(expr_from_par_proto).collect();

        let mut validators = Vec::new();
        let mut total_stake: i64 = 0;

        // getBonds returns a Rholang map: {pubkey: stake, ...}
        // ExprMap keys are already String (extracted by extract_key_from_expr)
        if let Some(RhoExpr::ExprMap { data }) = exprs.first() {
            for (public_key, value) in data {
                let stake = match value {
                    RhoExpr::ExprInt { data } => *data,
                    other => {
                        return Err(eyre!(
                            "Unexpected stake type for validator {}: {:?}",
                            public_key,
                            other
                        ))
                    }
                };
                total_stake += stake;
                validators.push(ValidatorInfo {
                    public_key: public_key.clone(),
                    stake,
                });
            }
        }

        Ok(ValidatorsResponse {
            validators,
            total_stake,
            block_number,
            block_hash: resolved_hash,
        })
    }

    async fn get_epoch(&self, block_hash: Option<String>) -> Result<EpochResponse> {
        let (resolved_hash, block_number) = self.resolve_block(block_hash).await?;

        let epoch_length = self.epoch_length as i64;
        let quarantine_length = self.quarantine_length as i64;

        let current_epoch = if epoch_length > 0 {
            block_number / epoch_length
        } else {
            0
        };
        let blocks_until_next_epoch = if epoch_length > 0 {
            epoch_length - (block_number % epoch_length)
        } else {
            0
        };

        Ok(EpochResponse {
            current_epoch,
            epoch_length,
            quarantine_length,
            blocks_until_next_epoch,
            last_finalized_block_number: block_number,
            block_hash: resolved_hash,
        })
    }

    async fn estimate_cost(
        &self,
        term: String,
        block_hash: Option<String>,
        deployer: Option<String>,
    ) -> Result<EstimateCostResponse> {
        let (resolved_hash, block_number) = self.resolve_block(block_hash).await?;

        let deployer_pk = deployer
            .map(|hex_str| {
                validate_and_decode_pubkey(&hex_str).map(|bytes| PublicKey::from_bytes(&bytes))
            })
            .transpose()?;

        let deployer_identity = if deployer_pk.is_some() {
            DeployerIdentity::Provided
        } else {
            DeployerIdentity::Ephemeral
        };

        let (_pars, _block, cost) = BlockAPI::exploratory_deploy(
            &self.engine_cell,
            term,
            Some(resolved_hash.clone()),
            false,
            self.dev_mode,
            deployer_pk,
        )
        .await?;

        Ok(EstimateCostResponse {
            cost,
            block_number,
            block_hash: resolved_hash,
            deployer_identity,
        })
    }

    async fn get_epoch_rewards(&self, block_hash: Option<String>) -> Result<EpochRewardsResponse> {
        let term = r#"new return, rl(`rho:registry:lookup`), poSCh in {
  rl!(`rho:system:pos`, *poSCh) |
  for(@(_, PoS) <- poSCh) {
    @PoS!("getCurrentEpochRewards", *return)
  }
}"#
        .to_string();

        let (resolved_hash, block_number) = self.resolve_block(block_hash).await?;

        let (pars, _block, _cost) = BlockAPI::exploratory_deploy(
            &self.engine_cell,
            term,
            Some(resolved_hash.clone()),
            false,
            self.dev_mode,
            None,
        )
        .await?;

        let exprs: Vec<RhoExpr> = pars.into_iter().filter_map(expr_from_par_proto).collect();
        let rewards = exprs
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("No result from getCurrentEpochRewards"))?;

        Ok(EpochRewardsResponse {
            rewards,
            block_number,
            block_hash: resolved_hash,
        })
    }

    async fn get_validator(
        &self,
        pubkey: String,
        block_hash: Option<String>,
    ) -> Result<ValidatorStatusResponse> {
        let _ = validate_and_decode_pubkey(&pubkey)?;

        let term = r#"new return, rl(`rho:registry:lookup`), poSCh in {
            rl!(`rho:system:pos`, *poSCh) |
            for(@(_, PoS) <- poSCh) { @PoS!("getBonds", *return) }
        }"#
        .to_string();

        let (resolved_hash, block_number) = self.resolve_block(block_hash).await?;

        let (pars, _block, _cost) = BlockAPI::exploratory_deploy(
            &self.engine_cell,
            term,
            Some(resolved_hash.clone()),
            false,
            self.dev_mode,
            None,
        )
        .await?;

        let exprs: Vec<RhoExpr> = pars.into_iter().filter_map(expr_from_par_proto).collect();

        let mut is_bonded = false;
        let mut stake = None;

        if let Some(RhoExpr::ExprMap { data }) = exprs.first() {
            if let Some(value) = data.get(&pubkey) {
                is_bonded = true;
                if let RhoExpr::ExprInt { data } = value {
                    stake = Some(*data);
                }
            }
        }

        Ok(ValidatorStatusResponse {
            public_key: pubkey,
            is_bonded,
            stake,
            block_number,
            block_hash: resolved_hash,
        })
    }

    async fn get_bond_status(&self, pubkey: String) -> Result<BondStatusResponse> {
        let pk_bytes = validate_and_decode_pubkey(&pubkey)?;

        let is_bonded = BlockAPI::bond_status(&self.engine_cell, &pk_bytes).await?;

        Ok(BondStatusResponse {
            public_key: pubkey,
            is_bonded,
        })
    }
}

// Rholang terms interesting for translation to JSON
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(no_recursion)]
pub enum RhoExpr {
    // === Collections ===
    ExprPar {
        data: Vec<RhoExpr>,
    },
    ExprTuple {
        data: Vec<RhoExpr>,
    },
    ExprList {
        data: Vec<RhoExpr>,
    },
    ExprSet {
        data: Vec<RhoExpr>,
    },
    ExprMap {
        data: HashMap<String, RhoExpr>,
    },

    // === Primitives ===
    ExprBool {
        data: bool,
    },
    ExprInt {
        data: i64,
    },
    ExprString {
        data: String,
    },
    ExprUri {
        data: String,
    },
    ExprBytes {
        data: String,
    },

    // === Extended numerics ===
    ExprFloat {
        data: f64,
    },
    ExprBigInt {
        data: String,
    },
    ExprBigRat {
        numerator: String,
        denominator: String,
    },
    ExprFixedPoint {
        value: String,
        scale: u32,
    },

    // === Unforgeable names ===
    ExprUnforg {
        data: RhoUnforg,
    },

    // === Bundle (with permissions) ===
    ExprBundle {
        data: Box<RhoExpr>,
        read: bool,
        write: bool,
    },

    // === Unary operators ===
    ExprNot {
        data: Box<RhoExpr>,
    },
    ExprNeg {
        data: Box<RhoExpr>,
    },

    // === Binary arithmetic ===
    ExprPlus {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprMinus {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprMult {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprDiv {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprMod {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },

    // === Comparison ===
    ExprLt {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprLte {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprGt {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprGte {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprEq {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprNeq {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },

    // === Logical ===
    ExprAnd {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprOr {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },

    // === String operations ===
    ExprConcat {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprInterpolate {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },
    ExprDiff {
        left: Box<RhoExpr>,
        right: Box<RhoExpr>,
    },

    // === Pattern matching ===
    ExprMatches {
        target: Box<RhoExpr>,
        pattern: Box<RhoExpr>,
    },

    // === Method call ===
    ExprMethod {
        target: Box<RhoExpr>,
        name: String,
        args: Vec<RhoExpr>,
    },

    // === Variable reference ===
    ExprVar {
        index: i32,
    },

    // === System ===
    ExprSysAuthToken,

    // === Catch-all for unknown/unhandled types ===
    ExprUnknown {
        type_name: String,
    },
}

/// Unforgeable name types
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub enum RhoUnforg {
    UnforgPrivate { data: String },
    UnforgDeploy { data: String },
    UnforgDeployer { data: String },
    UnforgAuthority { data: String },
    UnforgPrincipal { key_family: u32, data: String },
    UnforgSysAuthToken,
}

// API request & response types
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeployRequest {
    pub data: DeployData,
    /// Primary signer's public key (hex-encoded).
    pub deployer: String,
    /// Primary signer's signature (hex-encoded).
    pub signature: String,
    #[serde(rename = "sigAlgorithm")]
    pub sig_algorithm: String,
    /// Additional protocol-v6 policy members beyond the first member.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cosigners: Vec<CosignerJson>,
    /// Selected-signature quorum. Omission encodes AllOf over every member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<u32>,
}

/// JSON shape for one cosigner in a multi-signature [`DeployRequest`].
/// Mirrors the wire `CompoundSigner` proto message (proto field 14 inside
/// `DeployDataProto`).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CosignerJson {
    /// Cosigner public key (hex-encoded).
    pub pk: String,
    /// Signature over the protocol-v6 envelope signing hash. Empty for an
    /// unselected threshold-policy member.
    pub signature: String,
    #[serde(rename = "sigAlgorithm")]
    pub sig_algorithm: String,
    // D3 (DR-9): no per-signer phlo_share — funding is the per-signature supply
    // pool Σ⟦s⟧, not an envelope escrow share.
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExploreDeployRequest {
    pub term: String,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
    #[serde(rename = "usePreStateHash", default)]
    pub use_pre_state_hash: bool,
}

/// Simple explore deploy request with only the term field.
/// Used by the /explore-deploy endpoint which doesn't require block hash or pre-state hash.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SimpleExploreDeployRequest {
    pub term: String,
}

/// Request body for POST /api/estimate-cost.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EstimateCostRequest {
    pub term: String,
    /// Hex-encoded 65-byte uncompressed secp256k1 public key (`04`-prefixed).
    /// For identity-dependent terms such as vault transfers, pass the deployer
    /// public key; without it the term executes under an ephemeral identity and
    /// the returned cost can be significantly lower than the real deploy cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataAtNameRequest {
    /// For simplicity only one Unforgeable name is allowed
    /// instead of the whole RhoExpr (proto Par)
    pub name: RhoUnforg,
    /// Number of blocks in the past to search for data
    pub depth: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataAtNameByBlockHashRequest {
    pub name: RhoUnforg,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
    #[serde(rename = "usePreStateHash", default)]
    pub use_pre_state_hash: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataAtNameResponse {
    pub exprs: Vec<RhoExprWithBlock>,
    pub length: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RhoExprWithBlock {
    pub expr: RhoExpr,
    pub block: LightBlockInfoSerde,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExploratoryDeployResponse {
    pub expr: Vec<RhoExpr>,
    pub block: LightBlockInfoSerde,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RhoDataResponse {
    pub expr: Vec<RhoExpr>,
    pub block: LightBlockInfoSerde,
    pub cost: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PrepareRequest {
    pub deployer: String,
    pub timestamp: i64,
    #[serde(rename = "nameQty")]
    pub name_qty: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PrepareResponse {
    pub names: Vec<String>,
    #[serde(rename = "seqNumber")]
    pub seq_number: i32,
}

fn prepare_deploy_names(request: Option<&PrepareRequest>) -> Result<Vec<String>> {
    match request {
        Some(request) => {
            let deployer = hex::decode(&request.deployer)
                .map_err(|error| eyre!("Deployer is not valid hex format: {}", error))?;
            BlockAPI::preview_private_names(
                &deployer,
                request.timestamp,
                request.name_qty,
                casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            )
            .map(|names| names.into_iter().map(hex::encode).collect())
        }
        None => Ok(Vec::new()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PeerInfoData {
    pub address: String,
    #[serde(rename = "nodeId")]
    pub node_id: String,
    pub host: String,
    #[serde(rename = "protocolPort")]
    pub protocol_port: i32,
    #[serde(rename = "discoveryPort")]
    pub discovery_port: i32,
    #[serde(rename = "isConnected")]
    pub is_connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiStatus {
    pub version: VersionInfo,
    pub address: String,
    #[serde(rename = "networkId")]
    pub network_id: String,
    #[serde(rename = "shardId")]
    pub shard_id: String,
    pub peers: i32,
    pub nodes: i32,
    #[serde(rename = "minPhloPrice")]
    pub min_phlo_price: i64,
    #[serde(rename = "peerList")]
    pub peer_list: Vec<PeerInfoData>,
    /// Full display name of the native token. Baked into genesis state via
    /// the `TokenMetadata` Rholang contract at `rho:system:tokenMetadata`.
    #[serde(rename = "nativeTokenName")]
    pub native_token_name: String,
    /// Ticker symbol of the native token (e.g. "F1R3").
    #[serde(rename = "nativeTokenSymbol")]
    pub native_token_symbol: String,
    /// Decimal places used to display the native token (dust per token = 10^decimals).
    #[serde(rename = "nativeTokenDecimals")]
    pub native_token_decimals: u32,
    /// Block number of the last finalized block. -1 if casper not yet initialized.
    #[serde(rename = "lastFinalizedBlockNumber")]
    pub last_finalized_block_number: i64,
    /// Whether this node is a validator (can propose blocks).
    #[serde(rename = "isValidator")]
    pub is_validator: bool,
    /// Whether this node is running in read-only mode.
    #[serde(rename = "isReadOnly")]
    pub is_read_only: bool,
    /// Whether the node has completed initialization and entered running state.
    #[serde(rename = "isReady")]
    pub is_ready: bool,
    /// Current epoch number (lastFinalizedBlockNumber / epochLength).
    #[serde(rename = "currentEpoch")]
    pub current_epoch: i64,
    /// Blocks per epoch, from genesis configuration.
    #[serde(rename = "epochLength")]
    pub epoch_length: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VersionInfo {
    pub api: String,
    pub node: String,
}

/// Unified deploy response. Default (full) includes all fields.
/// Summary view (`?view=summary`) omits Optional fields for lightweight polling.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeployResponse {
    // === Always present (summary + full) ===
    #[serde(rename = "deployId")]
    pub deploy_id: String,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
    #[serde(rename = "blockNumber")]
    pub block_number: i64,
    pub timestamp: i64,
    pub cost: u64,
    pub errored: bool,
    #[serde(rename = "isFinalized")]
    pub is_finalized: bool,
    #[serde(rename = "finalizationState")]
    pub finalization_state: String,
    #[serde(rename = "rejectionCount")]
    pub rejection_count: u32,

    // === Full view only (omitted in summary) ===
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub term: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "systemDeployError")]
    pub system_deploy_error: Option<String>,
    // D3 (DR-9): phloPrice / phloLimit removed from the deploy response — a
    // deploy's cost is the per-COMM token count (in `cost`), no escrow.
    #[serde(skip_serializing_if = "Option::is_none", rename = "sigAlgorithm")]
    pub sig_algorithm: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "validAfterBlockNumber"
    )]
    pub valid_after_block_number: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfers: Option<Vec<TransferInfoSerde>>,
}

/// View mode for deploy lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// All fields populated (default).
    Full,
    /// Core fields only — for polling.
    Summary,
}

/// Balance query response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BalanceResponse {
    pub address: String,
    pub balance: i64,
    #[serde(rename = "blockNumber")]
    pub block_number: i64,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
}

/// Registry lookup response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegistryResponse {
    pub uri: String,
    pub data: Vec<RhoExpr>,
    #[serde(rename = "blockNumber")]
    pub block_number: i64,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
}

/// Validator info in the active set
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ValidatorInfo {
    #[serde(rename = "publicKey")]
    pub public_key: String,
    pub stake: i64,
}

/// Active validator set response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ValidatorsResponse {
    pub validators: Vec<ValidatorInfo>,
    #[serde(rename = "totalStake")]
    pub total_stake: i64,
    #[serde(rename = "blockNumber")]
    pub block_number: i64,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
}

/// Epoch info response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EpochResponse {
    #[serde(rename = "currentEpoch")]
    pub current_epoch: i64,
    #[serde(rename = "epochLength")]
    pub epoch_length: i64,
    #[serde(rename = "quarantineLength")]
    pub quarantine_length: i64,
    #[serde(rename = "blocksUntilNextEpoch")]
    pub blocks_until_next_epoch: i64,
    #[serde(rename = "lastFinalizedBlockNumber")]
    pub last_finalized_block_number: i64,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
}

/// Which identity produced a cost estimate.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeployerIdentity {
    /// The estimate was produced under the caller-supplied deployer public key.
    Provided,
    /// The estimate was produced under an ephemeral, process-wide random key.
    /// This may significantly underestimate the real deploy cost for
    /// identity-dependent terms.
    Ephemeral,
}

/// Cost estimation response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EstimateCostResponse {
    pub cost: u64,
    #[serde(rename = "blockNumber")]
    pub block_number: i64,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
    #[serde(rename = "deployerIdentity")]
    pub deployer_identity: DeployerIdentity,
}

/// Epoch rewards response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EpochRewardsResponse {
    pub rewards: RhoExpr,
    #[serde(rename = "blockNumber")]
    pub block_number: i64,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
}

/// Individual validator status response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ValidatorStatusResponse {
    #[serde(rename = "publicKey")]
    pub public_key: String,
    #[serde(rename = "isBonded")]
    pub is_bonded: bool,
    pub stake: Option<i64>,
    #[serde(rename = "blockNumber")]
    pub block_number: i64,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
}

/// Bond status response (HTTP equivalent of gRPC bondStatus)
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BondStatusResponse {
    #[serde(rename = "publicKey")]
    pub public_key: String,
    #[serde(rename = "isBonded")]
    pub is_bonded: bool,
}

// Error types

#[derive(Debug)]
pub enum WebApiError {
    BlockApiError(String),
    SignatureError(String),
    InvalidFormat(String),
}

impl std::fmt::Display for WebApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebApiError::BlockApiError(msg) => write!(f, "Block API error: {}", msg),
            WebApiError::SignatureError(msg) => write!(f, "Signature error: {}", msg),
            WebApiError::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
        }
    }
}

impl std::error::Error for WebApiError {}

fn validate_and_decode_pubkey(pubkey_hex: &str) -> Result<Vec<u8>> {
    let bytes = hex::decode(pubkey_hex).map_err(|e| {
        eyre::Report::new(InvalidPublicKeyError(format!(
            "invalid public key hex: {}",
            e
        )))
    })?;
    PublicKey::validate_secp256k1_bytes(&bytes).map_err(|e| {
        eyre::Report::new(InvalidPublicKeyError(format!("invalid public key: {}", e)))
    })?;
    Ok(bytes)
}

// Conversion functions

/// Look up a signature algorithm by name, supporting all wire-recognized
/// algorithms. Shared between `to_signed_deploy` and `to_cosigned_deploy`.
fn lookup_sig_algorithm(name: &str) -> Result<Box<dyn SignaturesAlg>> {
    match name {
        "secp256k1" => Ok(Box::new(crypto::rust::signatures::secp256k1::Secp256k1)),
        crypto::rust::signatures::secp256k1_eth::Secp256k1Eth::NAME
        | crypto::rust::signatures::secp256k1_eth::Secp256k1Eth::LEGACY_NAME => Ok(Box::new(
            crypto::rust::signatures::secp256k1_eth::Secp256k1Eth,
        )),
        "ed25519" => Ok(Box::new(crypto::rust::signatures::ed25519::Ed25519)),
        #[cfg(feature = "schnorr_secp256k1_experimental")]
        "schnorr-secp256k1" => Ok(Box::new(SchnorrSecp256k1)),
        #[cfg(feature = "schnorr_secp256k1_experimental")]
        "frost-secp256k1" => Ok(Box::new(FrostSecp256k1)),
        _ => Err(eyre!("Signature algorithm not supported: {}", name)),
    }
}

/// Convert DeployRequest to a protocol-v6 [`Cosigned<DeployData>`] envelope.
/// Invariants enforced by `Cosigned::from_envelope_signed_data_threshold`:
///
/// - Every selected signature verifies against the scheme-bound envelope hash.
/// - Canonical principal ordering with no duplicate ground authority.
///
/// D3 (DR-9): no per-signer phlo_share and no share-sum invariant — funding is
/// the per-signature supply pool Σ⟦s⟧.
fn to_cosigned_deploy(
    request: &DeployRequest,
) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>> {
    use crypto::rust::signatures::signed::{Cosigned, Cosigner};

    let primary_pk_bytes = hex::decode(&request.deployer)
        .map_err(|e| eyre!("Primary public key is not valid base16: {}", e))?;
    let primary_sig_bytes = hex::decode(&request.signature)
        .map_err(|e| eyre!("Primary signature is not valid base16: {}", e))?;

    // D3 (DR-9): no per-signer phlo_share — funding is the per-signature supply
    // pool Σ⟦s⟧, not an envelope escrow share.
    let mut signers = Vec::with_capacity(1 + request.cosigners.len());
    signers.push(Cosigner {
        pk: PublicKey::from_bytes(&primary_pk_bytes),
        sig: primary_sig_bytes.into(),
        sig_algorithm: lookup_sig_algorithm(&request.sig_algorithm)?,
    });
    for (i, cs) in request.cosigners.iter().enumerate() {
        let pk_bytes = hex::decode(&cs.pk)
            .map_err(|e| eyre!("Cosigner {} public key is not valid base16: {}", i, e))?;
        let sig_bytes = hex::decode(&cs.signature)
            .map_err(|e| eyre!("Cosigner {} signature is not valid base16: {}", i, e))?;
        signers.push(Cosigner {
            pk: PublicKey::from_bytes(&pk_bytes),
            sig: sig_bytes.into(),
            sig_algorithm: lookup_sig_algorithm(&cs.sig_algorithm)?,
        });
    }

    let threshold = request
        .threshold
        .unwrap_or_else(|| signers.len().try_into().unwrap_or(u32::MAX));
    Cosigned::from_envelope_signed_data_threshold(request.data.clone(), signers, threshold)
        .map_err(|e| eyre!("Cosigned envelope validation failed: {}", e))
}

// Conversion functions for protobuf generated types
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Bundle, Expr, GAuthorityId, GDeployId, GDeployerId, GPrincipalId, GPrivate, GUnforgeable, Par,
};

/// Convert RhoUnforg to protobuf GUnforgeable.
/// Hex decode errors produce empty bytes with a warning log.
fn unforg_to_unforg_proto(unforg: RhoUnforg) -> eyre::Result<UnfInstance> {
    fn decode_hex(data: &str) -> eyre::Result<Vec<u8>> {
        hex::decode(data)
            .map_err(|e| eyre::eyre!("Invalid hex in unforgeable name '{}': {}", data, e))
    }
    Ok(match unforg {
        RhoUnforg::UnforgPrivate { data } => UnfInstance::GPrivateBody(GPrivate {
            id: decode_hex(&data)?.into(),
        }),
        RhoUnforg::UnforgDeploy { data } => UnfInstance::GDeployIdBody(GDeployId {
            sig: decode_hex(&data)?.into(),
        }),
        RhoUnforg::UnforgDeployer { data } => UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: decode_hex(&data)?.into(),
        }),
        RhoUnforg::UnforgAuthority { data } => UnfInstance::GAuthorityIdBody(GAuthorityId {
            id: decode_hex(&data)?.into(),
        }),
        RhoUnforg::UnforgPrincipal { key_family, data } => {
            UnfInstance::GPrincipalIdBody(GPrincipalId {
                key_family,
                public_key: decode_hex(&data)?.into(),
            })
        }
        RhoUnforg::UnforgSysAuthToken => {
            use models::rhoapi::GSysAuthToken;
            UnfInstance::GSysAuthTokenBody(GSysAuthToken {})
        }
    })
}

/// Convert DataAtNameRequest to Par. Returns error if hex decode fails.
fn to_par(rho_unforg: RhoUnforg) -> eyre::Result<Par> {
    Ok(Par {
        unforgeables: vec![GUnforgeable {
            unf_instance: Some(unforg_to_unforg_proto(rho_unforg)?),
        }],
        ..Default::default()
    })
}

/// Convert Par to RhoExpr - equivalent to Scala's exprFromParProto function
fn expr_from_par_proto(par: Par) -> Option<RhoExpr> {
    let has_process_fields = !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.connectives.is_empty();

    let exprs = par.exprs.into_iter().filter_map(expr_from_expr_proto);
    let unforg_exprs = par.unforgeables.into_iter().filter_map(unforg_from_proto);
    let bundle_exprs = par.bundles.into_iter().filter_map(expr_from_bundle_proto);

    let all_exprs: Vec<RhoExpr> = exprs.chain(unforg_exprs).chain(bundle_exprs).collect();

    if all_exprs.len() == 1 {
        all_exprs.into_iter().next()
    } else if all_exprs.is_empty() {
        if has_process_fields {
            // Par has process-level constructs (sends, receives, etc.) but no data expressions
            Some(RhoExpr::ExprUnknown {
                type_name: "Process".to_string(),
            })
        } else {
            None // Truly empty Par (Nil)
        }
    } else {
        Some(RhoExpr::ExprPar { data: all_exprs })
    }
}

/// Convert Expr to RhoExpr — handles all Rholang expression types.
fn expr_from_expr_proto(expr: Expr) -> Option<RhoExpr> {
    use models::rhoapi::expr::ExprInstance;
    use num_bigint::BigInt;

    let instance = expr.expr_instance?;
    Some(match instance {
        // Primitives
        ExprInstance::GBool(v) => RhoExpr::ExprBool { data: v },
        ExprInstance::GInt(v) => RhoExpr::ExprInt { data: v },
        ExprInstance::GString(v) => RhoExpr::ExprString { data: v },
        ExprInstance::GUri(v) => RhoExpr::ExprUri { data: v },
        ExprInstance::GByteArray(bytes) => RhoExpr::ExprBytes {
            data: hex::encode(&bytes),
        },

        // Extended numerics
        ExprInstance::GDouble(bits) => RhoExpr::ExprFloat {
            data: f64::from_bits(bits),
        },
        ExprInstance::GBigInt(bytes) => {
            let n = BigInt::from_signed_bytes_be(&bytes);
            RhoExpr::ExprBigInt {
                data: n.to_string(),
            }
        }
        ExprInstance::GBigRat(rat) => {
            let num = BigInt::from_signed_bytes_be(&rat.numerator);
            let den = BigInt::from_signed_bytes_be(&rat.denominator);
            RhoExpr::ExprBigRat {
                numerator: num.to_string(),
                denominator: den.to_string(),
            }
        }
        ExprInstance::GFixedPoint(fp) => {
            let unscaled = BigInt::from_signed_bytes_be(&fp.unscaled);
            RhoExpr::ExprFixedPoint {
                value: unscaled.to_string(),
                scale: fp.scale,
            }
        }

        // Collections
        ExprInstance::ETupleBody(tuple) => RhoExpr::ExprTuple {
            data: tuple
                .ps
                .into_iter()
                .filter_map(expr_from_par_proto)
                .collect(),
        },
        ExprInstance::EListBody(list) => RhoExpr::ExprList {
            data: list
                .ps
                .into_iter()
                .filter_map(expr_from_par_proto)
                .collect(),
        },
        ExprInstance::ESetBody(set) => RhoExpr::ExprSet {
            data: set.ps.into_iter().filter_map(expr_from_par_proto).collect(),
        },
        ExprInstance::EMapBody(map) => {
            let mut data = HashMap::new();
            for kv in map.kvs {
                if let (Some(key_par), Some(value_par)) = (kv.key, kv.value) {
                    if let (Some(key_expr), Some(value_expr)) =
                        (expr_from_par_proto(key_par), expr_from_par_proto(value_par))
                    {
                        let key = extract_key_from_expr(&key_expr);
                        data.insert(key, value_expr);
                    }
                }
            }
            RhoExpr::ExprMap { data }
        }
        ExprInstance::EPathmapBody(pm) => RhoExpr::ExprList {
            data: pm.ps.into_iter().filter_map(expr_from_par_proto).collect(),
        },
        ExprInstance::EZipperBody(z) => {
            let pathmap = z.pathmap.map(|pm| RhoExpr::ExprList {
                data: pm.ps.into_iter().filter_map(expr_from_par_proto).collect(),
            });
            RhoExpr::ExprTuple {
                data: vec![
                    pathmap.unwrap_or(RhoExpr::ExprList { data: vec![] }),
                    RhoExpr::ExprList {
                        data: z
                            .current_path
                            .into_iter()
                            .map(|b| RhoExpr::ExprBytes {
                                data: hex::encode(&b),
                            })
                            .collect(),
                    },
                ],
            }
        }

        // Unary operators
        ExprInstance::ENotBody(op) => RhoExpr::ExprNot {
            data: Box::new(par_to_expr(op.p)),
        },
        ExprInstance::ENegBody(op) => RhoExpr::ExprNeg {
            data: Box::new(par_to_expr(op.p)),
        },

        // Binary arithmetic
        ExprInstance::EPlusBody(op) => RhoExpr::ExprPlus {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::EMinusBody(op) => RhoExpr::ExprMinus {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::EMultBody(op) => RhoExpr::ExprMult {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::EDivBody(op) => RhoExpr::ExprDiv {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::EModBody(op) => RhoExpr::ExprMod {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },

        // Comparison
        ExprInstance::ELtBody(op) => RhoExpr::ExprLt {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::ELteBody(op) => RhoExpr::ExprLte {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::EGtBody(op) => RhoExpr::ExprGt {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::EGteBody(op) => RhoExpr::ExprGte {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::EEqBody(op) => RhoExpr::ExprEq {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::ENeqBody(op) => RhoExpr::ExprNeq {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },

        // Logical
        ExprInstance::EAndBody(op) => RhoExpr::ExprAnd {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::EOrBody(op) => RhoExpr::ExprOr {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },

        // String operations
        ExprInstance::EPlusPlusBody(op) => RhoExpr::ExprConcat {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::EPercentPercentBody(op) => RhoExpr::ExprInterpolate {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },
        ExprInstance::EMinusMinusBody(op) => RhoExpr::ExprDiff {
            left: Box::new(par_to_expr(op.p1)),
            right: Box::new(par_to_expr(op.p2)),
        },

        // Pattern matching
        ExprInstance::EMatchesBody(op) => RhoExpr::ExprMatches {
            target: Box::new(par_to_expr(op.target)),
            pattern: Box::new(par_to_expr(op.pattern)),
        },

        // Method call
        ExprInstance::EMethodBody(method) => RhoExpr::ExprMethod {
            target: Box::new(par_to_expr(method.target)),
            name: method.method_name,
            args: method
                .arguments
                .into_iter()
                .filter_map(expr_from_par_proto)
                .collect(),
        },

        // Variable
        ExprInstance::EVarBody(var) => {
            let index = var
                .v
                .and_then(|v| v.var_instance)
                .map(|vi| match vi {
                    models::rhoapi::var::VarInstance::BoundVar(i) => i,
                    models::rhoapi::var::VarInstance::FreeVar(i) => i,
                    models::rhoapi::var::VarInstance::Wildcard(_) => -1,
                })
                .unwrap_or(-1);
            RhoExpr::ExprVar { index }
        }
    })
}

/// Convert an optional Par to RhoExpr, falling back to ExprUnknown for None.
fn par_to_expr(par: Option<Par>) -> RhoExpr {
    par.and_then(expr_from_par_proto)
        .unwrap_or(RhoExpr::ExprUnknown {
            type_name: "Nil".to_string(),
        })
}

/// Convert GUnforgeable to RhoExpr.
fn unforg_from_proto(unforg: GUnforgeable) -> Option<RhoExpr> {
    use models::rhoapi::g_unforgeable::UnfInstance;

    Some(match unforg.unf_instance? {
        UnfInstance::GPrivateBody(private) => RhoExpr::ExprUnforg {
            data: RhoUnforg::UnforgPrivate {
                data: hex::encode(&private.id),
            },
        },
        UnfInstance::GDeployIdBody(deploy_id) => RhoExpr::ExprUnforg {
            data: RhoUnforg::UnforgDeploy {
                data: hex::encode(&deploy_id.sig),
            },
        },
        UnfInstance::GDeployerIdBody(deployer_id) => RhoExpr::ExprUnforg {
            data: RhoUnforg::UnforgDeployer {
                data: hex::encode(&deployer_id.public_key),
            },
        },
        UnfInstance::GAuthorityIdBody(authority_id) => RhoExpr::ExprUnforg {
            data: RhoUnforg::UnforgAuthority {
                data: hex::encode(&authority_id.id),
            },
        },
        UnfInstance::GPrincipalIdBody(principal_id) => RhoExpr::ExprUnforg {
            data: RhoUnforg::UnforgPrincipal {
                key_family: principal_id.key_family,
                data: hex::encode(&principal_id.public_key),
            },
        },
        UnfInstance::GSysAuthTokenBody(_) => RhoExpr::ExprUnforg {
            data: RhoUnforg::UnforgSysAuthToken,
        },
    })
}

/// Convert Bundle to RhoExpr, preserving read/write permissions.
fn expr_from_bundle_proto(bundle: Bundle) -> Option<RhoExpr> {
    let body_expr = bundle
        .body
        .and_then(expr_from_par_proto)
        .unwrap_or(RhoExpr::ExprUnknown {
            type_name: "Nil".to_string(),
        });
    Some(RhoExpr::ExprBundle {
        data: Box::new(body_expr),
        read: bundle.read_flag,
        write: bundle.write_flag,
    })
}

/// Extract a string key from a RhoExpr for map keys.
/// Primitive types use natural string representation; complex types use JSON serialization.
fn extract_key_from_expr(expr: &RhoExpr) -> String {
    match expr {
        RhoExpr::ExprString { data } => data.clone(),
        RhoExpr::ExprInt { data } => data.to_string(),
        RhoExpr::ExprBool { data } => data.to_string(),
        RhoExpr::ExprFloat { data } => data.to_string(),
        RhoExpr::ExprBigInt { data } => data.clone(),
        RhoExpr::ExprUri { data } => data.clone(),
        RhoExpr::ExprBytes { data } => data.clone(),
        RhoExpr::ExprUnforg { data } => match data {
            RhoUnforg::UnforgPrivate { data } => data.clone(),
            RhoUnforg::UnforgDeploy { data } => data.clone(),
            RhoUnforg::UnforgDeployer { data } => data.clone(),
            RhoUnforg::UnforgAuthority { data } => data.clone(),
            RhoUnforg::UnforgPrincipal { key_family, data } => {
                format!("{}:{}", key_family, data)
            }
            RhoUnforg::UnforgSysAuthToken => "SysAuthToken".to_string(),
        },
        // Complex types: serialize to JSON string
        other => serde_json::to_string(other).unwrap_or_else(|_| format!("{:?}", other)),
    }
}

/// Convert (Vec<Par>, LightBlockInfo) to RhoDataResponse
/// Equivalent to Scala's toRhoDataResponse function
fn to_rho_data_response(
    pars: Vec<Par>,
    light_block_info: LightBlockInfo,
    cost: u64,
) -> RhoDataResponse {
    let rho_exprs: Vec<RhoExpr> = pars.into_iter().filter_map(expr_from_par_proto).collect();
    let block = LightBlockInfoSerde::from(light_block_info);

    RhoDataResponse {
        expr: rho_exprs,
        block,
        cost,
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{
        Bundle, EList, EMap, ESet, ETuple, GDeployId, GDeployerId, GPrivate, KeyValuePair,
    };

    use super::*;

    #[test]
    fn prepare_deploy_get_keeps_the_empty_name_list() {
        assert!(prepare_deploy_names(None).unwrap().is_empty());
    }

    #[test]
    fn prepare_deploy_post_fails_closed_for_protocol_v6() {
        let request = PrepareRequest {
            deployer: hex::encode([2; 33]),
            timestamp: 1,
            name_qty: 1,
        };
        let error = prepare_deploy_names(Some(&request)).expect_err("preview must fail closed");

        assert!(error.to_string().contains("authenticated deploy envelope"));
    }

    #[test]
    fn test_deploy_response_full_view_includes_all_fields() {
        let response = DeployResponse {
            deploy_id: "abc123".to_string(),
            block_hash: "hash1".to_string(),
            block_number: 100,
            timestamp: 1700000000000,
            cost: 500,
            errored: false,
            is_finalized: true,
            finalization_state: "Finalized".to_string(),
            rejection_count: 0,
            deployer: Some("deployer1".to_string()),
            term: Some("new ret in { ret!(42) }".to_string()),
            system_deploy_error: Some(String::new()),
            sig_algorithm: Some("secp256k1".to_string()),
            valid_after_block_number: Some(0),
            transfers: Some(vec![]),
        };

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["deployId"], "abc123");
        assert_eq!(json["blockHash"], "hash1");
        assert_eq!(json["blockNumber"], 100);
        assert_eq!(json["cost"], 500);
        assert_eq!(json["isFinalized"], true);
        assert_eq!(json["finalizationState"], "Finalized");
        assert_eq!(json["rejectionCount"], 0);
        assert!(json.get("deployer").is_some());
        assert!(json.get("term").is_some());
        // D3 (DR-9): no phloPrice / phloLimit in the response.
        assert!(json.get("phloPrice").is_none());
        assert!(json.get("phloLimit").is_none());
        assert!(json.get("transfers").is_some());
    }

    #[test]
    fn test_deploy_response_summary_view_omits_optional_fields() {
        let response = DeployResponse {
            deploy_id: "abc123".to_string(),
            block_hash: "hash1".to_string(),
            block_number: 100,
            timestamp: 1700000000000,
            cost: 500,
            errored: false,
            is_finalized: true,
            finalization_state: "Pending".to_string(),
            rejection_count: 2,
            deployer: None,
            term: None,
            system_deploy_error: None,
            sig_algorithm: None,
            valid_after_block_number: None,
            transfers: None,
        };

        let json = serde_json::to_value(&response).unwrap();

        // Core fields present
        assert_eq!(json["deployId"], "abc123");
        assert_eq!(json["blockHash"], "hash1");
        assert_eq!(json["cost"], 500);
        assert_eq!(json["isFinalized"], true);
        assert_eq!(json["finalizationState"], "Pending");
        assert_eq!(json["rejectionCount"], 2);

        // Optional fields omitted
        assert!(json.get("deployer").is_none());
        assert!(json.get("term").is_none());
        assert!(json.get("phloPrice").is_none());
        assert!(json.get("phloLimit").is_none());
        assert!(json.get("sigAlgorithm").is_none());
        assert!(json.get("validAfterBlockNumber").is_none());
        assert!(json.get("transfers").is_none());
    }

    #[test]
    fn test_deploy_request_serialization() {
        let request = DeployRequest {
            data: DeployData {
                term: "contract".to_string(),
                language: "rholang".to_string(),
                time_stamp: 1234567890,
                valid_after_block_number: 0,
                shard_id: "".to_string(),
                expiration_timestamp: None,
                authority_presentations: Vec::new(),
            },
            deployer: "0123456789abcdef".to_string(),
            signature: "fedcba9876543210".to_string(),
            sig_algorithm: "secp256k1".to_string(),
            cosigners: Vec::new(),
            threshold: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: DeployRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(request.deployer, deserialized.deployer);
        assert_eq!(request.signature, deserialized.signature);
        assert_eq!(request.sig_algorithm, deserialized.sig_algorithm);
        assert_eq!(request.threshold, deserialized.threshold);
    }

    #[test]
    fn rest_deploy_request_consumes_protocol_v61_threshold_signatures() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../test-vectors/deploy-envelope-v6.1.json"
        ))
        .expect("v6.1 vectors");
        let vector = &vectors["positive"]["threshold2Of3Selected0And2"];
        let members = vector["members"].as_array().expect("members");
        let request = DeployRequest {
            data: DeployData {
                term: vector["term"].as_str().expect("term").to_string(),
                language: vector["language"].as_str().expect("language").to_string(),
                time_stamp: vector["timestamp"].as_i64().expect("timestamp"),
                valid_after_block_number: vector["validAfterBlockNumber"]
                    .as_i64()
                    .expect("valid after"),
                shard_id: vector["shardId"].as_str().expect("shard").to_string(),
                expiration_timestamp: Some(
                    vector["expirationTimestamp"].as_i64().expect("expiration"),
                ),
                authority_presentations: Vec::new(),
            },
            deployer: members[0]["publicKeyHex"]
                .as_str()
                .expect("public key")
                .to_string(),
            signature: members[0]["signatureHex"]
                .as_str()
                .expect("signature")
                .to_string(),
            sig_algorithm: "secp256k1".to_string(),
            cosigners: members[1..]
                .iter()
                .map(|member| CosignerJson {
                    pk: member["publicKeyHex"]
                        .as_str()
                        .expect("public key")
                        .to_string(),
                    signature: member["signatureHex"]
                        .as_str()
                        .expect("signature")
                        .to_string(),
                    sig_algorithm: "secp256k1".to_string(),
                })
                .collect(),
            threshold: Some(2),
        };
        let envelope = to_cosigned_deploy(&request).expect("v6.1 REST envelope");
        assert!(envelope.is_envelope_bound());
        assert_eq!(envelope.cosigner_threshold(), 2);
        assert_eq!(envelope.selected_signers_v61().expect("selected").len(), 2);
        assert_eq!(
            hex::encode(envelope.envelope_commitment().expect("deploy id")),
            vector["deployIdHex"].as_str().expect("deploy id")
        );

        let mut insufficient = request;
        insufficient.threshold = Some(3);
        assert!(to_cosigned_deploy(&insufficient).is_err());
    }

    #[test]
    fn test_rho_expr_serialization() {
        let expr = RhoExpr::ExprBool { data: true };
        let json = serde_json::to_string(&expr).unwrap();
        let deserialized: RhoExpr = serde_json::from_str(&json).unwrap();

        match deserialized {
            RhoExpr::ExprBool { data } => assert!(data),
            _ => panic!("Expected ExprBool"),
        }
    }

    #[test]
    fn test_expr_from_par_proto_empty() {
        let par = Par::default();
        let result = expr_from_par_proto(par);
        assert!(result.is_none());
    }

    #[test]
    fn test_expr_from_par_proto_single_bool() {
        let par = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GBool(true)),
            }],
            ..Default::default()
        };
        let result = expr_from_par_proto(par);
        assert!(matches!(result, Some(RhoExpr::ExprBool { data: true })));
    }

    #[test]
    fn test_expr_from_par_proto_multiple_exprs() {
        let par = Par {
            exprs: vec![
                Expr {
                    expr_instance: Some(ExprInstance::GBool(true)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GInt(42)),
                },
            ],
            ..Default::default()
        };
        let result = expr_from_par_proto(par);
        match result {
            Some(RhoExpr::ExprPar { data }) => {
                assert_eq!(data.len(), 2);
                assert!(matches!(data[0], RhoExpr::ExprBool { data: true }));
                assert!(matches!(data[1], RhoExpr::ExprInt { data: 42 }));
            }
            _ => panic!("Expected ExprPar with 2 elements"),
        }
    }

    #[test]
    fn test_expr_from_expr_proto_primitive_types() {
        // Test GBool
        let expr = Expr {
            expr_instance: Some(ExprInstance::GBool(true)),
        };
        let result = expr_from_expr_proto(expr);
        assert!(matches!(result, Some(RhoExpr::ExprBool { data: true })));

        // Test GInt
        let expr = Expr {
            expr_instance: Some(ExprInstance::GInt(42)),
        };
        let result = expr_from_expr_proto(expr);
        assert!(matches!(result, Some(RhoExpr::ExprInt { data: 42 })));

        // Test GString
        let expr = Expr {
            expr_instance: Some(ExprInstance::GString("hello".to_string())),
        };
        let result = expr_from_expr_proto(expr);
        assert!(matches!(result, Some(RhoExpr::ExprString { data }) if data == "hello"));

        // Test GUri
        let expr = Expr {
            expr_instance: Some(ExprInstance::GUri("rho:io:stdout".to_string())),
        };
        let result = expr_from_expr_proto(expr);
        assert!(matches!(result, Some(RhoExpr::ExprUri { data }) if data == "rho:io:stdout"));

        // Test GByteArray
        let expr = Expr {
            expr_instance: Some(ExprInstance::GByteArray(vec![0x01, 0x02, 0x03])),
        };
        let result = expr_from_expr_proto(expr);
        assert!(matches!(result, Some(RhoExpr::ExprBytes { data }) if data == "010203"));
    }

    #[test]
    fn test_expr_from_expr_proto_tuple() {
        let tuple = ETuple {
            ps: vec![
                Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GInt(1)),
                    }],
                    ..Default::default()
                },
                Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GString("hello".to_string())),
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let expr = Expr {
            expr_instance: Some(ExprInstance::ETupleBody(tuple)),
        };
        let result = expr_from_expr_proto(expr);
        match result {
            Some(RhoExpr::ExprTuple { data }) => {
                assert_eq!(data.len(), 2);
                assert!(matches!(data[0], RhoExpr::ExprInt { data: 1 }));
                assert!(matches!(data[1], RhoExpr::ExprString { data: ref d } if d == "hello"));
            }
            _ => panic!("Expected ExprTuple"),
        }
    }

    #[test]
    fn test_expr_from_expr_proto_list() {
        let list = EList {
            ps: vec![
                Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GInt(1)),
                    }],
                    ..Default::default()
                },
                Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GInt(2)),
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let expr = Expr {
            expr_instance: Some(ExprInstance::EListBody(list)),
        };
        let result = expr_from_expr_proto(expr);
        match result {
            Some(RhoExpr::ExprList { data }) => {
                assert_eq!(data.len(), 2);
                assert!(matches!(data[0], RhoExpr::ExprInt { data: 1 }));
                assert!(matches!(data[1], RhoExpr::ExprInt { data: 2 }));
            }
            _ => panic!("Expected ExprList"),
        }
    }

    #[test]
    fn test_expr_from_expr_proto_set() {
        let set = ESet {
            ps: vec![
                Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GString("a".to_string())),
                    }],
                    ..Default::default()
                },
                Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GString("b".to_string())),
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let expr = Expr {
            expr_instance: Some(ExprInstance::ESetBody(set)),
        };
        let result = expr_from_expr_proto(expr);
        match result {
            Some(RhoExpr::ExprSet { data }) => {
                assert_eq!(data.len(), 2);
                assert!(matches!(data[0], RhoExpr::ExprString { data: ref d } if d == "a"));
                assert!(matches!(data[1], RhoExpr::ExprString { data: ref d } if d == "b"));
            }
            _ => panic!("Expected ExprSet"),
        }
    }

    #[test]
    fn test_expr_from_expr_proto_map() {
        let map = EMap {
            kvs: vec![
                KeyValuePair {
                    key: Some(Par {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::GString("key1".to_string())),
                        }],
                        ..Default::default()
                    }),
                    value: Some(Par {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::GInt(42)),
                        }],
                        ..Default::default()
                    }),
                },
                KeyValuePair {
                    key: Some(Par {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::GString("key2".to_string())),
                        }],
                        ..Default::default()
                    }),
                    value: Some(Par {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::GString("value2".to_string())),
                        }],
                        ..Default::default()
                    }),
                },
            ],
            ..Default::default()
        };

        let expr = Expr {
            expr_instance: Some(ExprInstance::EMapBody(map)),
        };
        let result = expr_from_expr_proto(expr);
        match result {
            Some(RhoExpr::ExprMap { data }) => {
                assert_eq!(data.len(), 2);
                assert!(data.contains_key("key1"));
                assert!(data.contains_key("key2"));
                assert!(matches!(data["key1"], RhoExpr::ExprInt { data: 42 }));
                assert!(
                    matches!(data["key2"], RhoExpr::ExprString { data: ref d } if d == "value2")
                );
            }
            _ => panic!("Expected ExprMap"),
        }
    }

    #[test]
    fn test_unforg_from_proto_private() {
        let unforg = GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: vec![0x01, 0x02, 0x03],
            })),
        };
        let result = unforg_from_proto(unforg);
        match result {
            Some(RhoExpr::ExprUnforg { data }) => {
                assert!(matches!(data, RhoUnforg::UnforgPrivate { data: ref d } if d == "010203"));
            }
            _ => panic!("Expected ExprUnforg with UnforgPrivate"),
        }
    }

    #[test]
    fn test_unforg_from_proto_deploy() {
        let unforg = GUnforgeable {
            unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
                sig: vec![0x04, 0x05, 0x06],
            })),
        };
        let result = unforg_from_proto(unforg);
        match result {
            Some(RhoExpr::ExprUnforg { data }) => {
                assert!(matches!(data, RhoUnforg::UnforgDeploy { data: ref d } if d == "040506"));
            }
            _ => panic!("Expected ExprUnforg with UnforgDeploy"),
        }
    }

    #[test]
    fn test_unforg_from_proto_deployer() {
        let unforg = GUnforgeable {
            unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
                public_key: vec![0x07, 0x08, 0x09],
            })),
        };
        let result = unforg_from_proto(unforg);
        match result {
            Some(RhoExpr::ExprUnforg { data }) => {
                assert!(matches!(data, RhoUnforg::UnforgDeployer { data: ref d } if d == "070809"));
            }
            _ => panic!("Expected ExprUnforg with UnforgDeployer"),
        }
    }

    #[test]
    fn test_unforg_from_proto_authority() {
        let unforg = GUnforgeable {
            unf_instance: Some(UnfInstance::GAuthorityIdBody(GAuthorityId {
                id: vec![0x0a, 0x0b, 0x0c],
            })),
        };
        let result = unforg_from_proto(unforg);
        assert!(matches!(
            result,
            Some(RhoExpr::ExprUnforg {
                data: RhoUnforg::UnforgAuthority { ref data }
            }) if data == "0a0b0c"
        ));
    }

    #[test]
    fn test_unforg_from_proto_principal() {
        let unforg = GUnforgeable {
            unf_instance: Some(UnfInstance::GPrincipalIdBody(GPrincipalId {
                key_family: 1,
                public_key: vec![0x0d, 0x0e, 0x0f],
            })),
        };
        let result = unforg_from_proto(unforg);
        assert!(matches!(
            result,
            Some(RhoExpr::ExprUnforg {
                data: RhoUnforg::UnforgPrincipal {
                    key_family: 1,
                    ref data
                }
            }) if data == "0d0e0f"
        ));
    }

    #[test]
    fn test_expr_from_bundle_proto() {
        let bundle = Bundle {
            body: Some(Par {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GString("bundle_content".to_string())),
                }],
                ..Default::default()
            }),
            write_flag: true,
            read_flag: false,
        };
        let result = expr_from_bundle_proto(bundle);
        assert!(matches!(
            result,
            Some(RhoExpr::ExprBundle { ref data, write: true, read: false })
            if matches!(data.as_ref(), RhoExpr::ExprString { data } if data == "bundle_content")
        ));
    }

    #[test]
    fn test_expr_from_bundle_proto_empty() {
        let bundle = Bundle {
            body: None,
            write_flag: false,
            read_flag: true,
        };
        let result = expr_from_bundle_proto(bundle);
        // Empty body bundle returns ExprBundle with ExprUnknown body
        assert!(matches!(
            result,
            Some(RhoExpr::ExprBundle {
                read: true,
                write: false,
                ..
            })
        ));
    }

    #[test]
    fn test_extract_key_from_expr() {
        // Test string key
        let expr = RhoExpr::ExprString {
            data: "hello".to_string(),
        };
        assert_eq!(extract_key_from_expr(&expr), "hello");

        // Test int key
        let expr = RhoExpr::ExprInt { data: 42 };
        assert_eq!(extract_key_from_expr(&expr), "42");

        // Test bool key
        let expr = RhoExpr::ExprBool { data: true };
        assert_eq!(extract_key_from_expr(&expr), "true");

        // Test URI key
        let expr = RhoExpr::ExprUri {
            data: "rho:io:stdout".to_string(),
        };
        assert_eq!(extract_key_from_expr(&expr), "rho:io:stdout");

        // Test bytes key
        let expr = RhoExpr::ExprBytes {
            data: "010203".to_string(),
        };
        assert_eq!(extract_key_from_expr(&expr), "010203");

        // Test unforgeable keys
        let expr = RhoExpr::ExprUnforg {
            data: RhoUnforg::UnforgPrivate {
                data: "private".to_string(),
            },
        };
        assert_eq!(extract_key_from_expr(&expr), "private");

        // Test complex key type — serialized to JSON
        let expr = RhoExpr::ExprPar { data: vec![] };
        let key = extract_key_from_expr(&expr);
        assert!(
            !key.is_empty(),
            "complex keys should serialize to non-empty string"
        );
    }

    fn int_par(value: i64) -> Par {
        Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }],
            ..Default::default()
        }
    }

    fn as_json(expr: &RhoExpr) -> serde_json::Value { serde_json::to_value(expr).unwrap() }

    #[test]
    fn test_expr_from_expr_proto_extended_numerics() {
        let double = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::GDouble(2.5f64.to_bits())),
        });
        assert!(matches!(double, Some(RhoExpr::ExprFloat { data }) if data == 2.5));

        let big_int = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::GBigInt(vec![0x01, 0x00])),
        });
        assert!(matches!(big_int, Some(RhoExpr::ExprBigInt { data }) if data == "256"));

        let big_rat = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::GBigRat(models::rhoapi::GBigRational {
                numerator: vec![0x03],
                denominator: vec![0x07],
            })),
        });
        assert!(matches!(
            big_rat,
            Some(RhoExpr::ExprBigRat { numerator, denominator })
            if numerator == "3" && denominator == "7"
        ));

        let fixed_point = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::GFixedPoint(models::rhoapi::GFixedPoint {
                unscaled: vec![0x7b],
                scale: 2,
            })),
        });
        assert!(matches!(
            fixed_point,
            Some(RhoExpr::ExprFixedPoint { value, scale: 2 }) if value == "123"
        ));
    }

    #[test]
    fn test_expr_from_expr_proto_unary_operators() {
        use models::rhoapi::{ENeg, ENot};

        let not = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::ENotBody(ENot {
                p: Some(int_par(1)),
            })),
        })
        .unwrap();
        assert_eq!(
            as_json(&not),
            serde_json::json!({"ExprNot": {"data": {"ExprInt": {"data": 1}}}})
        );

        let neg = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::ENegBody(ENeg {
                p: Some(int_par(2)),
            })),
        })
        .unwrap();
        assert!(matches!(neg, RhoExpr::ExprNeg { .. }));
    }

    #[test]
    fn test_expr_from_expr_proto_binary_operators() {
        use models::rhoapi::{
            EAnd, EDiv, EEq, EGt, EGte, ELt, ELte, EMinus, EMinusMinus, EMod, EMult, ENeq, EOr,
            EPercentPercent, EPlus, EPlusPlus,
        };

        let plus = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                p1: Some(int_par(1)),
                p2: Some(int_par(2)),
            })),
        })
        .unwrap();
        assert_eq!(
            as_json(&plus),
            serde_json::json!({"ExprPlus": {
                "left": {"ExprInt": {"data": 1}},
                "right": {"ExprInt": {"data": 2}},
            }})
        );

        let cases = vec![
            (
                ExprInstance::EMinusBody(EMinus {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprMinus",
            ),
            (
                ExprInstance::EMultBody(EMult {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprMult",
            ),
            (
                ExprInstance::EDivBody(EDiv {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprDiv",
            ),
            (
                ExprInstance::EModBody(EMod {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprMod",
            ),
            (
                ExprInstance::ELtBody(ELt {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprLt",
            ),
            (
                ExprInstance::ELteBody(ELte {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprLte",
            ),
            (
                ExprInstance::EGtBody(EGt {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprGt",
            ),
            (
                ExprInstance::EGteBody(EGte {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprGte",
            ),
            (
                ExprInstance::EEqBody(EEq {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprEq",
            ),
            (
                ExprInstance::ENeqBody(ENeq {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprNeq",
            ),
            (
                ExprInstance::EAndBody(EAnd {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprAnd",
            ),
            (
                ExprInstance::EOrBody(EOr {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprOr",
            ),
            (
                ExprInstance::EPlusPlusBody(EPlusPlus {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprConcat",
            ),
            (
                ExprInstance::EPercentPercentBody(EPercentPercent {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprInterpolate",
            ),
            (
                ExprInstance::EMinusMinusBody(EMinusMinus {
                    p1: Some(int_par(1)),
                    p2: Some(int_par(2)),
                }),
                "ExprDiff",
            ),
        ];

        for (instance, expected_variant) in cases {
            let result = expr_from_expr_proto(Expr {
                expr_instance: Some(instance),
            })
            .unwrap();
            let json = as_json(&result);
            assert!(
                json.get(expected_variant).is_some(),
                "expected {expected_variant}, got {json}"
            );
            assert_eq!(json[expected_variant]["left"]["ExprInt"]["data"], 1);
            assert_eq!(json[expected_variant]["right"]["ExprInt"]["data"], 2);
        }
    }

    #[test]
    fn test_expr_from_expr_proto_matches_and_method() {
        use models::rhoapi::{EMatches, EMethod};

        let matches_expr = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::EMatchesBody(EMatches {
                target: Some(int_par(1)),
                pattern: Some(int_par(2)),
            })),
        })
        .unwrap();
        let json = as_json(&matches_expr);
        assert_eq!(json["ExprMatches"]["target"]["ExprInt"]["data"], 1);
        assert_eq!(json["ExprMatches"]["pattern"]["ExprInt"]["data"], 2);

        let method_expr = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_string(),
                target: Some(int_par(1)),
                arguments: vec![int_par(2), int_par(3)],
                ..Default::default()
            })),
        })
        .unwrap();
        match method_expr {
            RhoExpr::ExprMethod { name, args, target } => {
                assert_eq!(name, "nth");
                assert_eq!(args.len(), 2);
                assert!(matches!(*target, RhoExpr::ExprInt { data: 1 }));
            }
            other => panic!("expected ExprMethod, got {:?}", other),
        }
    }

    #[test]
    fn test_expr_from_expr_proto_var_instances() {
        use models::rhoapi::var::VarInstance;
        use models::rhoapi::{EVar, Var};

        let var_of = |instance: Option<VarInstance>| {
            expr_from_expr_proto(Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: instance.map(|var_instance| Var {
                        var_instance: Some(var_instance),
                    }),
                })),
            })
            .unwrap()
        };

        assert!(matches!(
            var_of(Some(VarInstance::BoundVar(3))),
            RhoExpr::ExprVar { index: 3 }
        ));
        assert!(matches!(
            var_of(Some(VarInstance::FreeVar(2))),
            RhoExpr::ExprVar { index: 2 }
        ));
        assert!(matches!(
            var_of(Some(VarInstance::Wildcard(
                models::rhoapi::var::WildcardMsg {}
            ))),
            RhoExpr::ExprVar { index: -1 }
        ));
        assert!(matches!(var_of(None), RhoExpr::ExprVar { index: -1 }));
    }

    #[test]
    fn test_expr_from_expr_proto_pathmap_and_zipper() {
        use models::rhoapi::{EPathMap, EZipper};

        let pathmap = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap {
                ps: vec![int_par(1), int_par(2)],
                ..Default::default()
            })),
        })
        .unwrap();
        assert!(matches!(&pathmap, RhoExpr::ExprList { data } if data.len() == 2));

        let zipper = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap {
                    ps: vec![int_par(1)],
                    ..Default::default()
                }),
                current_path: vec![vec![0xaa]],
                ..Default::default()
            })),
        })
        .unwrap();
        match zipper {
            RhoExpr::ExprTuple { data } => {
                assert_eq!(data.len(), 2);
                assert!(matches!(&data[0], RhoExpr::ExprList { data } if data.len() == 1));
                assert!(matches!(&data[1], RhoExpr::ExprList { data }
                        if matches!(&data[0], RhoExpr::ExprBytes { data } if data == "aa")));
            }
            other => panic!("expected ExprTuple, got {:?}", other),
        }

        let empty_zipper = expr_from_expr_proto(Expr {
            expr_instance: Some(ExprInstance::EZipperBody(EZipper::default())),
        })
        .unwrap();
        assert!(matches!(
            &empty_zipper,
            RhoExpr::ExprTuple { data }
            if matches!(&data[0], RhoExpr::ExprList { data } if data.is_empty())
        ));
    }

    #[test]
    fn test_expr_from_par_proto_process_par_is_unknown_process() {
        let par = Par {
            sends: vec![models::rhoapi::Send::default()],
            ..Default::default()
        };
        let result = expr_from_par_proto(par);
        assert!(matches!(
            result,
            Some(RhoExpr::ExprUnknown { type_name }) if type_name == "Process"
        ));
    }

    #[test]
    fn test_par_to_expr_none_falls_back_to_nil_unknown() {
        let result = par_to_expr(None);
        assert!(matches!(result, RhoExpr::ExprUnknown { type_name } if type_name == "Nil"));
    }

    #[test]
    fn test_unforg_from_proto_sys_auth_token() {
        let unforg = GUnforgeable {
            unf_instance: Some(UnfInstance::GSysAuthTokenBody(
                models::rhoapi::GSysAuthToken {},
            )),
        };
        let result = unforg_from_proto(unforg);
        assert!(matches!(
            result,
            Some(RhoExpr::ExprUnforg {
                data: RhoUnforg::UnforgSysAuthToken
            })
        ));
    }

    #[test]
    fn test_to_par_builds_unforgeable_for_each_variant() {
        let private = to_par(RhoUnforg::UnforgPrivate {
            data: "0102".to_string(),
        })
        .unwrap();
        assert!(matches!(
            private.unforgeables[0].unf_instance.as_ref().unwrap(),
            UnfInstance::GPrivateBody(body) if body.id == vec![1u8, 2]
        ));

        let deploy = to_par(RhoUnforg::UnforgDeploy {
            data: "0304".to_string(),
        })
        .unwrap();
        assert!(matches!(
            deploy.unforgeables[0].unf_instance.as_ref().unwrap(),
            UnfInstance::GDeployIdBody(body) if body.sig == vec![3u8, 4]
        ));

        let deployer = to_par(RhoUnforg::UnforgDeployer {
            data: "0506".to_string(),
        })
        .unwrap();
        assert!(matches!(
            deployer.unforgeables[0].unf_instance.as_ref().unwrap(),
            UnfInstance::GDeployerIdBody(body) if body.public_key == vec![5u8, 6]
        ));

        let token = to_par(RhoUnforg::UnforgSysAuthToken).unwrap();
        assert!(matches!(
            token.unforgeables[0].unf_instance.as_ref().unwrap(),
            UnfInstance::GSysAuthTokenBody(_)
        ));
    }

    #[test]
    fn test_to_par_rejects_invalid_hex() {
        let result = to_par(RhoUnforg::UnforgPrivate {
            data: "not-hex".to_string(),
        });
        assert!(result.unwrap_err().to_string().contains("Invalid hex"));
    }

    #[test]
    fn test_validate_and_decode_pubkey() {
        let bad_hex = validate_and_decode_pubkey("zz");
        assert!(bad_hex
            .unwrap_err()
            .to_string()
            .contains("invalid public key hex"));

        let bad_key = validate_and_decode_pubkey("0102");
        assert!(bad_key.is_err());

        let (_sk, pk) = crypto::rust::signatures::secp256k1::Secp256k1.new_key_pair();
        let valid = validate_and_decode_pubkey(&hex::encode(&pk.bytes)).unwrap();
        assert_eq!(valid, pk.bytes.to_vec());
    }

    fn sample_deploy_data() -> DeployData {
        DeployData {
            term: "new x in { x!(1) }".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        }
    }

    #[test]
    fn test_to_cosigned_deploy_accepts_a_correctly_signed_request() {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signed::Cosigned;

        let (sk, _pk) = Secp256k1.new_key_pair();
        let envelope =
            Cosigned::create_single_envelope(sample_deploy_data(), Box::new(Secp256k1), sk)
                .unwrap();
        let signer = &envelope.signers()[0];

        let request = DeployRequest {
            data: envelope.data().clone(),
            deployer: hex::encode(&signer.pk.bytes),
            signature: hex::encode(&signer.sig),
            sig_algorithm: "secp256k1".to_string(),
            cosigners: Vec::new(),
            threshold: Some(1),
        };

        let result = to_cosigned_deploy(&request).unwrap();
        assert_eq!(result.signers()[0].sig, signer.sig);
        assert_eq!(result.signers()[0].pk.bytes, signer.pk.bytes);
        assert_eq!(
            result.envelope_commitment().unwrap(),
            envelope.envelope_commitment().unwrap()
        );
    }

    #[test]
    fn test_to_cosigned_deploy_error_paths() {
        let base = DeployRequest {
            data: sample_deploy_data(),
            deployer: "zz".to_string(),
            signature: "00".to_string(),
            sig_algorithm: "secp256k1".to_string(),
            cosigners: Vec::new(),
            threshold: Some(1),
        };
        assert!(to_cosigned_deploy(&base)
            .unwrap_err()
            .to_string()
            .contains("Primary public key is not valid base16"));

        let bad_sig_hex = DeployRequest {
            deployer: "00".to_string(),
            signature: "zz".to_string(),
            ..base.clone()
        };
        assert!(to_cosigned_deploy(&bad_sig_hex)
            .unwrap_err()
            .to_string()
            .contains("Primary signature is not valid base16"));

        let unsupported_alg = DeployRequest {
            deployer: "00".to_string(),
            signature: "00".to_string(),
            sig_algorithm: "rot13".to_string(),
            ..base.clone()
        };
        assert!(to_cosigned_deploy(&unsupported_alg)
            .unwrap_err()
            .to_string()
            .contains("Signature algorithm not supported"));

        let (_sk, pk) = crypto::rust::signatures::secp256k1::Secp256k1.new_key_pair();
        let wrong_sig = DeployRequest {
            deployer: hex::encode(&pk.bytes),
            signature: "0011".to_string(),
            ..base
        };
        assert!(to_cosigned_deploy(&wrong_sig).is_err());
    }

    #[test]
    fn test_web_api_error_display() {
        assert_eq!(
            WebApiError::BlockApiError("a".to_string()).to_string(),
            "Block API error: a"
        );
        assert_eq!(
            WebApiError::SignatureError("b".to_string()).to_string(),
            "Signature error: b"
        );
        assert_eq!(
            WebApiError::InvalidFormat("c".to_string()).to_string(),
            "Invalid format: c"
        );
    }

    #[test]
    fn test_deploy_state_json_labels() {
        use casper::rust::api::deploy_finalization_status::DeployFinalizationState as S;
        assert_eq!(deploy_state_json_label(S::Finalized), "Finalized");
        assert_eq!(deploy_state_json_label(S::Failed), "Failed");
        assert_eq!(deploy_state_json_label(S::Pending), "Pending");
        assert_eq!(deploy_state_json_label(S::Expired), "Expired");
    }

    #[test]
    fn test_to_rho_data_response_converts_pars_and_keeps_cost() {
        let response = to_rho_data_response(vec![int_par(9)], LightBlockInfo::default(), 42);
        assert_eq!(response.cost, 42);
        assert_eq!(response.expr.len(), 1);
        assert!(matches!(response.expr[0], RhoExpr::ExprInt { data: 9 }));
    }

    #[test]
    fn test_extract_key_from_expr_remaining_variants() {
        assert_eq!(
            extract_key_from_expr(&RhoExpr::ExprFloat { data: 1.5 }),
            "1.5"
        );
        assert_eq!(
            extract_key_from_expr(&RhoExpr::ExprBigInt {
                data: "12345".to_string()
            }),
            "12345"
        );
        assert_eq!(
            extract_key_from_expr(&RhoExpr::ExprUnforg {
                data: RhoUnforg::UnforgDeploy {
                    data: "dd".to_string()
                }
            }),
            "dd"
        );
        assert_eq!(
            extract_key_from_expr(&RhoExpr::ExprUnforg {
                data: RhoUnforg::UnforgDeployer {
                    data: "ee".to_string()
                }
            }),
            "ee"
        );
        assert_eq!(
            extract_key_from_expr(&RhoExpr::ExprUnforg {
                data: RhoUnforg::UnforgSysAuthToken
            }),
            "SysAuthToken"
        );
    }
}
