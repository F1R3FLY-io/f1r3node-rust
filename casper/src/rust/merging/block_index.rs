// See casper/src/main/scala/coop/rchain/casper/merging/BlockIndex.scala

use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    Event, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
};
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsDiff;
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::trace::event::Produce;

use crate::rust::errors::CasperError;
use crate::rust::merging::deploy_chain_index::DeployChainIndex;
use crate::rust::merging::deploy_index::DeployIndex;
use crate::rust::util::event_converter;

#[derive(Clone)]
pub struct BlockIndex {
    pub block_hash: BlockHash,
    pub deploy_chains: Vec<DeployChainIndex>,
}

impl BlockIndex {
    pub fn retained_bytes(&self) -> usize {
        self.deploy_chains.iter().fold(
            std::mem::size_of::<Self>().saturating_add(self.block_hash.len()),
            |total, chain| total.saturating_add(chain.retained_bytes()),
        )
    }
}

pub fn create_event_log_index(
    events: &[Event],
    history_repository: RhoHistoryRepository,
    pre_state_hash: &Blake2b256Hash,
    mergeable_chs: NumberChannelsDiff,
) -> EventLogIndex {
    let pre_state_reader = history_repository
        .get_history_reader(pre_state_hash)
        .unwrap();

    let produce_exists_in_pre_state = |p: &Produce| {
        pre_state_reader
            .get_data(&p.channel_hash)
            .is_ok_and(|data| data.iter().any(|d| d.source == *p))
    };

    let produce_touches_pre_state_join = |p: &Produce| {
        pre_state_reader
            .get_joins(&p.channel_hash)
            .is_ok_and(|joins| joins.iter().any(|j| j.len() > 1))
    };

    EventLogIndex::new(
        events
            .iter()
            .map(event_converter::to_rspace_event)
            .collect(),
        produce_exists_in_pre_state,
        produce_touches_pre_state_join,
        mergeable_chs,
    )
}

fn effect_bearing_user_deploys(deploys: &[ProcessedDeploy]) -> Vec<&ProcessedDeploy> {
    deploys
        .iter()
        .filter(|deploy| !deploy.is_admission_rejected())
        .collect()
}

pub fn new(
    block_hash: &BlockHash,
    block_number: i64,
    usr_processed_deploys: &Vec<ProcessedDeploy>,
    sys_processed_deploys: &Vec<ProcessedSystemDeploy>,
    pre_state_hash: &Blake2b256Hash,
    post_state_hash: &Blake2b256Hash,
    history_repository: &RhoHistoryRepository,
    mergeable_chs: &Vec<NumberChannelsDiff>,
) -> Result<BlockIndex, CasperError> {
    let usr_effect_deploys = effect_bearing_user_deploys(usr_processed_deploys);
    let usr_count = usr_effect_deploys.len();
    let sys_count = sys_processed_deploys.len();
    let deploy_count = usr_count + sys_count;
    let mrg_count = mergeable_chs.len();
    if mrg_count != deploy_count {
        let msg = format!(
            "Mergeable channel count mismatch for block {}: mergeable_maps={}, effect_records={}",
            hex::encode(&block_hash[..std::cmp::min(10, block_hash.len())]),
            mrg_count,
            deploy_count
        );
        tracing::error!(
            block_hash = %hex::encode(&block_hash[..std::cmp::min(10, block_hash.len())]),
            mergeable_maps = mrg_count,
            effect_records = deploy_count,
            "mergeable channel count does not match effect-record count"
        );
        return Err(CasperError::RuntimeError(msg));
    }
    let aligned_mergeable_chs = mergeable_chs.clone();

    let mut witness_count = 0usize;
    let mut empty_witness_count = 0usize;
    for (pre, post) in usr_effect_deploys
        .iter()
        .map(|deploy| (&deploy.pre_state_hash, &deploy.post_state_hash))
        .chain(
            sys_processed_deploys
                .iter()
                .map(ProcessedSystemDeploy::state_hashes),
        )
    {
        match (pre.is_empty(), post.is_empty()) {
            (false, false) => witness_count += 1,
            (true, true) => empty_witness_count += 1,
            _ => {
                return Err(CasperError::RuntimeError(format!(
                    "Incomplete execution state witness in block {}",
                    hex::encode(block_hash)
                )))
            }
        }
    }
    if witness_count > 0 && empty_witness_count > 0 {
        return Err(CasperError::RuntimeError(format!(
            "Mixed exact and legacy execution state witnesses in block {}",
            hex::encode(block_hash)
        )));
    }
    let has_exact_state_witness = witness_count == deploy_count && deploy_count > 0;
    if has_exact_state_witness {
        let mut expected_pre = pre_state_hash.to_bytes_prost();
        for (execution_index, (effect_pre, effect_post)) in usr_effect_deploys
            .iter()
            .map(|deploy| (&deploy.pre_state_hash, &deploy.post_state_hash))
            .chain(
                sys_processed_deploys
                    .iter()
                    .map(ProcessedSystemDeploy::state_hashes),
            )
            .enumerate()
        {
            if effect_pre != &expected_pre {
                return Err(CasperError::RuntimeError(format!(
                    "Non-contiguous execution state witness in block {} at effect {}",
                    hex::encode(block_hash),
                    execution_index
                )));
            }
            expected_pre = effect_post.clone();
        }
        if expected_pre != post_state_hash.to_bytes_prost() {
            return Err(CasperError::RuntimeError(format!(
                "Final execution state witness does not match block post-state in block {}",
                hex::encode(block_hash)
            )));
        }
    }

    // Connect deploy with corresponding mergeable channels map
    let (usr_mergeable_chs, sys_mergeable_chs) = aligned_mergeable_chs.split_at(usr_count);
    let usr_deploys_with_mergeable: Vec<_> = usr_effect_deploys
        .into_iter()
        .zip(usr_mergeable_chs.iter())
        .collect();
    let sys_deploys_with_mergeable: Vec<_> = sys_processed_deploys
        .iter()
        .zip(sys_mergeable_chs.iter())
        .collect();

    // Create user deploy indices - filter out failed deploys
    let mut usr_deploy_indices = Vec::new();
    for (execution_index, (deploy, merge_chs)) in usr_deploys_with_mergeable.into_iter().enumerate()
    {
        if !deploy.is_failed {
            let effect_pre = if has_exact_state_witness {
                Blake2b256Hash::from_bytes_prost(&deploy.pre_state_hash)
            } else {
                pre_state_hash.clone()
            };
            let event_log_index = create_event_log_index(
                &deploy.deploy_log,
                history_repository.clone(),
                &effect_pre,
                merge_chs.clone(),
            );

            let state_changes = if has_exact_state_witness {
                let effect_post = Blake2b256Hash::from_bytes_prost(&deploy.post_state_hash);
                Some(
                    StateChange::new(
                        history_repository
                            .get_history_reader_struct(&effect_pre)
                            .map_err(CasperError::HistoryError)?,
                        history_repository
                            .get_history_reader_struct(&effect_post)
                            .map_err(CasperError::HistoryError)?,
                        &event_log_index,
                    )
                    .map_err(CasperError::HistoryError)?,
                )
            } else {
                None
            };

            let deploy_index = DeployIndex {
                deploy_id: deploy.deploy_id().clone(),
                cost: deploy.cost.cost,
                event_log_index,
                execution_index: u32::try_from(execution_index).map_err(|_| {
                    CasperError::RuntimeError("Execution index exceeds u32".to_string())
                })?,
                state_changes,
            };

            usr_deploy_indices.push(deploy_index);
        }
    }

    // Create system deploy indices - collect successful system deploys
    let mut sys_deploy_indices = Vec::new();
    for (sys_index, (sys_deploy, merge_chs)) in sys_deploys_with_mergeable.into_iter().enumerate() {
        match sys_deploy {
            ProcessedSystemDeploy::Succeeded {
                system_deploy,
                event_list,
                pre_state_hash: effect_pre_hash,
                post_state_hash: effect_post_hash,
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
                    SystemDeployData::Redeem { .. } => {
                        let mut sig_bytes = block_hash.to_vec();
                        sig_bytes.extend_from_slice(DeployIndex::SYS_REDEEM_DEPLOY_ID);
                        (sig_bytes.into(), DeployIndex::SYS_REDEEM_DEPLOY_COST)
                    }
                    SystemDeployData::Empty => {
                        let mut sig_bytes = block_hash.to_vec();
                        sig_bytes.extend_from_slice(DeployIndex::SYS_EMPTY_DEPLOY_ID);
                        (sig_bytes.into(), DeployIndex::SYS_EMPTY_DEPLOY_COST)
                    }
                };

                let effect_pre = if has_exact_state_witness {
                    Blake2b256Hash::from_bytes_prost(effect_pre_hash)
                } else {
                    pre_state_hash.clone()
                };
                let event_log_index = create_event_log_index(
                    event_list,
                    history_repository.clone(),
                    &effect_pre,
                    merge_chs.clone(),
                );

                let state_changes = if has_exact_state_witness {
                    let effect_post = Blake2b256Hash::from_bytes_prost(effect_post_hash);
                    Some(
                        StateChange::new(
                            history_repository
                                .get_history_reader_struct(&effect_pre)
                                .map_err(CasperError::HistoryError)?,
                            history_repository
                                .get_history_reader_struct(&effect_post)
                                .map_err(CasperError::HistoryError)?,
                            &event_log_index,
                        )
                        .map_err(CasperError::HistoryError)?,
                    )
                } else {
                    None
                };

                let deploy_index = DeployIndex {
                    deploy_id: sig,
                    cost,
                    event_log_index,
                    execution_index: u32::try_from(usr_count + sys_index).map_err(|_| {
                        CasperError::RuntimeError("Execution index exceeds u32".to_string())
                    })?,
                    state_changes,
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
                d.deploy_id().clone(),
                d.deploy.data.valid_after_block_number,
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
        deploy_chains: deploy_chain_indices,
    })
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::{DeployAdmissionStatus, ProcessedDeploy};
    use proptest::prelude::*;

    use super::effect_bearing_user_deploys;
    use crate::rust::util::construct_deploy::basic_processed_deploy;

    fn processed_deploy(
        id: i32,
        admission_rejected: bool,
        execution_failed: bool,
    ) -> ProcessedDeploy {
        let mut deploy = basic_processed_deploy(id, None).unwrap();
        deploy.is_failed = execution_failed || admission_rejected;
        deploy.admission_status = if admission_rejected {
            DeployAdmissionStatus::Rejected
        } else {
            DeployAdmissionStatus::Executed
        };
        deploy
    }

    #[test]
    fn funding_rejection_and_close_block_require_one_mergeable_map() {
        let rejected = processed_deploy(0, true, true);
        assert_eq!(effect_bearing_user_deploys(&[rejected]).len() + 1, 1);
    }

    #[test]
    fn ordinary_execution_failure_retains_its_mergeable_map() {
        let failed = processed_deploy(0, false, true);
        assert_eq!(effect_bearing_user_deploys(&[failed]).len() + 1, 2);
    }

    #[test]
    fn effect_projection_preserves_executed_order() {
        let deploys = vec![
            processed_deploy(0, false, false),
            processed_deploy(1, true, true),
            processed_deploy(2, false, true),
        ];
        let projected = effect_bearing_user_deploys(&deploys);
        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].deploy.data.term, "@0!(0)");
        assert_eq!(projected[1].deploy.data.term, "@2!(2)");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn mergeable_cardinality_tracks_only_effect_bearing_records(
            dispositions in proptest::collection::vec((any::<bool>(), any::<bool>()), 0..64),
            system_deploy_count in 0usize..16,
        ) {
            let deploys: Vec<_> = dispositions
                .iter()
                .enumerate()
                .map(|(index, (admission_rejected, execution_failed))| {
                    processed_deploy(
                        i32::try_from(index).unwrap(),
                        *admission_rejected,
                        *execution_failed,
                    )
                })
                .collect();
            let expected_user_effects = dispositions
                .iter()
                .filter(|(admission_rejected, _)| !admission_rejected)
                .count();

            prop_assert_eq!(
                effect_bearing_user_deploys(&deploys).len() + system_deploy_count,
                expected_user_effects + system_deploy_count,
            );

            let mut reversed = deploys;
            reversed.reverse();
            prop_assert_eq!(
                effect_bearing_user_deploys(&reversed).len() + system_deploy_count,
                expected_user_effects + system_deploy_count,
            );
        }
    }
}
