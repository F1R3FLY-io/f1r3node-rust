// See casper/src/main/scala/coop/rchain/casper/genesis/contracts/Vault.scala

use rholang::rust::interpreter::util::vault_address::VaultAddress;
use shared::rust::token_amount::TokenAmount;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Vault {
    pub vault_address: VaultAddress,
    pub initial_balance: TokenAmount,
}
