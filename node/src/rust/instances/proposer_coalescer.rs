use std::sync::atomic::{AtomicU8, Ordering};

use casper::rust::ProposeRequestKind;

const IDLE: u8 = 0;
const ACTIVE: u8 = 1;
const ACTIVE_DIRTY: u8 = 3;

pub(crate) trait AtomicGate: Send + Sync {
    fn load(&self, order: Ordering) -> u8;

    fn compare_exchange_weak(
        &self,
        current: u8,
        new: u8,
        success: Ordering,
        failure: Ordering,
    ) -> Result<u8, u8>;
}

impl AtomicGate for AtomicU8 {
    fn load(&self, order: Ordering) -> u8 { AtomicU8::load(self, order) }

    fn compare_exchange_weak(
        &self,
        current: u8,
        new: u8,
        success: Ordering,
        failure: Ordering,
    ) -> Result<u8, u8> {
        AtomicU8::compare_exchange_weak(self, current, new, success, failure)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProposalRequestKind {
    Manual,
    PendingDeploy,
    FinalityRecovery,
}

impl From<&ProposeRequestKind> for ProposalRequestKind {
    fn from(kind: &ProposeRequestKind) -> Self {
        match kind {
            ProposeRequestKind::Manual => Self::Manual,
            ProposeRequestKind::PendingDeploy => Self::PendingDeploy,
            ProposeRequestKind::FinalityRecovery(_) => Self::FinalityRecovery,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdmissionOutcome {
    Acquired,
    Coalesced,
    Busy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FinishOutcome {
    Idle,
    PendingFollowUp,
}

#[derive(Debug)]
pub(crate) struct ProposerCoalescer<A: AtomicGate = AtomicU8> {
    state: A,
}

impl ProposerCoalescer<AtomicU8> {
    pub(crate) fn new() -> Self {
        Self {
            state: AtomicU8::new(IDLE),
        }
    }
}

impl<A: AtomicGate> ProposerCoalescer<A> {
    #[cfg(test)]
    fn with_atomic(state: A) -> Self { Self { state } }

    pub(crate) fn try_admit(&self, kind: ProposalRequestKind) -> AdmissionOutcome {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            match current {
                IDLE => {
                    match self.state.compare_exchange_weak(
                        IDLE,
                        ACTIVE,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => return AdmissionOutcome::Acquired,
                        Err(observed) => current = observed,
                    }
                }
                ACTIVE if kind == ProposalRequestKind::PendingDeploy => {
                    match self.state.compare_exchange_weak(
                        ACTIVE,
                        ACTIVE_DIRTY,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => return AdmissionOutcome::Coalesced,
                        Err(observed) => current = observed,
                    }
                }
                ACTIVE_DIRTY if kind == ProposalRequestKind::PendingDeploy => {
                    return AdmissionOutcome::Coalesced;
                }
                ACTIVE | ACTIVE_DIRTY => return AdmissionOutcome::Busy,
                invalid => panic!("invalid proposer coalescer state: {invalid}"),
            }
        }
    }

    pub(crate) fn finish(&self) -> FinishOutcome {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            let (next, outcome) = match current {
                ACTIVE => (IDLE, FinishOutcome::Idle),
                ACTIVE_DIRTY => (ACTIVE, FinishOutcome::PendingFollowUp),
                IDLE => panic!("cannot finish an idle proposer coalescer"),
                invalid => panic!("invalid proposer coalescer state: {invalid}"),
            };
            match self.state.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return outcome,
                Err(observed) => current = observed,
            }
        }
    }

    pub(crate) fn cancel(&self) {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            match current {
                ACTIVE | ACTIVE_DIRTY => {}
                IDLE => panic!("cannot cancel an idle proposer coalescer"),
                invalid => panic!("invalid proposer coalescer state: {invalid}"),
            }
            match self.state.compare_exchange_weak(
                current,
                IDLE,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    #[cfg(test)]
    fn raw_state(&self) -> u8 { self.state.load(Ordering::Acquire) }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use loom::sync::atomic::AtomicU8 as LoomAtomicU8;
    use loom::sync::Arc as LoomArc;

    use super::*;

    impl AtomicGate for LoomAtomicU8 {
        fn load(&self, order: Ordering) -> u8 { LoomAtomicU8::load(self, order) }

        fn compare_exchange_weak(
            &self,
            current: u8,
            new: u8,
            success: Ordering,
            failure: Ordering,
        ) -> Result<u8, u8> {
            LoomAtomicU8::compare_exchange_weak(self, current, new, success, failure)
        }
    }

    fn assert_legal(state: u8) {
        assert!(matches!(state, IDLE | ACTIVE | ACTIVE_DIRTY));
    }

    #[test]
    fn every_kind_acquires_an_idle_gate() {
        for kind in [
            ProposalRequestKind::Manual,
            ProposalRequestKind::PendingDeploy,
            ProposalRequestKind::FinalityRecovery,
        ] {
            let gate = ProposerCoalescer::new();
            assert_eq!(gate.try_admit(kind), AdmissionOutcome::Acquired);
            assert_eq!(gate.raw_state(), ACTIVE);
            assert_eq!(gate.finish(), FinishOutcome::Idle);
            assert_eq!(gate.raw_state(), IDLE);
        }
    }

    #[test]
    fn casper_request_kinds_map_exhaustively_without_permit_data() {
        use casper::rust::FinalityRecoveryPermit;

        assert_eq!(
            ProposalRequestKind::from(&ProposeRequestKind::Manual),
            ProposalRequestKind::Manual
        );
        assert_eq!(
            ProposalRequestKind::from(&ProposeRequestKind::PendingDeploy),
            ProposalRequestKind::PendingDeploy
        );
        assert_eq!(
            ProposalRequestKind::from(&ProposeRequestKind::FinalityRecovery(
                FinalityRecoveryPermit {
                    lfb_hash: Default::default(),
                    lfb_height: 7,
                    recovery_round: 11,
                }
            )),
            ProposalRequestKind::FinalityRecovery
        );
    }

    #[test]
    fn pending_collisions_create_one_forced_follow_up() {
        let gate = ProposerCoalescer::new();
        assert_eq!(
            gate.try_admit(ProposalRequestKind::Manual),
            AdmissionOutcome::Acquired
        );
        for _ in 0..8 {
            assert_eq!(
                gate.try_admit(ProposalRequestKind::PendingDeploy),
                AdmissionOutcome::Coalesced
            );
        }
        assert_eq!(gate.finish(), FinishOutcome::PendingFollowUp);
        assert_eq!(gate.raw_state(), ACTIVE);
        assert_eq!(gate.finish(), FinishOutcome::Idle);
        assert_eq!(gate.raw_state(), IDLE);
    }

    #[test]
    fn manual_and_recovery_collisions_are_not_replayed() {
        let gate = ProposerCoalescer::new();
        assert_eq!(
            gate.try_admit(ProposalRequestKind::PendingDeploy),
            AdmissionOutcome::Acquired
        );
        assert_eq!(
            gate.try_admit(ProposalRequestKind::Manual),
            AdmissionOutcome::Busy
        );
        assert_eq!(
            gate.try_admit(ProposalRequestKind::FinalityRecovery),
            AdmissionOutcome::Busy
        );
        assert_eq!(gate.finish(), FinishOutcome::Idle);
        assert_eq!(gate.raw_state(), IDLE);
    }

    #[test]
    fn pending_finish_race_never_loses_work() {
        for _ in 0..128 {
            let gate = Arc::new(ProposerCoalescer::new());
            assert_eq!(
                gate.try_admit(ProposalRequestKind::Manual),
                AdmissionOutcome::Acquired
            );
            let barrier = Arc::new(Barrier::new(3));
            let collision = {
                let gate = gate.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    gate.try_admit(ProposalRequestKind::PendingDeploy)
                })
            };
            let completion = {
                let gate = gate.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    gate.finish()
                })
            };
            barrier.wait();
            let admitted = collision.join().unwrap();
            let finished = completion.join().unwrap();
            match (admitted, finished) {
                (AdmissionOutcome::Coalesced, FinishOutcome::PendingFollowUp)
                | (AdmissionOutcome::Acquired, FinishOutcome::Idle) => {}
                unexpected => panic!("lost or duplicated pending work: {unexpected:?}"),
            }
            assert_eq!(gate.finish(), FinishOutcome::Idle);
            assert_eq!(gate.raw_state(), IDLE);
        }
    }

    #[test]
    fn finite_dirty_epochs_eventually_return_to_idle() {
        let gate = ProposerCoalescer::new();
        assert_eq!(
            gate.try_admit(ProposalRequestKind::FinalityRecovery),
            AdmissionOutcome::Acquired
        );
        for _ in 0..4 {
            assert_eq!(
                gate.try_admit(ProposalRequestKind::PendingDeploy),
                AdmissionOutcome::Coalesced
            );
            assert_eq!(gate.finish(), FinishOutcome::PendingFollowUp);
            assert_legal(gate.raw_state());
        }
        assert_eq!(gate.finish(), FinishOutcome::Idle);
        assert_eq!(gate.raw_state(), IDLE);
    }

    #[test]
    fn cancellation_discards_an_unserviceable_follow_up() {
        let gate = ProposerCoalescer::new();
        assert_eq!(
            gate.try_admit(ProposalRequestKind::Manual),
            AdmissionOutcome::Acquired
        );
        assert_eq!(
            gate.try_admit(ProposalRequestKind::PendingDeploy),
            AdmissionOutcome::Coalesced
        );
        assert_eq!(gate.raw_state(), ACTIVE_DIRTY);
        gate.cancel();
        assert_eq!(gate.raw_state(), IDLE);
    }

    #[test]
    fn pending_cancel_race_leaves_no_abandoned_owner() {
        for _ in 0..128 {
            let gate = Arc::new(ProposerCoalescer::new());
            assert_eq!(
                gate.try_admit(ProposalRequestKind::Manual),
                AdmissionOutcome::Acquired
            );
            let barrier = Arc::new(Barrier::new(3));
            let collision = {
                let gate = gate.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    gate.try_admit(ProposalRequestKind::PendingDeploy)
                })
            };
            let cancellation = {
                let gate = gate.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    gate.cancel();
                })
            };
            barrier.wait();
            let admitted = collision.join().unwrap();
            cancellation.join().unwrap();
            match admitted {
                AdmissionOutcome::Coalesced => assert_eq!(gate.raw_state(), IDLE),
                AdmissionOutcome::Acquired => gate.cancel(),
                AdmissionOutcome::Busy => panic!("pending collision cannot be busy"),
            }
            assert_eq!(gate.raw_state(), IDLE);
        }
    }

    #[test]
    fn loom_pending_finish_race_never_loses_work() {
        loom::model(|| {
            let gate = LoomArc::new(ProposerCoalescer::with_atomic(LoomAtomicU8::new(IDLE)));
            assert_eq!(
                gate.try_admit(ProposalRequestKind::Manual),
                AdmissionOutcome::Acquired
            );
            let collision = {
                let gate = gate.clone();
                loom::thread::spawn(move || gate.try_admit(ProposalRequestKind::PendingDeploy))
            };
            let completion = {
                let gate = gate.clone();
                loom::thread::spawn(move || gate.finish())
            };
            let admitted = collision.join().unwrap();
            let finished = completion.join().unwrap();
            match (admitted, finished) {
                (AdmissionOutcome::Coalesced, FinishOutcome::PendingFollowUp)
                | (AdmissionOutcome::Acquired, FinishOutcome::Idle) => {}
                unexpected => panic!("lost or duplicated pending work: {unexpected:?}"),
            }
            assert_legal(gate.raw_state());
            assert_eq!(gate.finish(), FinishOutcome::Idle);
            assert_eq!(gate.raw_state(), IDLE);
        });
    }

    #[test]
    fn loom_many_pending_collisions_create_one_follow_up() {
        loom::model(|| {
            let gate = LoomArc::new(ProposerCoalescer::with_atomic(LoomAtomicU8::new(IDLE)));
            assert_eq!(
                gate.try_admit(ProposalRequestKind::Manual),
                AdmissionOutcome::Acquired
            );
            let first = {
                let gate = gate.clone();
                loom::thread::spawn(move || gate.try_admit(ProposalRequestKind::PendingDeploy))
            };
            let second = {
                let gate = gate.clone();
                loom::thread::spawn(move || gate.try_admit(ProposalRequestKind::PendingDeploy))
            };
            assert_eq!(first.join().unwrap(), AdmissionOutcome::Coalesced);
            assert_eq!(second.join().unwrap(), AdmissionOutcome::Coalesced);
            assert_eq!(gate.raw_state(), ACTIVE_DIRTY);
            assert_eq!(gate.finish(), FinishOutcome::PendingFollowUp);
            assert_eq!(gate.raw_state(), ACTIVE);
            assert_eq!(gate.finish(), FinishOutcome::Idle);
        });
    }

    #[test]
    fn loom_manual_and_recovery_collisions_do_not_dirty() {
        loom::model(|| {
            let gate = LoomArc::new(ProposerCoalescer::with_atomic(LoomAtomicU8::new(IDLE)));
            assert_eq!(
                gate.try_admit(ProposalRequestKind::PendingDeploy),
                AdmissionOutcome::Acquired
            );
            let manual = {
                let gate = gate.clone();
                loom::thread::spawn(move || gate.try_admit(ProposalRequestKind::Manual))
            };
            let recovery = {
                let gate = gate.clone();
                loom::thread::spawn(move || gate.try_admit(ProposalRequestKind::FinalityRecovery))
            };
            assert_eq!(manual.join().unwrap(), AdmissionOutcome::Busy);
            assert_eq!(recovery.join().unwrap(), AdmissionOutcome::Busy);
            assert_eq!(gate.raw_state(), ACTIVE);
            assert_eq!(gate.finish(), FinishOutcome::Idle);
        });
    }

    #[test]
    fn loom_all_observed_states_are_legal() {
        loom::model(|| {
            let gate = LoomArc::new(ProposerCoalescer::with_atomic(LoomAtomicU8::new(IDLE)));
            let first = {
                let gate = gate.clone();
                loom::thread::spawn(move || {
                    let outcome = gate.try_admit(ProposalRequestKind::PendingDeploy);
                    assert_legal(gate.raw_state());
                    outcome
                })
            };
            let second = {
                let gate = gate.clone();
                loom::thread::spawn(move || {
                    let outcome = gate.try_admit(ProposalRequestKind::PendingDeploy);
                    assert_legal(gate.raw_state());
                    outcome
                })
            };
            let first = first.join().unwrap();
            let second = second.join().unwrap();
            assert_legal(gate.raw_state());
            assert!(matches!(
                (first, second),
                (AdmissionOutcome::Acquired, AdmissionOutcome::Coalesced)
                    | (AdmissionOutcome::Coalesced, AdmissionOutcome::Acquired)
            ));
            assert_eq!(gate.finish(), FinishOutcome::PendingFollowUp);
            assert_eq!(gate.finish(), FinishOutcome::Idle);
        });
    }

    #[test]
    fn loom_finite_work_eventually_returns_idle() {
        loom::model(|| {
            let gate = LoomArc::new(ProposerCoalescer::with_atomic(LoomAtomicU8::new(IDLE)));
            assert_eq!(
                gate.try_admit(ProposalRequestKind::Manual),
                AdmissionOutcome::Acquired
            );
            let pending = {
                let gate = gate.clone();
                loom::thread::spawn(move || gate.try_admit(ProposalRequestKind::PendingDeploy))
            };
            assert_eq!(pending.join().unwrap(), AdmissionOutcome::Coalesced);
            assert_eq!(gate.finish(), FinishOutcome::PendingFollowUp);
            assert_eq!(
                gate.try_admit(ProposalRequestKind::FinalityRecovery),
                AdmissionOutcome::Busy
            );
            assert_eq!(gate.finish(), FinishOutcome::Idle);
            assert_eq!(gate.raw_state(), IDLE);
        });
    }

    #[test]
    fn loom_pending_cancel_race_leaves_no_abandoned_owner() {
        loom::model(|| {
            let gate = LoomArc::new(ProposerCoalescer::with_atomic(LoomAtomicU8::new(IDLE)));
            assert_eq!(
                gate.try_admit(ProposalRequestKind::Manual),
                AdmissionOutcome::Acquired
            );
            let collision = {
                let gate = gate.clone();
                loom::thread::spawn(move || gate.try_admit(ProposalRequestKind::PendingDeploy))
            };
            let cancellation = {
                let gate = gate.clone();
                loom::thread::spawn(move || gate.cancel())
            };
            let admitted = collision.join().unwrap();
            cancellation.join().unwrap();
            match admitted {
                AdmissionOutcome::Coalesced => assert_eq!(gate.raw_state(), IDLE),
                AdmissionOutcome::Acquired => gate.cancel(),
                AdmissionOutcome::Busy => panic!("pending collision cannot be busy"),
            }
            assert_eq!(gate.raw_state(), IDLE);
        });
    }
}
