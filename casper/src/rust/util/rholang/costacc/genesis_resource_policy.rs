use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloAcquisitionDemand, NativePhloExecutionContract, NativePhloRules,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    CheckedPhloControls, PhloScheduleBinding,
};
use rholang::rust::interpreter::accounting::phlo_execution::PhloFundingIntentView;

use crate::rust::casper::CasperShardConf;
use crate::rust::errors::CasperError;
use crate::rust::util::proto_util;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub(crate) const OFFERED_PRODUCTION_READY: bool = true;
const GENESIS_POLICY_COMMITMENT_DOMAIN: &[u8] = b"f1r3node:genesis-resource-policy-commitment:v1";

#[derive(Clone, Debug)]
pub struct GenesisResourcePolicy {
    genesis_root: StateHash,
    record: PhloGenesisPolicy,
    minimum_price: u64,
}

#[derive(Clone, Debug)]
pub struct AdoptedResourcePolicy {
    genesis: GenesisResourcePolicy,
    native_rules: NativePhloRules,
    protocol_version: u64,
    shard: String,
}

#[derive(Debug)]
pub struct CompatibleAcquisitionTerms<'policy, 'terms> {
    adopted: &'policy AdoptedResourcePolicy,
    bytes: &'terms [u8],
    schedule: PhloScheduleV1<'terms>,
}

/// DR-99: the genesis block of the chain whose approved block is `approved`.
/// A genesis participant's approved block is the genesis itself. An
/// LFS-joined node's approved block is its restore anchor, and the genesis is
/// the block that the node learned during restore: its hash comes from the
/// DAG's genesis register (or the held height-0 block), and its body from the
/// block store. The body must re-hash to the learned hash, have no parents and
/// belong to the anchor's shard. Without such a block the node fails closed:
/// it never falls back to the anchor, whose post-state is not the genesis
/// identity.
pub(crate) fn resolve_policy_genesis(
    approved: &BlockMessage,
    dag: &BlockDagKeyValueStorage,
    store: &KeyValueBlockStore,
) -> Result<BlockMessage, CasperError> {
    if approved.header.parents_hash_list.is_empty() {
        return Ok(approved.clone());
    }
    let learned = dag
        .genesis_hash()
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?
        .ok_or_else(|| {
            CasperError::RuntimeError(
                "resource policy: this node has not learned the shard genesis hash".to_string(),
            )
        })?;
    let genesis = store
        .get(&learned)
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?
        .ok_or_else(|| {
            CasperError::RuntimeError(format!(
                "resource policy: this node holds no copy of genesis {}",
                hex::encode(&learned)
            ))
        })?;
    if genesis.block_hash != learned
        || proto_util::hash_block(&genesis) != learned
        || !genesis.header.parents_hash_list.is_empty()
        || genesis.shard_id != approved.shard_id
    {
        return Err(CasperError::RuntimeError(format!(
            "resource policy: the held copy of genesis {} is not an authenticated genesis",
            hex::encode(&learned)
        )));
    }
    Ok(genesis)
}

impl<'policy, 'terms> CompatibleAcquisitionTerms<'policy, 'terms> {
    pub fn adopted(&self) -> &'policy AdoptedResourcePolicy { self.adopted }

    pub fn bytes(&self) -> &'terms [u8] { self.bytes }

    pub fn schedule(&self) -> &PhloScheduleV1<'terms> { &self.schedule }
}

impl AdoptedResourcePolicy {
    pub async fn load(
        manager: &RuntimeManager,
        approved_genesis: &BlockMessage,
        adopted: &CasperShardConf,
    ) -> Result<Self, CasperError> {
        GenesisResourcePolicy::load(manager, approved_genesis)
            .await?
            .adopt(adopted)
    }

    /// DR-99: the adopted policy of the authenticated genesis block, with its
    /// sealed constants read at `sealed_state` (`GenesisResourcePolicy::load_at`).
    pub async fn load_at(
        manager: &RuntimeManager,
        approved_genesis: &BlockMessage,
        sealed_state: &StateHash,
        adopted: &CasperShardConf,
    ) -> Result<Self, CasperError> {
        GenesisResourcePolicy::load_at(manager, approved_genesis, sealed_state)
            .await?
            .adopt(adopted)
    }

    pub fn genesis(&self) -> &GenesisResourcePolicy { &self.genesis }

    pub fn genesis_policy_commitment(&self) -> Result<[u8; 32], CasperError> {
        self.genesis.commitment()
    }

    pub fn offered_funded_v6_active(&self) -> bool {
        self.genesis.record.offered_funded_v6_active()
    }

    pub fn native_rules(&self) -> &NativePhloRules { &self.native_rules }

    pub fn check_acquisition_terms<'policy, 'terms>(
        &'policy self,
        bytes: &'terms [u8],
    ) -> Result<CompatibleAcquisitionTerms<'policy, 'terms>, CasperError> {
        let schedule = PhloScheduleV1::decode(bytes, PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let policy = self
            .genesis
            .record
            .schedule()
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let required = PhloScheduleBinding::new(&policy, PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        PhloScheduleBinding::new(&schedule, PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?
            .bind_policy(required.policy())
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        Ok(CompatibleAcquisitionTerms {
            adopted: self,
            bytes,
            schedule,
        })
    }

    pub fn check_controls(&self, controls: CheckedPhloControls<'_>) -> Result<(), CasperError> {
        let environment = controls.schedule().environment;
        if controls.minimum_price() != self.genesis.minimum_price
            || environment.protocol_version != self.protocol_version
            || environment.shard != self.shard.as_bytes()
        {
            return Err(CasperError::RuntimeError(
                "captured phlo context differs from the adopted genesis policy".to_string(),
            ));
        }
        Ok(())
    }

    pub fn check_measured_acquisition(
        &self,
        controls: CheckedPhloControls<'_>,
        demand: &NativePhloAcquisitionDemand<'_>,
    ) -> Result<(), CasperError> {
        self.check_controls(controls)?;
        demand
            .check_controls(controls)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        self.check_acquisition_terms(demand.terms())?;
        Ok(())
    }

    // Changed by DR-113: the caller names the source of the bound.
    // pub fn bind_execution_contract<'a>(
    //     &self,
    //     controls: CheckedPhloControls<'a>,
    //     binding: &PhloScheduleBinding<'_>,
    // ) -> Result<NativePhloExecutionContract<'a>, CasperError> {
    pub fn bind_execution_contract<'a>(
        &self,
        controls: CheckedPhloControls<'a>,
        binding: &PhloScheduleBinding<'_>,
        bound_source: rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource,
    ) -> Result<NativePhloExecutionContract<'a>, CasperError> {
        self.check_controls(controls)?;
        let policy = self
            .genesis
            .record
            .schedule()
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let required = PhloScheduleBinding::new(&policy, PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        binding
            .bind_policy(required.policy())
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        NativePhloExecutionContract::new(controls, binding, bound_source)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))
    }
}

impl GenesisResourcePolicy {
    // Changed by DR-99 (joined-node policy adoption): the identity of the
    // policy is the authenticated genesis block, and its sealed constants can
    // be read at any held state of the chain. An LFS-joined node holds no
    // genesis post-state below its restore horizon.
    // pub async fn load(
    //     manager: &RuntimeManager,
    //     approved_genesis: &BlockMessage,
    // ) -> Result<Self, CasperError> {
    //     if !approved_genesis.header.parents_hash_list.is_empty() {
    //         return Err(CasperError::RuntimeError(
    //             "resource policy requires the approved genesis block".to_string(),
    //         ));
    //     }
    //     let genesis_root = approved_genesis.body.state.post_state_hash.clone();
    //     let record = manager.get_genesis_resource_policy(&genesis_root).await?;
    //     let (_, _, minimum) =
    //         crate::rust::util::token_metadata_check::read_on_chain_consensus_parameters(
    //             manager,
    //             &genesis_root,
    //         )
    //         .await?;
    //     let minimum_price = u64::try_from(minimum).map_err(|_| {
    //         CasperError::RuntimeError("genesis phlo minimum must be nonnegative".to_string())
    //     })?;
    //     let (_, _, decimals) =
    //         crate::rust::util::token_metadata_check::read_on_chain_token_metadata(
    //             manager,
    //             &genesis_root,
    //         )
    //         .await?;
    //     record
    //         .validate_context(
    //             approved_genesis.header.version,
    //             &approved_genesis.shard_id,
    //             decimals,
    //         )
    //         .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
    //     Ok(Self {
    //         genesis_root,
    //         record,
    //         minimum_price,
    //     })
    // }

    /// The policy of a genesis block whose post-state this node holds.
    pub async fn load(
        manager: &RuntimeManager,
        approved_genesis: &BlockMessage,
    ) -> Result<Self, CasperError> {
        Self::load_at(
            manager,
            approved_genesis,
            &approved_genesis.body.state.post_state_hash,
        )
        .await
    }

    /// DR-99: the policy of the authenticated genesis block, with its sealed
    /// constants read at `sealed_state`, a held state of the same chain. The
    /// policy record, the consensus parameters and the token metadata are
    /// genesis template constants under nonce `i64::MAX`, so every held state
    /// returns the genesis values. The identity (`genesis_root`) and the
    /// checked context (header version, shard) stay those of the genesis
    /// block.
    pub async fn load_at(
        manager: &RuntimeManager,
        approved_genesis: &BlockMessage,
        sealed_state: &StateHash,
    ) -> Result<Self, CasperError> {
        if !approved_genesis.header.parents_hash_list.is_empty() {
            return Err(CasperError::RuntimeError(
                "resource policy requires the approved genesis block".to_string(),
            ));
        }
        let genesis_root = approved_genesis.body.state.post_state_hash.clone();
        let record = manager.get_genesis_resource_policy(sealed_state).await?;
        let (_, _, minimum) =
            crate::rust::util::token_metadata_check::read_on_chain_consensus_parameters(
                manager,
                sealed_state,
            )
            .await?;
        let minimum_price = u64::try_from(minimum).map_err(|_| {
            CasperError::RuntimeError("genesis phlo minimum must be nonnegative".to_string())
        })?;
        let (_, _, decimals) =
            crate::rust::util::token_metadata_check::read_on_chain_token_metadata(
                manager,
                sealed_state,
            )
            .await?;
        record
            .validate_context(
                approved_genesis.header.version,
                &approved_genesis.shard_id,
                decimals,
            )
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        Ok(Self {
            genesis_root,
            record,
            minimum_price,
        })
    }

    pub fn genesis_root(&self) -> &StateHash { &self.genesis_root }

    pub fn record(&self) -> &PhloGenesisPolicy { &self.record }

    pub fn commitment(&self) -> Result<[u8; 32], CasperError> {
        let policy_bytes = self
            .record
            .encode()
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        Ok(
            Blake2b256::hash_parts([GENESIS_POLICY_COMMITMENT_DOMAIN, policy_bytes.as_slice()])
                .try_into()
                .expect("Blake2b256 emits 32 bytes"),
        )
    }

    pub fn minimum_price(&self) -> u64 { self.minimum_price }

    pub fn adopt(self, adopted: &CasperShardConf) -> Result<AdoptedResourcePolicy, CasperError> {
        let schedule = self
            .record
            .schedule()
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        if u64::try_from(adopted.min_phlo_price).ok() != Some(self.minimum_price)
            || u64::try_from(adopted.casper_version).ok() != Some(schedule.protocol_version)
            || adopted.shard_name.as_bytes() != schedule.shard
        {
            return Err(CasperError::RuntimeError(
                "adopted phlo parameters differ from the approved genesis policy".to_string(),
            ));
        }
        let protocol_version = schedule.protocol_version;
        let native_rules = NativePhloRules::resolve(&schedule)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        Ok(AdoptedResourcePolicy {
            genesis: self,
            native_rules,
            protocol_version,
            shard: adopted.shard_name.clone(),
        })
    }

    pub fn check_funding_intent(
        &self,
        view: &PhloFundingIntentView<'_>,
    ) -> Result<(), CasperError> {
        let descriptor = self
            .record
            .schedule()
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        view.check_schedule_policy(binding.policy())
            .map_err(|error| CasperError::RuntimeError(error.to_string()))
    }
}

#[cfg(test)]
pub(in crate::rust::util::rholang::costacc) mod tests;
