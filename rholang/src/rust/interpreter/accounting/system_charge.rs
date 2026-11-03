use super::rev_stack::{RevAmount, WalletOwner};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemCharge {
    pub payer: WalletOwner,
    pub hold: RevAmount,
}

impl SystemCharge {
    pub fn new(payer: WalletOwner, hold: RevAmount) -> Self { SystemCharge { payer, hold } }
}
