use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::accounting::phlo_execution::PhloFundingIntentView;

use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Clone, Debug)]
pub struct GenesisResourcePolicy {
    genesis_root: StateHash,
    record: PhloGenesisPolicy,
    minimum_price: u64,
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
