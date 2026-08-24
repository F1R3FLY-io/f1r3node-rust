//! Measurement-only accepted-match transaction experiment.
//!
//! This file is compiled as a child of `rspace::hot_store` only when the
//! `accepted-match-transition-experiment` feature is selected. The atomic arm
//! calls the production `HotStore::apply_accepted_match` operation. The split
//! arm freezes the exact pre-transaction call sequence recovered from git:
//! optional continuation removal, one datum-delta application, one join
//! removal per selected candidate, and a later insertion for an unconsumed
//! arrival. Ordinary builds compile neither the fixture nor its control.

use std::collections::{BTreeSet, HashMap};

use super::*;
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::internal::{Datum, WaitingContinuation};

type ExperimentStore = Box<dyn HotStore<String, String, String, String>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExperimentArm {
    FrozenSplit,
    Atomic,
}

impl ExperimentArm {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FrozenSplit => "frozen_split",
            Self::Atomic => "atomic",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExperimentCase {
    Singleton,
    Repeated2,
    Repeated16,
    Join2,
    Join8,
    Join32,
    PersistentContinuation,
    StaleRejection,
    NamedNonleast,
    UnconsumedArrival,
}

impl ExperimentCase {
    pub const ALL: [Self; 10] = [
        Self::Singleton,
        Self::Repeated2,
        Self::Repeated16,
        Self::Join2,
        Self::Join8,
        Self::Join32,
        Self::PersistentContinuation,
        Self::StaleRejection,
        Self::NamedNonleast,
        Self::UnconsumedArrival,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Singleton => "singleton",
            Self::Repeated2 => "repeated_2",
            Self::Repeated16 => "repeated_16",
            Self::Join2 => "join_2",
            Self::Join8 => "join_8",
            Self::Join32 => "join_32",
            Self::PersistentContinuation => "persistent_continuation",
            Self::StaleRejection => "stale_rejection",
            Self::NamedNonleast => "named_nonleast",
            Self::UnconsumedArrival => "unconsumed_arrival",
        }
    }

    pub const fn supports(self, arm: ExperimentArm) -> bool {
        !matches!((self, arm), (Self::StaleRejection, ExperimentArm::FrozenSplit))
    }

    pub const fn expected_write_locks(self, arm: ExperimentArm) -> u64 {
        match arm {
            ExperimentArm::Atomic => 1,
            ExperimentArm::FrozenSplit => match self {
                Self::Singleton | Self::NamedNonleast => 3,
                Self::Repeated2 | Self::Join2 => 4,
                Self::Repeated16 => 18,
                Self::Join8 => 10,
                Self::Join32 => 34,
                Self::PersistentContinuation => 2,
                Self::UnconsumedArrival => 4,
                Self::StaleRejection => 0,
            },
        }
    }

    pub const fn expected_root_publications(self, arm: ExperimentArm) -> u64 {
        match (self, arm) {
            (Self::StaleRejection, ExperimentArm::Atomic) => 0,
            (Self::UnconsumedArrival, ExperimentArm::FrozenSplit) => 2,
            (_, ExperimentArm::Atomic | ExperimentArm::FrozenSplit) => 1,
        }
    }

    const fn arity(self) -> usize {
        match self {
            Self::Repeated2 | Self::Join2 => 2,
            Self::Repeated16 => 16,
            Self::Join8 => 8,
            Self::Join32 => 32,
            _ => 1,
        }
    }

    const fn repeats_one_channel(self) -> bool {
        matches!(self, Self::Repeated2 | Self::Repeated16)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExperimentStatus {
    Committed,
    Stale,
}

impl ExperimentStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::Stale => "stale",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExperimentOutcome {
    digest: [u8; 32],
    datum_cardinality: usize,
    continuation_count: usize,
    join_count: usize,
}

impl ExperimentOutcome {
    pub fn digest_hex(&self) -> String { hex::encode(self.digest) }

    pub const fn datum_cardinality(&self) -> usize { self.datum_cardinality }

    pub const fn continuation_count(&self) -> usize { self.continuation_count }

    pub const fn join_count(&self) -> usize { self.join_count }

    fn from_state(state: HotStoreState<String, String, String, String>) -> Self {
        let datum_cardinality = state.data.values().map(Vec::len).sum();
        let continuation_count = state.continuations.values().map(Vec::len).sum::<usize>() +
            state.installed_continuations.len();
        let join_count = state.joins.values().map(Vec::len).sum::<usize>() +
            state.installed_joins.values().map(Vec::len).sum::<usize>();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"RSpace/AcceptedMatchTransitionExperiment/outcome/v1");

        let mut continuations = state.continuations.into_iter().collect::<Vec<_>>();
        continuations.sort_by(|left, right| left.0.cmp(&right.0));
        hasher.update(b"continuations");
        for (channels, values) in continuations {
            hash_strings(&mut hasher, &channels);
            hash_waiting(&mut hasher, &values);
        }

        let mut installed_continuations = state
            .installed_continuations
            .into_iter()
            .collect::<Vec<_>>();
        installed_continuations.sort_by(|left, right| left.0.cmp(&right.0));
        hasher.update(b"installed-continuations");
        for (channels, value) in installed_continuations {
            hash_strings(&mut hasher, &channels);
            hash_waiting(&mut hasher, std::slice::from_ref(&value));
        }

        let mut data = state.data.into_iter().collect::<Vec<_>>();
        data.sort_by(|left, right| left.0.cmp(&right.0));
        hasher.update(b"data");
        for (channel, values) in data {
            hash_string(&mut hasher, &channel);
            for datum in values {
                hash_string(&mut hasher, &datum.a);
                hasher.update(&[u8::from(datum.persist)]);
                hasher.update(&datum.source.hash.bytes());
            }
        }

        hash_joins(&mut hasher, b"joins", state.joins);
        hash_joins(&mut hasher, b"installed-joins", state.installed_joins);
        Self {
            digest: *hasher.finalize().as_bytes(),
            datum_cardinality,
            continuation_count,
            join_count,
        }
    }
}

fn hash_string(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(
        &u64::try_from(value.len())
            .expect("experiment string length fits u64")
            .to_be_bytes(),
    );
    hasher.update(value.as_bytes());
}

fn hash_strings(hasher: &mut blake3::Hasher, values: &[String]) {
    hasher.update(
        &u64::try_from(values.len())
            .expect("experiment vector length fits u64")
            .to_be_bytes(),
    );
    for value in values {
        hash_string(hasher, value);
    }
}

fn hash_waiting(
    hasher: &mut blake3::Hasher,
    continuations: &[WaitingContinuation<String, String>],
) {
    for continuation in continuations {
        hash_strings(hasher, &continuation.patterns);
        hash_string(hasher, &continuation.continuation);
        hasher.update(&[u8::from(continuation.persist)]);
        for peek in &continuation.peeks {
            hasher.update(&peek.to_be_bytes());
        }
        hasher.update(&continuation.source.hash.bytes());
    }
}

fn hash_joins(hasher: &mut blake3::Hasher, label: &[u8], joins: HashMap<String, Vec<Vec<String>>>) {
    hasher.update(label);
    let mut joins = joins.into_iter().collect::<Vec<_>>();
    joins.sort_by(|left, right| left.0.cmp(&right.0));
    for (channel, groups) in joins {
        hash_string(hasher, &channel);
        for group in groups {
            hash_strings(hasher, &group);
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExperimentAudit {
    pub case: ExperimentCase,
    pub atomic_status: ExperimentStatus,
    pub atomic: ExperimentOutcome,
    pub frozen_split: Option<ExperimentOutcome>,
}

pub struct AcceptedMatchTransitionExperiment {
    case: ExperimentCase,
    store: ExperimentStore,
    transition: Option<AcceptedMatch<String>>,
    frozen_delta: BagDelta,
    arrival: Option<(String, Datum<String>)>,
}

impl AcceptedMatchTransitionExperiment {
    pub fn new(case: ExperimentCase) -> Self {
        let arity = case.arity();
        let candidate_channels = if case.repeats_one_channel() {
            vec!["channel-0".to_owned(); arity]
        } else {
            (0..arity)
                .map(|index| format!("channel-{index}"))
                .collect::<Vec<_>>()
        };
        let channels = candidate_channels.clone();
        let persistent_continuation = matches!(case, ExperimentCase::PersistentContinuation);
        let patterns = (0..arity)
            .map(|index| format!("pattern-{index}"))
            .collect::<Vec<_>>();
        let selected = WaitingContinuation::create(
            &channels,
            &patterns,
            &"selected-continuation".to_owned(),
            persistent_continuation,
            BTreeSet::new(),
        );
        let (continuations, selected_index) = if matches!(case, ExperimentCase::NamedNonleast) {
            let decoy_patterns = vec!["decoy-pattern".to_owned(); arity];
            let decoy = WaitingContinuation::create(
                &channels,
                &decoy_patterns,
                &"decoy-continuation".to_owned(),
                false,
                BTreeSet::new(),
            );
            (vec![decoy, selected.clone()], 1)
        } else {
            (vec![selected.clone()], 0)
        };

        let mut data: HashMap<String, Vec<Datum<String>>> = HashMap::new();
        let mut selected_data = Vec::with_capacity(arity);
        for (index, channel) in candidate_channels.iter().enumerate() {
            let payload = if case.repeats_one_channel() {
                "repeated-datum".to_owned()
            } else {
                format!("datum-{index}")
            };
            let datum = Datum::create(channel, payload, false);
            data.entry(channel.clone()).or_default().push(datum.clone());
            selected_data.push((channel.clone(), datum));
        }

        let mut joins = HashMap::new();
        for channel in &candidate_channels {
            joins
                .entry(channel.clone())
                .or_insert_with(|| vec![channels.clone()]);
        }
        let state = HotStoreState {
            continuations: HashMap::from([(channels.clone(), continuations)]),
            installed_continuations: HashMap::new(),
            data,
            joins,
            installed_joins: HashMap::new(),
        };
        let store = HotStoreInstances::create_from_hs_and_hr(state, Box::new(EmptyHistory));
        let (version, loaded) = store.get_continuations_versioned(&channels);
        assert_eq!(loaded.get(selected_index), Some(&selected));

        let mut frozen_delta = BagDelta::new();
        for (channel, datum) in &selected_data {
            frozen_delta
                .record_removal(
                    &RSpaceDataIndex::datum_path(channel, datum),
                    super::super::bag::Multiplicity::ONE,
                )
                .expect("the fixture multiplicity fits u64");
        }
        let arrival = matches!(case, ExperimentCase::UnconsumedArrival).then(|| {
            let channel = "channel-0".to_owned();
            let datum = Datum::create(&channel, "unconsumed-arrival".to_owned(), false);
            (channel, datum)
        });
        let mut atomic_delta = frozen_delta.clone();
        if let Some((channel, datum)) = &arrival {
            atomic_delta
                .record_addition(
                    &RSpaceDataIndex::datum_path(channel, datum),
                    super::super::bag::Multiplicity::ONE,
                )
                .expect("the fixture multiplicity fits u64");
        }
        let transition = AcceptedMatch {
            channels: channels.clone(),
            continuation: Some(StoredContinuationCommit {
                address: ContinuationAddress::new(version, selected_index as i32),
                source: selected.source.clone(),
                persist: selected.persist,
                peeks: selected.peeks.clone(),
            }),
            datum_delta: atomic_delta,
            required_persistent_paths: Vec::new(),
            join_channels: candidate_channels,
        };

        if matches!(case, ExperimentCase::StaleRejection) {
            let interfering = WaitingContinuation::create(
                &channels,
                &vec!["interfering-pattern".to_owned(); arity],
                &"interfering-continuation".to_owned(),
                false,
                BTreeSet::new(),
            );
            assert_eq!(store.put_continuation(&channels, interfering), Some(true));
        }

        Self {
            case,
            store,
            transition: Some(transition),
            frozen_delta,
            arrival,
        }
    }

    #[inline(never)]
    pub fn execute(&mut self, arm: ExperimentArm) -> ExperimentStatus {
        assert!(self.case.supports(arm), "stale split control is semantically invalid");
        let transition = self
            .transition
            .take()
            .expect("an experiment fixture executes exactly once");
        match arm {
            ExperimentArm::Atomic => match self
                .store
                .apply_accepted_match(transition)
                .expect("the accepted-match fixture is valid")
            {
                AcceptedMatchOutcome::Committed { .. } => ExperimentStatus::Committed,
                AcceptedMatchOutcome::Stale => ExperimentStatus::Stale,
            },
            ExperimentArm::FrozenSplit => {
                let AcceptedMatch {
                    channels,
                    continuation,
                    join_channels,
                    ..
                } = transition;
                let continuation = continuation.expect("the split fixture names a continuation");
                if !continuation.persist {
                    assert_eq!(
                        self.store
                            .remove_continuation(&channels, continuation.address.index),
                        Some(())
                    );
                }
                self.store
                    .apply_datum_delta(&self.frozen_delta)
                    .expect("the frozen datum removal is valid");
                for channel in join_channels {
                    self.store.remove_join(&channel, &channels);
                }
                if let Some((channel, datum)) = self.arrival.take() {
                    self.store.put_datum(&channel, datum);
                }
                ExperimentStatus::Committed
            }
        }
    }

    pub fn outcome(&self) -> ExperimentOutcome {
        ExperimentOutcome::from_state(self.store.snapshot())
    }

    pub fn audit(case: ExperimentCase) -> ExperimentAudit {
        if matches!(case, ExperimentCase::StaleRejection) {
            let mut fixture = Self::new(case);
            let before = fixture.outcome();
            let status = fixture.execute(ExperimentArm::Atomic);
            let after = fixture.outcome();
            assert_eq!(status, ExperimentStatus::Stale);
            assert_eq!(after, before, "stale rejection mutated the hot store");
            return ExperimentAudit {
                case,
                atomic_status: status,
                atomic: after,
                frozen_split: None,
            };
        }

        let mut atomic = Self::new(case);
        let atomic_status = atomic.execute(ExperimentArm::Atomic);
        let atomic_outcome = atomic.outcome();
        let mut split = Self::new(case);
        let split_status = split.execute(ExperimentArm::FrozenSplit);
        let split_outcome = split.outcome();
        assert_eq!(atomic_status, ExperimentStatus::Committed);
        assert_eq!(split_status, ExperimentStatus::Committed);
        assert_eq!(atomic_outcome, split_outcome, "atomic and split post-states differ");
        ExperimentAudit {
            case,
            atomic_status,
            atomic: atomic_outcome,
            frozen_split: Some(split_outcome),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransitionInstrumentation {
    pub write_locks: u64,
    pub write_wait_ns: u64,
    pub write_hold_ns: u64,
    pub pathmap_root_publications: u64,
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
mod instrumentation {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::Duration;

    use super::TransitionInstrumentation;

    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static WRITE_LOCKS: AtomicU64 = AtomicU64::new(0);
    static WRITE_WAIT_NS: AtomicU64 = AtomicU64::new(0);
    static WRITE_HOLD_NS: AtomicU64 = AtomicU64::new(0);
    static ROOT_PUBLICATIONS: AtomicU64 = AtomicU64::new(0);

    fn duration_ns(duration: Duration) -> u64 {
        u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
    }

    pub fn begin() {
        assert!(!ACTIVE.swap(true, Ordering::SeqCst), "instrumentation is not reentrant");
        WRITE_LOCKS.store(0, Ordering::Relaxed);
        WRITE_WAIT_NS.store(0, Ordering::Relaxed);
        WRITE_HOLD_NS.store(0, Ordering::Relaxed);
        ROOT_PUBLICATIONS.store(0, Ordering::Relaxed);
    }

    pub fn end() -> TransitionInstrumentation {
        assert!(ACTIVE.swap(false, Ordering::SeqCst), "instrumentation was not active");
        TransitionInstrumentation {
            write_locks: WRITE_LOCKS.load(Ordering::Relaxed),
            write_wait_ns: WRITE_WAIT_NS.load(Ordering::Relaxed),
            write_hold_ns: WRITE_HOLD_NS.load(Ordering::Relaxed),
            pathmap_root_publications: ROOT_PUBLICATIONS.load(Ordering::Relaxed),
        }
    }

    pub fn record_acquisition(wait: Duration) -> bool {
        if !ACTIVE.load(Ordering::Relaxed) {
            return false;
        }
        WRITE_LOCKS.fetch_add(1, Ordering::Relaxed);
        WRITE_WAIT_NS.fetch_add(duration_ns(wait), Ordering::Relaxed);
        true
    }

    pub fn record_hold(hold: Duration) {
        WRITE_HOLD_NS.fetch_add(duration_ns(hold), Ordering::Relaxed);
    }

    pub fn record_root_publication() {
        if ACTIVE.load(Ordering::Relaxed) {
            ROOT_PUBLICATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
pub fn instrumentation_begin() { instrumentation::begin(); }

#[cfg(feature = "accepted-match-transition-instrumentation")]
pub fn instrumentation_end() -> TransitionInstrumentation { instrumentation::end() }

#[cfg(feature = "accepted-match-transition-instrumentation")]
pub(super) fn record_write_acquisition(wait: std::time::Duration) -> bool {
    instrumentation::record_acquisition(wait)
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
pub(super) fn record_write_hold(hold: std::time::Duration) { instrumentation::record_hold(hold); }

#[cfg(feature = "accepted-match-transition-instrumentation")]
pub(crate) fn record_pathmap_root_publication() { instrumentation::record_root_publication(); }

#[derive(Clone, Copy, Debug, Default)]
struct EmptyHistory;

impl HistoryReaderBase<String, String, String, String> for EmptyHistory {
    fn get_data_proj(&self, _key: &String) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_proj(
        &self,
        _key: &Vec<String>,
    ) -> Vec<WaitingContinuation<String, String>> {
        Vec::new()
    }

    fn get_joins_proj(&self, _key: &String) -> Vec<Vec<String>> { Vec::new() }
}
