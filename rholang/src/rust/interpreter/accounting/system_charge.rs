use std::sync::{Arc, Mutex};

use super::rev_stack::{RevAmount, WalletOwner};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseCover {
    pub owed: RevAmount,
    pub balance: RevAmount,
}

impl CloseCover {
    pub fn covered(&self) -> bool { self.owed <= self.balance }
}

#[derive(Clone, Debug)]
pub struct SystemCharge {
    pub payer: WalletOwner,
    cover: Arc<Mutex<Option<CloseCover>>>,
}

impl SystemCharge {
    pub fn new(payer: WalletOwner) -> Self {
        SystemCharge {
            payer,
            cover: Arc::new(Mutex::new(None)),
        }
    }

    pub fn record(&self, owed: RevAmount, balance: RevAmount) {
        *self.cover.lock().unwrap_or_else(|p| p.into_inner()) = Some(CloseCover { owed, balance });
    }

    pub fn cover(&self) -> Option<CloseCover> { *self.cover.lock().unwrap_or_else(|p| p.into_inner()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proposer_covers_the_close_only_when_its_balance_meets_the_charge() {
        let charge = SystemCharge::new(WalletOwner::Unforgeable(vec![1; 32]));
        assert_eq!(charge.cover(), None);
        charge.record(50, 50);
        assert!(charge.cover().unwrap().covered());
        let shared = charge.clone();
        shared.record(51, 50);
        assert!(!charge.cover().unwrap().covered());
    }
}
