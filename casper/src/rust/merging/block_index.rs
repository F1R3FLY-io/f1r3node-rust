// See casper/src/main/scala/coop/rchain/casper/merging/BlockIndex.scala

use std::collections::HashMap;

use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    Event, ProcessedDeploy, ProcessedSystemDeploy, ProcessedUserDeploy, SystemDeployData,
};
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsDiff;
use rspace_plus_plus::rspace::trace::event::{Event as RspaceEvent, IOEvent, Produce};

use crate::rust::errors::CasperError;
use crate::rust::merging::deploy_chain_index::DeployChainIndex;
use crate::rust::merging::deploy_index::DeployIndex;
use crate::rust::util::event_converter;

#[derive(Clone)]
pub struct BlockIndex {
    pub block_hash: BlockHash,
    pub event_epoch: EventCodecEpoch,
    pub deploy_chains: Vec<DeployChainIndex>,
}

pub trait ProcessedIndexDeploy {
    fn identity_bytes(&self) -> &[u8];
    fn valid_after_block_number(&self) -> i64;
    fn deploy_log(&self) -> &[Event];
    fn cost(&self) -> u64;
    fn is_failed(&self) -> bool;
}

impl ProcessedIndexDeploy for ProcessedDeploy {
    fn identity_bytes(&self) -> &[u8] { ProcessedDeploy::identity_bytes(self) }
    fn valid_after_block_number(&self) -> i64 { self.body().valid_after_block_number }
    fn deploy_log(&self) -> &[Event] { &self.deploy_log }
    fn cost(&self) -> u64 { self.cost.cost }
    fn is_failed(&self) -> bool { self.is_failed }
}

impl ProcessedIndexDeploy for ProcessedUserDeploy {
    fn identity_bytes(&self) -> &[u8] { ProcessedUserDeploy::identity_bytes(self) }
    fn valid_after_block_number(&self) -> i64 {
        ProcessedUserDeploy::valid_after_block_number(self)
    }
    fn deploy_log(&self) -> &[Event] { ProcessedUserDeploy::deploy_log(self) }
    fn cost(&self) -> u64 { ProcessedUserDeploy::cost(self).cost }
    fn is_failed(&self) -> bool { ProcessedUserDeploy::is_failed(self) }
}

pub fn create_event_log_index(
    events: &[Event],
    epoch: EventCodecEpoch,
    history_repository: RhoHistoryRepository,
    pre_state_hash: &Blake2b256Hash,
    mergeable_chs: NumberChannelsDiff,
) -> Result<EventLogIndex, CasperError> {
    let events = events
        .iter()
        .map(|event| event_converter::to_rspace_event_for_epoch(event, epoch))
        .collect::<Result<Vec<_>, _>>()?;
    let pre_state_reader = history_repository
        .get_history_reader_for_epoch(pre_state_hash, epoch)
        .map_err(CasperError::HistoryError)?;

    // Event channel hashes are already computed in the adopted namespace.
    // Use the epoch-bound reader's by-hash accessors to avoid hashing them a
    // second time while decoding values under the same adopted epoch.
    let mut prestate_flags = HashMap::new();
    for event in &events {
        let RspaceEvent::IoEvent(IOEvent::Produce(produce)) = event else {
            continue;
        };
        if prestate_flags.contains_key(&produce.hash) {
            continue;
        }

        let exists = pre_state_reader
            .get_data_by_hash(&produce.channel_hash)
            .map_err(CasperError::HistoryError)?
            .iter()
            .any(|datum| datum.source == *produce);

        let touches_join = pre_state_reader
            .get_joins_by_hash(&produce.channel_hash)
            .map_err(CasperError::HistoryError)?
            .iter()
            .any(|join| join.len() > 1);
        prestate_flags.insert(produce.hash.clone(), (exists, touches_join));
    }

    let produce_exists_in_pre_state =
        |p: &Produce| prestate_flags.get(&p.hash).is_some_and(|flags| flags.0);
    let produce_touches_pre_state_join =
        |p: &Produce| prestate_flags.get(&p.hash).is_some_and(|flags| flags.1);
    Ok(EventLogIndex::new(
        events,
        produce_exists_in_pre_state,
        produce_touches_pre_state_join,
        mergeable_chs,
    ))
}

pub fn new<D: ProcessedIndexDeploy>(
    block_hash: &BlockHash,
    block_number: i64,
    epoch: EventCodecEpoch,
    usr_processed_deploys: &[D],
    sys_processed_deploys: &Vec<ProcessedSystemDeploy>,
    pre_state_hash: &Blake2b256Hash,
    post_state_hash: &Blake2b256Hash,
    history_repository: &RhoHistoryRepository,
    mergeable_chs: &Vec<NumberChannelsDiff>,
) -> Result<BlockIndex, CasperError> {
    // Connect mergeable channels data with processed deploys by index
    let usr_count = usr_processed_deploys.len();
    let sys_count = sys_processed_deploys.len();
    let deploy_count = usr_count + sys_count;
    let mrg_count = mergeable_chs.len();
    if mrg_count != deploy_count {
        let msg = format!(
            "Mergeable channel count mismatch for block {}: mergeable_maps={}, deploys={}",
            hex::encode(&block_hash[..std::cmp::min(10, block_hash.len())]),
            mrg_count,
            deploy_count
        );
        tracing::error!(
            block_hash = %hex::encode(&block_hash[..std::cmp::min(10, block_hash.len())]),
            mergeable_maps = mrg_count,
            deploys = deploy_count,
            "mergeable channel count does not match deploy count"
        );
        return Err(CasperError::RuntimeError(msg));
    }
    let aligned_mergeable_chs = mergeable_chs.clone();

    // Connect deploy with corresponding mergeable channels map
    let (usr_mergeable_chs, sys_mergeable_chs) = aligned_mergeable_chs.split_at(usr_count);
    let usr_deploys_with_mergeable: Vec<_> = usr_processed_deploys
        .iter()
        .zip(usr_mergeable_chs.iter())
        .collect();
    let sys_deploys_with_mergeable: Vec<_> = sys_processed_deploys
        .iter()
        .zip(sys_mergeable_chs.iter())
        .collect();

    // Create user deploy indices - filter out failed deploys
    let mut usr_deploy_indices = Vec::new();
    for (deploy, merge_chs) in usr_deploys_with_mergeable {
        if !deploy.is_failed() {
            let event_log_index = create_event_log_index(
                deploy.deploy_log(),
                epoch,
                history_repository.clone(),
                pre_state_hash,
                merge_chs.clone(),
            )?;

            let deploy_index = DeployIndex {
                deploy_id: deploy.identity_bytes().to_vec().into(),
                cost: deploy.cost(),
                event_log_index,
            };

            usr_deploy_indices.push(deploy_index);
        }
    }

    // Create system deploy indices - collect successful system deploys
    let mut sys_deploy_indices = Vec::new();
    for (sys_deploy, merge_chs) in sys_deploys_with_mergeable {
        match sys_deploy {
            ProcessedSystemDeploy::Succeeded {
                system_deploy,
                event_list,
            } => {
                let (sig, cost) = match system_deploy {
                    SystemDeployData::Slash { .. } => {
                        let mut sig_bytes = block_hash.to_vec();
                        sig_bytes.extend_from_slice(DeployIndex::SYS_SLASH_DEPLOY_ID);
                        (sig_bytes.into(), DeployIndex::SYS_SLASH_DEPLOY_COST)
                    }
                    SystemDeployData::CloseBlockSystemDeployData => {
                        let mut sig_bytes = block_hash.to_vec();
                        sig_bytes.extend_from_slice(DeployIndex::SYS_CLOSE_BLOCK_DEPLOY_ID);
                        (sig_bytes.into(), DeployIndex::SYS_CLOSE_BLOCK_DEPLOY_COST)
                    }
                    SystemDeployData::Empty => {
                        let mut sig_bytes = block_hash.to_vec();
                        sig_bytes.extend_from_slice(DeployIndex::SYS_EMPTY_DEPLOY_ID);
                        (sig_bytes.into(), DeployIndex::SYS_EMPTY_DEPLOY_COST)
                    }
                };

                let event_log_index = create_event_log_index(
                    event_list,
                    epoch,
                    history_repository.clone(),
                    pre_state_hash,
                    merge_chs.clone(),
                )?;

                let deploy_index = DeployIndex {
                    deploy_id: sig,
                    cost,
                    event_log_index,
                };

                sys_deploy_indices.push(deploy_index);
            }
            ProcessedSystemDeploy::Failed { .. } => {
                // Skip failed system deploys
            }
        }
    }

    // Combine all deploy indices
    let mut all_deploy_indices = usr_deploy_indices;
    all_deploy_indices.extend(sys_deploy_indices);

    // Here deploys from a single block are examined. Atm deploys in block are executed sequentially,
    // so all conflicts are resolved according to order of sequential execution.
    // Therefore there won't be any conflicts between event logs. But there can be dependencies.
    let deploy_chains = merging_logic::compute_related_sets(
        &all_deploy_indices.into_iter().collect(),
        |l: &DeployIndex, r: &DeployIndex| {
            merging_logic::depends(&l.event_log_index, &r.event_log_index)
        },
    );

    // Validity windows per user deploy sig, for the merge-time window rule.
    // System deploys carry no window and are absent by construction.
    let deploy_windows: std::collections::HashMap<prost::bytes::Bytes, i64> = usr_processed_deploys
        .iter()
        .map(|d| {
            (
                d.identity_bytes().to_vec().into(),
                d.valid_after_block_number(),
            )
        })
        .collect();

    // Convert deploy chains to DeployChainIndex
    let mut deploy_chain_indices = Vec::new();
    for deploy_chain in deploy_chains.0.iter() {
        let chain_index = DeployChainIndex::new(
            deploy_chain,
            pre_state_hash,
            post_state_hash,
            history_repository.clone(),
            block_hash.clone(),
            block_number,
            deploy_windows.clone(),
        )
        .map_err(|e| CasperError::HistoryError(e))?;
        deploy_chain_indices.push(chain_index);
    }

    Ok(BlockIndex {
        block_hash: block_hash.clone(),
        event_epoch: epoch,
        deploy_chains: deploy_chain_indices,
    })
}
