use crypto::rust::public_key::PublicKey;
use rholang::rust::interpreter::accounting::rev_stack::{RevAmount, WalletOwner};

pub const CLOSE_BASE_PHLO: u64 = 200_000;

pub const CLOSE_PHLO_PER_BOND: u64 = 13_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseCharge {
    pub price: u64,
    pub pre_charge: RevAmount,
}

impl CloseCharge {
    pub fn new(bonds: u64, lag: u64, floor: u64) -> Self {
        CloseCharge {
            price: floor,
            pre_charge: pre_charge(bonds, lag, floor),
        }
    }

    pub fn charge(&self, close_phlo: u64, balance: RevAmount) -> RevAmount {
        close_phlo
            .saturating_mul(self.price)
            .min(self.pre_charge)
            .min(balance)
    }

    pub fn fingerprint(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&self.price.to_le_bytes());
        bytes[8..].copy_from_slice(&self.pre_charge.to_le_bytes());
        bytes
    }
}

pub fn pre_charge_phlo(bonds: u64, lag: u64) -> u64 {
    CLOSE_PHLO_PER_BOND
        .saturating_mul(bonds.saturating_add(lag))
        .saturating_add(CLOSE_BASE_PHLO)
}

pub fn pre_charge(bonds: u64, lag: u64, floor: u64) -> RevAmount {
    pre_charge_phlo(bonds, lag).saturating_mul(floor)
}

pub async fn shard_floor(
    runtime_manager: &crate::rust::util::rholang::runtime_manager::RuntimeManager,
    root: &models::rust::block::state_hash::StateHash,
) -> Result<u64, crate::rust::errors::CasperError> {
    let (_, _, floor) =
        crate::rust::util::token_metadata_check::read_on_chain_consensus_parameters(runtime_manager, root).await?;
    u64::try_from(floor).map_err(|_| {
        crate::rust::errors::CasperError::RuntimeError("the shard price floor is negative".to_string())
    })
}

pub fn proposer(sender: &[u8]) -> Option<WalletOwner> {
    if sender.is_empty() {
        return None;
    }
    WalletOwner::from_public_key(&PublicKey::from_bytes(sender)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pre_charge_is_a_base_plus_a_rate_for_each_bond_and_each_block_of_lag() {
        assert_eq!(pre_charge_phlo(0, 0), CLOSE_BASE_PHLO);
        assert_eq!(pre_charge_phlo(100, 2), CLOSE_BASE_PHLO + 102 * CLOSE_PHLO_PER_BOND);
        assert_eq!(pre_charge(100, 2, 3), 3 * pre_charge_phlo(100, 2));
    }

    #[test]
    fn the_charge_is_the_actual_close_work_at_the_floor_price() {
        let close = CloseCharge::new(10, 0, 2);
        assert_eq!(close.charge(60_000, RevAmount::MAX), 120_000);
    }

    #[test]
    fn the_charge_never_passes_the_pre_charge_or_the_balance() {
        let close = CloseCharge::new(0, 0, 1);
        assert_eq!(close.charge(CLOSE_BASE_PHLO + 1, RevAmount::MAX), close.pre_charge);
        assert_eq!(close.charge(60_000, 10), 10);
    }

    #[test]
    fn a_zero_floor_charges_nothing() {
        assert_eq!(CloseCharge::new(100, 0, 0).charge(60_000, RevAmount::MAX), 0);
    }

    #[test]
    fn genesis_has_no_proposer() {
        assert!(proposer(&[]).is_none());
    }
}
