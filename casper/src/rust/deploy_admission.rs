use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
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
    resources: VecDeque<LocalServiceSample>,
    last_sequence: Option<i32>,
    recovery_budget: usize,
}

impl ServiceSamples {
    fn queue_budget(&self) -> usize {
        let sampled_budget = if self.inclusions.is_empty() {
            INITIAL_DEPLOYS_PER_BLOCK.saturating_mul(DEPLOY_QUEUE_BLOCKS)
        } else {
            let total: u128 = self.inclusions.iter().map(|count| *count as u128).sum();
            (total * DEPLOY_QUEUE_BLOCKS as u128 / self.inclusions.len() as u128)
                .min(usize::MAX as u128) as usize
        };
        sampled_budget.max(self.recovery_budget)
    }

    fn resource_limits(&self, capacity: usize) -> ResourceLimits {
        let byte_ceiling =
            (crate::rust::blocks::proposer::block_creator::USER_DEPLOY_BYTE_PROPOSAL_BUDGET
                as u128)
                .saturating_mul(DEPLOY_QUEUE_BLOCKS as u128);
        let (count, bytes, phlo) =
            self.resources
                .iter()
                .fold((0u128, 0u128, 0u128), |(count, bytes, phlo), sample| {
                    (
                        count.saturating_add(sample.included as u128),
                        bytes.saturating_add(sample.record_bytes),
                        phlo.saturating_add(sample.phlo_limit),
                    )
                });
        if count == 0 {
            return ResourceLimits {
                count: capacity,
                record_bytes: byte_ceiling,
                phlo_limit: None,
            };
        }
        ResourceLimits {
            count: capacity,
            record_bytes: bytes
                .saturating_mul(capacity as u128 + u128::from(capacity != 0))
                .div_ceil(count)
                .min(byte_ceiling),
            phlo_limit: Some(phlo.saturating_mul(capacity as u128).div_ceil(count)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LocalServiceSample {
    pub included: usize,
    pub has_remaining_demand: bool,
    pub record_bytes: u128,
    pub phlo_limit: u128,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResourceLimits {
    pub count: usize,
    pub record_bytes: u128,
    pub phlo_limit: Option<u128>,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    serde::Deserialize,
    serde::Serialize
)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RetainedPoolLimits {
    pub max_records: usize,
    pub max_bytes: u64,
    pub max_inflight: usize,
}

impl From<RetainedPoolLimits> for ResourceLimits {
    fn from(limits: RetainedPoolLimits) -> Self {
        Self {
            count: limits.max_records,
            record_bytes: limits.max_bytes as u128,
            phlo_limit: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ResourceUsage {
    record_bytes: u128,
    phlo_limit: u128,
}

impl ResourceUsage {
    fn add(&mut self, deploy: &PendingDeploy) {
        self.record_bytes += deploy.record_bytes as u128;
        self.phlo_limit += deploy.phlo_limit as u128;
    }

    fn remove(&mut self, deploy: &PendingDeploy) {
        self.record_bytes -= deploy.record_bytes as u128;
        self.phlo_limit -= deploy.phlo_limit as u128;
    }
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
    pub(crate) fn log_pool_busy(&self, capacity: usize, pending_ahead: usize, occupied: usize) {
        if !tracing::enabled!(target: "f1r3fly.casper.deploy_admission", tracing::Level::DEBUG) {
            return;
        }
        let samples = self.samples.lock();
        tracing::debug!(
            target: "f1r3fly.casper.deploy_admission",
            capacity,
            pending_ahead,
            occupied,
            latest_samples = ?samples.inclusions,
            latest_sequence = ?samples.last_sequence,
            recovery_budget = samples.recovery_budget,
            "Deploy pool busy"
        );
    }

    #[cfg(test)]
    pub(crate) fn observe(&self, sequence: i32, included: usize, has_remaining_demand: bool) {
        self.observe_sample(sequence, LocalServiceSample {
            included,
            has_remaining_demand,
            record_bytes: 0,
            phlo_limit: 0,
        });
    }

    pub(crate) fn observe_sample(&self, sequence: i32, sample: LocalServiceSample) {
        let mut samples = self.samples.lock();
        if samples.last_sequence.is_some_and(|last| sequence <= last) {
            return;
        }
        samples.last_sequence = Some(sequence);
        let LocalServiceSample {
            included,
            has_remaining_demand,
            ..
        } = sample;
        if included > 0 && sample.record_bytes > 0 {
            if samples.resources.len() == SERVICE_SAMPLE_BLOCKS {
                samples.resources.pop_front();
            }
            samples.resources.push_back(sample);
        }
        if !has_remaining_demand {
            let budget = samples.queue_budget();
            let supported_budget = included.saturating_mul(DEPLOY_QUEUE_BLOCKS);
            if supported_budget > budget {
                let step = budget.div_ceil(DEPLOY_QUEUE_BLOCKS).max(1);
                samples.recovery_budget = supported_budget.min(budget.saturating_add(step));
            }
            return;
        }
        samples.recovery_budget = 0;
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

    #[cfg(test)]
    pub(crate) fn capacity(&self, shard_cap: usize) -> usize { self.limits(shard_cap).count }

    pub(crate) fn limits(&self, shard_cap: usize) -> ResourceLimits {
        let samples = self.samples.lock();
        if shard_cap == 0 {
            return samples.resource_limits(0);
        }
        let capacity = samples
            .queue_budget()
            .min(shard_cap.saturating_mul(DEPLOY_QUEUE_BLOCKS));
        samples.resource_limits(capacity)
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
    pub(crate) fn allows_arrival(self, deploy: &PendingDeploy, next_block: i64) -> bool {
        const MAX_FUTURE_HEIGHTS: i64 = 3;
        self.contains(deploy)
            && deploy.valid_after_block_number <= next_block.saturating_add(MAX_FUTURE_HEIGHTS)
    }

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
    persisted: bool,
}

struct State {
    entries: HashMap<DeployId, Entry>,
    resources: ResourceUsage,
    valid: bool,
}

impl State {
    fn remove(&mut self, signature: &[u8]) {
        if let Some(entry) = self.entries.remove(signature) {
            self.resources.remove(&entry.deploy);
        }
    }
}

pub struct AdmissionReservations {
    state: Mutex<State>,
    occupied: AtomicUsize,
    revision: AtomicU64,
    retained: bool,
}

pub(crate) enum ReservationUpdate {
    Full(HashMap<DeployId, PendingDeploy>),
    Delta {
        fresh: HashMap<DeployId, PendingDeploy>,
        removals: Vec<DeployId>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReserveError {
    Duplicate,
    NotFresh,
    Full {
        occupied: usize,
        capacity: usize,
    },
    ResourceFull {
        occupied: usize,
        capacity: usize,
        resource: &'static str,
        used: u128,
        limit: u128,
    },
    RecordTooLarge {
        record_bytes: usize,
        limit: u128,
    },
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
        Self::from_entries(deploys, Some(window))
    }

    pub(crate) fn from_retained(
        deploys: impl IntoIterator<Item = PendingDeploy>,
    ) -> Result<Arc<Self>, ReserveError> {
        Self::from_entries(deploys, None)
    }

    fn from_entries(
        deploys: impl IntoIterator<Item = PendingDeploy>,
        window: Option<FreshWindow>,
    ) -> Result<Arc<Self>, ReserveError> {
        let mut entries = HashMap::new();
        let mut resources = ResourceUsage::default();
        for deploy in deploys {
            if window.is_some_and(|window| !window.contains(&deploy)) {
                continue;
            }
            if entries.contains_key(&deploy.signature) {
                return Err(ReserveError::Duplicate);
            }
            resources.add(&deploy);
            entries.insert(deploy.signature.clone(), Entry {
                deploy,
                marker: Marker::Pending,
                persisted: true,
            });
        }
        Ok(Arc::new(Self {
            occupied: AtomicUsize::new(entries.len()),
            revision: AtomicU64::new(0),
            retained: window.is_none(),
            state: Mutex::new(State {
                entries,
                resources,
                valid: true,
            }),
        }))
    }

    pub fn refresh_window(&self, window: FreshWindow) {
        self.expire_entries(&mut self.state.lock(), window);
    }

    pub(crate) fn refresh_window_at_revision(
        &self,
        window: FreshWindow,
        expected_revision: u64,
    ) -> Result<u64, ReserveError> {
        let mut state = self.state.lock();
        if !state.valid || self.revision() != expected_revision {
            return Err(ReserveError::NeedsRefresh);
        }
        self.expire_entries(&mut state, window);
        Ok(self.revision())
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

    pub(crate) fn revision(&self) -> u64 { self.revision.load(AtomicOrdering::Acquire) }

    fn advance_revision(&self) { self.revision.fetch_add(1, AtomicOrdering::AcqRel); }

    pub(crate) fn reconcile(
        &self,
        update: ReservationUpdate,
        window: FreshWindow,
        expected_revision: u64,
    ) -> Result<u64, ReserveError> {
        let mut state = self.state.lock();
        if !state.valid || self.revision() != expected_revision {
            return Err(ReserveError::NeedsRefresh);
        }
        let mut changed = false;
        let fresh = match update {
            ReservationUpdate::Full(fresh) => {
                let mut released = ResourceUsage::default();
                state.entries.retain(|signature, entry| {
                    if self.retained {
                        let persisted = fresh.contains_key(signature);
                        changed |= entry.persisted != persisted;
                        entry.persisted = persisted;
                    }
                    let keep = matches!(entry.marker, Marker::Reserved { .. })
                        || fresh.contains_key(signature);
                    changed |= !keep;
                    if !keep {
                        released.add(&entry.deploy);
                    }
                    keep
                });
                state.resources.record_bytes -= released.record_bytes;
                state.resources.phlo_limit -= released.phlo_limit;
                fresh
            }
            ReservationUpdate::Delta { fresh, removals } => {
                for signature in removals {
                    if fresh.contains_key(&signature) {
                        continue;
                    }
                    match state.entries.get_mut(&signature) {
                        Some(Entry {
                            marker: Marker::Reserved { excluded },
                            persisted,
                            ..
                        }) => {
                            changed |= !*excluded;
                            *excluded = true;
                            if self.retained {
                                changed |= *persisted;
                                *persisted = false;
                            }
                        }
                        Some(_) => {
                            state.remove(&signature);
                            changed = true;
                        }
                        None => {}
                    }
                }
                fresh
            }
        };
        for (signature, deploy) in fresh {
            if !self.retained && !window.contains(&deploy) {
                continue;
            }
            match state.entries.get_mut(&signature) {
                Some(entry) => {
                    if let Marker::Reserved { excluded } = &mut entry.marker {
                        changed |= *excluded;
                        *excluded = false;
                        if !self.retained {
                            continue;
                        }
                    }
                    changed |= !entry.persisted;
                    entry.persisted = true;
                    changed |= entry.deploy != deploy;
                    let previous_bytes = entry.deploy.record_bytes as u128;
                    let previous_phlo = entry.deploy.phlo_limit as u128;
                    let next_bytes = deploy.record_bytes as u128;
                    let next_phlo = deploy.phlo_limit as u128;
                    entry.deploy = deploy;
                    state.resources.record_bytes =
                        state.resources.record_bytes - previous_bytes + next_bytes;
                    state.resources.phlo_limit =
                        state.resources.phlo_limit - previous_phlo + next_phlo;
                }
                None => {
                    changed = true;
                    state.resources.add(&deploy);
                    state.entries.insert(signature, Entry {
                        deploy,
                        marker: Marker::Pending,
                        persisted: true,
                    });
                }
            }
        }
        self.occupied
            .store(state.entries.len(), AtomicOrdering::Release);
        self.expire_entries(&mut state, window);
        if changed {
            self.advance_revision();
        }
        Ok(self.revision())
    }

    pub fn try_reserve(
        self: &Arc<Self>,
        deploy: PendingDeploy,
        capacity: usize,
        window: FreshWindow,
    ) -> Result<Reservation, ReserveError> {
        self.try_reserve_at_revision(deploy, capacity, window, None)
    }

    pub(crate) fn try_reserve_at_revision(
        self: &Arc<Self>,
        deploy: PendingDeploy,
        capacity: usize,
        window: FreshWindow,
        expected_revision: Option<u64>,
    ) -> Result<Reservation, ReserveError> {
        self.try_reserve_with_limits(
            deploy,
            ResourceLimits {
                count: capacity,
                record_bytes: u128::MAX,
                phlo_limit: None,
            },
            window,
            expected_revision,
        )
    }

    pub(crate) fn try_reserve_with_limits(
        self: &Arc<Self>,
        deploy: PendingDeploy,
        limits: ResourceLimits,
        window: FreshWindow,
        expected_revision: Option<u64>,
    ) -> Result<Reservation, ReserveError> {
        let mut state = self.state.lock();
        if !state.valid || expected_revision.is_some_and(|revision| revision != self.revision()) {
            return Err(ReserveError::NeedsRefresh);
        }
        if state.entries.contains_key(&deploy.signature) {
            return Err(ReserveError::Duplicate);
        }
        if !window.contains(&deploy) {
            return Err(ReserveError::NotFresh);
        }
        if self.retained && deploy.record_bytes as u128 > limits.record_bytes {
            return Err(ReserveError::RecordTooLarge {
                record_bytes: deploy.record_bytes,
                limit: limits.record_bytes,
            });
        }
        self.expire_entries(&mut state, window);
        let occupied = self.occupied();
        let capacity = limits.count;
        if occupied >= capacity {
            return Err(ReserveError::Full { occupied, capacity });
        }
        let next_bytes = state.resources.record_bytes + deploy.record_bytes as u128;
        let next_phlo = state.resources.phlo_limit + deploy.phlo_limit as u128;
        if self.retained || occupied != 0 {
            if next_bytes > limits.record_bytes {
                return Err(ReserveError::ResourceFull {
                    occupied,
                    capacity,
                    resource: "record_bytes",
                    used: next_bytes,
                    limit: limits.record_bytes,
                });
            }
            if let Some(limit) = limits.phlo_limit {
                if next_phlo > limit {
                    return Err(ReserveError::ResourceFull {
                        occupied,
                        capacity,
                        resource: "phlo_limit",
                        used: next_phlo,
                        limit,
                    });
                }
            }
        }
        self.occupied.store(occupied + 1, AtomicOrdering::Release);
        state.resources.add(&deploy);
        let signature = deploy.signature.clone();
        state.entries.insert(signature.clone(), Entry {
            deploy,
            marker: Marker::Reserved { excluded: false },
            persisted: false,
        });
        Ok(Reservation {
            book: self.clone(),
            signature,
            completed: false,
        })
    }

    pub fn exclude(&self, signature: &[u8]) -> Exclusion {
        let mut state = self.state.lock();
        match state.entries.get_mut(signature) {
            Some(Entry {
                marker: Marker::Reserved { excluded },
                persisted,
                ..
            }) => {
                if self.retained {
                    *persisted = false;
                }
                if !*excluded {
                    *excluded = true;
                    self.advance_revision();
                }
                Exclusion::DeferredUntilWriteCompletes
            }
            Some(_) => {
                state.remove(signature);
                self.release(1);
                self.advance_revision();
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
        if !self.retained && !window.contains(&deploy) {
            return Ok(false);
        }
        if let Some(entry) = state.entries.get_mut(&deploy.signature) {
            if self.retained && !entry.persisted {
                entry.persisted = true;
                self.advance_revision();
            }
            if let Marker::Reserved { excluded, .. } = &mut entry.marker {
                if *excluded {
                    *excluded = false;
                    self.advance_revision();
                }
            }
            return Ok(false);
        }
        let count = self
            .occupied()
            .checked_add(1)
            .ok_or(ReserveError::CounterOverflow)?;
        self.occupied.store(count, AtomicOrdering::Release);
        state.resources.add(&deploy);
        state.entries.insert(deploy.signature.clone(), Entry {
            deploy,
            marker: Marker::Pending,
            persisted: true,
        });
        self.advance_revision();
        Ok(true)
    }

    #[cfg(test)]
    pub fn invalidate(&self) { self.state.lock().valid = false; }

    fn expire_entries(&self, state: &mut State, window: FreshWindow) {
        if self.retained {
            return;
        }
        let mut removed = 0;
        let mut released = ResourceUsage::default();
        let mut changed = false;
        state.entries.retain(|_, entry| {
            if window.contains(&entry.deploy) {
                return true;
            }
            match &mut entry.marker {
                Marker::Reserved { excluded, .. } => {
                    changed |= !*excluded;
                    *excluded = true;
                    true
                }
                Marker::Pending => {
                    removed += 1;
                    released.add(&entry.deploy);
                    false
                }
            }
        });
        state.resources.record_bytes -= released.record_bytes;
        state.resources.phlo_limit -= released.phlo_limit;
        self.release(removed);
        if changed || removed != 0 {
            self.advance_revision();
        }
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
            let stale = excluded || !window.contains(&entry.deploy);
            let result = if self.book.retained {
                entry.persisted = true;
                entry.marker = Marker::Pending;
                if stale {
                    CommitResult::Excluded
                } else {
                    CommitResult::Pending
                }
            } else if stale {
                state.remove(&self.signature);
                self.book.release(1);
                CommitResult::Excluded
            } else {
                entry.marker = Marker::Pending;
                CommitResult::Pending
            };
            self.book.advance_revision();
            result
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
        let Some(entry) = state.entries.get_mut(&self.signature) else {
            return;
        };
        if !matches!(entry.marker, Marker::Reserved { .. }) {
            return;
        }
        if self.book.retained && entry.persisted {
            entry.marker = Marker::Pending;
        } else {
            state.remove(&self.signature);
            self.book.release(1);
        }
        self.book.advance_revision();
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;

    fn window() -> FreshWindow {
        FreshWindow {
            earliest_valid_after: -1,
            latest_valid_after: 100,
            now_millis: 10,
        }
    }

    fn deploy(id: u8, record_bytes: usize, phlo_limit: u64) -> PendingDeploy {
        PendingDeploy {
            signature: vec![id],
            valid_after_block_number: 0,
            timestamp: id.into(),
            expiration_timestamp: None,
            record_bytes,
            phlo_limit,
        }
    }

    fn limits(record_bytes: u128, phlo_limit: u128) -> ResourceLimits {
        ResourceLimits {
            count: 24,
            record_bytes,
            phlo_limit: Some(phlo_limit),
        }
    }

    fn reserve(
        book: &Arc<AdmissionReservations>,
        deploy: PendingDeploy,
        limits: ResourceLimits,
    ) -> Result<Reservation, ReserveError> {
        book.try_reserve_with_limits(deploy, limits, window(), None)
    }

    #[test]
    fn learned_resource_budgets_use_the_included_work_and_current_count_budget() {
        let admission = DeployAdmission::default();
        assert_eq!(admission.limits(128).count, 24);
        assert_eq!(admission.limits(128).record_bytes, 6 * 1024 * 1024);
        assert_eq!(admission.limits(128).phlo_limit, None);
        admission.observe_sample(1, LocalServiceSample {
            included: 8,
            has_remaining_demand: true,
            record_bytes: 800,
            phlo_limit: 80,
        });
        let budget = admission.limits(128);
        assert_eq!(
            (budget.count, budget.record_bytes, budget.phlo_limit),
            (24, 2500, Some(240))
        );
        admission.observe_sample(2, LocalServiceSample {
            included: 1,
            has_remaining_demand: false,
            record_bytes: 1000,
            phlo_limit: 1000,
        });
        let budget = admission.limits(128);
        assert_eq!(
            (budget.count, budget.record_bytes, budget.phlo_limit),
            (24, 5000, Some(2880))
        );
        assert_eq!(admission.limits(0).count, 0);
        assert_eq!(admission.limits(0).record_bytes, 0);
    }

    #[test]
    fn resource_history_ignores_repeated_and_idle_observations_and_evicts_old_work() {
        let admission = DeployAdmission::default();
        let sample = LocalServiceSample {
            included: 8,
            has_remaining_demand: true,
            record_bytes: 800,
            phlo_limit: 80,
        };
        admission.observe_sample(2, sample);
        admission.observe_sample(1, LocalServiceSample {
            record_bytes: u128::MAX,
            ..sample
        });
        admission.observe_sample(2, LocalServiceSample {
            phlo_limit: u128::MAX,
            ..sample
        });
        admission.observe_sample(3, LocalServiceSample {
            included: 0,
            has_remaining_demand: false,
            record_bytes: 0,
            phlo_limit: 0,
        });
        assert_eq!(admission.samples.lock().resources.len(), 1);
        assert_eq!(admission.limits(128).record_bytes, 2500);
        for sequence in 4..12 {
            admission.observe_sample(sequence, LocalServiceSample {
                record_bytes: 1600,
                phlo_limit: 160,
                ..sample
            });
        }
        assert_eq!(admission.samples.lock().resources.len(), 8);
        assert_eq!(admission.limits(128).record_bytes, 5000);
        assert_eq!(admission.limits(128).phlo_limit, Some(480));
    }

    #[test]
    fn small_record_size_variations_do_not_consume_an_extra_signature_position() {
        let admission = DeployAdmission::default();
        admission.observe_sample(1, LocalServiceSample {
            included: 8,
            has_remaining_demand: true,
            record_bytes: 800,
            phlo_limit: 80,
        });
        let book = AdmissionReservations::from_fresh([], window()).unwrap();
        let budget = admission.limits(128);
        for id in 0..24 {
            reserve(&book, deploy(id, 101, 10), budget)
                .unwrap()
                .commit(window());
        }
        assert!(matches!(
            reserve(&book, deploy(24, 101, 10), budget),
            Err(ReserveError::Full { capacity: 24, .. })
        ));
    }

    #[test]
    fn byte_and_phlo_refusals_release_the_unwritten_work_on_drop() {
        for (budget, second, resource) in [
            (limits(150, 1000), deploy(2, 100, 10), "record_bytes"),
            (limits(1000, 15), deploy(2, 100, 10), "phlo_limit"),
        ] {
            let book = AdmissionReservations::from_fresh([], window()).unwrap();
            let first = reserve(&book, deploy(1, 100, 10), budget).unwrap();
            assert!(
                matches!(reserve(&book, second.clone(), budget), Err(ReserveError::ResourceFull { resource: cause, .. }) if cause == resource)
            );
            assert_eq!(book.occupied(), 1);
            drop(first);
            assert_eq!(book.state.lock().resources, ResourceUsage::default());
            assert!(reserve(&book, second, budget).is_ok());
        }
    }

    #[test]
    fn an_empty_queue_admits_one_oversized_probe_but_zero_count_disables_it() {
        let book = AdmissionReservations::from_fresh([], window()).unwrap();
        let budget = limits(1, 1);
        let probe = reserve(&book, deploy(1, 1000, 1000), budget).unwrap();
        assert!(matches!(
            reserve(&book, deploy(2, 1, 1), budget),
            Err(ReserveError::ResourceFull { .. })
        ));
        drop(probe);
        assert!(matches!(
            reserve(&book, deploy(3, 1000, 1000), ResourceLimits {
                count: 0,
                ..budget
            }),
            Err(ReserveError::Full { capacity: 0, .. })
        ));
    }

    #[test]
    fn deferred_exclusion_and_expiry_release_committed_resources_once() {
        let book = AdmissionReservations::from_fresh([], window()).unwrap();
        let held = reserve(&book, deploy(1, 100, 10), limits(100, 10)).unwrap();
        assert_eq!(book.exclude(&[1]), Exclusion::DeferredUntilWriteCompletes);
        assert_eq!(book.state.lock().resources.record_bytes, 100);
        assert_eq!(held.commit(window()), CommitResult::Excluded);
        assert_eq!(book.state.lock().resources, ResourceUsage::default());
        let mut expiring = deploy(2, 200, 20);
        expiring.expiration_timestamp = Some(11);
        reserve(&book, expiring, limits(200, 20))
            .unwrap()
            .commit(window());
        book.refresh_window(FreshWindow {
            now_millis: 12,
            ..window()
        });
        assert_eq!(book.state.lock().resources, ResourceUsage::default());
        assert_eq!(book.occupied(), 0);
    }

    #[test]
    fn reconciliation_accounts_for_replacement_removal_and_restored_accepted_work() {
        let book = AdmissionReservations::from_fresh([deploy(1, 100, 10)], window()).unwrap();
        let next = HashMap::from([(vec![1], deploy(1, 200, 20)), (vec![2], deploy(2, 300, 30))]);
        book.reconcile(ReservationUpdate::Full(next), window(), book.revision())
            .unwrap();
        assert_eq!(book.state.lock().resources, ResourceUsage {
            record_bytes: 500,
            phlo_limit: 50
        });
        book.reconcile(
            ReservationUpdate::Delta {
                fresh: HashMap::new(),
                removals: vec![vec![1]],
            },
            window(),
            book.revision(),
        )
        .unwrap();
        assert_eq!(book.state.lock().resources, ResourceUsage {
            record_bytes: 300,
            phlo_limit: 30
        });
        book.restore_accepted(deploy(3, 500, 50), window()).unwrap();
        assert_eq!(book.state.lock().resources, ResourceUsage {
            record_bytes: 800,
            phlo_limit: 80
        });
        assert!(matches!(
            reserve(&book, deploy(4, 1, 1), limits(100, 10)),
            Err(ReserveError::ResourceFull { .. })
        ));
        book.exclude(&[2]);
        book.exclude(&[3]);
        assert_eq!(book.state.lock().resources, ResourceUsage::default());
    }

    #[test]
    fn concurrent_arrivals_cannot_share_the_last_resource_allowance() {
        let book = AdmissionReservations::from_fresh([deploy(0, 100, 10)], window()).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let workers: Vec<_> = (1..=2)
            .map(|id| {
                let book = book.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    reserve(&book, deploy(id, 100, 10), limits(200, 20))
                        .map(|reservation| reservation.commit(window()))
                })
            })
            .collect();
        barrier.wait();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(ReserveError::ResourceFull { .. })))
                .count(),
            1
        );
        assert_eq!(book.state.lock().resources, ResourceUsage {
            record_bytes: 200,
            phlo_limit: 20
        });
    }

    #[test]
    fn resource_totals_handle_maximum_metadata_values_and_keep_duplicate_priority() {
        let book =
            AdmissionReservations::from_fresh([deploy(1, usize::MAX, u64::MAX)], window()).unwrap();
        let budget = ResourceLimits {
            count: 2,
            record_bytes: u128::MAX,
            phlo_limit: Some(u128::MAX),
        };
        reserve(&book, deploy(2, usize::MAX, u64::MAX), budget)
            .unwrap()
            .commit(window());
        assert_eq!(
            book.state.lock().resources.record_bytes,
            2 * usize::MAX as u128
        );
        assert_eq!(book.state.lock().resources.phlo_limit, 2 * u64::MAX as u128);
        assert!(matches!(
            reserve(&book, deploy(1, usize::MAX, u64::MAX), limits(1, 1)),
            Err(ReserveError::Duplicate)
        ));
        book.exclude(&[1]);
        book.exclude(&[2]);
        assert_eq!(book.state.lock().resources, ResourceUsage::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drained_capacity_recovers_one_estimated_block_at_a_time() {
        let admission = DeployAdmission::default();
        for (sequence, included, expected) in [(1, 24, 32), (2, 32, 43), (3, 43, 58)] {
            admission.observe(sequence, included, false);
            assert_eq!(admission.capacity(128), expected);
        }
        assert!(admission.samples.lock().inclusions.is_empty());
    }

    #[test]
    fn small_drained_batches_do_not_change_the_budget() {
        let admission = DeployAdmission::default();
        for (sequence, included) in [(1, 0), (2, 1), (3, 8)] {
            admission.observe(sequence, included, false);
            assert_eq!(admission.capacity(128), 24);
        }
        assert_eq!(admission.samples.lock().recovery_budget, 0);
    }

    #[test]
    fn saturated_samples_reset_recovery_and_keep_the_original_window() {
        let admission = DeployAdmission::default();
        admission.observe(1, 24, false);
        assert_eq!(admission.capacity(128), 32);
        admission.observe(2, 6, true);
        assert_eq!(admission.capacity(128), 18);
        admission.observe(3, 18, false);
        assert_eq!(admission.capacity(128), 24);
        admission.observe(4, 24, false);
        assert_eq!(admission.capacity(128), 32);
        admission.observe(5, 0, true);
        assert_eq!(admission.capacity(128), 9);
        let samples = admission.samples.lock();
        assert_eq!(samples.recovery_budget, 0);
        assert_eq!(
            samples.inclusions.iter().copied().collect::<Vec<_>>(),
            vec![6, 0]
        );
    }

    #[test]
    fn drained_capacity_recovery_respects_limits_and_saturates_without_overflow() {
        let admission = DeployAdmission::default();
        admission.observe(1, usize::MAX, false);
        assert_eq!(admission.capacity(128), 32);
        assert_eq!(admission.capacity(4), 12);
        assert_eq!(admission.capacity(0), 0);
        for sequence in 2..=256 {
            admission.observe(sequence, usize::MAX, false);
            assert!(admission.capacity(128) <= 384);
        }
        assert_eq!(admission.capacity(usize::MAX), usize::MAX);
    }

    #[test]
    fn an_actual_drained_probe_can_recover_a_zero_service_budget() {
        let admission = DeployAdmission::default();
        admission.observe(1, 0, true);
        admission.observe(2, 0, false);
        assert_eq!(admission.capacity(128), 0);
        for (sequence, included, expected) in [(3, 1, 1), (4, 1, 2), (5, 2, 3)] {
            admission.observe(sequence, included, false);
            assert_eq!(admission.capacity(128), expected);
        }
    }

    #[test]
    fn repeated_and_older_sequences_do_not_reset_recovery() {
        let admission = DeployAdmission::default();
        admission.observe(2, 24, false);
        admission.observe(2, 0, true);
        admission.observe(1, 0, true);
        assert_eq!(admission.capacity(128), 32);
        admission.observe(3, 6, true);
        assert_eq!(admission.capacity(128), 18);
    }

    #[test]
    fn drained_recovery_does_not_evict_saturated_history() {
        let admission = DeployAdmission::default();
        admission.observe(1, 128, true);
        for sequence in 2..=8 {
            admission.observe(sequence, 0, true);
        }
        assert_eq!(admission.capacity(128), 48);
        admission.observe(9, 24, false);
        assert_eq!(admission.capacity(128), 64);
        {
            let samples = admission.samples.lock();
            assert_eq!(samples.inclusions.len(), 8);
            assert_eq!(samples.inclusions.front(), Some(&128));
        }
        admission.observe(10, 0, true);
        assert_eq!(admission.capacity(128), 0);
    }

    #[test]
    fn drained_recovery_never_lowers_a_higher_service_estimate() {
        let admission = DeployAdmission::default();
        admission.observe(1, 100, true);
        admission.observe(2, 1, false);
        admission.observe(3, 24, false);
        assert_eq!(admission.capacity(128), 300);
        assert_eq!(admission.samples.lock().recovery_budget, 0);
    }

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
    fn draining_blocks_do_not_replace_saturated_service_samples() {
        let admission = DeployAdmission::default();
        admission.observe(1, 8, true);
        for sequence in 2..=10 {
            admission.observe(sequence, 1, false);
        }
        assert_eq!(admission.capacity(128), 24);
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
            record_bytes: 0,
            phlo_limit: 0,
        }
    }

    fn book(count: u8) -> Arc<AdmissionReservations> {
        AdmissionReservations::from_fresh((0..count).map(deploy), window()).unwrap()
    }

    #[test]
    fn arrival_bounds_preserve_stored_freshness_and_handle_height_overflow() {
        let mut candidate = deploy(1);
        candidate.valid_after_block_number = 104;
        assert!(window().contains(&candidate));
        assert!(window().allows_arrival(&candidate, 101));
        assert!(!window().allows_arrival(&candidate, 100));
        candidate.valid_after_block_number = i64::MAX;
        let window = FreshWindow {
            latest_valid_after: i64::MAX,
            ..window()
        };
        assert!(window.allows_arrival(&candidate, i64::MAX - 1));
    }

    #[test]
    fn refreshing_a_snapshot_preserves_unwritten_reservations() {
        let book = book(0);
        let reservation = book.try_reserve(deploy(1), 1, window()).unwrap();
        let revision = book.revision();
        book.reconcile(
            ReservationUpdate::Full(HashMap::new()),
            window(),
            book.revision(),
        )
        .unwrap();
        assert_eq!(book.revision(), revision);
        assert_eq!(book.occupied(), 1);
        assert!(matches!(
            book.try_reserve(deploy(2), 1, window()),
            Err(ReserveError::Full { .. })
        ));
        assert_eq!(reservation.commit(window()), CommitResult::Pending);
        assert_eq!(book.occupied(), 1);
    }

    #[test]
    fn stale_snapshots_cannot_overwrite_a_completed_reservation() {
        let book = book(0);
        let reservation = book.try_reserve(deploy(1), 1, window()).unwrap();
        let revision = book.revision();
        assert_eq!(reservation.commit(window()), CommitResult::Pending);
        assert_eq!(
            book.reconcile(ReservationUpdate::Full(HashMap::new()), window(), revision),
            Err(ReserveError::NeedsRefresh)
        );
        assert_eq!(book.occupied(), 1);
    }

    #[test]
    fn a_rolled_back_write_invalidates_snapshots_without_leaking_a_slot() {
        let book = book(0);
        let reservation = book.try_reserve(deploy(1), 1, window()).unwrap();
        let revision = book.revision();
        drop(reservation);
        assert_eq!(
            book.reconcile(ReservationUpdate::Full(HashMap::new()), window(), revision),
            Err(ReserveError::NeedsRefresh)
        );
        assert_eq!(book.occupied(), 0);
        assert!(book.try_reserve(deploy(2), 1, window()).is_ok());
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

#[cfg(test)]
mod retained_resource_tests {
    use super::*;

    fn deploy(id: u8, bytes: usize) -> PendingDeploy {
        PendingDeploy {
            signature: vec![id],
            valid_after_block_number: 1,
            timestamp: 0,
            expiration_timestamp: None,
            record_bytes: bytes,
            phlo_limit: u64::MAX,
        }
    }

    fn window() -> FreshWindow {
        FreshWindow {
            earliest_valid_after: 0,
            latest_valid_after: 100,
            now_millis: 10,
        }
    }

    fn limits(count: usize, bytes: u64) -> ResourceLimits {
        RetainedPoolLimits {
            max_records: count,
            max_bytes: bytes,
            max_inflight: 2,
        }
        .into()
    }

    fn reconcile(book: &AdmissionReservations, rows: Vec<PendingDeploy>) {
        let entries = rows
            .into_iter()
            .map(|row| (row.signature.clone(), row))
            .collect();
        book.reconcile(ReservationUpdate::Full(entries), window(), book.revision())
            .unwrap();
    }

    #[test]
    fn retained_limits_require_both_fields_and_reject_typographical_errors() {
        let explicit =
            serde_json::json!({"max-records": 128, "max-bytes": 4096, "max-inflight": 2});
        let configured: RetainedPoolLimits = serde_json::from_value(explicit).unwrap();
        assert_eq!(configured.max_records, 128);
        assert_eq!(configured.max_bytes, 4096);
        assert_eq!(configured.max_inflight, 2);
        for invalid in [
            serde_json::json!({"max-records": 128}),
            serde_json::json!({"max-bytes": 4096}),
            serde_json::json!({"max-records": 128, "max-bytes": 4096}),
            serde_json::json!({"max-records": 128, "max-bytes": 4096, "max-record-byte": 4096}),
        ] {
            assert!(serde_json::from_value::<RetainedPoolLimits>(invalid).is_err());
        }
    }

    #[test]
    fn stored_expired_records_still_consume_limits_after_reconstruction() {
        let mut expired = deploy(1, 100);
        expired.expiration_timestamp = Some(5);
        let mut old = deploy(2, 150);
        old.valid_after_block_number = -10;
        let book = AdmissionReservations::from_retained([expired, old]).unwrap();
        book.refresh_window(FreshWindow {
            earliest_valid_after: 100,
            ..window()
        });
        assert_eq!(book.occupied(), 2);
        assert_eq!(book.state.lock().resources.record_bytes, 250);
        assert!(matches!(
            book.try_reserve_with_limits(deploy(3, 10), limits(2, 500), window(), None),
            Err(ReserveError::Full {
                occupied: 2,
                capacity: 2
            })
        ));
        reconcile(&book, vec![deploy(2, 150)]);
        assert_eq!(book.occupied(), 1);
        let reservation = book
            .try_reserve_with_limits(deploy(3, 10), limits(2, 500), window(), None)
            .unwrap();
        assert_eq!(book.occupied(), 2);
        drop(reservation);
        assert_eq!(book.state.lock().resources.record_bytes, 150);
    }

    #[test]
    fn an_empty_pool_cannot_bypass_its_byte_limit_and_duplicates_keep_priority() {
        let book = AdmissionReservations::from_retained([]).unwrap();
        assert!(matches!(
            book.try_reserve_with_limits(deploy(1, 101), limits(2, 100), window(), None),
            Err(ReserveError::RecordTooLarge {
                record_bytes: 101,
                limit: 100
            })
        ));
        assert_eq!(book.occupied(), 0);
        let reservation = book
            .try_reserve_with_limits(deploy(1, 100), limits(2, 100), window(), None)
            .unwrap();
        assert_eq!(reservation.commit(window()), CommitResult::Pending);
        assert!(matches!(
            book.try_reserve_with_limits(deploy(1, 100), limits(0, 0), window(), None),
            Err(ReserveError::Duplicate)
        ));
    }

    #[test]
    fn only_a_changed_storage_snapshot_releases_stored_resources() {
        let rows = vec![deploy(1, 100), deploy(2, 100)];
        let book = AdmissionReservations::from_retained(rows.clone()).unwrap();
        reconcile(&book, rows);
        assert!(matches!(
            book.try_reserve_with_limits(deploy(3, 60), limits(4, 250), window(), None),
            Err(ReserveError::ResourceFull {
                resource: "record_bytes",
                used: 260,
                limit: 250,
                ..
            })
        ));
        reconcile(&book, vec![deploy(2, 100)]);
        let reservation = book
            .try_reserve_with_limits(deploy(3, 60), limits(4, 250), window(), None)
            .unwrap();
        assert_eq!(book.state.lock().resources.record_bytes, 160);
        drop(reservation);
        assert_eq!(book.state.lock().resources.record_bytes, 100);
    }

    #[test]
    fn reconciliation_and_rollback_do_not_erase_an_independently_persisted_record() {
        let book = AdmissionReservations::from_retained([]).unwrap();
        let reservation = book
            .try_reserve_with_limits(deploy(1, 100), limits(2, 300), window(), None)
            .unwrap();
        reconcile(&book, vec![deploy(1, 150)]);
        assert_eq!(book.occupied(), 1);
        assert_eq!(book.state.lock().resources.record_bytes, 150);
        drop(reservation);
        assert_eq!(book.occupied(), 1);
        assert_eq!(book.state.lock().resources.record_bytes, 150);
        assert!(matches!(
            book.try_reserve_with_limits(deploy(1, 100), limits(2, 300), window(), None),
            Err(ReserveError::Duplicate)
        ));
        reconcile(&book, vec![]);
        assert_eq!(book.occupied(), 0);
        assert_eq!(book.state.lock().resources.record_bytes, 0);
    }

    #[test]
    fn a_write_that_expires_before_commit_remains_charged_until_removed() {
        let mut row = deploy(1, 200);
        row.expiration_timestamp = Some(20);
        let book = AdmissionReservations::from_retained([]).unwrap();
        let reservation = book
            .try_reserve_with_limits(row.clone(), limits(2, 300), window(), None)
            .unwrap();
        assert_eq!(
            reservation.commit(FreshWindow {
                now_millis: 30,
                ..window()
            }),
            CommitResult::Excluded
        );
        reconcile(&book, vec![row]);
        assert_eq!(book.state.lock().resources.record_bytes, 200);
        assert!(matches!(
            book.try_reserve_with_limits(deploy(2, 200), limits(2, 300), window(), None),
            Err(ReserveError::ResourceFull { .. })
        ));
        reconcile(&book, vec![]);
        assert_eq!(book.state.lock().resources.record_bytes, 0);
    }

    #[test]
    fn lower_limits_preserve_stored_records_and_stop_unwritten_reservations() {
        let rows = vec![deploy(1, 100), deploy(2, 100)];
        let book = AdmissionReservations::from_retained(rows.clone()).unwrap();
        assert!(matches!(
            book.try_reserve_with_limits(deploy(3, 100), limits(1, 200), window(), None),
            Err(ReserveError::Full { .. })
        ));
        assert_eq!(book.occupied(), 2);
        reconcile(&book, rows);
        assert_eq!(book.occupied(), 2);
        reconcile(&book, vec![]);
        assert!(matches!(
            book.try_reserve_with_limits(deploy(3, 100), limits(0, 200), window(), None),
            Err(ReserveError::Full {
                occupied: 0,
                capacity: 0
            })
        ));
        drop(
            book.try_reserve_with_limits(deploy(3, 100), limits(1, 200), window(), None)
                .unwrap(),
        );
        assert_eq!(book.occupied(), 0);
    }

    #[test]
    fn distinct_concurrent_reservations_cannot_exceed_the_byte_budget() {
        let book = AdmissionReservations::from_retained([]).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(16));
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = (1..=16)
                .map(|id| {
                    let book = book.clone();
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        book.try_reserve_with_limits(
                            deploy(id, 100),
                            limits(16, 300),
                            window(),
                            None,
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 3);
        assert_eq!(book.occupied(), 3);
        assert_eq!(book.state.lock().resources.record_bytes, 300);
        drop(results);
        assert_eq!(book.occupied(), 0);
        assert_eq!(book.state.lock().resources.record_bytes, 0);
    }
}
