use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rust::deploy_envelope::DeployEnvelope;
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::vault_cost_deploy::lifecycle_random;
use crate::rust::errors::CasperError;

pub struct OfferedSettlementContext {
    pub reservation_id: [u8; 32],
    pub fee_address: VaultAddress,
    pub initial_rand: Blake2b512Random,
}

impl OfferedSettlementContext {
    pub fn from_block_inputs(
        envelope: &DeployEnvelope,
        block_data: &BlockData,
    ) -> Result<Self, CasperError> {
        let reservation_id: [u8; 32] = envelope.identity().as_bytes().try_into().map_err(|_| {
            CasperError::RuntimeError("offered reservation identity must be 32 bytes".into())
        })?;
        let fee_address = VaultAddress::from_public_key(&block_data.sender).ok_or_else(|| {
            CasperError::RuntimeError("offered block sender has no valid fee address".into())
        })?;
        Ok(Self {
            reservation_id,
            fee_address,
            initial_rand: lifecycle_random(&reservation_id, 0),
        })
    }
}
