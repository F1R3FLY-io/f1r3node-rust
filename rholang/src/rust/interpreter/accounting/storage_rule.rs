use std::collections::HashMap;

use super::rev_stack::seal::SealRecord;

type Payer = Option<SealRecord>;

#[derive(Debug, Default)]
pub struct LeftInSpace {
    data: HashMap<Vec<u8>, Vec<Payer>>,
    continuations: HashMap<Vec<u8>, Vec<Payer>>,
    taken: HashMap<Vec<u8>, u32>,
    taken_continuations: HashMap<Vec<u8>, u32>,
    announced: HashMap<Vec<u8>, Vec<Payer>>,
}

fn add(items: &mut HashMap<Vec<u8>, u32>, id: &[u8]) {
    *items.entry(id.to_vec()).or_insert(0) += 1;
}

fn take(items: &mut HashMap<Vec<u8>, u32>, id: &[u8]) -> bool {
    match items.get_mut(id) {
        Some(count) if *count > 1 => {
            *count -= 1;
            true
        }
        Some(_) => {
            items.remove(id);
            true
        }
        None => false,
    }
}

fn keep(items: &mut HashMap<Vec<u8>, Vec<Payer>>, id: &[u8], payer: Payer) {
    items.entry(id.to_vec()).or_default().push(payer);
}

fn release(items: &mut HashMap<Vec<u8>, Vec<Payer>>, id: &[u8]) -> Option<Payer> {
    let payers = items.get_mut(id)?;
    let payer = payers.remove(0);
    if payers.is_empty() {
        items.remove(id);
    }
    Some(payer)
}

impl LeftInSpace {
    pub fn stored_datum(&mut self, id: &[u8], payer: Payer) { keep(&mut self.data, id, payer); }

    pub fn stored_continuation(&mut self, id: &[u8], payer: Payer) {
        keep(&mut self.continuations, id, payer);
    }

    pub fn removed_datum(&mut self, id: &[u8]) -> Option<Payer> {
        let payer = release(&mut self.data, id);
        if payer.is_none() {
            add(&mut self.taken, id);
        }
        payer
    }

    pub fn removed_continuation(&mut self, id: &[u8]) -> Option<Payer> {
        let payer = release(&mut self.continuations, id);
        if payer.is_none() {
            add(&mut self.taken_continuations, id);
        }
        payer
    }

    pub fn left_datum(&mut self, id: &[u8], payer: Payer) -> bool {
        if take(&mut self.taken, id) {
            return false;
        }
        self.stored_datum(id, payer);
        true
    }

    pub fn left_continuation(&mut self, id: &[u8], payer: Payer) -> bool {
        if take(&mut self.taken_continuations, id) {
            return false;
        }
        self.stored_continuation(id, payer);
        true
    }

    pub fn restores(&mut self, id: &[u8]) -> bool { take(&mut self.taken, id) }

    pub fn announce_datum(&mut self, id: &[u8], payer: Payer) { keep(&mut self.announced, id, payer); }

    pub fn withdraw_announcement(&mut self, id: &[u8]) { release(&mut self.announced, id); }

    pub fn datum_payer(&self, id: &[u8]) -> Option<Payer> {
        self.data
            .get(id)
            .or_else(|| self.announced.get(id))
            .and_then(|payers| payers.first().cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::LeftInSpace;
    use crate::rust::interpreter::accounting::rev_stack::seal::{Origin, SealKind, SealRecord};
    use crate::rust::interpreter::accounting::rev_stack::WalletOwner;

    fn payer(owner: u8) -> Option<SealRecord> {
        Some(SealRecord {
            owner: WalletOwner::Unforgeable(vec![owner; 32]),
            kind: SealKind::Signed,
            origin: Origin {
                envelope: vec![9; 64],
                signers: vec![WalletOwner::Unforgeable(vec![owner; 32])],
                phlo_price: 1,
            },
            from_data: false,
        })
    }

    #[test]
    fn only_items_the_run_stored_count_as_its_own() {
        let mut rule = LeftInSpace::default();
        rule.stored_datum(b"a", None);
        rule.stored_datum(b"a", None);
        assert!(rule.removed_datum(b"a").is_some());
        assert!(rule.removed_datum(b"a").is_some());
        assert!(rule.removed_datum(b"a").is_none());
        assert!(rule.removed_datum(b"pre").is_none());
        assert!(rule.restores(b"pre"));
        assert!(!rule.restores(b"pre"));
        rule.stored_continuation(b"k", None);
        assert!(rule.removed_continuation(b"k").is_some());
        assert!(rule.removed_continuation(b"k").is_none());
    }

    #[test]
    fn an_item_taken_before_its_store_is_recorded_is_not_left_in_space() {
        let mut rule = LeftInSpace::default();
        assert!(rule.removed_datum(b"d").is_none());
        assert!(!rule.left_datum(b"d", None));
        assert!(rule.removed_datum(b"d").is_none());
        assert!(rule.left_datum(b"e", None));
        assert!(rule.removed_datum(b"e").is_some());
        assert!(rule.removed_continuation(b"k").is_none());
        assert!(!rule.left_continuation(b"k", None));
        assert!(rule.left_continuation(b"j", None));
        assert!(rule.removed_continuation(b"j").is_some());
    }

    #[test]
    fn a_removed_item_names_the_record_that_paid_for_it() {
        let mut rule = LeftInSpace::default();
        rule.stored_datum(b"a", payer(1));
        rule.stored_datum(b"a", payer(2));
        assert!(rule.left_continuation(b"k", payer(3)));
        assert_eq!(rule.removed_datum(b"a"), Some(payer(1)));
        assert_eq!(rule.removed_datum(b"a"), Some(payer(2)));
        assert_eq!(rule.removed_continuation(b"k"), Some(payer(3)));
    }

    #[test]
    fn a_refund_record_keeps_the_mark_of_data_received_at_run_time() {
        let mut rule = LeftInSpace::default();
        let marked = payer(4).map(|record| SealRecord { from_data: true, ..record });
        rule.stored_datum(b"m", marked.clone());
        assert!(rule.left_continuation(b"n", marked.clone()));
        assert_eq!(rule.removed_datum(b"m"), Some(marked.clone()));
        assert_eq!(rule.removed_continuation(b"n"), Some(marked.clone()));
        assert!(marked.is_some_and(|record| record.from_data));
    }
}
