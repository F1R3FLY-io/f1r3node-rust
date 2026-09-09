use imbl::OrdSet;

use super::ordered_snapshot::OrderedSnapshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Presence,
    Admission,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartupScanAction<K> {
    CheckPresence(K),
    Process(K),
    SkippedMetadata,
    PhaseChanged,
    Parked,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupScanError {
    AwaitingResult,
    UnexpectedResult,
}

pub struct StartupScan<K> {
    source: Option<OrderedSnapshot<K>>,
    selected: Option<OrdSet<K>>,
    pending: Option<K>,
    awaiting_result: bool,
    phase: Phase,
}

impl<K: Ord + Clone> StartupScan<K> {
    pub fn new(snapshot: OrderedSnapshot<K>) -> Self {
        let selected = snapshot.shared_values();
        Self {
            source: Some(OrderedSnapshot::new(selected.clone())),
            selected: Some(selected),
            pending: None,
            awaiting_result: false,
            phase: Phase::Presence,
        }
    }

    pub fn next_action(
        &mut self,
        mut has_capacity: impl FnMut() -> bool,
        is_block: impl FnOnce(&K) -> bool,
    ) -> Result<StartupScanAction<K>, StartupScanError> {
        if self.awaiting_result {
            return Err(StartupScanError::AwaitingResult);
        }
        match self.phase {
            Phase::Complete => return Ok(StartupScanAction::Complete),
            Phase::Failed => return Ok(StartupScanAction::Failed),
            Phase::Cancelled => return Ok(StartupScanAction::Cancelled),
            _ => {}
        }
        if self.pending.is_none() {
            self.pending = self.source.as_mut().and_then(Iterator::next);
        }
        let Some(key) = self.pending.as_ref() else {
            match self.phase {
                Phase::Presence => {
                    self.source = self.selected.take().map(OrderedSnapshot::new);
                    self.phase = Phase::Admission;
                    return Ok(StartupScanAction::PhaseChanged);
                }
                Phase::Admission => {
                    self.source = None;
                    self.phase = Phase::Complete;
                    return Ok(StartupScanAction::Complete);
                }
                _ => unreachable!(),
            }
        };
        if self.phase == Phase::Presence && !is_block(key) {
            if let Some(selected) = self.selected.as_mut() {
                selected.remove(key);
            }
            self.pending = None;
            return Ok(StartupScanAction::SkippedMetadata);
        }
        if !has_capacity() {
            return Ok(StartupScanAction::Parked);
        }
        self.awaiting_result = true;
        Ok(match self.phase {
            Phase::Presence => StartupScanAction::CheckPresence(key.clone()),
            Phase::Admission => StartupScanAction::Process(key.clone()),
            _ => unreachable!(),
        })
    }

    pub fn record_presence(&mut self, present: bool) -> Result<(), StartupScanError> {
        if self.phase != Phase::Presence || !self.awaiting_result {
            return Err(StartupScanError::UnexpectedResult);
        }
        if !present {
            if let (Some(selected), Some(key)) = (self.selected.as_mut(), self.pending.as_ref()) {
                selected.remove(key);
            }
        }
        self.pending = None;
        self.awaiting_result = false;
        Ok(())
    }

    pub fn record_processed(&mut self) -> Result<(), StartupScanError> {
        if self.phase != Phase::Admission || !self.awaiting_result {
            return Err(StartupScanError::UnexpectedResult);
        }
        self.pending = None;
        self.awaiting_result = false;
        Ok(())
    }

    pub fn fail(&mut self) { self.close(Phase::Failed); }

    pub fn cancel(&mut self) { self.close(Phase::Cancelled); }

    fn close(&mut self, phase: Phase) {
        if matches!(
            self.phase,
            Phase::Complete | Phase::Failed | Phase::Cancelled
        ) {
            return;
        }
        self.phase = phase;
        self.awaiting_result = false;
        self.pending = None;
        self.source = None;
        self.selected = None;
    }
}
