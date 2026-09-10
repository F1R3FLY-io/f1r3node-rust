#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryPass {
    remaining: usize,
    failed: bool,
    proposal: bool,
}

impl RecoveryPass {
    pub fn new(candidates: usize, proposal: bool) -> Self {
        Self {
            remaining: candidates,
            failed: false,
            proposal,
        }
    }

    pub fn remaining(&self) -> usize { self.remaining }

    pub fn visit(&mut self) -> bool {
        if self.remaining == 0 {
            false
        } else {
            self.remaining -= 1;
            true
        }
    }

    pub fn fail(&mut self) { self.failed = true; }

    pub fn proposal_ready(&self) -> bool { self.remaining == 0 && !self.failed && self.proposal }
}
