//! The work bound of a space operation (readiness review S5, P4 design A10).
//!
//! A space operation meters only what its equivalent host operation meters: its COMMs and the
//! bytes that it stores, through the observer calls of the host (I-12). The host-work meter of
//! the space jobs is removed with the host-work metering of the cost-accounting branch (DR-131).
//!
//! The fire loop of a collection operation (P4 design B.4) is bounded. A bounded chain fires
//! in the transaction for at most [`FIRE_ROUND_CAP`] rounds. Then the agent commits the staged
// Changed by decision 90: racing evaluation has no `Intent::Space`. The text before:
// //! prefix and re-issues the rest as a new `Intent::Space`. A divergent pair re-issues after
//! prefix and re-issues the rest as a new space job. A divergent pair re-issues after
//! each round (A10).

/// The most fires of one collection operation in one transaction. Each round matches one head
/// with at most two matcher calls, so one operation makes at most `2 * (FIRE_ROUND_CAP + 1)`
/// matcher calls.
pub const FIRE_ROUND_CAP: usize = 64;

// Changed by decision 90: racing evaluation has no `Intent::Space`. The text before:
// /// Why a collection operation re-issues as a new `Intent::Space`.
/// Why a collection operation re-issues as a new space job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReissueCause {
    /// Neither side of the fire left the store and the incoming side survives: a persistent
    /// send that fired a persistent waiter, or a persistent receive that read a persistent or
    /// peeked record. It re-issues after each round (A10).
    DivergentPair,
    /// The fire loop reached [`FIRE_ROUND_CAP`] (readiness review S5).
    RoundCap,
    /// Added by decision 124 (plan v4 C7.7, rule CR-31): the finish of a deviating transaction
    /// took the Stop branch. The re-issue stages its anchor first (CR-19 case 2).
    OwnerRefusal,
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

    /// Added by decision 124 (plan v4 C7.11, ruling RDOJ 5.2): a loop that has fired `fired`
    /// times. The finish rebuilds the step of row d with the fire count before it (CR-31).
    pub const fn at(fired: usize) -> Self { Self { fired } }

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
    use super::*;

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

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RDOJ 5.2): a loop rebuilt with
    /// the fire count before row d takes the step of the fire of row d in a loop from the start,
    /// so the finish reads the cap of row d from the count alone.
    #[test]
    fn fire_rounds_at_rebuilds_the_step_of_row_d() {
        for fired in [0, 1, 62, 63] {
            let mut rounds = FireRounds::new();
            for _ in 0..fired {
                rounds.after_fire(false, false);
            }
            assert_eq!(
                FireRounds::at(fired).after_fire(false, false),
                rounds.after_fire(false, false),
                "{fired}"
            );
        }
        assert_eq!(
            FireRounds::at(63).after_fire(false, false),
            FireStep::Reissue(ReissueCause::RoundCap)
        );
    }
}
