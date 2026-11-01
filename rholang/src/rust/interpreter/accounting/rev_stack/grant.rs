use crypto::rust::hash::blake2b256::Blake2b256;

use super::owner::WalletOwner;

pub const GRANT_DOMAIN: &[u8] = b"f1r3fly/rev-stack/grant/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub slot: Vec<u8>,
    pub opens: i64,
    pub closes: i64,
}

impl Grant {
    pub fn new(slot: Vec<u8>, opens: i64, closes: i64) -> Result<Self, String> {
        if opens > closes {
            return Err("a grant must open no later than it closes".to_string());
        }
        Ok(Grant {
            slot,
            opens,
            closes,
        })
    }

    pub fn owner(&self) -> WalletOwner {
        let mut input = GRANT_DOMAIN.to_vec();
        input.extend_from_slice(&(self.slot.len() as u32).to_be_bytes());
        input.extend_from_slice(&self.slot);
        input.extend_from_slice(&self.opens.to_be_bytes());
        input.extend_from_slice(&self.closes.to_be_bytes());
        WalletOwner::Unforgeable(Blake2b256::hash(input))
    }

    pub fn open_at(&self, block_number: i64) -> bool {
        self.opens <= block_number && block_number <= self.closes
    }
}

#[cfg(test)]
mod tests {
    use super::Grant;

    #[test]
    fn a_grant_is_located_by_its_slot_and_its_window() {
        let a = Grant::new(vec![1; 32], 10, 20).unwrap();
        assert_eq!(a.owner(), Grant::new(vec![1; 32], 10, 20).unwrap().owner());
        assert_ne!(a.owner(), Grant::new(vec![1; 32], 10, 21).unwrap().owner());
        assert_ne!(a.owner(), Grant::new(vec![2; 32], 10, 20).unwrap().owner());
        assert!(Grant::new(vec![1; 32], 21, 20).is_err());
    }

    #[test]
    fn a_grant_is_open_only_inside_its_window() {
        let g = Grant::new(vec![1; 32], 10, 20).unwrap();
        assert!(!g.open_at(9));
        assert!(g.open_at(10));
        assert!(g.open_at(20));
        assert!(!g.open_at(21));
    }
}
