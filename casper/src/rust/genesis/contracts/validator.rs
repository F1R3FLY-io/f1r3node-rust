// See casper/src/main/scala/coop/rchain/casper/genesis/contracts/Validator.scala

use crypto::rust::public_key::PublicKey;
use shared::rust::token_amount::TokenAmount;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Validator {
    pub pk: PublicKey,
    pub stake: TokenAmount,
}
