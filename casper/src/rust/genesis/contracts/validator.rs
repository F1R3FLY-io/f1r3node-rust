// See casper/src/main/scala/coop/rchain/casper/genesis/contracts/Validator.scala

use crypto::rust::public_key::PublicKey;
use num_bigint::BigInt;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Validator {
    pub pk: PublicKey,
    pub stake: BigInt,
}
