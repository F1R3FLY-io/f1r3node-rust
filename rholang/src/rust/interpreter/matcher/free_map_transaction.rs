//! Drive-local `FreeMap` checkpoints for the production matcher PDA.
//!
//! The public [`SpatialMatcherContext`] remains the completed-result surface.
//! An active undo journal exists only while one PDA drive owns a mutable borrow
//! of that context, so compatibility cloning and `IsolatableState` cannot copy
//! or bypass transaction state.

#[cfg(test)]
use std::cell::{Cell, RefCell};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
#[cfg(test)]
use std::process::Command;
#[cfg(test)]
use std::rc::Rc;

use models::rhoapi::Par;
use models::rust::utils::FreeMap;

use super::spatial_matcher::SpatialMatcherContext;

#[cfg(test)]
const UNWIND_PROBE_SUBJECT: &str = "RHOLANG_FREEMAP_UNWIND_PROBE_SUBJECT";

/// Observe the public map after [`FreeMapTransaction::drop`] while the child
/// process is still unwinding.  The witness is declared before the mutable
/// `RefCell` borrow, so reverse-order destruction releases the transaction and
/// borrow before this inspection runs.
#[cfg(test)]
pub(super) struct UnwindRollbackWitness {
    context: Rc<RefCell<SpatialMatcherContext>>,
    baseline: FreeMap,
    subject: String,
}

#[cfg(test)]
impl UnwindRollbackWitness {
    pub(super) fn new(
        context: Rc<RefCell<SpatialMatcherContext>>,
        baseline: FreeMap,
        subject: impl Into<String>,
    ) -> Self {
        Self {
            context,
            baseline,
            subject: subject.into(),
        }
    }
}

#[cfg(test)]
impl Drop for UnwindRollbackWitness {
    fn drop(&mut self) {
        let panicking = std::thread::panicking();
        let restored = self
            .context
            .try_borrow()
            .is_ok_and(|context| context.free_map == self.baseline);
        eprintln!(
            "FREEMAP_UNWIND_WITNESS subject={} panicking={panicking} restored={restored}",
            self.subject
        );
    }
}

#[cfg(test)]
pub(super) fn unwind_probe_subject() -> Option<String> { std::env::var(UNWIND_PROBE_SUBJECT).ok() }

/// Re-execute exactly one child test and require anti-vacuous evidence that
/// the intended failure was reached and transaction `Drop` restored the map
/// during real unwinding. `panic_marker` must be a complete fragment unique to
/// the intended panic payload, never merely a subject name that can occur in
/// the reachability or witness markers. This keeps panic interception out of
/// the test process while testing the property that matters at the process
/// boundary.
#[cfg(test)]
pub(super) fn assert_unwind_probe(test_name: &str, subject: &str, panic_marker: &str) {
    let output = Command::new(std::env::current_exe().expect("the test binary knows its path"))
        .env(UNWIND_PROBE_SUBJECT, subject)
        .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .output()
        .expect("the FreeMap unwind child process starts");
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "the FreeMap unwind child unexpectedly succeeded for {subject}:\n{text}"
    );
    assert!(
        text.contains(&format!("FREEMAP_UNWIND_REACHED subject={subject}")),
        "the FreeMap unwind child never reached {subject}:\n{text}"
    );
    assert!(
        text.contains(panic_marker),
        "the FreeMap unwind child failed for the wrong reason at {subject}:\n{text}"
    );
    assert!(
        text.contains(&format!(
            "FREEMAP_UNWIND_WITNESS subject={subject} panicking=true restored=true"
        )),
        "the FreeMap unwind witness did not observe an exact restored map for {subject}:\n{text}"
    );
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct FreeMapTransactionStats {
    pub(super) checkpoints: usize,
    pub(super) writes: usize,
    pub(super) rollback_entries: usize,
    pub(super) delta_entries: usize,
    pub(super) peak_entries: usize,
}

#[cfg(test)]
thread_local! {
    static FREE_MAP_TRANSACTION_STATS: Cell<FreeMapTransactionStats> =
        Cell::new(FreeMapTransactionStats::default());
    static EXTRACTION_FAILPOINT: Cell<Option<ExtractionFailpoint>> = const { Cell::new(None) };
    static PANIC_AFTER_WRITES: Cell<Option<usize>> = const { Cell::new(None) };
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExtractionFailpoint {
    BeforeReserve,
    AfterReserve,
    AfterRollback,
    DuringDeltaBuild,
    AfterDeltaBuild,
}

#[cfg(test)]
fn set_extraction_failpoint(failpoint: ExtractionFailpoint) {
    EXTRACTION_FAILPOINT.with(|active| active.set(Some(failpoint)));
}

#[cfg(test)]
fn trigger_extraction_failpoint(failpoint: ExtractionFailpoint) {
    EXTRACTION_FAILPOINT.with(|active| {
        if active.get() == Some(failpoint) {
            active.set(None);
            panic!("FreeMap extraction failpoint: {failpoint:?}");
        }
    });
}

#[cfg(test)]
pub(super) fn panic_after_writes_for_test(writes: usize) {
    assert!(
        writes != 0,
        "the matcher failpoint needs a positive write count"
    );
    PANIC_AFTER_WRITES.with(|remaining| remaining.set(Some(writes)));
}

#[cfg(test)]
fn trigger_write_failpoint() {
    PANIC_AFTER_WRITES.with(|remaining| {
        let Some(next) = remaining.get() else {
            return;
        };
        if next == 1 {
            remaining.set(None);
            panic!("FreeMap matcher write failpoint");
        }
        remaining.set(Some(next - 1));
    });
}

#[cfg(test)]
fn update_stats(update: impl FnOnce(&mut FreeMapTransactionStats)) {
    FREE_MAP_TRANSACTION_STATS.with(|stats| {
        let mut current = stats.get();
        update(&mut current);
        stats.set(current);
    });
}

#[cfg(test)]
pub(super) fn reset_stats() {
    FREE_MAP_TRANSACTION_STATS.with(|stats| stats.set(FreeMapTransactionStats::default()));
}

#[cfg(test)]
pub(super) fn stats() -> FreeMapTransactionStats { FREE_MAP_TRANSACTION_STATS.with(Cell::get) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FreeMapCheckpoint(usize);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct BindingDelta(FreeMap);

impl BindingDelta {
    pub(super) fn empty() -> Self { Self(FreeMap::new()) }

    #[cfg(test)]
    pub(super) fn from_map(map: FreeMap) -> Self { Self(map) }

    pub(super) fn is_empty(&self) -> bool { self.0.is_empty() }

    #[cfg(test)]
    pub(super) fn as_map(&self) -> &FreeMap { &self.0 }

    pub(super) fn into_map(self) -> FreeMap { self.0 }
}

struct UndoEntry {
    level: i32,
    previous: Option<Par>,
}

pub(super) struct FreeMapTransaction<'a> {
    context: &'a mut SpatialMatcherContext,
    undo: Vec<UndoEntry>,
    finished: bool,
}

impl<'a> FreeMapTransaction<'a> {
    pub(super) fn new(context: &'a mut SpatialMatcherContext) -> Self {
        Self {
            context,
            undo: Vec::new(),
            finished: false,
        }
    }

    pub(super) fn context(&self) -> &SpatialMatcherContext { self.context }

    pub(super) fn free_map(&self) -> &FreeMap { &self.context.free_map }

    #[inline]
    pub(super) fn checkpoint(&self) -> FreeMapCheckpoint {
        #[cfg(test)]
        update_stats(|stats| stats.checkpoints += 1);
        FreeMapCheckpoint(self.undo.len())
    }

    #[inline]
    pub(super) fn write(&mut self, level: i32, value: Par) {
        // Allocation failure must precede mutation so unwinding cannot strand
        // an unjournaled value in the caller's public map.
        self.undo.reserve(1);
        let previous = self.context.free_map.insert(level, value);
        self.undo.push(UndoEntry { level, previous });
        #[cfg(test)]
        update_stats(|stats| {
            stats.writes += 1;
            stats.peak_entries = stats.peak_entries.max(self.undo.len());
        });
        #[cfg(test)]
        trigger_write_failpoint();
    }

    pub(super) fn extend(&mut self, entries: impl IntoIterator<Item = (i32, Par)>) {
        for (level, value) in entries {
            self.write(level, value);
        }
    }

    pub(super) fn rollback(&mut self, checkpoint: FreeMapCheckpoint) {
        assert!(
            checkpoint.0 <= self.undo.len(),
            "a FreeMap checkpoint must belong to the active PDA drive"
        );
        #[cfg(test)]
        let rollback_entries = self.undo.len() - checkpoint.0;
        while self.undo.len() > checkpoint.0 {
            let undo = self
                .undo
                .pop()
                .expect("the checkpoint proved one undo record remains");
            match undo.previous {
                Some(previous) => {
                    self.context.free_map.insert(undo.level, previous);
                }
                None => {
                    self.context.free_map.remove(&undo.level);
                }
            }
        }
        #[cfg(test)]
        update_stats(|stats| stats.rollback_entries += rollback_entries);
    }

    /// Move final writes after `checkpoint` into one exact delta while
    /// restoring the checkpoint map. Repeated writes to a level collapse to
    /// its final value; a final value equal to the baseline is omitted.
    pub(super) fn take_delta_and_rollback(
        &mut self,
        checkpoint: FreeMapCheckpoint,
    ) -> BindingDelta {
        assert!(
            checkpoint.0 <= self.undo.len(),
            "a FreeMap checkpoint must belong to the active PDA drive"
        );
        let rollback_entries = self.undo.len() - checkpoint.0;
        #[cfg(test)]
        trigger_extraction_failpoint(ExtractionFailpoint::BeforeReserve);
        let mut observed = Vec::with_capacity(rollback_entries);
        #[cfg(test)]
        trigger_extraction_failpoint(ExtractionFailpoint::AfterReserve);

        for undo in &self.undo[checkpoint.0..] {
            assert!(
                self.context.free_map.contains_key(&undo.level),
                "a journaled write must leave a live value at its level"
            );
        }

        let end = self.undo.len();
        for index in (checkpoint.0..end).rev() {
            let undo = &mut self.undo[index];
            let level = undo.level;
            let current = match undo.previous.take() {
                Some(previous) => {
                    let slot = self
                        .context
                        .free_map
                        .get_mut(&level)
                        .expect("the journal preflight proved the level remains live");
                    std::mem::replace(slot, previous)
                }
                None => self
                    .context
                    .free_map
                    .remove(&level)
                    .expect("the journal preflight proved the level remains live"),
            };
            observed.push((level, current));
        }
        self.undo.truncate(checkpoint.0);
        #[cfg(test)]
        trigger_extraction_failpoint(ExtractionFailpoint::AfterRollback);

        let mut final_values = BTreeMap::new();
        for (level, value) in observed {
            #[cfg(test)]
            trigger_extraction_failpoint(ExtractionFailpoint::DuringDeltaBuild);
            if let Entry::Vacant(entry) = final_values.entry(level) {
                entry.insert(value);
            }
        }
        #[cfg(test)]
        trigger_extraction_failpoint(ExtractionFailpoint::AfterDeltaBuild);
        final_values.retain(|level, value| self.context.free_map.get(level) != Some(value));
        #[cfg(test)]
        update_stats(|stats| {
            stats.rollback_entries += rollback_entries;
            stats.delta_entries += final_values.len();
        });
        BindingDelta(final_values)
    }

    pub(super) fn finish(mut self, matched: bool) -> Option<()> {
        if !matched {
            self.rollback(FreeMapCheckpoint(0));
        }
        self.undo.clear();
        self.finished = true;
        matched.then_some(())
    }

    #[cfg(test)]
    pub(super) fn journal_len(&self) -> usize { self.undo.len() }
}

impl Drop for FreeMapTransaction<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.rollback(FreeMapCheckpoint(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use models::rust::utils::new_gint_par;

    use super::*;

    fn value(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    #[test]
    fn nested_checkpoints_restore_exact_maps_without_discarding_outer_history() {
        let mut context = SpatialMatcherContext::new();
        context.free_map.insert(1, value(10));
        let baseline = context.free_map.clone();
        let mut transaction = FreeMapTransaction::new(&mut context);
        let outer = transaction.checkpoint();
        transaction.write(2, value(20));
        let inner = transaction.checkpoint();
        transaction.write(1, value(11));
        transaction.write(3, value(30));
        transaction.rollback(inner);
        assert_eq!(transaction.free_map().get(&1), Some(&value(10)));
        assert_eq!(transaction.free_map().get(&2), Some(&value(20)));
        assert!(!transaction.free_map().contains_key(&3));
        transaction.rollback(outer);
        assert_eq!(transaction.free_map(), &baseline);
        assert!(transaction.finish(true).is_some());
    }

    #[test]
    fn delta_extraction_moves_final_writes_and_restores_the_baseline() {
        let mut context = SpatialMatcherContext::new();
        context.free_map.insert(1, value(10));
        context.free_map.insert(4, value(40));
        let baseline = context.free_map.clone();
        let mut transaction = FreeMapTransaction::new(&mut context);
        let checkpoint = transaction.checkpoint();
        transaction.write(1, value(11));
        transaction.write(2, value(20));
        transaction.write(1, value(12));
        transaction.write(4, value(41));
        transaction.write(4, value(40));
        let delta = transaction.take_delta_and_rollback(checkpoint);
        assert_eq!(transaction.free_map(), &baseline);
        assert_eq!(delta.as_map().get(&1), Some(&value(12)));
        assert_eq!(delta.as_map().get(&2), Some(&value(20)));
        assert!(!delta.as_map().contains_key(&4));
        assert!(transaction.finish(true).is_some());
    }

    #[test]
    fn root_refusal_and_unwind_restore_the_entry_map() {
        let mut refused = SpatialMatcherContext::new();
        refused.free_map.insert(1, value(10));
        let baseline = refused.free_map.clone();
        let mut transaction = FreeMapTransaction::new(&mut refused);
        transaction.write(1, value(11));
        transaction.write(2, value(20));
        assert!(transaction.finish(false).is_none());
        assert_eq!(refused.free_map, baseline);

        let module = module_path!()
            .split_once("::")
            .map(|(_, rest)| rest)
            .expect("module_path! is crate-qualified");
        let child = format!("{module}::free_map_transaction_unwind_probe_child");
        assert_unwind_probe(&child, "root-write", "transaction unwind control");
    }

    #[test]
    fn every_delta_extraction_phase_is_root_atomic_under_unwind() {
        for failpoint in [
            ExtractionFailpoint::BeforeReserve,
            ExtractionFailpoint::AfterReserve,
            ExtractionFailpoint::AfterRollback,
            ExtractionFailpoint::DuringDeltaBuild,
            ExtractionFailpoint::AfterDeltaBuild,
        ] {
            let module = module_path!()
                .split_once("::")
                .map(|(_, rest)| rest)
                .expect("module_path! is crate-qualified");
            let child = format!("{module}::free_map_transaction_unwind_probe_child");
            let subject = format!("extract-{failpoint:?}");
            assert_unwind_probe(
                &child,
                &subject,
                &format!("FreeMap extraction failpoint: {failpoint:?}"),
            );
        }
    }

    #[test]
    fn free_map_transaction_unwind_probe_child() {
        let Some(subject) = unwind_probe_subject() else {
            return;
        };

        let shared = Rc::new(RefCell::new(SpatialMatcherContext::new()));
        shared.borrow_mut().free_map.insert(9, value(90));
        let baseline = shared.borrow().free_map.clone();
        let _witness = UnwindRollbackWitness::new(Rc::clone(&shared), baseline, &subject);
        let mut context = shared.borrow_mut();
        eprintln!("FREEMAP_UNWIND_REACHED subject={subject}");

        if subject == "root-write" {
            let mut transaction = FreeMapTransaction::new(&mut context);
            transaction.write(9, value(99));
            transaction.write(2, value(20));
            panic!("transaction unwind control");
        }

        let failpoint = match subject.as_str() {
            "extract-BeforeReserve" => ExtractionFailpoint::BeforeReserve,
            "extract-AfterReserve" => ExtractionFailpoint::AfterReserve,
            "extract-AfterRollback" => ExtractionFailpoint::AfterRollback,
            "extract-DuringDeltaBuild" => ExtractionFailpoint::DuringDeltaBuild,
            "extract-AfterDeltaBuild" => ExtractionFailpoint::AfterDeltaBuild,
            _ => panic!("unknown FreeMap unwind probe subject: {subject}"),
        };
        let mut transaction = FreeMapTransaction::new(&mut context);
        transaction.write(7, value(70));
        let checkpoint = transaction.checkpoint();
        transaction.write(1, value(10));
        transaction.write(1, value(11));
        transaction.write(2, value(20));
        set_extraction_failpoint(failpoint);
        let _ = transaction.take_delta_and_rollback(checkpoint);
        panic!("FreeMap extraction failpoint did not fire: {failpoint:?}");
    }

    #[test]
    fn successful_root_retains_values_and_clears_history() {
        let mut context = SpatialMatcherContext::new();
        let mut transaction = FreeMapTransaction::new(&mut context);
        transaction.write(1, value(10));
        transaction.write(2, value(20));
        assert_eq!(transaction.journal_len(), 2);
        assert!(transaction.finish(true).is_some());
        assert_eq!(context.free_map.get(&1), Some(&value(10)));
        assert_eq!(context.free_map.get(&2), Some(&value(20)));
    }

    #[derive(Clone, Copy)]
    enum TraceOperation {
        Write(i32, i64),
        Checkpoint,
        Rollback,
        ExtractDelta,
    }

    fn run_trace_against_snapshot_oracle(trace: &[TraceOperation], matched: bool) {
        let mut context = SpatialMatcherContext::new();
        context.free_map.insert(9, value(90));
        let root = context.free_map.clone();
        let mut oracle = root.clone();
        let mut snapshots = Vec::new();
        let mut checkpoints = Vec::new();

        let mut transaction = FreeMapTransaction::new(&mut context);
        for operation in trace {
            match operation {
                TraceOperation::Write(level, scalar) => {
                    transaction.write(*level, value(*scalar));
                    oracle.insert(*level, value(*scalar));
                }
                TraceOperation::Checkpoint => {
                    checkpoints.push(transaction.checkpoint());
                    snapshots.push(oracle.clone());
                }
                TraceOperation::Rollback => {
                    let (Some(checkpoint), Some(snapshot)) = (checkpoints.pop(), snapshots.pop())
                    else {
                        continue;
                    };
                    transaction.rollback(checkpoint);
                    oracle = snapshot;
                }
                TraceOperation::ExtractDelta => {
                    let (Some(checkpoint), Some(snapshot)) = (checkpoints.pop(), snapshots.pop())
                    else {
                        continue;
                    };
                    let before = oracle.clone();
                    let delta = transaction.take_delta_and_rollback(checkpoint);
                    let mut expected_delta = FreeMap::new();
                    for (level, current) in &before {
                        if snapshot.get(level) != Some(current) {
                            expected_delta.insert(*level, current.clone());
                        }
                    }
                    assert_eq!(delta.as_map(), &expected_delta);
                    let mut replay = snapshot.clone();
                    replay.extend(delta.into_map());
                    assert_eq!(replay, before);
                    oracle = snapshot;
                }
            }
            assert_eq!(transaction.free_map(), &oracle);
        }

        let expected = if matched { oracle } else { root };
        assert_eq!(transaction.finish(matched).is_some(), matched);
        assert_eq!(context.free_map, expected);
    }

    #[test]
    fn free_map_transaction_trace_refines_snapshot_oracle() {
        const OPERATIONS: [TraceOperation; 9] = [
            TraceOperation::Write(0, 10),
            TraceOperation::Write(0, 11),
            TraceOperation::Write(0, 12),
            TraceOperation::Write(1, 10),
            TraceOperation::Write(1, 11),
            TraceOperation::Write(1, 12),
            TraceOperation::Checkpoint,
            TraceOperation::Rollback,
            TraceOperation::ExtractDelta,
        ];

        let mut trace = [TraceOperation::Checkpoint; 4];
        for first in OPERATIONS {
            trace[0] = first;
            for second in OPERATIONS {
                trace[1] = second;
                for third in OPERATIONS {
                    trace[2] = third;
                    for fourth in OPERATIONS {
                        trace[3] = fourth;
                        run_trace_against_snapshot_oracle(&trace, true);
                        run_trace_against_snapshot_oracle(&trace, false);
                    }
                }
            }
        }
    }
}
