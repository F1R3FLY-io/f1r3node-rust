//! The host-work charges and the work bound of a space operation (readiness review S5,
//! revised M-3, I-12, P4 design A10, and decisions 40 and 42).
//!
//! A space job runs in its own task, where `deterministic_reduction::current()` is `None`, so
//! `reserve_host_work` charges nothing there. The scheduler captures the host-work budget of
//! the deploy before it submits the job (`SpaceRequest`), and [`ScanMeter`] charges it with
//! the rule of `feature/cost-accounted-rho` (DR-88, decision 42): the agent reserves every
//! read and every copy that it makes, before it makes it. A reservation of `operations`,
//! `scanned` and `backing` units charges the host-work dimensions as
//! `reserve_native_observation_work` (`rho_runtime.rs`) does:
//!
//! * `VerificationOperations`: the operations;
//! * `VerificationBytes`: the bytes that the work reads;
//! * `SearchStateBytes`: the bytes that the work allocates.
//!
//! The metered matcher reserves its own reads through the same meter (`SourceMeter`), and the
//! copy walks of `shared::clone_backing` reserve through it as a `BackingMeter`. Without a
//! budget the meter accepts every charge and skips every walk, as the host does. The charges do
//! not depend on cache state.
//!
//! The fire loop of a collection operation (P4 design B.4) is bounded. A bounded chain fires
//! in the transaction for at most [`FIRE_ROUND_CAP`] rounds. Then the agent commits the staged
//! prefix and re-issues the rest as a new `Intent::Space`. A divergent pair re-issues after
//! each round (A10).

use std::mem::size_of;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::space_store_handle::metered_encode;
use serde::Serialize;
use shared::rust::clone_backing::{
    self, BackingError, BackingMeter, CloneBacking, BLOCK_SHARED_HEADER_SCANNED,
};

use crate::rust::interpreter::host_work::HostWorkBudget;

/// The most fires of one collection operation in one transaction. Each round matches one head
/// with at most two metered matcher calls, so one operation makes at most
/// `2 * (FIRE_ROUND_CAP + 1)` matcher calls.
pub const FIRE_ROUND_CAP: usize = 64;

/// The host-work meter of one space job.
#[derive(Clone)]
pub struct ScanMeter {
    host_work: Option<HostWorkBudget>,
}

impl ScanMeter {
    /// A meter over the host-work budget of the deploy, or a meter that accepts every charge.
    pub fn new(host_work: Option<HostWorkBudget>) -> Self { Self { host_work } }

    /// The host-work budget of the meter.
    pub fn host_work(&self) -> Option<&HostWorkBudget> { self.host_work.as_ref() }

    /// True when the meter charges a budget. Without one, the walks of a reservation are
    /// skipped.
    pub fn is_metered(&self) -> bool { self.host_work.is_some() }

    // Disabled by decision 42 (DR-88 of feature/cost-accounted-rho, 3d3f79b07): the metered
    // matcher reserves its own reads, and the caller adds no unit for each call. The branch
    // charges `SearchCandidates` only in its funding search, never in RSpace matching.
    // /// Charges one call of a metered matcher method (`SearchCandidates`). Call it before each
    // /// `get_metered` and each `check_commit_metered`.
    // pub fn charge_call(&self) -> Result<(), RSpaceError> {
    //     self.charge(HostWorkDimension::SearchCandidates, 1)
    // }

    /// Reserves `operations`, `scanned` and `backing` units on `VerificationOperations`,
    /// `VerificationBytes` and `SearchStateBytes`, in this order, as
    /// `reserve_native_observation_work` does. A rejection is `HostWorkRejected`.
    pub fn charge(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), RSpaceError> {
        let Some(budget) = &self.host_work else {
            return Ok(());
        };
        for (dimension, amount) in [
            (HostWorkDimension::VerificationOperations, operations),
            (HostWorkDimension::VerificationBytes, scanned),
            (HostWorkDimension::SearchStateBytes, backing),
        ] {
            let units = u64::try_from(amount).map_err(|_| RSpaceError::HostWorkRejected)?;
            budget
                .reserve(dimension, HostWorkUnits::new(units))
                .map_err(|_| RSpaceError::HostWorkRejected)?;
        }
        Ok(())
    }

    /// Reserves a read of `value`: the walk over its parts, with no copy.
    pub fn inspect<T: CloneBacking>(&self, value: &T) -> Result<(), RSpaceError> {
        match self.is_metered() {
            true => clone_backing::inspect(value, self).map_err(rejected),
            false => Ok(()),
        }
    }

    /// Reserves a copy of `value` and its cleanup, as `SorterMeter::clone` of the branch does.
    /// The caller then makes the copy.
    pub fn reserve_copy<T: CloneBacking>(&self, value: &T) -> Result<(), RSpaceError> {
        match self.is_metered() {
            true => clone_backing::reserve_copy_and_cleanup(value, self).map_err(rejected),
            false => Ok(()),
        }
    }

    /// Reserves a copy of the slice `values`, its allocation included, and its cleanup. The
    /// caller then makes the copy.
    pub fn reserve_copy_slice<T: CloneBacking>(&self, values: &[T]) -> Result<(), RSpaceError> {
        match self.is_metered() {
            true => clone_backing::reserve_slice_copy_and_cleanup(values, self).map_err(rejected),
            false => Ok(()),
        }
    }

    /// Reserves a copy of `value` and its cleanup, then copies it.
    pub fn copy<T: Clone + CloneBacking>(&self, value: &T) -> Result<T, RSpaceError> {
        self.reserve_copy(value)?;
        Ok(value.clone())
    }

    /// Reserves a copy of the slice `values` and its cleanup, then copies it.
    pub fn copy_slice<T: Clone + CloneBacking>(&self, values: &[T]) -> Result<Vec<T>, RSpaceError> {
        self.reserve_copy_slice(values)?;
        Ok(values.to_vec())
    }

    /// Reserves the allocation of a vector with room for `length` values: `length + 1`
    /// operations and `length · size_of::<T>()` bytes, as the metered candidate buffer of the
    /// branch does (`native_candidate/metered.rs`, `buffer`). The caller then allocates it.
    pub fn reserve_buffer<T>(&self, length: usize) -> Result<(), RSpaceError> {
        if !self.is_metered() {
            return Ok(());
        }
        let bytes = length
            .checked_mul(size_of::<T>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        let operations = length.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?;
        self.charge(operations, bytes, bytes)
    }

    /// Reserves and allocates an empty vector with room for `length` values.
    pub fn buffer<T>(&self, length: usize) -> Result<Vec<T>, RSpaceError> {
        self.reserve_buffer::<T>(length)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(length)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        Ok(values)
    }

    /// Reserves one comparison that reads `left` and `right` bytes, as `MeteredOrder::scan` of
    /// the branch does.
    pub fn compare(&self, left: usize, right: usize) -> Result<(), RSpaceError> {
        let scanned = left
            .checked_add(right)
            .ok_or(RSpaceError::HostWorkRejected)?;
        self.charge(1, scanned, 0)
    }

    /// Reserves one move of a slot of `bytes` bytes, as the bound values of the branch's
    /// matcher move out of its free map (DR-91).
    pub fn moved(&self, bytes: usize) -> Result<(), RSpaceError> { self.charge(1, bytes, 0) }

    /// Reserves `count` copies of a shared pointer: each reads and writes the strong count in
    /// the header of its allocation (`BLOCK_SHARED_HEADER_SCANNED`, DR-92).
    pub fn shared(&self, count: usize) -> Result<(), RSpaceError> {
        let scanned = count
            .checked_mul(BLOCK_SHARED_HEADER_SCANNED)
            .ok_or(RSpaceError::HostWorkRejected)?;
        self.charge(count, scanned, 0)
    }

    /// Reserves the bytes of `random.to_bytes()`, with the formula of
    /// `reserve_reducer_random_bytes` (`reduce.rs`).
    pub fn random_bytes(&self, random: &Blake2b512Random) -> Result<(), RSpaceError> {
        if !self.is_metered() {
            return Ok(());
        }
        let count_bytes = random
            .count_view
            .len()
            .checked_mul(8)
            .ok_or(RSpaceError::HostWorkRejected)?;
        let output_bytes = 168usize
            .checked_add(random.last_block.len())
            .and_then(|size| size.checked_add(random.path_view.len()))
            .and_then(|size| size.checked_add(count_bytes))
            .ok_or(RSpaceError::HostWorkRejected)?;
        let allocation = output_bytes
            .checked_mul(4)
            .and_then(|size| size.checked_add(144))
            .ok_or(RSpaceError::HostWorkRejected)?;
        self.charge(output_bytes, output_bytes, allocation)
    }

    /// The bincode bytes of `value`. A metered encode reserves each write and each growth of
    /// its buffer with the writer of the host (`native_source::encode`).
    pub fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, RSpaceError> {
        match self.is_metered() {
            true => metered_encode(value, self),
            false => bincode::serialize(value)
                .map_err(|error| RSpaceError::InterpreterError(error.to_string())),
        }
    }
}

fn rejected(_: BackingError) -> RSpaceError { RSpaceError::HostWorkRejected }

impl SourceMeter for ScanMeter {
    /// Charges a reservation of the metered matcher or of a metered encode.
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), RSpaceError> {
        self.charge(operations, scanned, backing)
    }
}

impl BackingMeter for ScanMeter {
    /// Charges a reservation of a copy walk.
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), BackingError> {
        self.charge(operations, scanned, backing)
            .map_err(|_| BackingError::Rejected)
    }
}

// Disabled by decision 42 (DR-88 of feature/cost-accounted-rho, 3d3f79b07): the branch maps
// the operations, the scanned bytes and the backing bytes of a reservation to three host-work
// dimensions, and this meter charged only the scanned bytes, on `SearchStateBytes`.
// impl SourceMeter for ScanMeter {
//     /// Charges the bytes that a matcher call scans (`SearchStateBytes`).
//     fn reserve(
//         &self,
//         _operations: usize,
//         scanned: usize,
//         _backing: usize,
//     ) -> Result<(), RSpaceError> {
//         self.charge(HostWorkDimension::SearchStateBytes, scanned)
//     }
// }

/// Why a collection operation re-issues as a new `Intent::Space`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReissueCause {
    /// Neither side of the fire left the store and the incoming side survives: a persistent
    /// send that fired a persistent waiter, or a persistent receive that read a persistent or
    /// peeked record. It re-issues after each round (A10).
    DivergentPair,
    /// The fire loop reached [`FIRE_ROUND_CAP`] (readiness review S5).
    RoundCap,
}

/// The step of a fire loop after one fire (P4 design B.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireStep {
    /// The incoming side is spent. The operation ends.
    Done,
    /// The incoming side survives and its partner left. Offer the incoming side to the next
    /// head in this transaction.
    Continue,
    /// Commit the staged prefix and re-issue the operation.
    Reissue(ReissueCause),
}

/// The fires of the fire loop of one collection operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FireRounds {
    fired: usize,
}

impl FireRounds {
    /// A loop that has not fired.
    pub const fn new() -> Self { Self { fired: 0 } }

    /// The fires so far.
    pub const fn fired(&self) -> usize { self.fired }

    /// Records one fire and returns the next step (A10, readiness review S5).
    ///
    /// * `incoming_spent`: the fire used up the datum or the continuation of the operation. For
    ///   a produce, the datum is not persistent and the fired waiter does not peek. For a
    ///   consume, the continuation is not persistent.
    /// * `partner_stays`: the fired partner stays in the store. For a produce, the fired waiter
    ///   is persistent. For a consume, the fired record is persistent or the consume peeks.
    pub fn after_fire(&mut self, incoming_spent: bool, partner_stays: bool) -> FireStep {
        let Some(fired) = self.fired.checked_add(1) else {
            return FireStep::Reissue(ReissueCause::RoundCap);
        };
        self.fired = fired;
        match (incoming_spent, partner_stays) {
            (true, _) => FireStep::Done,
            (false, true) => FireStep::Reissue(ReissueCause::DivergentPair),
            (false, false) if fired >= FIRE_ROUND_CAP => FireStep::Reissue(ReissueCause::RoundCap),
            (false, false) => FireStep::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::{BindPattern, ListParWithRandom};
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits, HostWorkUsage};
    use models::rust::utils::{new_freevar_par, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::r#match::Match;

    use super::*;
    use crate::rust::interpreter::matcher::r#match::Matcher;

    fn budget(limit: u64) -> HostWorkBudget {
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(limit)))
    }

    /// The dimensions that a space job charges (decision 42).
    const CHARGED: [HostWorkDimension; 3] = [
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::VerificationBytes,
        HostWorkDimension::SearchStateBytes,
    ];

    #[test]
    fn scan_meter_maps_amounts_to_the_branch_dimensions() {
        let host_work = budget(1_000);
        let meter = ScanMeter::new(Some(host_work.clone()));
        meter.charge(5, 40, 7).expect("the amounts fit the budget");
        let expected = [5, 40, 7];
        for (dimension, amount) in CHARGED.into_iter().zip(expected) {
            assert_eq!(
                host_work.usage(dimension),
                HostWorkUsage::new(amount),
                "{dimension}"
            );
        }
        for dimension in HostWorkDimension::ALL {
            if !CHARGED.contains(&dimension) {
                assert_eq!(
                    host_work.usage(dimension),
                    HostWorkUsage::ZERO,
                    "{dimension}"
                );
            }
        }
    }

    #[test]
    fn scan_meter_accepts_exact_credit_and_rejects_each_smaller_dimension() {
        let amounts = [3_u64, 11, 5];
        let mut exact = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        for (dimension, amount) in CHARGED.into_iter().zip(amounts) {
            exact.set(dimension, HostWorkLimit::new(amount));
        }
        ScanMeter::new(Some(HostWorkBudget::new(exact)))
            .charge(3, 11, 5)
            .expect("the exact credit suffices");
        for (dimension, amount) in CHARGED.into_iter().zip(amounts) {
            let mut cut = exact;
            cut.set(dimension, HostWorkLimit::new(amount - 1));
            let host_work = HostWorkBudget::new(cut);
            assert_eq!(
                ScanMeter::new(Some(host_work.clone())).charge(3, 11, 5),
                Err(RSpaceError::HostWorkRejected),
                "{dimension}"
            );
            assert!(host_work.is_rejected(), "{dimension}");
        }
        let unmetered = ScanMeter::new(None);
        unmetered
            .charge(usize::MAX, usize::MAX, usize::MAX)
            .expect("no budget accepts every charge");
        assert!(unmetered.host_work().is_none());
        assert!(!unmetered.is_metered());
    }

    #[test]
    fn scan_meter_copy_equals_clone_backing_copy_and_cleanup() {
        let value = ListParWithRandom {
            pars: vec![
                new_gstring_par("payload".to_string(), Vec::new(), false),
                new_gint_par(7, Vec::new(), false),
            ],
            random_state: vec![9; 64],
            cost_authority: None,
            cost_stack: None,
        };
        let recorded = std::cell::Cell::new([0_usize; 3]);
        let recorder = |operations: usize, scanned: usize, backing: usize| {
            let [o, s, b] = recorded.get();
            recorded.set([o + operations, s + scanned, b + backing]);
            Ok::<(), BackingError>(())
        };
        clone_backing::reserve_copy_and_cleanup(&value, &recorder)
            .expect("the recording meter accepts every charge");
        let host_work = budget(u64::MAX);
        let copy = ScanMeter::new(Some(host_work.clone()))
            .copy(&value)
            .expect("the copy fits the budget");
        assert_eq!(copy, value);
        let charged: Vec<usize> = CHARGED
            .iter()
            .map(|dimension| {
                usize::try_from(host_work.usage(*dimension).get()).expect("a small usage")
            })
            .collect();
        assert_eq!(charged, recorded.get().to_vec());
        assert!(recorded.get().iter().all(|amount| *amount > 0));
    }

    #[test]
    fn scan_meter_skips_walks_without_a_budget() {
        let value = vec![new_gstring_par("x".repeat(4096), Vec::new(), false); 16];
        let unmetered = ScanMeter::new(None);
        unmetered.inspect(&value).expect("no budget");
        unmetered.reserve_copy(&value).expect("no budget");
        assert_eq!(unmetered.copy(&value).expect("no budget"), value);
        assert_eq!(unmetered.copy_slice(&value).expect("no budget"), value);
        assert_eq!(unmetered.buffer::<u8>(8).expect("no budget").capacity(), 8);
        unmetered
            .random_bytes(&Blake2b512Random::create_from_bytes(b"random"))
            .expect("no budget");
        let host_work = budget(u64::MAX);
        let metered = ScanMeter::new(Some(host_work.clone()));
        assert_eq!(
            metered.encode(&value).expect("the encode fits"),
            unmetered.encode(&value).expect("no budget"),
            "a metered encode gives the bytes of bincode"
        );
        assert!(host_work.usage(HostWorkDimension::VerificationBytes) > HostWorkUsage::ZERO);
    }

    #[test]
    fn metered_matching_reserves_on_the_branch_dimensions() {
        let host_work = budget(1_000_000);
        let meter = ScanMeter::new(Some(host_work.clone()));
        let pattern = BindPattern {
            patterns: vec![new_freevar_par(0, Vec::new())],
            remainder: None,
            free_count: 1,
        };
        let data = ListParWithRandom {
            pars: vec![new_gstring_par("payload".to_string(), Vec::new(), false)],
            random_state: Vec::new(),
            cost_authority: None,
            cost_stack: None,
        };
        let matched = Matcher
            .get_metered(&pattern, &data, &meter)
            .expect("the metered match fits the budget");
        assert!(matched.is_some());
        assert_eq!(matched, Matcher.get(&pattern, &data));
        assert_eq!(
            host_work.usage(HostWorkDimension::SearchCandidates),
            HostWorkUsage::ZERO,
            "the branch charges candidates only in its funding search"
        );
        for dimension in CHARGED {
            assert!(
                host_work.usage(dimension) > HostWorkUsage::ZERO,
                "{dimension}"
            );
        }
        let mismatch = BindPattern {
            patterns: vec![new_gint_par(1, Vec::new(), false)],
            remainder: None,
            free_count: 0,
        };
        assert_eq!(
            Matcher
                .get_metered(&mismatch, &data, &meter)
                .expect("the metered match fits the budget"),
            None
        );
    }

    #[test]
    fn fire_rounds_follow_the_a10_rule_and_the_cap() {
        let mut spent = FireRounds::new();
        assert_eq!(spent.after_fire(true, true), FireStep::Done);
        assert_eq!(spent.after_fire(true, false), FireStep::Done);
        let mut divergent = FireRounds::new();
        assert_eq!(
            divergent.after_fire(false, true),
            FireStep::Reissue(ReissueCause::DivergentPair)
        );
        let mut bounded = FireRounds::new();
        for round in 1..FIRE_ROUND_CAP {
            assert_eq!(
                bounded.after_fire(false, false),
                FireStep::Continue,
                "{round}"
            );
            assert_eq!(bounded.fired(), round);
        }
        assert_eq!(
            bounded.after_fire(false, false),
            FireStep::Reissue(ReissueCause::RoundCap)
        );
        assert_eq!(bounded.fired(), FIRE_ROUND_CAP);
        let mut last = FireRounds::new();
        for _ in 1..FIRE_ROUND_CAP {
            last.after_fire(false, false);
        }
        assert_eq!(last.after_fire(true, false), FireStep::Done);
        assert_eq!(FireRounds::default(), FireRounds::new());
    }
}
