use std::sync::atomic::{AtomicBool, Ordering};

pub const DEFAULT_LAG_THRESHOLD: u64 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lag {
    Blocks(u64),
    Unknown,
}

impl Lag {
    pub fn measure(lfb_number: Option<i64>, executed_number: Option<i64>) -> Self {
        match (lfb_number, executed_number) {
            (Some(lfb), Some(executed)) => Lag::Blocks(lfb.saturating_sub(executed).max(0) as u64),
            _ => Lag::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    Engaged,
    Released,
    Unchanged,
}

pub struct LagGate {
    threshold: u64,
    engaged: AtomicBool,
}

impl LagGate {
    pub fn new(threshold: u64) -> Self {
        LagGate {
            threshold,
            engaged: AtomicBool::new(false),
        }
    }

    pub fn threshold(&self) -> u64 { self.threshold }

    pub fn engaged(&self) -> bool { self.engaged.load(Ordering::SeqCst) }

    pub fn admits(&self, lag: Lag) -> (bool, Transition) {
        let hold = match lag {
            Lag::Blocks(blocks) => blocks > self.threshold,
            Lag::Unknown => true,
        };
        let was = self.engaged.swap(hold, Ordering::SeqCst);
        let transition = match (was, hold) {
            (false, true) => Transition::Engaged,
            (true, false) => Transition::Released,
            _ => Transition::Unchanged,
        };
        (!hold, transition)
    }
}

impl Default for LagGate {
    fn default() -> Self { LagGate::new(DEFAULT_LAG_THRESHOLD) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lag_is_the_block_number_distance_and_never_negative() {
        assert_eq!(Lag::measure(Some(40), Some(8)), Lag::Blocks(32));
        assert_eq!(Lag::measure(Some(8), Some(8)), Lag::Blocks(0));
        assert_eq!(Lag::measure(Some(3), Some(8)), Lag::Blocks(0));
        assert_eq!(Lag::measure(None, Some(8)), Lag::Unknown);
        assert_eq!(Lag::measure(Some(8), None), Lag::Unknown);
    }

    #[test]
    fn envelopes_are_admitted_up_to_the_threshold_and_held_above_it() {
        let gate = LagGate::new(32);
        assert_eq!(gate.admits(Lag::Blocks(0)), (true, Transition::Unchanged));
        assert_eq!(gate.admits(Lag::Blocks(32)), (true, Transition::Unchanged));
        assert!(!gate.engaged());
        assert_eq!(gate.admits(Lag::Blocks(33)), (false, Transition::Engaged));
        assert!(gate.engaged());
    }

    #[test]
    fn engaging_and_releasing_happen_once_per_crossing() {
        let gate = LagGate::new(2);
        let transitions: Vec<_> = [0, 3, 5, 4, 2, 1, 3]
            .into_iter()
            .map(|blocks| gate.admits(Lag::Blocks(blocks)).1)
            .collect();
        assert_eq!(transitions, vec![
            Transition::Unchanged,
            Transition::Engaged,
            Transition::Unchanged,
            Transition::Unchanged,
            Transition::Released,
            Transition::Unchanged,
            Transition::Engaged,
        ]);
    }

    #[test]
    fn an_unknown_lag_holds_envelopes_back() {
        let gate = LagGate::new(32);
        assert_eq!(gate.admits(Lag::Unknown), (false, Transition::Engaged));
        assert_eq!(gate.admits(Lag::Blocks(1)), (true, Transition::Released));
    }

    #[test]
    fn the_default_threshold_is_32_blocks() {
        assert_eq!(LagGate::default().threshold(), 32);
    }
}
