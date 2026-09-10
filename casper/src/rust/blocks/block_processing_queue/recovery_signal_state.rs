use super::{AtomicU8, Ordering};

const IDLE: u8 = 0;
const REQUESTED: u8 = 1;
const PROPOSAL: u8 = 2;
const STOPPED: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryWake {
    Idle,
    Work { proposal: bool },
    Stopped,
}

impl RecoveryWake {
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Stopped, _) | (_, Self::Stopped) => Self::Stopped,
            (Self::Idle, work) | (work, Self::Idle) => work,
            (Self::Work { proposal: left }, Self::Work { proposal: right }) => Self::Work {
                proposal: left || right,
            },
        }
    }
}

#[derive(Debug)]
pub struct RecoverySignalState {
    state: AtomicU8,
}

impl Default for RecoverySignalState {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(IDLE),
        }
    }
}

impl RecoverySignalState {
    pub fn request(&self, proposal: bool) -> bool {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            if current == STOPPED {
                return false;
            }
            let next = current | REQUESTED | if proposal { PROPOSAL } else { 0 };
            match self.state.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return current == IDLE,
                Err(observed) => current = observed,
            }
        }
    }

    pub fn take(&self) -> RecoveryWake {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            match current {
                IDLE => return RecoveryWake::Idle,
                STOPPED => return RecoveryWake::Stopped,
                _ => match self.state.compare_exchange_weak(
                    current,
                    IDLE,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        return RecoveryWake::Work {
                            proposal: current & PROPOSAL != 0,
                        }
                    }
                    Err(observed) => current = observed,
                },
            }
        }
    }

    pub fn stop(&self) -> bool { self.state.swap(STOPPED, Ordering::AcqRel) != STOPPED }

    pub fn is_stopped(&self) -> bool { self.state.load(Ordering::Acquire) == STOPPED }
}
