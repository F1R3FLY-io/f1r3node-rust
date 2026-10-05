//! The host-work charges and the work bound of a space operation (readiness review S5,
//! revised M-3, I-12 and P4 design A10).
//!
//! A space job runs in its own task, where `deterministic_reduction::current()` is `None`, so
//! `reserve_host_work` charges nothing there. The scheduler captures the host-work budget of
//! the deploy before it submits the job (`SpaceRequest`), and [`ScanMeter`] charges it:
//!
//! * `SearchCandidates`: one unit for each call of a metered matcher method;
//! * `SearchStateBytes`: one unit for each byte that a matcher call scans.
//!
//! Without a budget the meter accepts every charge, as `reserve_host_work` does. The charges
//! do not depend on cache state.
//!
//! The fire loop of a collection operation (P4 design B.4) is bounded. A bounded chain fires
//! in the transaction for at most [`FIRE_ROUND_CAP`] rounds. Then the agent commits the staged
//! prefix and re-issues the rest as a new `Intent::Space`. A divergent pair re-issues after
//! each round (A10).

use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;

use crate::rust::interpreter::host_work::HostWorkBudget;

/// The most fires of one collection operation in one transaction. Each round matches one head
/// with at most two metered matcher calls, so one operation makes at most
/// `2 * (FIRE_ROUND_CAP + 1)` matcher calls.
pub const FIRE_ROUND_CAP: usize = 64;

/// The host-work meter of the matcher calls of one space job.
#[derive(Clone)]
pub struct ScanMeter {
    host_work: Option<HostWorkBudget>,
}

impl ScanMeter {
    /// A meter over the host-work budget of the deploy, or a meter that accepts every charge.
    pub fn new(host_work: Option<HostWorkBudget>) -> Self { Self { host_work } }

    /// The host-work budget of the meter.
    pub fn host_work(&self) -> Option<&HostWorkBudget> { self.host_work.as_ref() }

    /// Charges one call of a metered matcher method (`SearchCandidates`). Call it before each
    /// `get_metered` and each `check_commit_metered`.
    pub fn charge_call(&self) -> Result<(), RSpaceError> {
        self.charge(HostWorkDimension::SearchCandidates, 1)
    }

    fn charge(&self, dimension: HostWorkDimension, units: usize) -> Result<(), RSpaceError> {
        let Some(budget) = &self.host_work else {
            return Ok(());
        };
        let units = u64::try_from(units).map_err(|_| RSpaceError::HostWorkRejected)?;
        budget
            .reserve(dimension, HostWorkUnits::new(units))
            .map(|_| ())
            .map_err(|_| RSpaceError::HostWorkRejected)
    }
}

impl SourceMeter for ScanMeter {
    /// Charges the bytes that a matcher call scans (`SearchStateBytes`).
    fn reserve(
        &self,
        _operations: usize,
        scanned: usize,
        _backing: usize,
    ) -> Result<(), RSpaceError> {
        self.charge(HostWorkDimension::SearchStateBytes, scanned)
    }
}

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

    #[test]
    fn scan_meter_charges_search_candidates_and_scanned_bytes() {
        let host_work = budget(1_000);
        let meter = ScanMeter::new(Some(host_work.clone()));
        meter.charge_call().expect("a call fits the budget");
        meter.charge_call().expect("a call fits the budget");
        meter.reserve(5, 40, 7).expect("the bytes fit the budget");
        assert_eq!(
            host_work.usage(HostWorkDimension::SearchCandidates),
            HostWorkUsage::new(2)
        );
        assert_eq!(
            host_work.usage(HostWorkDimension::SearchStateBytes),
            HostWorkUsage::new(40)
        );
        for dimension in HostWorkDimension::ALL {
            if !matches!(
                dimension,
                HostWorkDimension::SearchCandidates | HostWorkDimension::SearchStateBytes
            ) {
                assert_eq!(
                    host_work.usage(dimension),
                    HostWorkUsage::ZERO,
                    "{dimension}"
                );
            }
        }
    }

    #[test]
    fn scan_meter_rejects_a_charge_over_the_limit() {
        let host_work = budget(1);
        let meter = ScanMeter::new(Some(host_work.clone()));
        meter.charge_call().expect("the first call fits");
        assert_eq!(meter.charge_call(), Err(RSpaceError::HostWorkRejected));
        assert_eq!(meter.reserve(0, 2, 0), Err(RSpaceError::HostWorkRejected));
        assert!(host_work.is_rejected());
        let unmetered = ScanMeter::new(None);
        unmetered
            .charge_call()
            .expect("no budget accepts every charge");
        unmetered
            .reserve(0, usize::MAX, 0)
            .expect("no budget accepts every charge");
        assert!(unmetered.host_work().is_none());
    }

    #[test]
    fn metered_matching_charges_the_scanned_bytes() {
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
        meter.charge_call().expect("a call fits");
        let matched = Matcher
            .get_metered(&pattern, &data, &meter)
            .expect("the metered match fits the budget");
        assert!(matched.is_some());
        assert_eq!(matched, Matcher.get(&pattern, &data));
        assert_eq!(
            host_work.usage(HostWorkDimension::SearchCandidates),
            HostWorkUsage::new(1)
        );
        assert!(host_work.usage(HostWorkDimension::SearchStateBytes) > HostWorkUsage::ZERO);
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
