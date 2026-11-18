use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use shared::rust::ByteString;

pub const OFFER_REJECTION_LOG_CAPACITY_DEFAULT: usize = 4096;
pub const OFFER_REJECTION_REASON_BYTES_DEFAULT: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferRejection {
    pub deploy_id: ByteString,
    pub pre_state_root: Option<ByteString>,
    pub block_number: i64,
    pub reason: String,
}

struct Entries {
    capacity: usize,
    reason_bytes: usize,
    order: VecDeque<ByteString>,
    entries: HashMap<ByteString, OfferRejection>,
}

impl Entries {
    fn evict_to_capacity(&mut self) {
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

#[derive(Clone)]
pub struct OfferRejectionLog(Arc<parking_lot::Mutex<Entries>>);

impl Default for OfferRejectionLog {
    fn default() -> Self {
        Self::with_bounds(
            OFFER_REJECTION_LOG_CAPACITY_DEFAULT,
            OFFER_REJECTION_REASON_BYTES_DEFAULT,
        )
    }
}

impl OfferRejectionLog {
    pub fn with_bounds(capacity: usize, reason_bytes: usize) -> Self {
        Self(Arc::new(parking_lot::Mutex::new(Entries {
            capacity,
            reason_bytes,
            order: VecDeque::new(),
            entries: HashMap::new(),
        })))
    }

    pub fn set_bounds(&self, capacity: usize, reason_bytes: usize) {
        let mut log = self.0.lock();
        log.capacity = capacity;
        log.reason_bytes = reason_bytes;
        log.evict_to_capacity();
    }

    pub fn record(&self, mut rejection: OfferRejection) {
        let mut log = self.0.lock();
        if log.capacity == 0 {
            return;
        }
        if rejection.reason.len() > log.reason_bytes {
            let mut end = log.reason_bytes;
            while !rejection.reason.is_char_boundary(end) {
                end -= 1;
            }
            rejection.reason.truncate(end);
        }
        let deploy_id = rejection.deploy_id.clone();
        if log.entries.insert(deploy_id.clone(), rejection).is_none() {
            log.order.push_back(deploy_id);
        }
        log.evict_to_capacity();
    }

    pub fn get(&self, deploy_id: &[u8]) -> Option<OfferRejection> {
        self.0.lock().entries.get(deploy_id).cloned()
    }

    pub fn clear(&self, deploy_id: &[u8]) {
        let mut log = self.0.lock();
        if log.entries.remove(deploy_id).is_some() {
            log.order
                .retain(|recorded| recorded.as_slice() != deploy_id);
        }
    }

    pub fn len(&self) -> usize { self.0.lock().entries.len() }

    pub fn is_empty(&self) -> bool { self.len() == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejection(deploy_id: ByteString, reason: &str) -> OfferRejection {
        OfferRejection {
            deploy_id,
            pre_state_root: Some(vec![9; 32]),
            block_number: 7,
            reason: reason.to_string(),
        }
    }

    fn id_at(index: u64) -> ByteString { index.to_be_bytes().to_vec() }

    #[test]
    fn the_rejection_log_is_bounded_and_truncates_reasons() {
        let log = OfferRejectionLog::with_bounds(16, 8);
        for index in 0..26 {
            log.record(rejection(id_at(index), "x"));
        }
        assert_eq!(log.len(), 16);
        assert_eq!(log.get(&id_at(9)), None);
        assert!(log.get(&id_at(10)).is_some());
        assert!(log.get(&id_at(25)).is_some());
        let long = "\u{e9}".repeat(8);
        log.record(rejection(id_at(u64::MAX), &long));
        let recorded = log.get(&id_at(u64::MAX)).unwrap().reason;
        assert!(recorded.len() <= 8);
        assert!(long.starts_with(&recorded));
        assert_eq!(log.len(), 16);
    }

    #[test]
    fn a_second_decision_replaces_the_first_and_clear_removes_it() {
        let log = OfferRejectionLog::default();
        log.record(rejection(id_at(1), "first"));
        log.record(rejection(id_at(1), "second"));
        assert_eq!(log.len(), 1);
        assert_eq!(log.get(&id_at(1)).unwrap().reason, "second");
        log.clear(&id_at(1));
        assert!(log.is_empty());
        log.record(rejection(id_at(2), "x"));
        log.clear(&id_at(1));
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn shrinking_the_bounds_evicts_the_oldest_and_zero_turns_the_log_off() {
        let log = OfferRejectionLog::default();
        for index in 0..10 {
            log.record(rejection(id_at(index), "x"));
        }
        log.set_bounds(4, 8);
        assert_eq!(log.len(), 4);
        assert!(log.get(&id_at(5)).is_none());
        assert!(log.get(&id_at(6)).is_some());
        log.set_bounds(0, 8);
        assert!(log.is_empty());
        log.record(rejection(id_at(11), "x"));
        assert!(log.is_empty());
    }
}
