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
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

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

    pub fn genesis(&self) -> &GenesisResourcePolicy { &self.genesis }

    pub fn native_rules(&self) -> &NativePhloRules { &self.native_rules }

    pub fn check_acquisition_terms<'policy, 'terms>(
        &'policy self,
        bytes: &'terms [u8],
    ) -> Result<CompatibleAcquisitionTerms<'policy, 'terms>, CasperError> {
        let schedule = PhloScheduleV1::decode(bytes, PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let policy = self
            .genesis
            .record
            .schedule()
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let required = PhloScheduleBinding::new(&policy, PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        PhloScheduleBinding::new(&schedule, PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
            .bind_policy(required.policy())
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
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
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        self.check_acquisition_terms(demand.terms())?;
        Ok(())
    }

    pub fn bind_execution_contract<'a>(
        &self,
        controls: CheckedPhloControls<'a>,
        binding: &PhloScheduleBinding<'_>,
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
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        NativePhloExecutionContract::new(controls, binding)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))
    }
}

impl GenesisResourcePolicy {
    pub async fn load(
        manager: &RuntimeManager,
        approved_genesis: &BlockMessage,
    ) -> Result<Self, CasperError> {
        if !approved_genesis.header.parents_hash_list.is_empty() {
            return Err(CasperError::RuntimeError(
                "resource policy requires the approved genesis block".to_string(),
            ));
        }
        let genesis_root = approved_genesis.body.state.post_state_hash.clone();
        let record = manager.get_genesis_resource_policy(&genesis_root).await?;
        let (_, _, minimum) =
            crate::rust::util::token_metadata_check::read_on_chain_consensus_parameters(
                manager,
                &genesis_root,
            )
            .await?;
        let minimum_price = u64::try_from(minimum).map_err(|_| {
            CasperError::RuntimeError("genesis phlo minimum must be nonnegative".to_string())
        })?;
        let (_, _, decimals) =
            crate::rust::util::token_metadata_check::read_on_chain_token_metadata(
                manager,
                &genesis_root,
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
