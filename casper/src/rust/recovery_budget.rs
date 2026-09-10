use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryRegistration {
    Inserted,
    Present,
    Capacity,
}

#[derive(Debug)]
pub(crate) struct RecoveryDispatch<K> {
    key: K,
    identity: Arc<RecoveryDispatchIdentity>,
    attempt: u32,
}

impl<K> RecoveryDispatch<K> {
    pub(crate) fn key(&self) -> &K { &self.key }

    pub(crate) fn attempt(&self) -> u32 { self.attempt }
}

#[derive(Debug)]
struct RecoveryDispatchIdentity;

#[derive(Debug)]
struct RecoveryClaim {
    identity: Weak<RecoveryDispatchIdentity>,
    expires_at: Instant,
}

#[derive(Debug)]
struct RecoveryEntry {
    attempts: u32,
    ready_at: Instant,
    in_flight: Option<RecoveryClaim>,
}

#[derive(Debug)]
pub(crate) struct RecoveryWindow<K> {
    entries: HashMap<K, RecoveryEntry>,
    order: VecDeque<K>,
    capacity: usize,
    base_delay: Duration,
    max_delay: Duration,
    dispatch_timeout: Duration,
}

impl<K> RecoveryWindow<K>
where K: Clone + Eq + Hash
{
    pub(crate) fn new(
        capacity: usize,
        base_delay: Duration,
        max_delay: Duration,
        dispatch_timeout: Duration,
    ) -> Self {
        assert!(capacity > 0);
        assert!(!base_delay.is_zero());
        assert!(max_delay >= base_delay);
        assert!(!dispatch_timeout.is_zero());
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
            base_delay,
            max_delay,
            dispatch_timeout,
        }
    }

    pub(crate) fn register(&mut self, key: K, now: Instant) -> RecoveryRegistration {
        if self.entries.contains_key(&key) {
            return RecoveryRegistration::Present;
        }
        if self.entries.len() >= self.capacity {
            return RecoveryRegistration::Capacity;
        }
        self.entries.insert(key.clone(), RecoveryEntry {
            attempts: 0,
            ready_at: now,
            in_flight: None,
        });
        self.order.push_back(key);
        RecoveryRegistration::Inserted
    }

    pub(crate) fn take_ready_key(&mut self, key: &K, now: Instant) -> Option<RecoveryDispatch<K>> {
        let max_delay = self.max_delay;
        let dispatch_timeout = self.dispatch_timeout;
        let entry = self.entries.get_mut(key)?;
        Self::reclaim_abandoned(entry, now, max_delay);
        if entry.in_flight.is_some() || now < entry.ready_at {
            return None;
        }
        let identity = Arc::new(RecoveryDispatchIdentity);
        entry.attempts = entry.attempts.saturating_add(1);
        entry.in_flight = Some(RecoveryClaim {
            identity: Arc::downgrade(&identity),
            expires_at: now + dispatch_timeout,
        });
        Some(RecoveryDispatch {
            key: key.clone(),
            identity,
            attempt: entry.attempts,
        })
    }

    pub(crate) fn take_ready_batch(
        &mut self,
        now: Instant,
        limit: usize,
    ) -> Vec<RecoveryDispatch<K>> {
        let scan = self.order.len();
        let mut ready = Vec::with_capacity(limit.min(scan));
        for _ in 0..scan {
            if ready.len() == limit {
                break;
            }
            let Some(key) = self.order.pop_front() else {
                break;
            };
            self.order.push_back(key.clone());
            if let Some(dispatch) = self.take_ready_key(&key, now) {
                ready.push(dispatch);
            }
        }
        ready
    }

    pub(crate) fn finish_dispatch(&mut self, dispatch: RecoveryDispatch<K>, now: Instant) -> bool {
        let base_delay = self.base_delay;
        let max_delay = self.max_delay;
        let Some(entry) = self.entries.get_mut(dispatch.key()) else {
            return false;
        };
        if !Self::claim_matches(entry, &dispatch) {
            return false;
        }
        entry.in_flight = None;
        entry.ready_at = now + retry_delay(base_delay, max_delay, entry.attempts);
        true
    }

    pub(crate) fn expire_dispatch(&mut self, dispatch: RecoveryDispatch<K>, now: Instant) -> bool {
        let max_delay = self.max_delay;
        let Some(entry) = self.entries.get_mut(dispatch.key()) else {
            return false;
        };
        if !Self::claim_matches(entry, &dispatch) {
            return false;
        }
        entry.in_flight = None;
        entry.ready_at = now + max_delay;
        true
    }

    pub(crate) fn record_progress(&mut self, key: &K, now: Instant) -> bool {
        let Some(entry) = self.entries.get_mut(key) else {
            return false;
        };
        entry.attempts = 0;
        entry.ready_at = now;
        entry.in_flight = None;
        true
    }

    pub(crate) fn defer_to_maximum(&mut self, key: &K, now: Instant) -> bool {
        let Some(entry) = self.entries.get_mut(key) else {
            return false;
        };
        entry.attempts = u32::MAX;
        entry.ready_at = now + self.max_delay;
        entry.in_flight = None;
        true
    }

    pub(crate) fn resolve(&mut self, key: &K) -> bool {
        let removed = self.entries.remove(key).is_some();
        if removed {
            self.order.retain(|candidate| candidate != key);
        }
        removed
    }

    pub(crate) fn retain_active(&mut self, active: &HashSet<K>) {
        self.entries.retain(|key, _| active.contains(key));
        self.order.retain(|key| active.contains(key));
    }

    pub(crate) fn contains(&self, key: &K) -> bool { self.entries.contains_key(key) }

    pub(crate) fn len(&self) -> usize { self.entries.len() }

    fn claim_matches(entry: &RecoveryEntry, dispatch: &RecoveryDispatch<K>) -> bool {
        entry
            .in_flight
            .as_ref()
            .and_then(|claim| claim.identity.upgrade())
            .is_some_and(|active| Arc::ptr_eq(&active, &dispatch.identity))
    }

    fn reclaim_abandoned(entry: &mut RecoveryEntry, now: Instant, max_delay: Duration) {
        let Some(claim) = entry.in_flight.as_ref() else {
            return;
        };
        if claim.identity.upgrade().is_some() || now < claim.expires_at {
            return;
        }
        entry.ready_at = claim.expires_at + max_delay;
        entry.in_flight = None;
    }

    #[cfg(test)]
    pub(crate) fn attempts(&self, key: &K) -> Option<u32> {
        self.entries.get(key).map(|entry| entry.attempts)
    }

    #[cfg(test)]
    pub(crate) fn make_ready(&mut self, key: &K, now: Instant) -> bool {
        let Some(entry) = self.entries.get_mut(key) else {
            return false;
        };
        entry.ready_at = now;
        entry.in_flight = None;
        true
    }
}

fn retry_delay(base_delay: Duration, max_delay: Duration, attempts: u32) -> Duration {
    let mut delay = base_delay.min(max_delay);
    for _ in 0..attempts.saturating_sub(1).min(128) {
        delay = delay.saturating_mul(2).min(max_delay);
        if delay == max_delay {
            break;
        }
    }
    delay
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn capacity_and_duplicate_registration_are_independent() {
        let now = Instant::now();
        let mut window = RecoveryWindow::new(
            2,
            Duration::from_millis(1),
            Duration::from_secs(1),
            Duration::from_millis(10),
        );
        assert_eq!(window.register(1, now), RecoveryRegistration::Inserted);
        assert_eq!(window.register(1, now), RecoveryRegistration::Present);
        assert_eq!(window.register(2, now), RecoveryRegistration::Inserted);
        assert_eq!(window.register(3, now), RecoveryRegistration::Capacity);
        assert_eq!(window.len(), 2);
    }

    #[test]
    fn stale_dispatch_cannot_change_a_reused_key() {
        let now = Instant::now();
        let mut window = RecoveryWindow::new(
            1,
            Duration::from_millis(1),
            Duration::from_secs(1),
            Duration::from_millis(10),
        );
        window.register(1, now);
        let stale = window.take_ready_key(&1, now).unwrap();
        assert!(window.resolve(&1));
        assert_eq!(window.register(1, now), RecoveryRegistration::Inserted);
        let replacement = window.take_ready_key(&1, now).unwrap();
        assert!(!window.finish_dispatch(stale, now));
        assert_eq!(window.attempts(&1), Some(1));
        assert!(window.finish_dispatch(replacement, now));
    }

    #[test]
    fn batches_rotate_fairly() {
        let now = Instant::now();
        let mut window = RecoveryWindow::new(
            3,
            Duration::from_millis(1),
            Duration::from_secs(1),
            Duration::from_millis(10),
        );
        for key in 1..=3 {
            window.register(key, now);
        }
        let first = window.take_ready_batch(now, 2);
        assert_eq!(
            first.iter().map(|item| *item.key()).collect::<Vec<_>>(),
            vec![1, 2]
        );
        for item in first {
            let key = *item.key();
            assert!(window.finish_dispatch(item, now));
            window.make_ready(&key, now);
        }
        let second = window.take_ready_batch(now, 2);
        assert_eq!(
            second.iter().map(|item| *item.key()).collect::<Vec<_>>(),
            vec![3, 1]
        );
    }

    #[test]
    fn abandoned_dispatch_waits_for_lease_and_maximum_backoff() {
        let now = Instant::now();
        let timeout = Duration::from_millis(10);
        let maximum = Duration::from_millis(20);
        let mut window = RecoveryWindow::new(1, Duration::from_millis(1), maximum, timeout);
        window.register(1, now);
        drop(window.take_ready_key(&1, now).unwrap());

        assert!(window
            .take_ready_key(&1, now + Duration::from_millis(9))
            .is_none());
        assert!(window.take_ready_key(&1, now + timeout).is_none());
        assert!(window.take_ready_key(&1, now + timeout + maximum).is_some());
    }

    #[test]
    fn explicit_timeout_applies_maximum_backoff() {
        let now = Instant::now();
        let maximum = Duration::from_millis(20);
        let mut window = RecoveryWindow::new(
            1,
            Duration::from_millis(1),
            maximum,
            Duration::from_millis(10),
        );
        window.register(1, now);
        let dispatch = window.take_ready_key(&1, now).unwrap();
        assert!(window.expire_dispatch(dispatch, now));
        assert!(window
            .take_ready_key(&1, now + Duration::from_millis(19))
            .is_none());
        assert!(window.take_ready_key(&1, now + maximum).is_some());
    }

    #[test]
    fn exponential_backoff_reaches_large_configured_maximum() {
        let maximum = Duration::from_secs(60 * 60);
        assert_eq!(retry_delay(Duration::from_nanos(1), maximum, 64), maximum);
    }

    #[test]
    fn repeated_reuse_has_no_identity_exhaustion() {
        let now = Instant::now();
        let mut window = RecoveryWindow::new(
            1,
            Duration::from_millis(1),
            Duration::from_secs(1),
            Duration::from_millis(10),
        );
        window.register(1, now);
        for _ in 0..100_000 {
            let dispatch = window.take_ready_key(&1, now).unwrap();
            assert!(window.finish_dispatch(dispatch, now));
            assert!(window.make_ready(&1, now));
        }
    }

    #[test]
    fn retained_stale_dispatches_cannot_mutate_later_generations() {
        let now = Instant::now();
        let mut window = RecoveryWindow::new(
            1,
            Duration::from_millis(1),
            Duration::from_secs(1),
            Duration::from_millis(10),
        );
        let mut stale = Vec::new();
        for _ in 0..2_048 {
            window.register(1, now);
            stale.push(window.take_ready_key(&1, now).unwrap());
            assert!(window.resolve(&1));
        }
        window.register(1, now);
        let current = window.take_ready_key(&1, now).unwrap();
        for dispatch in stale.into_iter().rev() {
            assert!(!window.finish_dispatch(dispatch, now));
            assert_eq!(window.attempts(&1), Some(1));
        }
        assert!(window.finish_dispatch(current, now));
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ModelEntry {
        attempts: u32,
        ready_at: Instant,
        claim: Option<(u64, Instant)>,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ModelDispatch {
        key: u8,
        identity: u64,
        attempt: u32,
    }

    struct ModelWindow {
        entries: HashMap<u8, ModelEntry>,
        order: VecDeque<u8>,
        capacity: usize,
        base_delay: Duration,
        max_delay: Duration,
        dispatch_timeout: Duration,
        next_identity: u64,
    }

    impl ModelWindow {
        fn new(
            capacity: usize,
            base_delay: Duration,
            max_delay: Duration,
            dispatch_timeout: Duration,
        ) -> Self {
            Self {
                entries: HashMap::new(),
                order: VecDeque::new(),
                capacity,
                base_delay,
                max_delay,
                dispatch_timeout,
                next_identity: 0,
            }
        }

        fn register(&mut self, key: u8, now: Instant) -> RecoveryRegistration {
            if self.entries.contains_key(&key) {
                return RecoveryRegistration::Present;
            }
            if self.entries.len() >= self.capacity {
                return RecoveryRegistration::Capacity;
            }
            self.entries.insert(key, ModelEntry {
                attempts: 0,
                ready_at: now,
                claim: None,
            });
            self.order.push_back(key);
            RecoveryRegistration::Inserted
        }

        fn take_ready_key(
            &mut self,
            key: u8,
            now: Instant,
            live: &HashSet<u64>,
        ) -> Option<ModelDispatch> {
            let entry = self.entries.get_mut(&key)?;
            if let Some((identity, expires_at)) = entry.claim {
                if !live.contains(&identity) && now >= expires_at {
                    entry.ready_at = expires_at + self.max_delay;
                    entry.claim = None;
                }
            }
            if entry.claim.is_some() || now < entry.ready_at {
                return None;
            }
            let identity = self.next_identity;
            self.next_identity = self.next_identity.checked_add(1).unwrap();
            entry.attempts = entry.attempts.saturating_add(1);
            entry.claim = Some((identity, now + self.dispatch_timeout));
            Some(ModelDispatch {
                key,
                identity,
                attempt: entry.attempts,
            })
        }

        fn take_ready_batch(
            &mut self,
            now: Instant,
            limit: usize,
            live: &HashSet<u64>,
        ) -> Vec<ModelDispatch> {
            let scan = self.order.len();
            let mut ready = Vec::with_capacity(limit.min(scan));
            for _ in 0..scan {
                if ready.len() == limit {
                    break;
                }
                let Some(key) = self.order.pop_front() else {
                    break;
                };
                self.order.push_back(key);
                if let Some(dispatch) = self.take_ready_key(key, now, live) {
                    ready.push(dispatch);
                }
            }
            ready
        }

        fn finish_dispatch(&mut self, dispatch: ModelDispatch, now: Instant) -> bool {
            let Some(entry) = self.entries.get_mut(&dispatch.key) else {
                return false;
            };
            if entry.claim.map(|claim| claim.0) != Some(dispatch.identity) {
                return false;
            }
            entry.claim = None;
            entry.ready_at = now + retry_delay(self.base_delay, self.max_delay, entry.attempts);
            true
        }

        fn expire_dispatch(&mut self, dispatch: ModelDispatch, now: Instant) -> bool {
            let Some(entry) = self.entries.get_mut(&dispatch.key) else {
                return false;
            };
            if entry.claim.map(|claim| claim.0) != Some(dispatch.identity) {
                return false;
            }
            entry.claim = None;
            entry.ready_at = now + self.max_delay;
            true
        }

        fn record_progress(&mut self, key: u8, now: Instant) -> bool {
            let Some(entry) = self.entries.get_mut(&key) else {
                return false;
            };
            entry.attempts = 0;
            entry.ready_at = now;
            entry.claim = None;
            true
        }

        fn defer_to_maximum(&mut self, key: u8, now: Instant) -> bool {
            let Some(entry) = self.entries.get_mut(&key) else {
                return false;
            };
            entry.attempts = u32::MAX;
            entry.ready_at = now + self.max_delay;
            entry.claim = None;
            true
        }

        fn resolve(&mut self, key: u8) -> bool {
            let removed = self.entries.remove(&key).is_some();
            if removed {
                self.order.retain(|candidate| *candidate != key);
            }
            removed
        }

        fn retain_active(&mut self, active: &HashSet<u8>) {
            self.entries.retain(|key, _| active.contains(key));
            self.order.retain(|key| active.contains(key));
        }
    }

    struct TrackedDispatch {
        actual: RecoveryDispatch<u8>,
        model: ModelDispatch,
    }

    fn live_model_identities(handles: &[TrackedDispatch]) -> HashSet<u64> {
        handles.iter().map(|handle| handle.model.identity).collect()
    }

    fn assert_window_equivalent(
        window: &RecoveryWindow<u8>,
        model: &ModelWindow,
        handles: &[TrackedDispatch],
    ) -> Result<(), TestCaseError> {
        prop_assert!(window.entries.len() <= window.capacity);
        prop_assert_eq!(&window.order, &model.order);
        prop_assert_eq!(window.entries.len(), model.entries.len());
        prop_assert_eq!(window.capacity, model.capacity);

        let mut live = Vec::new();
        let live_model = live_model_identities(handles);
        for (key, entry) in &window.entries {
            let expected = model.entries.get(key).unwrap();
            prop_assert_eq!(entry.attempts, expected.attempts);
            prop_assert_eq!(entry.ready_at, expected.ready_at);
            prop_assert_eq!(entry.in_flight.is_some(), expected.claim.is_some());
            if let Some(claim) = entry.in_flight.as_ref() {
                prop_assert!(entry.attempts > 0);
                let expected_identity = expected.claim.unwrap().0;
                prop_assert_eq!(
                    claim.identity.upgrade().is_some(),
                    live_model.contains(&expected_identity)
                );
                if let Some(identity) = claim.identity.upgrade() {
                    prop_assert_eq!(
                        handles
                            .iter()
                            .filter(|handle| Arc::ptr_eq(&handle.actual.identity, &identity))
                            .count(),
                        1
                    );
                    prop_assert!(live.iter().all(|other| !Arc::ptr_eq(other, &identity)));
                    live.push(identity);
                }
            }
        }
        Ok(())
    }

    proptest! {
        #[test]
        fn retry_delay_is_monotonic_and_bounded(
            base_micros in 1u64..1_000_000,
            maximum_factor in 1u32..1_000_000,
            attempts in 1u32..128,
        ) {
            let base = Duration::from_micros(base_micros);
            let maximum = base.saturating_mul(maximum_factor);
            let delay = retry_delay(base, maximum, attempts);
            let next = retry_delay(base, maximum, attempts.saturating_add(1));
            prop_assert!(delay >= base);
            prop_assert!(delay <= maximum);
            prop_assert!(next >= delay);
            prop_assert!(next <= maximum);
        }

        #[test]
        fn arbitrary_operations_preserve_recovery_window_invariants(
            operations in prop::collection::vec((0u8..11, 0u8..32, any::<u16>()), 0..256)
        ) {
            let mut now = Instant::now();
            let mut window = RecoveryWindow::new(
                8,
                Duration::from_millis(1),
                Duration::from_millis(32),
                Duration::from_millis(10),
            );
            let mut model = ModelWindow::new(
                8,
                Duration::from_millis(1),
                Duration::from_millis(32),
                Duration::from_millis(10),
            );
            let mut handles = Vec::new();
            for (operation, key, value) in operations {
                match operation {
                    0 => {
                        prop_assert_eq!(window.register(key, now), model.register(key, now));
                    }
                    1 => {
                        let live = live_model_identities(&handles);
                        let actual = window.take_ready_key(&key, now);
                        let expected = model.take_ready_key(key, now, &live);
                        prop_assert_eq!(actual.is_some(), expected.is_some());
                        if let (Some(actual), Some(model)) = (actual, expected) {
                            prop_assert_eq!(*actual.key(), model.key);
                            prop_assert_eq!(actual.attempt(), model.attempt);
                            handles.push(TrackedDispatch { actual, model });
                        }
                    }
                    2 | 3 if !handles.is_empty() => {
                        let index = usize::from(value) % handles.len();
                        let dispatch = handles.swap_remove(index);
                        let (actual, expected) = if operation == 2 {
                            (
                                window.finish_dispatch(dispatch.actual, now),
                                model.finish_dispatch(dispatch.model, now),
                            )
                        } else {
                            (
                                window.expire_dispatch(dispatch.actual, now),
                                model.expire_dispatch(dispatch.model, now),
                            )
                        };
                        prop_assert_eq!(actual, expected);
                    }
                    4 => {
                        prop_assert_eq!(window.resolve(&key), model.resolve(key));
                    }
                    5 => {
                        prop_assert_eq!(window.record_progress(&key, now), model.record_progress(key, now));
                    }
                    6 => {
                        prop_assert_eq!(window.defer_to_maximum(&key, now), model.defer_to_maximum(key, now));
                    }
                    7 if !handles.is_empty() => {
                        let index = usize::from(value) % handles.len();
                        drop(handles.swap_remove(index));
                    }
                    8 => {
                        let limit = usize::from(value % 5);
                        let live = live_model_identities(&handles);
                        let actual = window.take_ready_batch(now, limit);
                        let expected = model.take_ready_batch(now, limit, &live);
                        prop_assert_eq!(actual.len(), expected.len());
                        for (actual, model) in actual.into_iter().zip(expected) {
                            prop_assert_eq!(*actual.key(), model.key);
                            prop_assert_eq!(actual.attempt(), model.attempt);
                            handles.push(TrackedDispatch { actual, model });
                        }
                    }
                    9 => {
                        now += Duration::from_millis(u64::from(value % 65));
                    }
                    10 => {
                        let active = window
                            .entries
                            .keys()
                            .filter(|candidate| u16::from(**candidate) <= value % 32)
                            .copied()
                            .collect::<HashSet<_>>();
                        window.retain_active(&active);
                        model.retain_active(&active);
                    }
                    _ => {}
                }
                assert_window_equivalent(&window, &model, &handles)?;
            }
        }
    }
}
