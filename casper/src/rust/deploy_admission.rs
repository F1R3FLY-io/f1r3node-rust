use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;

use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use parking_lot::Mutex;

pub const DEPLOY_QUEUE_BLOCKS: usize = 3;
const SERVICE_SAMPLE_BLOCKS: usize = 8;
const INITIAL_DEPLOYS_PER_BLOCK: usize = 8;

#[derive(Default)]
struct ServiceSamples {
    inclusions: VecDeque<usize>,
    last_sequence: Option<i32>,
}

#[derive(Default)]
pub struct DeployAdmission {
    samples: parking_lot::Mutex<ServiceSamples>,
    pub(crate) reservations: parking_lot::Mutex<ReservationCache>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdmissionRevision {
    pub pool: u64,
    pub rejected: u64,
    pub dag: u64,
    pub lifecycle: Option<(usize, usize)>,
}

#[derive(Default)]
pub(crate) struct ReservationCache {
    pub revision: Option<AdmissionRevision>,
    pub book: Option<Arc<AdmissionReservations>>,
    pub window: Option<FreshWindow>,
}

impl DeployAdmission {
    pub(crate) fn observe(&self, sequence: i32, included: usize, had_demand: bool) {
        let mut samples = self.samples.lock();
        if samples.last_sequence.is_some_and(|last| sequence <= last) {
            return;
        }
        samples.last_sequence = Some(sequence);
        if !had_demand {
            return;
        }
        if samples.inclusions.len() == SERVICE_SAMPLE_BLOCKS {
            samples.inclusions.pop_front();
        }
        samples.inclusions.push_back(included);
        metrics::histogram!(
            crate::rust::metrics_constants::DEPLOY_ADMISSION_INCLUDED_METRIC,
            "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
        )
        .record(included as f64);
    }

    pub(crate) fn capacity(&self, shard_cap: usize) -> usize {
        if shard_cap == 0 {
            return 0;
        }
        let samples = self.samples.lock();
        if samples.inclusions.is_empty() {
            return INITIAL_DEPLOYS_PER_BLOCK
                .min(shard_cap)
                .saturating_mul(DEPLOY_QUEUE_BLOCKS);
        }
        let total: u128 = samples.inclusions.iter().map(|count| *count as u128).sum();
        (total * DEPLOY_QUEUE_BLOCKS as u128 / samples.inclusions.len() as u128)
            .min(shard_cap.saturating_mul(DEPLOY_QUEUE_BLOCKS) as u128) as usize
    }
}

pub(crate) fn compare_deploys(a: &Signed<DeployData>, b: &Signed<DeployData>) -> Ordering {
    a.data
        .valid_after_block_number
        .cmp(&b.data.valid_after_block_number)
        .then_with(|| a.data.time_stamp.cmp(&b.data.time_stamp))
        .then_with(|| a.sig.cmp(&b.sig))
}

pub type DeployId = Vec<u8>;

pub use block_storage::rust::deploy::key_value_deploy_storage::DeployMetadata as PendingDeploy;

#[derive(Clone, Copy, Debug)]
pub struct FreshWindow {
    pub earliest_valid_after: i64,
    pub latest_valid_after: i64,
    pub now_millis: i64,
}

impl FreshWindow {
    pub(crate) fn contains(self, deploy: &PendingDeploy) -> bool {
        deploy.valid_after_block_number > self.earliest_valid_after
            && deploy.valid_after_block_number <= self.latest_valid_after
            && deploy
                .expiration_timestamp
                .is_none_or(|expires| self.now_millis <= expires)
    }
}

enum Marker {
    Reserved { excluded: bool },
    Pending,
}

struct Entry {
    deploy: PendingDeploy,
    marker: Marker,
}

struct State {
    entries: HashMap<DeployId, Entry>,
    valid: bool,
}

pub struct AdmissionReservations {
    state: Mutex<State>,
    occupied: AtomicUsize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReserveError {
    Duplicate,
    NotFresh,
    Full { occupied: usize, capacity: usize },
    NeedsRefresh,
    CounterOverflow,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Exclusion {
    Released,
    DeferredUntilWriteCompletes,
    NotTracked,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommitResult {
    Pending,
    Excluded,
}

#[must_use]
pub struct Reservation {
    book: Arc<AdmissionReservations>,
    signature: DeployId,
    completed: bool,
}

impl AdmissionReservations {
    pub fn from_fresh(
        deploys: impl IntoIterator<Item = PendingDeploy>,
        window: FreshWindow,
    ) -> Result<Arc<Self>, ReserveError> {
        let mut entries = HashMap::new();
        for deploy in deploys {
            if !window.contains(&deploy) {
                continue;
            }
            if entries.contains_key(&deploy.signature) {
                return Err(ReserveError::Duplicate);
            }
            entries.insert(deploy.signature.clone(), Entry {
                deploy,
                marker: Marker::Pending,
            });
        }
        Ok(Arc::new(Self {
            occupied: AtomicUsize::new(entries.len()),
            state: Mutex::new(State {
                entries,
                valid: true,
            }),
        }))
    }

    pub fn refresh_window(&self, window: FreshWindow) {
        self.expire_entries(&mut self.state.lock(), window);
    }

    pub fn pending_ahead(&self, candidate: &PendingDeploy) -> usize {
        self.state
            .lock()
            .entries
            .values()
            .filter(|entry| {
                let deploy = &entry.deploy;
                (
                    deploy.valid_after_block_number,
                    deploy.timestamp,
                    &deploy.signature,
                ) < (
                    candidate.valid_after_block_number,
                    candidate.timestamp,
                    &candidate.signature,
                )
            })
            .count()
    }

    pub fn occupied(&self) -> usize { self.occupied.load(AtomicOrdering::Acquire) }

    pub fn try_reserve(
        self: &Arc<Self>,
        deploy: PendingDeploy,
        capacity: usize,
        window: FreshWindow,
    ) -> Result<Reservation, ReserveError> {
        let mut state = self.state.lock();
        if !state.valid {
            return Err(ReserveError::NeedsRefresh);
        }
        if state.entries.contains_key(&deploy.signature) {
            return Err(ReserveError::Duplicate);
        }
        if !window.contains(&deploy) {
            return Err(ReserveError::NotFresh);
        }
        self.expire_entries(&mut state, window);
        let occupied = self.occupied();
        if occupied >= capacity {
            return Err(ReserveError::Full { occupied, capacity });
        }
        self.occupied.store(occupied + 1, AtomicOrdering::Release);
        let signature = deploy.signature.clone();
        state.entries.insert(signature.clone(), Entry {
            deploy,
            marker: Marker::Reserved { excluded: false },
        });
        Ok(Reservation {
            book: self.clone(),
            signature,
            completed: false,
        })
    }

    pub fn exclude(&self, signature: &[u8]) -> Exclusion {
        let mut state = self.state.lock();
        match state
            .entries
            .get_mut(signature)
            .map(|entry| &mut entry.marker)
        {
            Some(Marker::Reserved { excluded, .. }) => {
                *excluded = true;
                Exclusion::DeferredUntilWriteCompletes
            }
            Some(Marker::Pending) => {
                state.entries.remove(signature);
                self.release(1);
                Exclusion::Released
            }
            None => Exclusion::NotTracked,
        }
    }

    pub fn restore_accepted(
        &self,
        deploy: PendingDeploy,
        window: FreshWindow,
    ) -> Result<bool, ReserveError> {
        let mut state = self.state.lock();
        if !state.valid {
            return Err(ReserveError::NeedsRefresh);
        }
        if !window.contains(&deploy) {
            return Ok(false);
        }
        if let Some(entry) = state.entries.get_mut(&deploy.signature) {
            if let Marker::Reserved { excluded, .. } = &mut entry.marker {
                *excluded = false;
            }
            return Ok(false);
        }
        let count = self
            .occupied()
            .checked_add(1)
            .ok_or(ReserveError::CounterOverflow)?;
        self.occupied.store(count, AtomicOrdering::Release);
        state.entries.insert(deploy.signature.clone(), Entry {
            deploy,
            marker: Marker::Pending,
        });
        Ok(true)
    }

    #[cfg(test)]
    pub fn invalidate(&self) { self.state.lock().valid = false; }

    fn expire_entries(&self, state: &mut State, window: FreshWindow) {
        let mut removed = 0;
        state.entries.retain(|_, entry| {
            if window.contains(&entry.deploy) {
                return true;
            }
            match &mut entry.marker {
                Marker::Reserved { excluded, .. } => {
                    *excluded = true;
                    true
                }
                Marker::Pending => {
                    removed += 1;
                    false
                }
            }
        });
        self.release(removed);
    }

    fn release(&self, count: usize) {
        if count != 0 {
            let count = self
                .occupied()
                .checked_sub(count)
                .expect("Admission reservation count is inconsistent");
            self.occupied.store(count, AtomicOrdering::Release);
        }
    }
}

impl Reservation {
    pub fn commit(mut self, window: FreshWindow) -> CommitResult {
        let result = {
            let mut state = self.book.state.lock();
            let entry = state
                .entries
                .get_mut(&self.signature)
                .expect("Reservation marker is missing");
            let excluded = match entry.marker {
                Marker::Reserved { excluded } => excluded,
                _ => panic!("Reservation marker is not reserved"),
            };
            if excluded || !window.contains(&entry.deploy) {
                state.entries.remove(&self.signature);
                self.book.release(1);
                CommitResult::Excluded
            } else {
                entry.marker = Marker::Pending;
                CommitResult::Pending
            }
        };
        self.completed = true;
        result
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let mut state = self.book.state.lock();
        let owned = matches!(
            state
                .entries
                .get(&self.signature)
                .map(|entry| &entry.marker),
            Some(Marker::Reserved { .. })
        );
        if owned {
            state.entries.remove(&self.signature);
            self.book.release(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_order_uses_height_then_timestamp_then_signature() {
        let deploy = |height, timestamp, term: &str| {
            crate::rust::util::construct_deploy::source_deploy(
                term.to_string(),
                timestamp,
                None,
                None,
                None,
                Some(height),
                Some("test".to_string()),
            )
            .unwrap()
        };
        let older_height = deploy(1, 100, "Nil");
        let newer_height = deploy(2, 1, "Nil");
        assert!(compare_deploys(&older_height, &newer_height).is_lt());
        let older_time = deploy(1, 99, "Nil");
        assert!(compare_deploys(&older_time, &older_height).is_lt());
        let same_time = deploy(1, 100, "@0!(0)");
        assert_eq!(
            compare_deploys(&older_height, &same_time),
            older_height.sig.cmp(&same_time.sig)
        );
        assert_eq!(
            compare_deploys(&older_height, &older_height),
            Ordering::Equal
        );
    }

    #[test]
    fn eight_inclusions_allow_twenty_four_positions() {
        let admission = DeployAdmission::default();
        admission.observe(1, 8, true);
        let capacity = admission.capacity(128);
        assert!(24 <= capacity);
        assert!(25 > capacity);
    }

    #[test]
    fn cold_start_and_shard_cap_bound_capacity() {
        let admission = DeployAdmission::default();
        assert_eq!(admission.capacity(128), 24);
        assert_eq!(admission.capacity(4), 12);
        assert_eq!(admission.capacity(0), 0);
        admission.observe(1, 128, true);
        assert_eq!(admission.capacity(4), 12);
    }

    #[test]
    fn idle_and_repeated_blocks_do_not_change_the_estimate() {
        let admission = DeployAdmission::default();
        admission.observe(1, 8, true);
        admission.observe(2, 0, false);
        admission.observe(2, 100, true);
        admission.observe(1, 100, true);
        assert_eq!(admission.capacity(128), 24);
        admission.observe(3, 0, true);
        assert_eq!(admission.capacity(128), 12);
    }

    #[test]
    fn recent_samples_replace_old_capacity_without_implicitly_enabling_a_probe() {
        let admission = DeployAdmission::default();
        admission.observe(0, 8, true);
        for sequence in 1..=8 {
            admission.observe(sequence, 0, true);
        }
        assert_eq!(admission.capacity(128), 0);
        assert_eq!(admission.capacity(0), 0);
        admission.observe(9, 8, true);
        assert_eq!(admission.capacity(128), 3);
    }

    #[test]
    fn fractional_service_uses_the_whole_three_block_budget() {
        let admission = DeployAdmission::default();
        admission.observe(1, 1, true);
        admission.observe(2, 0, true);
        assert_eq!(admission.capacity(128), 1);
    }

    #[test]
    fn missing_blocks_do_not_create_zero_service_samples() {
        let stalled = DeployAdmission::default();
        let zero_service = DeployAdmission::default();
        stalled.observe(1, 8, true);
        zero_service.observe(1, 8, true);

        for sequence in 2..=9 {
            zero_service.observe(sequence, 0, true);
            assert_eq!(stalled.capacity(128), 24);
        }

        assert_eq!(zero_service.capacity(128), 0);
        assert_eq!(stalled.capacity(128), 24);
    }
}

#[cfg(test)]
mod reservation_tests {
    use std::sync::Barrier;

    use super::*;

    fn window() -> FreshWindow {
        FreshWindow {
            earliest_valid_after: -1,
            latest_valid_after: 1000,
            now_millis: 100,
        }
    }

    fn deploy(id: u8) -> PendingDeploy {
        PendingDeploy {
            signature: vec![id],
            valid_after_block_number: 0,
            timestamp: id.into(),
            expiration_timestamp: None,
        }
    }

    fn book(count: u8) -> Arc<AdmissionReservations> {
        AdmissionReservations::from_fresh((0..count).map(deploy), window()).unwrap()
    }

    #[test]
    fn different_candidates_cannot_claim_the_same_final_slot() {
        let book = book(23);
        let barrier = Arc::new(Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|worker| {
                    let book = book.clone();
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        (0..16)
                            .map(|i| book.try_reserve(deploy(100 + worker * 16 + i), 24, window()))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(
                    r,
                    Err(ReserveError::Full {
                        occupied: 24,
                        capacity: 24
                    })
                ))
                .count(),
            31
        );
        assert_eq!(book.occupied(), 24);
        drop(results);
        assert_eq!(book.occupied(), 23);
    }

    #[test]
    fn duplicate_precedes_a_full_queue_error() {
        let book = book(1);
        assert!(matches!(
            book.try_reserve(deploy(0), 1, window()),
            Err(ReserveError::Duplicate)
        ));
        assert!(matches!(
            book.try_reserve(deploy(1), 1, window()),
            Err(ReserveError::Full { .. })
        ));
    }

    #[test]
    fn write_error_releases_the_uncommitted_reservation() {
        let book = book(0);
        let write = || -> Result<(), &'static str> {
            let _reservation = book.try_reserve(deploy(1), 1, window()).unwrap();
            Err("LMDB write failed")?;
            Ok(())
        };
        assert!(write().is_err());
        assert_eq!(book.occupied(), 0);
        assert!(book.try_reserve(deploy(1), 1, window()).is_ok());
    }

    #[test]
    fn unwinding_releases_the_uncommitted_reservation() {
        let book = book(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _reservation = book.try_reserve(deploy(1), 1, window()).unwrap();
            panic!("Writer stopped");
        }));
        assert!(result.is_err());
        assert_eq!(book.occupied(), 0);
    }

    #[test]
    fn committed_reservation_stays_until_explicit_exclusion() {
        let book = book(0);
        assert_eq!(
            book.try_reserve(deploy(1), 1, window())
                .unwrap()
                .commit(window()),
            CommitResult::Pending
        );
        assert_eq!(book.occupied(), 1);
        assert_eq!(book.exclude(&[1]), Exclusion::Released);
        assert_eq!(book.exclude(&[1]), Exclusion::NotTracked);
        assert_eq!(book.occupied(), 0);
    }

    #[test]
    fn lower_capacity_keeps_old_entries_and_admission_resumes_after_drain() {
        let book = book(23);
        assert!(matches!(
            book.try_reserve(deploy(100), 3, window()),
            Err(ReserveError::Full {
                occupied: 23,
                capacity: 3
            })
        ));
        assert_eq!(book.occupied(), 23);
        for id in 0..21 {
            assert_eq!(book.exclude(&[id]), Exclusion::Released);
        }
        let reserved = book.try_reserve(deploy(100), 3, window()).unwrap();
        assert_eq!(book.occupied(), 3);
        assert!(matches!(
            book.try_reserve(deploy(101), 3, window()),
            Err(ReserveError::Full { .. })
        ));
        assert_eq!(reserved.commit(window()), CommitResult::Pending);
        assert_eq!(book.occupied(), 3);
    }

    #[test]
    fn zero_capacity_does_not_implicitly_enable_the_probe_policy() {
        let book = book(0);
        assert!(matches!(
            book.try_reserve(deploy(1), 0, window()),
            Err(ReserveError::Full {
                occupied: 0,
                capacity: 0
            })
        ));
        let probe = book.try_reserve(deploy(1), 1, window()).unwrap();
        assert!(matches!(
            book.try_reserve(deploy(2), 1, window()),
            Err(ReserveError::Full { .. })
        ));
        drop(probe);
        assert_eq!(book.occupied(), 0);
    }

    #[test]
    fn expiry_matches_the_strict_timestamp_boundary() {
        let mut expiring = deploy(0);
        expiring.expiration_timestamp = Some(100);
        let book = AdmissionReservations::from_fresh([expiring], window()).unwrap();
        assert!(matches!(
            book.try_reserve(deploy(1), 1, window()),
            Err(ReserveError::Full { .. })
        ));
        let later = FreshWindow {
            now_millis: 101,
            ..window()
        };
        let reserved = book.try_reserve(deploy(1), 1, later).unwrap();
        assert_eq!(book.occupied(), 1);
        drop(reserved);
        assert_eq!(book.occupied(), 0);
    }

    #[test]
    fn height_expiry_releases_pending_but_near_future_entries_keep_their_slot() {
        let mut future = deploy(1);
        future.valid_after_block_number = 1000;
        let book = AdmissionReservations::from_fresh([deploy(0), future], window()).unwrap();
        let later = FreshWindow {
            earliest_valid_after: 0,
            ..window()
        };
        let mut candidate = deploy(2);
        candidate.valid_after_block_number = 1;
        let reserved = book.try_reserve(candidate, 2, later).unwrap();
        assert_eq!(book.occupied(), 2);
        assert!(matches!(
            book.try_reserve(deploy(1), 2, later),
            Err(ReserveError::Duplicate)
        ));
        assert_eq!(reserved.commit(later), CommitResult::Pending);
        assert_eq!(book.occupied(), 2);
    }

    #[test]
    fn out_of_window_arrivals_do_not_reserve_a_slot() {
        let book = AdmissionReservations::from_fresh([], window()).unwrap();
        for (height, expires) in [(1001, None), (0, Some(99)), (-1, None)] {
            let mut candidate = deploy(3);
            candidate.valid_after_block_number = height;
            candidate.expiration_timestamp = expires;
            assert!(matches!(
                book.try_reserve(candidate, 1, window()),
                Err(ReserveError::NotFresh)
            ));
            assert_eq!(book.occupied(), 0);
        }
    }

    #[test]
    fn inclusion_during_a_write_does_not_release_the_slot_twice() {
        let book = book(0);
        let reserved = book.try_reserve(deploy(1), 1, window()).unwrap();
        assert_eq!(book.exclude(&[1]), Exclusion::DeferredUntilWriteCompletes);
        assert_eq!(book.occupied(), 1);
        assert_eq!(reserved.commit(window()), CommitResult::Excluded);
        assert_eq!(book.occupied(), 0);
        assert_eq!(book.exclude(&[1]), Exclusion::NotTracked);
    }

    #[test]
    fn restored_accepted_work_is_retained_even_above_the_current_capacity() {
        let book = book(1);
        assert!(book.restore_accepted(deploy(1), window()).unwrap());
        assert!(!book.restore_accepted(deploy(1), window()).unwrap());
        assert_eq!(book.occupied(), 2);
        assert!(matches!(
            book.try_reserve(deploy(2), 1, window()),
            Err(ReserveError::Full {
                occupied: 2,
                capacity: 1
            })
        ));
    }

    #[test]
    fn invalidated_index_refuses_new_reservations() {
        let book = book(0);
        let reserved = book.try_reserve(deploy(1), 2, window()).unwrap();
        book.invalidate();
        assert!(matches!(
            book.try_reserve(deploy(2), 2, window()),
            Err(ReserveError::NeedsRefresh)
        ));
        assert_eq!(reserved.commit(window()), CommitResult::Pending);
        assert_eq!(book.occupied(), 1);
    }

    #[test]
    fn occupied_read_does_not_wait_for_the_marker_lock() {
        let book = book(3);
        let guard = book.state.lock();
        let (send, receive) = std::sync::mpsc::channel();
        let reader = {
            let book = book.clone();
            std::thread::spawn(move || send.send(book.occupied()).unwrap())
        };
        let result = receive.recv_timeout(std::time::Duration::from_secs(1));
        drop(guard);
        reader.join().unwrap();
        assert_eq!(result.unwrap(), 3);
    }

    #[test]
    fn concurrent_duplicate_gets_duplicate_instead_of_a_full_queue_error() {
        let book = book(0);
        let barrier = Arc::new(Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    let book = book.clone();
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        book.try_reserve(deploy(1), 1, window())
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(ReserveError::Duplicate)))
                .count(),
            1
        );
        assert_eq!(book.occupied(), 1);
        drop(results);
        assert_eq!(book.occupied(), 0);
    }

    #[test]
    fn restored_work_cancels_a_deferred_exclusion_before_commit() {
        let book = book(0);
        let reserved = book.try_reserve(deploy(1), 1, window()).unwrap();
        assert_eq!(book.exclude(&[1]), Exclusion::DeferredUntilWriteCompletes);
        assert!(!book.restore_accepted(deploy(1), window()).unwrap());
        assert_eq!(reserved.commit(window()), CommitResult::Pending);
        assert_eq!(book.occupied(), 1);
    }
}
