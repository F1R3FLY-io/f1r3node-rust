use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub struct FinalizationSchedule {
    request_sequence: AtomicU64,
    requested_through: AtomicU64,
    launched_through: AtomicU64,
    completed_through: AtomicU64,
    in_flight: AtomicU64,
    dispatcher_running: AtomicBool,
    retry_ready: AtomicBool,
    consecutive_failures: AtomicU32,
    parked_revision: Mutex<Option<u64>>,
    workers: Arc<Semaphore>,
    worker_limit: usize,
}

impl FinalizationSchedule {
    pub fn new(worker_limit: usize) -> Self {
        assert!(
            worker_limit > 0,
            "finalization worker limit must be at least one"
        );
        Self {
            request_sequence: AtomicU64::new(0),
            requested_through: AtomicU64::new(0),
            launched_through: AtomicU64::new(0),
            completed_through: AtomicU64::new(0),
            in_flight: AtomicU64::new(0),
            dispatcher_running: AtomicBool::new(false),
            retry_ready: AtomicBool::new(false),
            consecutive_failures: AtomicU32::new(0),
            parked_revision: Mutex::new(None),
            workers: Arc::new(Semaphore::new(worker_limit)),
            worker_limit,
        }
    }

    pub fn request(&self) -> Option<u64> {
        let previous = self
            .request_sequence
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_add(1)
            })
            .ok()?;
        let ticket = previous + 1;
        self.requested_through.fetch_max(ticket, Ordering::SeqCst);
        Some(ticket)
    }

    pub fn try_start_dispatcher(&self) -> bool {
        self.dispatcher_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn requested_through(&self) -> u64 { self.requested_through.load(Ordering::SeqCst) }

    pub fn launched_through(&self) -> u64 { self.launched_through.load(Ordering::SeqCst) }

    pub fn next_coverage(&self) -> Option<u64> {
        let requested = self.requested_through();
        if requested > self.launched_through() {
            self.retry_ready.store(false, Ordering::SeqCst);
            return Some(requested);
        }
        if self.retry_ready.swap(false, Ordering::SeqCst)
            && self.completed_through.load(Ordering::SeqCst) < requested
        {
            return Some(requested);
        }
        None
    }

    pub fn mark_launched(&self, covered_through: u64) {
        self.launched_through
            .fetch_max(covered_through, Ordering::SeqCst);
        self.in_flight.fetch_add(1, Ordering::SeqCst);
    }

    pub fn mark_succeeded(&self, covered_through: u64) {
        self.completed_through
            .fetch_max(covered_through, Ordering::SeqCst);
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        self.consecutive_failures.store(0, Ordering::SeqCst);
        if self.completed_through.load(Ordering::SeqCst) >= self.requested_through() {
            self.retry_ready.store(false, Ordering::SeqCst);
        }
    }

    pub fn mark_failed(&self, covered_through: u64) -> Option<Duration> {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        if self.completed_through.load(Ordering::SeqCst) >= covered_through {
            return None;
        }
        let failures = self
            .consecutive_failures
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1)
            .min(8);
        Some(Duration::from_millis(25_u64 << (failures - 1)))
    }

    pub fn make_retry_ready(&self, covered_through: u64) -> bool {
        if self.completed_through.load(Ordering::SeqCst) >= covered_through {
            return false;
        }
        self.retry_ready.store(true, Ordering::SeqCst);
        true
    }

    pub async fn acquire_worker(&self) -> Result<OwnedSemaphorePermit, String> {
        self.workers
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "finalization worker pool is closed".to_string())
    }

    pub fn release_dispatcher_or_reacquire(&self) -> bool {
        self.dispatcher_running.store(false, Ordering::SeqCst);
        (self.requested_through() > self.launched_through()
            || self.retry_ready.load(Ordering::SeqCst))
            && self.try_start_dispatcher()
    }

    pub fn clear_dispatcher(&self) { self.dispatcher_running.store(false, Ordering::SeqCst); }

    pub fn is_quiescent(&self) -> bool {
        !self.dispatcher_running.load(Ordering::SeqCst)
            && self.in_flight.load(Ordering::SeqCst) == 0
            && self.completed_through.load(Ordering::SeqCst) >= self.requested_through()
            && !self.retry_ready.load(Ordering::SeqCst)
    }

    pub fn worker_limit(&self) -> usize { self.worker_limit }

    pub fn park_certificate_carrier(&self, revision: u64) {
        let mut parked = self.parked_revision.lock();
        if parked.is_none_or(|current| current <= revision) {
            *parked = Some(revision);
        }
    }

    pub fn take_parked_certificate_carrier(&self, revision: u64) -> bool {
        let mut parked = self.parked_revision.lock();
        if *parked != Some(revision) {
            return false;
        }
        *parked = None;
        true
    }

    pub fn clear_parked_certificate_carrier(&self, revision: u64) {
        let mut parked = self.parked_revision.lock();
        if *parked == Some(revision) {
            *parked = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_completions_are_monotonic() {
        let schedule = FinalizationSchedule::new(2);
        assert_eq!(schedule.request(), Some(1));
        assert_eq!(schedule.request(), Some(2));
        assert!(schedule.try_start_dispatcher());
        schedule.mark_launched(2);
        schedule.mark_succeeded(2);
        assert!(!schedule.release_dispatcher_or_reacquire());
        assert!(schedule.is_quiescent());
    }

    #[test]
    fn release_reacquires_when_request_was_not_launched() {
        let schedule = FinalizationSchedule::new(2);
        assert_eq!(schedule.request(), Some(1));
        assert!(schedule.try_start_dispatcher());
        assert!(schedule.release_dispatcher_or_reacquire());
    }

    #[test]
    #[should_panic(expected = "finalization worker limit must be at least one")]
    fn zero_workers_fail_closed() { FinalizationSchedule::new(0); }

    #[tokio::test]
    async fn worker_pool_is_bounded_without_serializing_two_workers() {
        let schedule = FinalizationSchedule::new(2);
        let first = schedule.acquire_worker().await.unwrap();
        let second = schedule.acquire_worker().await.unwrap();
        assert_eq!(schedule.workers.available_permits(), 0);
        drop(first);
        assert_eq!(schedule.workers.available_permits(), 1);
        drop(second);
        assert_eq!(schedule.workers.available_permits(), 2);
    }

    #[test]
    fn failed_coverage_is_retried_without_completing_its_ticket() {
        let schedule = FinalizationSchedule::new(2);
        assert_eq!(schedule.request(), Some(1));
        assert_eq!(schedule.next_coverage(), Some(1));
        schedule.mark_launched(1);
        assert_eq!(schedule.mark_failed(1), Some(Duration::from_millis(25)));
        assert!(!schedule.is_quiescent());
        assert!(schedule.make_retry_ready(1));
        assert_eq!(schedule.next_coverage(), Some(1));
        schedule.mark_launched(1);
        schedule.mark_succeeded(1);
        assert!(schedule.is_quiescent());
    }

    #[test]
    fn newer_success_subsumes_an_older_failed_worker() {
        let schedule = FinalizationSchedule::new(2);
        assert_eq!(schedule.request(), Some(1));
        assert_eq!(schedule.next_coverage(), Some(1));
        schedule.mark_launched(1);
        assert_eq!(schedule.request(), Some(2));
        assert_eq!(schedule.next_coverage(), Some(2));
        schedule.mark_launched(2);
        assert_eq!(schedule.mark_failed(1), Some(Duration::from_millis(25)));
        schedule.mark_succeeded(2);
        assert!(!schedule.make_retry_ready(1));
        assert_eq!(schedule.next_coverage(), None);
        assert!(schedule.is_quiescent());
    }

    #[test]
    fn certificate_carrier_parking_is_monotonic_and_wakes_once() {
        let schedule = Arc::new(FinalizationSchedule::new(2));
        schedule.park_certificate_carrier(7);
        schedule.park_certificate_carrier(6);
        let wakes = (0..8)
            .map(|_| {
                let schedule = schedule.clone();
                std::thread::spawn(move || schedule.take_parked_certificate_carrier(7))
            })
            .map(|thread| thread.join().unwrap())
            .filter(|woke| *woke)
            .count();
        assert_eq!(wakes, 1);
        assert!(!schedule.take_parked_certificate_carrier(7));

        schedule.park_certificate_carrier(8);
        schedule.clear_parked_certificate_carrier(7);
        assert!(schedule.take_parked_certificate_carrier(8));
    }
}
