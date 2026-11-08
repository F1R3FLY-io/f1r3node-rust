use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Waker};

use proptest::prelude::*;
use tokio::sync::RwLock;

use super::checkpoint_wal::fixture_for_profile;
use super::durable_publication::{
    FORMAL_PUBLICATION_PHASE_ORDER, directory_snapshot, install_checkpoint_image,
};
use super::durable_recovery::{checkpoint_image_for_profile, limits};
use super::durable_rspace_authority::{
    Profile, Store, ZERO_ADMISSION, assert_authority_unwind_child, assert_empty,
    assert_journals_unchanged, checkpoint_with, minimal_publication_limits, poll_checkpoint_with,
    publication_limits,
};
use super::support::{PersistenceTrialDir, trial_scratch};
use super::trusted_history_production::{
    Failure, IO_FAULTS, IoFault, OutcomeClass, TORN_PREFIXES, active_pack_name, add, add_one,
    appointed_prefix, check_failure, classify, copy_snapshot, failure, file_length, hook_failure,
    index_name, isolated, make_directory, phases, read_image, require, same_files, select,
    torn_image,
};
use super::trusted_successor_production::{PublicationEffects, observed_effects};
use super::trusted_successor_properties::trusted_runner;
use crate::rspace::hot_store::{DurableHotStoreBootstrapLimits, HotStoreInstances, PendingDeltas};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableAdmissionLimitsV1, DurableAuthorityViewV1,
    DurableRSpaceAuthorityV1, DurableRSpacePublicationLimitsV1, DurableRSpaceTransactionError,
    GenerationCheckpointPublicationHookPointV1, JournalPublicationPhaseV1,
    arm_catalog_slot_write_prefix, arm_durable_io_failpoint,
    arm_generation_checkpoint_publication_failpoint, recover_read_authority,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CatalogSelectionV1, CatalogSlotV1, GenerationLeaseError,
    GenerationLeaseKeyV1, GenerationLeaseRegistryV1, LogPackDecodeLimits, SemanticProfile,
    StateVersionV1,
};

type HookPoint = GenerationCheckpointPublicationHookPointV1;
type Phase = JournalPublicationPhaseV1;
type Snapshot = BTreeMap<String, Vec<u8>>;
type Reopened = (DurableRSpaceAuthorityV1<Profile>, Store, Arc<RwLock<()>>);

const DATA_CHANNELS: [&str; 2] = ["owner-data-0", "owner-data-1"];
const DATA_VALUES: [&str; 3] = ["owner-value-0", "owner-value-1", "owner-value-2"];
const CONTINUATION_CHANNELS: [&str; 2] = ["owner-cont-0", "owner-cont-1"];
const CONTINUATION_PATTERNS: [&str; 2] = ["owner-pattern-0", "owner-pattern-1"];
const CONTINUATION_BODIES: [&str; 3] = ["owner-body-0", "owner-body-1", "owner-body-2"];
const GROUP_SIZES: [usize; 2] = [1, 2];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ModelContents {
    data: [[u32; 3]; 2],
    continuations: [[bool; 3]; 2],
    joins: [[bool; 2]; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnerHistoryEdit {
    DataPut(usize, usize),
    DataRemove(usize, usize),
    ContinuationPut(usize, usize),
    ContinuationRemove(usize, usize),
    JoinPut(usize, usize),
    JoinRemove(usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnerHistoryFault {
    Hook(usize),
    Io(IoFault),
    Torn(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnerHistoryAction {
    Checkpoint,
    Faulted(OwnerHistoryFault),
    PinOverflow,
    Cancel,
    Reconcile,
    ZeroLimitReconcile,
    MinimalCheckpoint,
    DecoyReconcile,
    Restart,
}

#[derive(Clone, Debug)]
struct OwnerHistoryStep {
    edits: Vec<OwnerHistoryEdit>,
    action: OwnerHistoryAction,
}

#[derive(Clone, Copy, Debug, Default)]
struct OwnerHistoryCoverage {
    hooks: [u32; 13],
    io: [u32; 5],
    physical_sync_sites: [u32; 2],
    torn: u32,
    outcomes: [u32; 4],
    reconciled: [u32; 4],
    decoy_reconciled: [u32; 2],
    zero_limit_refusals: u32,
    pin_overflows: u32,
    awaiting_retries: [u32; 4],
    cancellations: u32,
    empty_checkpoints: [u32; 2],
    restarts: [u32; 5],
    blocked_edits: u32,
    published: [u32; 3],
    removals: [u32; 3],
    refused_join_removals: u32,
    duplicate_data: u32,
    normalized: [u32; 2],
}

impl OwnerHistoryCoverage {
    fn merge(&mut self, other: Self) {
        add(&mut self.hooks, other.hooks);
        add(&mut self.io, other.io);
        add(&mut self.physical_sync_sites, other.physical_sync_sites);
        add_one(&mut self.torn, other.torn);
        add(&mut self.outcomes, other.outcomes);
        add(&mut self.reconciled, other.reconciled);
        add(&mut self.decoy_reconciled, other.decoy_reconciled);
        add_one(&mut self.zero_limit_refusals, other.zero_limit_refusals);
        add_one(&mut self.pin_overflows, other.pin_overflows);
        add(&mut self.awaiting_retries, other.awaiting_retries);
        add_one(&mut self.cancellations, other.cancellations);
        add(&mut self.empty_checkpoints, other.empty_checkpoints);
        add(&mut self.restarts, other.restarts);
        add_one(&mut self.blocked_edits, other.blocked_edits);
        add(&mut self.published, other.published);
        add(&mut self.removals, other.removals);
        add_one(&mut self.refused_join_removals, other.refused_join_removals);
        add_one(&mut self.duplicate_data, other.duplicate_data);
        add(&mut self.normalized, other.normalized);
    }

    fn assert_authority(&self) {
        require("owner fault hook", &self.hooks);
        require("owner I/O failpoint", &self.io);
        require("physical sync site", &self.physical_sync_sites);
        require("owner torn slot write", &[self.torn]);
        require("owner outcome class", &self.outcomes);
        require("owner recovery path", &self.reconciled);
        require("decoy reconcile path", &self.decoy_reconciled);
        require("zero-limit refusal", &[self.zero_limit_refusals]);
        require("successor pin overflow", &[self.pin_overflows]);
        require("acknowledgement retry", &self.awaiting_retries);
        require("cancellation at the gate", &[self.cancellations]);
        require("empty checkpoint", &self.empty_checkpoints);
        require("owner restart", &self.restarts);
        require("blocked edit", &[self.blocked_edits]);
        require("published namespace", &self.published);
        require("namespace removal", &self.removals);
        require("refused join removal", &[self.refused_join_removals]);
        require("duplicate datum", &[self.duplicate_data]);
    }
}

struct OwnerHistoryModel {
    hot: ModelContents,
    durable: ModelContents,
    disk: u64,
    phase: Phase,
    adopted: CatalogSlotV1,
    selected: CatalogSlotV1,
    terminal: Option<CatalogSlotV1>,
}

impl OwnerHistoryModel {
    fn origin(slot: CatalogSlotV1) -> Self {
        Self {
            hot: ModelContents::default(),
            durable: ModelContents::default(),
            disk: 0,
            phase: Phase::Ready,
            adopted: slot,
            selected: slot,
            terminal: None,
        }
    }

    fn changed(&self) -> bool { self.hot != self.durable }

    fn kept(&self) -> bool { self.selected == self.adopted }

    fn commit(&mut self, slot: CatalogSlotV1, coverage: &mut OwnerHistoryCoverage) {
        let (before, after) = (self.durable, self.hot);
        add(
            &mut coverage.published,
            [
                before.data != after.data,
                before.continuations != after.continuations,
                before.joins != after.joins,
            ]
            .map(u32::from),
        );
        add(
            &mut coverage.removals,
            [
                before
                    .data
                    .as_flattened()
                    .iter()
                    .zip(after.data.as_flattened())
                    .any(|(before, after)| after < before),
                removed(before.continuations.as_flattened(), after.continuations.as_flattened()),
                removed(before.joins.as_flattened(), after.joins.as_flattened()),
            ]
            .map(u32::from),
        );
        add_one(
            &mut coverage.duplicate_data,
            u32::from(after.data.as_flattened().iter().any(|count| *count >= 2)),
        );
        self.durable = after;
        self.disk += 1;
        self.selected = slot;
    }

    fn adopt(&mut self, slot: CatalogSlotV1) {
        self.phase = Phase::Ready;
        self.adopted = slot;
        self.terminal = None;
    }

    fn restart(&mut self) {
        self.hot = self.durable;
        self.phase = Phase::Ready;
        self.adopted = self.selected;
        self.terminal = None;
    }
}

struct OwnerHistorySettled {
    pre_image: [u8; CATALOG_IMAGE_LEN],
    pre_selection: CatalogSelectionV1,
    pre_prefix: Vec<u8>,
    suffix: bool,
    previous: CatalogSlotV1,
    disk: u64,
}

struct OwnerHistoryTrial {
    owner: Option<DurableRSpaceAuthorityV1<Profile>>,
    hot: Option<Store>,
    gate: Arc<RwLock<()>>,
    model: OwnerHistoryModel,
    origin: StateVersionV1,
    initial: Snapshot,
    strict: bool,
    clones: usize,
    live: PathBuf,
    moved: PathBuf,
    directory: PersistenceTrialDir,
}

impl OwnerHistoryTrial {
    fn open(strict: bool) -> Result<Self, TestCaseError> {
        let directory = trial_scratch("owner-history-");
        let live = directory.path().join("live");
        make_directory(&live);
        install_checkpoint_image(
            &live,
            &checkpoint_image_for_profile::<Profile>(fixture_for_profile(Profile::ID)),
        );
        let initial = directory_snapshot(&live);
        let (reopened, effects) = observed_effects(|| reopen(&live));
        let (owner, hot, gate) = reopened?;
        prop_assert_eq!(effects, recovered_effects(Vec::new()));
        let origin = ready_state(&owner, &hot)?;
        let slot = select(&read_image(&live))?.slot();
        prop_assert_eq!(slot.state(), origin);
        let mut trial = Self {
            owner: Some(owner),
            hot: Some(hot),
            gate,
            model: OwnerHistoryModel::origin(slot),
            origin,
            initial,
            strict,
            clones: 0,
            live,
            moved: directory.path().join("moved"),
            directory,
        };
        trial.judge()?;
        Ok(trial)
    }

    fn owner(&self) -> &DurableRSpaceAuthorityV1<Profile> {
        self.owner.as_ref().expect("the trial holds its owner")
    }

    fn owner_mut(&mut self) -> &mut DurableRSpaceAuthorityV1<Profile> {
        self.owner.as_mut().expect("the trial holds its owner")
    }

    fn hot(&self) -> &Store { self.hot.as_ref().expect("the trial holds its hot store") }

    fn handles(&self) -> (Arc<RwLock<()>>, Store) {
        (Arc::clone(&self.gate), Arc::clone(self.hot()))
    }

    fn finish(&mut self) -> Result<(), TestCaseError> {
        drop(self.owner.take());
        drop(self.hot.take());
        prop_assert!(
            self.gate.try_write().is_ok(),
            "a finished owner history left its operation gate held"
        );
        Ok(())
    }

    fn run_step(
        &mut self,
        step: &OwnerHistoryStep,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let settled = self.observe()?;
        for edit in &step.edits {
            self.edit(*edit, coverage)?;
        }
        let action = self.normalize(step.action, coverage);
        if self.strict {
            prop_assert_eq!(action, step.action);
        }
        self.act(action, &settled, coverage)?;
        self.check_settled(&settled)?;
        self.judge()
    }

    fn observe(&self) -> Result<OwnerHistorySettled, TestCaseError> {
        let pre_image = read_image(&self.live);
        let pre_selection = select(&pre_image)?;
        let pre_slot = pre_selection.slot();
        prop_assert_eq!(pre_slot, self.model.selected);
        Ok(OwnerHistorySettled {
            pre_image,
            pre_selection,
            pre_prefix: appointed_prefix(&self.live, self.model.adopted),
            suffix: file_length(&self.live.join(active_pack_name(pre_slot))) >
                pre_slot.durable_pack_tail(),
            previous: self.model.adopted,
            disk: self.model.disk,
        })
    }

    fn edit(
        &mut self,
        edit: OwnerHistoryEdit,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let gate = Arc::clone(&self.gate);
        let Ok(_reader) = gate.try_read() else {
            add_one(&mut coverage.blocked_edits, 1);
            return Ok(());
        };
        let hot = Arc::clone(self.hot());
        let strict = self.strict;
        let contents = &mut self.model.hot;
        match edit {
            OwnerHistoryEdit::DataPut(channel, value) => {
                hot.put_datum(&data_channel(channel), datum(channel, value));
                contents.data[channel][value] += 1;
            }
            OwnerHistoryEdit::DataRemove(channel, value) => {
                let name = data_channel(channel);
                let target = datum(channel, value);
                let position = hot
                    .get_data(&name)
                    .iter()
                    .position(|candidate| *candidate == target);
                prop_assert_eq!(
                    position.is_some(),
                    contents.data[channel][value] > 0,
                    "the hot data disagrees with the model"
                );
                match position {
                    Some(position) => {
                        hot.remove_datum(&name, ordinal(position))
                            .map_err(|error| failure("a present datum was not removed", error))?;
                        contents.data[channel][value] -= 1;
                    }
                    None => normalized_edit(strict, edit, coverage)?,
                }
            }
            OwnerHistoryEdit::ContinuationPut(index, body) => {
                let channels = group(index);
                let present = contents.continuations[index][body];
                match present {
                    true => normalized_edit(strict, edit, coverage)?,
                    false => {}
                }
                for member in &channels {
                    prop_assert_eq!(hot.put_join(member, &channels), Some(()));
                }
                prop_assert_eq!(
                    hot.put_continuation(&channels, waiting_continuation(index, body)),
                    Some(!present)
                );
                contents.joins[index][..channels.len()].fill(true);
                contents.continuations[index][body] = true;
            }
            OwnerHistoryEdit::ContinuationRemove(index, body) => {
                let channels = group(index);
                let target = waiting_continuation(index, body);
                let position = hot
                    .get_continuations(&channels)
                    .iter()
                    .position(|candidate| *candidate == target);
                prop_assert_eq!(
                    position.is_some(),
                    contents.continuations[index][body],
                    "the hot continuations disagree with the model"
                );
                match position {
                    Some(position) => {
                        prop_assert_eq!(
                            hot.remove_continuation(&channels, ordinal(position)),
                            Some(())
                        );
                        contents.continuations[index][body] = false;
                    }
                    None => normalized_edit(strict, edit, coverage)?,
                }
            }
            OwnerHistoryEdit::JoinPut(index, member) => {
                let channels = group(index);
                let member = member % channels.len();
                match contents.joins[index][member] {
                    true => normalized_edit(strict, edit, coverage)?,
                    false => {}
                }
                prop_assert_eq!(hot.put_join(&channels[member], &channels), Some(()));
                contents.joins[index][member] = true;
            }
            OwnerHistoryEdit::JoinRemove(index, member) => {
                let channels = group(index);
                let member = member % channels.len();
                match (contents.joins[index][member], contents.continuations[index].contains(&true))
                {
                    (false, _) => normalized_edit(strict, edit, coverage)?,
                    (true, refused) => {
                        prop_assert_eq!(hot.remove_join(&channels[member], &channels), Some(()));
                        match refused {
                            true => add_one(&mut coverage.refused_join_removals, 1),
                            false => contents.joins[index][member] = false,
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn normalize(
        &self,
        action: OwnerHistoryAction,
        coverage: &mut OwnerHistoryCoverage,
    ) -> OwnerHistoryAction {
        let model = &self.model;
        let changed = model.changed();
        let distinguishes =
            model.adopted.state().generation() > self.origin.generation() || !model.kept();
        let normalized = match (model.phase, action) {
            (_, OwnerHistoryAction::Checkpoint | OwnerHistoryAction::Restart) |
            (Phase::Ready, OwnerHistoryAction::Cancel) |
            (
                Phase::RecoveryRequired | Phase::AwaitingAck,
                OwnerHistoryAction::Reconcile | OwnerHistoryAction::ZeroLimitReconcile,
            ) |
            (Phase::AwaitingAck, OwnerHistoryAction::MinimalCheckpoint) => action,
            (Phase::Ready, OwnerHistoryAction::Faulted(_) | OwnerHistoryAction::PinOverflow)
                if changed =>
            {
                action
            }
            (Phase::Ready, OwnerHistoryAction::MinimalCheckpoint) if !changed => action,
            (Phase::RecoveryRequired, OwnerHistoryAction::DecoyReconcile) if distinguishes => {
                action
            }
            (Phase::RecoveryRequired | Phase::AwaitingAck, OwnerHistoryAction::DecoyReconcile) => {
                OwnerHistoryAction::Reconcile
            }
            _ => OwnerHistoryAction::Checkpoint,
        };
        add_one(&mut coverage.normalized[1], u32::from(normalized != action));
        normalized
    }

    fn act(
        &mut self,
        action: OwnerHistoryAction,
        settled: &OwnerHistorySettled,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let files = directory_snapshot(&self.live);
        let before = self
            .hot()
            .pending_deltas()
            .map_err(|error| failure("the hot journals are unreadable", error))?;
        match (self.model.phase, action) {
            (_, OwnerHistoryAction::Restart) => self.restart(&files, coverage),
            (Phase::Ready, OwnerHistoryAction::Checkpoint) if self.model.changed() => {
                self.publish(settled, coverage)
            }
            (Phase::Ready, OwnerHistoryAction::Checkpoint) => {
                self.empty_checkpoint(0, publication_limits(), &files, &before, coverage)
            }
            (Phase::Ready, OwnerHistoryAction::MinimalCheckpoint) => {
                self.empty_checkpoint(1, minimal_publication_limits(), &files, &before, coverage)
            }
            (Phase::Ready, OwnerHistoryAction::Faulted(fault)) => {
                self.faulted(fault, settled, &before, coverage)
            }
            (Phase::Ready, OwnerHistoryAction::PinOverflow) => {
                self.pin_overflow(settled, &before, coverage)
            }
            (Phase::Ready, OwnerHistoryAction::Cancel) => self.cancel(&files, &before, coverage),
            (Phase::RecoveryRequired, OwnerHistoryAction::Checkpoint) => {
                self.implicit_reconcile(settled, &files, coverage)
            }
            (Phase::RecoveryRequired, OwnerHistoryAction::Reconcile) => {
                self.explicit_reconcile(false, &files, &before, coverage)
            }
            (Phase::RecoveryRequired, OwnerHistoryAction::DecoyReconcile) => {
                self.explicit_reconcile(true, &files, &before, coverage)
            }
            (Phase::RecoveryRequired, OwnerHistoryAction::ZeroLimitReconcile) => {
                self.zero_limit_reconcile(&files, &before, coverage)
            }
            (Phase::AwaitingAck, OwnerHistoryAction::Checkpoint) => {
                self.retry(0, true, publication_limits(), &files, coverage)
            }
            (Phase::AwaitingAck, OwnerHistoryAction::MinimalCheckpoint) => {
                self.retry(1, true, minimal_publication_limits(), &files, coverage)
            }
            (Phase::AwaitingAck, OwnerHistoryAction::Reconcile) => {
                self.retry(2, false, publication_limits(), &files, coverage)
            }
            (Phase::AwaitingAck, OwnerHistoryAction::ZeroLimitReconcile) => {
                self.retry(3, false, zero_admission_limits(), &files, coverage)
            }
            (phase, action) => {
                Err(failure("a normalized action has no outcome in its phase", (phase, action)))
            }
        }
    }

    fn publish(
        &mut self,
        settled: &OwnerHistorySettled,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let (gate, hot) = self.handles();
        let (published, effects) = observed_effects(|| {
            poll_checkpoint_with(self.owner_mut(), &gate, &hot, publication_limits())
        });
        let state =
            published.map_err(|error| failure("a changed checkpoint was not published", error))?;
        prop_assert_eq!(effects, PublicationEffects::trusted());
        let next = self.published_successor(state, settled)?;
        assert_empty(self.hot());
        self.model.commit(next, coverage);
        self.model.adopt(next);
        Ok(())
    }

    fn published_successor(
        &self,
        state: StateVersionV1,
        settled: &OwnerHistorySettled,
    ) -> Result<CatalogSlotV1, TestCaseError> {
        prop_assert_eq!(state.generation(), settled.previous.state().generation() + 1);
        let next = select(&read_image(&self.live))?.slot();
        prop_assert_eq!(next.state(), state);
        Ok(next)
    }

    fn empty_checkpoint(
        &mut self,
        kind: usize,
        bounds: DurableRSpacePublicationLimitsV1,
        files: &Snapshot,
        before: &PendingDeltas,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let (gate, hot) = self.handles();
        let (checkpointed, effects) =
            observed_effects(|| poll_checkpoint_with(self.owner_mut(), &gate, &hot, bounds));
        let state =
            checkpointed.map_err(|error| failure("an empty checkpoint was refused", error))?;
        prop_assert_eq!(effects, no_effects());
        prop_assert_eq!(state, self.model.adopted.state());
        same_files(
            "an empty checkpoint changed the store",
            files,
            &directory_snapshot(&self.live),
        )?;
        assert_journals_unchanged(self.hot(), before);
        add_one(&mut coverage.empty_checkpoints[kind], 1);
        Ok(())
    }

    fn faulted(
        &mut self,
        fault: OwnerHistoryFault,
        settled: &OwnerHistorySettled,
        before: &PendingDeltas,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let (trace, expected, expected_class) = owner_expectation(fault, settled.suffix);
        let committed_write = trace.contains(&FORMAL_PUBLICATION_PHASE_ORDER[6]);
        let fired = Rc::new(Cell::new(false));
        let seen = Rc::clone(&fired);
        let armed = journal_owner_hooks::arm(JournalOwnerHookPoint::AfterPreparation, move || {
            seen.set(true);
            match fault {
                OwnerHistoryFault::Hook(hook) => arm_generation_checkpoint_publication_failpoint(
                    FORMAL_PUBLICATION_PHASE_ORDER[hook],
                ),
                OwnerHistoryFault::Io(io) => arm_durable_io_failpoint(io.failpoint()),
                OwnerHistoryFault::Torn(prefix) => arm_catalog_slot_write_prefix(prefix),
            }
        });
        let (gate, hot) = self.handles();
        let (published, effects) = observed_effects(|| {
            poll_checkpoint_with(self.owner_mut(), &gate, &hot, publication_limits())
        });
        drop(armed);
        prop_assert!(fired.get(), "the owner's preparation hook did not arm the fault");
        prop_assert_eq!(effects, traced_effects(trace));
        let error = match published {
            Err(DurableRSpaceTransactionError::Publication(error)) => error,
            published => {
                return Err(failure(
                    "a faulted checkpoint did not fail its publication",
                    published,
                ));
            }
        };
        check_failure(&error, expected, &self.live)?;
        let terminal = self.owner().pending_terminal();
        prop_assert_eq!(
            terminal.is_some(),
            committed_write,
            "the terminal was retained at another publication phase"
        );
        let pre_slot = settled.pre_selection.slot();
        let image = read_image(&self.live);
        let class = classify(error.publication_outcome(), &image, pre_slot, terminal)?;
        match (fault, expected_class, terminal) {
            (OwnerHistoryFault::Torn(prefix), _, Some(written)) => {
                prop_assert_eq!(
                    image,
                    torn_image(
                        &settled.pre_image,
                        written,
                        settled.pre_selection.inactive_slot_index(),
                        prefix,
                    )
                );
                prop_assert!(
                    matches!(
                        class,
                        OutcomeClass::IndeterminateOld | OutcomeClass::IndeterminateNew
                    ),
                    "a torn slot write was classified {:?}",
                    class
                );
            }
            (OwnerHistoryFault::Torn(_), _, None) => {
                return Err(TestCaseError::fail("a torn slot write retained no terminal"));
            }
            (_, expected_class, _) => prop_assert_eq!(Some(class), expected_class),
        }
        match (class, fault) {
            (OutcomeClass::NotPublished, OwnerHistoryFault::Io(IoFault::PhysicalSync))
                if settled.suffix =>
            {
                prop_assert_eq!(image, settled.pre_image);
                prop_assert_eq!(
                    file_length(&self.live.join(active_pack_name(pre_slot))),
                    pre_slot.durable_pack_tail()
                );
            }
            (OutcomeClass::NotPublished, _) |
            (OutcomeClass::IndeterminateOld, OwnerHistoryFault::Io(IoFault::CatalogSlotWrite)) => {
                prop_assert_eq!(image, settled.pre_image)
            }
            _ => {}
        }
        let selected = select(&image)?.slot();
        match (class.kept(), terminal) {
            (true, _) => prop_assert_eq!(selected, pre_slot),
            (false, Some(committed)) => {
                prop_assert_eq!(selected, committed);
                self.model.commit(committed, coverage);
            }
            (false, None) => {
                return Err(TestCaseError::fail("a published fault retained no terminal"));
            }
        }
        assert_journals_unchanged(self.hot(), before);
        self.model.phase = Phase::RecoveryRequired;
        self.model.terminal = terminal;
        match fault {
            OwnerHistoryFault::Hook(hook) => add_one(&mut coverage.hooks[hook - 2], 1),
            OwnerHistoryFault::Io(IoFault::PhysicalSync) => {
                add_one(&mut coverage.io[IoFault::PhysicalSync.index()], 1);
                add_one(&mut coverage.physical_sync_sites[usize::from(settled.suffix)], 1);
            }
            OwnerHistoryFault::Io(io) => add_one(&mut coverage.io[io.index()], 1),
            OwnerHistoryFault::Torn(_) => add_one(&mut coverage.torn, 1),
        }
        add_one(&mut coverage.outcomes[class.index()], 1);
        Ok(())
    }

    fn pin_overflow(
        &mut self,
        settled: &OwnerHistorySettled,
        before: &PendingDeltas,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let registry = self.owner().generation_leases();
        let counter = registry.replace_lease_counter_for_test(u64::MAX);
        let (gate, hot) = self.handles();
        let (published, effects) = observed_effects(|| {
            poll_checkpoint_with(self.owner_mut(), &gate, &hot, publication_limits())
        });
        registry.replace_lease_counter_for_test(counter);
        drop(registry);
        match published {
            Err(DurableRSpaceTransactionError::Lease(GenerationLeaseError::CounterOverflow)) => {}
            published => {
                return Err(failure(
                    "an exhausted pin counter did not refuse the successor pin",
                    published,
                ));
            }
        }
        prop_assert_eq!(effects, PublicationEffects::trusted());
        let terminal = select(&read_image(&self.live))?.slot();
        prop_assert_eq!(terminal.state().generation(), settled.previous.state().generation() + 1);
        prop_assert_eq!(self.owner().pending_terminal(), Some(terminal));
        assert_journals_unchanged(self.hot(), before);
        self.model.commit(terminal, coverage);
        self.model.phase = Phase::AwaitingAck;
        self.model.terminal = Some(terminal);
        add_one(&mut coverage.pin_overflows, 1);
        Ok(())
    }

    fn cancel(
        &mut self,
        files: &Snapshot,
        before: &PendingDeltas,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let gate = Arc::clone(&self.gate);
        let hot = Arc::clone(self.hot());
        let reader = gate
            .try_read()
            .map_err(|error| failure("a ready owner's gate refused a reader", error))?;
        let (pending, effects) = observed_effects(|| {
            let mut checkpoint = std::pin::pin!(checkpoint_with(
                self.owner_mut(),
                &gate,
                &hot,
                publication_limits()
            ));
            checkpoint
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        });
        drop(reader);
        prop_assert!(pending, "a checkpoint contended at the gate did not wait");
        prop_assert_eq!(effects, no_effects());
        let owner = self.owner();
        prop_assert_eq!(owner.phase(self.hot()), Phase::Ready);
        prop_assert_eq!(owner.pending_terminal(), None);
        prop_assert_eq!(ready_state(owner, self.hot())?, self.model.adopted.state());
        same_files(
            "a cancelled checkpoint changed the store",
            files,
            &directory_snapshot(&self.live),
        )?;
        assert_journals_unchanged(self.hot(), before);
        prop_assert!(
            self.gate.try_write().is_ok(),
            "a cancelled checkpoint left its operation gate held"
        );
        add_one(&mut coverage.cancellations, 1);
        Ok(())
    }

    fn implicit_reconcile(
        &mut self,
        settled: &OwnerHistorySettled,
        files: &Snapshot,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let kept = self.model.kept();
        let (gate, hot) = self.handles();
        let (checkpointed, effects) = observed_effects(|| {
            poll_checkpoint_with(self.owner_mut(), &gate, &hot, publication_limits())
        });
        let state = checkpointed.map_err(|error| failure("an implicit reconcile failed", error))?;
        assert_empty(self.hot());
        match kept {
            true => {
                prop_assert_eq!(effects, recovered_effects(phases(14)));
                let next = self.published_successor(state, settled)?;
                let predecessor = self.model.adopted;
                self.model.adopt(predecessor);
                self.model.commit(next, coverage);
                self.model.adopt(next);
            }
            false => {
                prop_assert_eq!(effects, recovered_effects(Vec::new()));
                let terminal = self.model.selected;
                prop_assert_eq!(state, terminal.state());
                reconciled_files(files, &directory_snapshot(&self.live), terminal)?;
                self.model.adopt(terminal);
            }
        }
        add_one(&mut coverage.reconciled[2 + usize::from(!kept)], 1);
        Ok(())
    }

    fn explicit_reconcile(
        &mut self,
        decoy: bool,
        files: &Snapshot,
        before: &PendingDeltas,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let kept = self.model.kept();
        let (reconciled, effects) = match decoy {
            true => self.under_decoy(|trial| {
                observed_effects(|| trial.owner_mut().reconcile(publication_limits()))
            })?,
            false => observed_effects(|| self.owner_mut().reconcile(publication_limits())),
        };
        let state = reconciled.map_err(|error| failure("a reconcile was refused", error))?;
        prop_assert_eq!(effects, recovered_effects(Vec::new()));
        let slot = self.model.selected;
        prop_assert_eq!(state, slot.state());
        match kept {
            true => assert_journals_unchanged(self.hot(), before),
            false => assert_empty(self.hot()),
        }
        reconciled_files(files, &directory_snapshot(&self.live), slot)?;
        self.model.adopt(slot);
        let path = usize::from(!kept);
        match decoy {
            true => add_one(&mut coverage.decoy_reconciled[path], 1),
            false => add_one(&mut coverage.reconciled[path], 1),
        }
        Ok(())
    }

    fn under_decoy<T>(&mut self, run: impl FnOnce(&mut Self) -> T) -> Result<T, TestCaseError> {
        fs::rename(&self.live, &self.moved).expect("the live directory moves aside");
        copy_snapshot(&self.initial, &self.live);
        let result = run(self);
        let decoy = directory_snapshot(&self.live);
        fs::remove_dir_all(&self.live).expect("the decoy directory is removed");
        fs::rename(&self.moved, &self.live).expect("the live directory moves back");
        same_files("a reconcile wrote the decoy", &self.initial, &decoy)?;
        Ok(result)
    }

    fn zero_limit_reconcile(
        &mut self,
        files: &Snapshot,
        before: &PendingDeltas,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let (refused, effects) =
            observed_effects(|| self.owner_mut().reconcile(zero_admission_limits()));
        match &refused {
            Err(DurableRSpaceTransactionError::Recovery(DurableAdmissionErrorV1::Startup(_))) => {
                return Err(failure(
                    "a zero-limit reconcile failed its physical recovery",
                    &refused,
                ));
            }
            Err(DurableRSpaceTransactionError::Recovery(_)) => {}
            _ => {
                return Err(failure(
                    "a zero-limit reconcile was not refused at admission",
                    &refused,
                ));
            }
        }
        prop_assert_eq!(effects, recovered_effects(Vec::new()));
        reconciled_files(files, &directory_snapshot(&self.live), self.model.selected)?;
        assert_journals_unchanged(self.hot(), before);
        add_one(&mut coverage.zero_limit_refusals, 1);
        Ok(())
    }

    fn retry(
        &mut self,
        kind: usize,
        checkpoint: bool,
        bounds: DurableRSpacePublicationLimitsV1,
        files: &Snapshot,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let (gate, hot) = self.handles();
        let (retried, effects) = observed_effects(|| match checkpoint {
            true => poll_checkpoint_with(self.owner_mut(), &gate, &hot, bounds),
            false => self.owner_mut().reconcile(bounds),
        });
        let state =
            retried.map_err(|error| failure("an acknowledgement retry was refused", error))?;
        prop_assert_eq!(effects, no_effects());
        let terminal = self.model.selected;
        prop_assert_eq!(state, terminal.state());
        assert_empty(self.hot());
        same_files(
            "an acknowledgement retry changed the store",
            files,
            &directory_snapshot(&self.live),
        )?;
        self.model.adopt(terminal);
        add_one(&mut coverage.awaiting_retries[kind], 1);
        Ok(())
    }

    fn restart(
        &mut self,
        files: &Snapshot,
        coverage: &mut OwnerHistoryCoverage,
    ) -> Result<(), TestCaseError> {
        let kind = match (self.model.phase, self.model.changed(), self.model.kept()) {
            (Phase::Ready, changed, _) => usize::from(changed),
            (Phase::RecoveryRequired, _, true) => 2,
            (Phase::RecoveryRequired, _, false) => 3,
            (Phase::AwaitingAck, ..) => 4,
            (phase, ..) => return Err(failure("a restart began in an unsettled phase", phase)),
        };
        drop(self.owner.take());
        drop(self.hot.take());
        prop_assert!(self.gate.try_write().is_ok(), "a dropped owner left its operation gate held");
        let (reopened, effects) = observed_effects(|| reopen(&self.live));
        let (owner, hot, gate) = reopened?;
        prop_assert_eq!(effects, recovered_effects(Vec::new()));
        self.owner = Some(owner);
        self.hot = Some(hot);
        self.gate = gate;
        reconciled_files(files, &directory_snapshot(&self.live), self.model.selected)?;
        self.model.restart();
        add_one(&mut coverage.restarts[kind], 1);
        Ok(())
    }

    fn check_settled(&self, settled: &OwnerHistorySettled) -> Result<(), TestCaseError> {
        let owner = self.owner();
        let model = &self.model;
        prop_assert_eq!(owner.phase(self.hot()), model.phase);
        prop_assert_eq!(
            self.gate.try_read().is_ok(),
            model.phase == Phase::Ready,
            "the operation gate disagrees with the owner phase"
        );
        let registry = owner.generation_leases();
        prop_assert!(pinned(&registry, model.adopted)?, "the adopted generation is not pinned");
        match model.phase {
            Phase::Ready => {
                prop_assert_eq!(ready_state(owner, self.hot())?, model.adopted.state());
                prop_assert_eq!(
                    owner.generation_key(),
                    GenerationLeaseKeyV1::from_catalog(model.adopted)
                );
            }
            Phase::RecoveryRequired | Phase::AwaitingAck => {
                unresolved(owner, self.hot(), model.phase)?;
                prop_assert_eq!(owner.pending_terminal(), model.terminal);
                if let Some(terminal) = model.terminal {
                    prop_assert!(
                        !pinned(&registry, terminal)?,
                        "an unacknowledged terminal is pinned"
                    );
                }
            }
            phase => return Err(failure("a step settled in an unobservable phase", phase)),
        }
        if model.adopted != settled.previous {
            prop_assert!(
                !pinned(&registry, settled.previous)?,
                "a superseded generation is still pinned"
            );
        }
        drop(registry);
        prop_assert_eq!(select(&read_image(&self.live))?.slot(), model.selected);
        prop_assert_eq!(
            &appointed_prefix(&self.live, settled.previous),
            &settled.pre_prefix,
            "the predecessor's appointed prefix changed"
        );
        if model.phase == Phase::Ready {
            let _reader = self
                .gate
                .try_read()
                .map_err(|error| failure("a ready owner's gate refused a reader", error))?;
            let before = self
                .hot()
                .pending_deltas()
                .map_err(|error| failure("the hot journals are unreadable", error))?;
            let contents = hot_contents(self.hot())?;
            assert_journals_unchanged(self.hot(), &before);
            prop_assert_eq!(contents, model.hot);
        }
        prop_assert!(model.disk <= settled.disk + 1, "a step committed more than one generation");
        Ok(())
    }

    fn judge(&mut self) -> Result<(), TestCaseError> {
        let clone = self.directory.path().join(format!("clone-{}", self.clones));
        self.clones += 1;
        copy_snapshot(&directory_snapshot(&self.live), &clone);
        let view = recover_read_authority::<Profile>(
            clone.as_path(),
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| failure("the byte copy was not admitted cold", error))?;
        prop_assert!(
            !view
                .generation_leases()
                .shares_registry(&self.owner().generation_leases()),
            "the byte copy shares the live lease registry"
        );
        prop_assert_eq!(view.catalog(), self.model.selected);
        prop_assert_eq!(view.state().generation(), self.origin.generation() + self.model.disk);
        self.check_owner_against(&view)?;
        prop_assert_eq!(durable_contents(&view)?, self.model.durable);
        let bootstrap: Store = Arc::new(
            HotStoreInstances::create_from_durable(
                &view,
                DurableHotStoreBootstrapLimits::default(),
            )
            .map_err(|error| failure("the byte copy did not bootstrap a hot store", error))?,
        );
        assert_empty(&bootstrap);
        prop_assert_eq!(hot_contents(&bootstrap)?, self.model.durable);
        drop(bootstrap);
        drop(view);
        fs::remove_dir_all(&clone).expect("a judged clone is removed");
        Ok(())
    }

    fn check_owner_against(
        &self,
        view: &DurableAuthorityViewV1<Profile>,
    ) -> Result<(), TestCaseError> {
        let owner = self.owner();
        match self.model.phase {
            Phase::Ready => {
                prop_assert_eq!(ready_state(owner, self.hot())?, view.state());
                prop_assert_eq!(
                    owner.generation_key(),
                    GenerationLeaseKeyV1::from_catalog(view.catalog())
                );
                Ok(())
            }
            phase => unresolved(owner, self.hot(), phase),
        }
    }
}

fn removed(before: &[bool], after: &[bool]) -> bool {
    before
        .iter()
        .zip(after)
        .any(|(before, after)| *before && !*after)
}

fn data_channel(channel: usize) -> String { DATA_CHANNELS[channel].to_owned() }

fn datum(channel: usize, value: usize) -> Datum<String> {
    Datum::create(&data_channel(channel), DATA_VALUES[value].to_owned(), false)
}

fn group(index: usize) -> Vec<String> {
    let mut channels = Vec::with_capacity(GROUP_SIZES[index]);
    channels.extend(
        CONTINUATION_CHANNELS[..GROUP_SIZES[index]]
            .iter()
            .map(|channel| (*channel).to_owned()),
    );
    channels
}

fn patterns(index: usize) -> Vec<String> {
    let mut patterns = Vec::with_capacity(GROUP_SIZES[index]);
    patterns.extend(
        CONTINUATION_PATTERNS[..GROUP_SIZES[index]]
            .iter()
            .map(|pattern| (*pattern).to_owned()),
    );
    patterns
}

fn waiting_continuation(index: usize, body: usize) -> WaitingContinuation<String, String> {
    WaitingContinuation::create(
        &group(index),
        &patterns(index),
        &CONTINUATION_BODIES[body].to_owned(),
        false,
        BTreeSet::new(),
    )
}

fn position_of(names: &[&str], name: &str, context: &str) -> Result<usize, TestCaseError> {
    names
        .iter()
        .position(|candidate| *candidate == name)
        .ok_or_else(|| failure(context, name))
}

fn group_of(channels: &[String]) -> Result<usize, TestCaseError> {
    (0..GROUP_SIZES.len())
        .find(|index| group(*index) == channels)
        .ok_or_else(|| failure("an unexpected continuation group", channels))
}

fn ordinal(position: usize) -> i32 { i32::try_from(position).expect("a store position fits i32") }

fn normalized_edit(
    strict: bool,
    edit: OwnerHistoryEdit,
    coverage: &mut OwnerHistoryCoverage,
) -> Result<(), TestCaseError> {
    prop_assert!(!strict, "a seeded edit was normalized: {:?}", edit);
    add_one(&mut coverage.normalized[0], 1);
    Ok(())
}

fn ready_state(
    owner: &DurableRSpaceAuthorityV1<Profile>,
    hot: &Store,
) -> Result<StateVersionV1, TestCaseError> {
    owner
        .state(hot)
        .map_err(|error| failure("a ready owner reported no state", error))
}

fn unresolved(
    owner: &DurableRSpaceAuthorityV1<Profile>,
    hot: &Store,
    phase: Phase,
) -> Result<(), TestCaseError> {
    match owner.state(hot) {
        Err(DurableRSpaceTransactionError::Unresolved(reported)) if reported == phase => Ok(()),
        state => Err(failure("an open owner reported another state", state)),
    }
}

fn pinned(
    registry: &GenerationLeaseRegistryV1,
    slot: CatalogSlotV1,
) -> Result<bool, TestCaseError> {
    registry
        .has_active(GenerationLeaseKeyV1::from_catalog(slot))
        .map_err(|error| failure("a generation pin is unreadable", error))
}

fn traced_effects(trace: Vec<HookPoint>) -> PublicationEffects {
    PublicationEffects {
        trace,
        cold_admissions: 0,
        index_constructions: 0,
    }
}

fn no_effects() -> PublicationEffects { traced_effects(Vec::new()) }

fn recovered_effects(trace: Vec<HookPoint>) -> PublicationEffects {
    PublicationEffects {
        trace,
        cold_admissions: 1,
        index_constructions: 1,
    }
}

fn zero_admission_limits() -> DurableRSpacePublicationLimitsV1 {
    DurableRSpacePublicationLimitsV1 {
        admission: ZERO_ADMISSION,
        ..publication_limits()
    }
}

fn owner_expectation(
    fault: OwnerHistoryFault,
    suffix: bool,
) -> (Vec<HookPoint>, Failure, Option<OutcomeClass>) {
    match fault {
        OwnerHistoryFault::Hook(hook) => (
            phases(hook),
            hook_failure(hook),
            Some(match hook {
                11 => OutcomeClass::IndeterminateNew,
                12..=14 => OutcomeClass::Published,
                _ => OutcomeClass::NotPublished,
            }),
        ),
        OwnerHistoryFault::Io(IoFault::PhysicalWrite) => {
            (Vec::new(), Failure::Append, Some(OutcomeClass::NotPublished))
        }
        OwnerHistoryFault::Io(IoFault::PhysicalSync) if suffix => {
            (Vec::new(), Failure::Io, Some(OutcomeClass::NotPublished))
        }
        OwnerHistoryFault::Io(IoFault::PhysicalSync) => {
            (phases(2), Failure::Append, Some(OutcomeClass::NotPublished))
        }
        OwnerHistoryFault::Io(IoFault::CatalogSlotWrite) => {
            (phases(10), Failure::Indeterminate, Some(OutcomeClass::IndeterminateOld))
        }
        OwnerHistoryFault::Torn(_) => (phases(10), Failure::Indeterminate, None),
        OwnerHistoryFault::Io(IoFault::CatalogSync) => {
            (phases(11), Failure::Indeterminate, Some(OutcomeClass::IndeterminateNew))
        }
        OwnerHistoryFault::Io(IoFault::CatalogDirectorySync) => {
            (phases(12), Failure::PublishedDirectory, Some(OutcomeClass::Published))
        }
    }
}

fn reconciled_files(
    previous: &Snapshot,
    after: &Snapshot,
    slot: CatalogSlotV1,
) -> Result<(), TestCaseError> {
    let tail =
        usize::try_from(slot.durable_index_tail()).expect("an appointed index tail fits usize");
    let mut expected = previous.clone();
    if let Some(bytes) = expected.get_mut(&index_name(slot)) {
        bytes.truncate(tail);
    }
    same_files("a recovery changed more than the index suffix", &expected, after)
}

fn reopen(live: &Path) -> Result<Reopened, TestCaseError> {
    let view = recover_read_authority::<Profile>(
        live,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .map_err(|error| failure("the owner's directory was not recovered", error))?;
    let hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .map_err(|error| failure("the recovered view did not bootstrap a hot store", error))?,
    );
    assert_empty(&hot);
    let gate = Arc::new(RwLock::new(()));
    let owner = view
        .into_rspace_authority(&hot)
        .map_err(|refused| failure("the recovered view refused its hot store", &refused.1))?;
    Ok((owner, hot, gate))
}

fn durable_contents(
    view: &DurableAuthorityViewV1<Profile>,
) -> Result<ModelContents, TestCaseError> {
    let mut contents = ModelContents::default();
    let data = view
        .data_entries(u64::MAX, u64::MAX)
        .map_err(|error| failure("the durable data did not enumerate", error))?;
    for (channel, entry, multiplicity) in data {
        let index = position_of(&DATA_CHANNELS, &channel, "an unexpected durable data channel")?;
        let value = position_of(&DATA_VALUES, &entry.a, "an unexpected durable datum value")?;
        prop_assert_eq!(&entry, &datum(index, value));
        prop_assert_eq!(contents.data[index][value], 0, "a durable datum was enumerated twice");
        contents.data[index][value] =
            u32::try_from(multiplicity.get()).expect("a durable multiplicity fits u32");
    }
    let continuations = view
        .continuation_entries(u64::MAX, u64::MAX, u64::MAX)
        .map_err(|error| failure("the durable continuations did not enumerate", error))?;
    for (channels, continuation) in continuations {
        let index = group_of(&channels)?;
        let body = position_of(
            &CONTINUATION_BODIES,
            &continuation.continuation,
            "an unexpected durable continuation body",
        )?;
        prop_assert_eq!(&continuation, &waiting_continuation(index, body));
        prop_assert!(
            !contents.continuations[index][body],
            "a durable continuation was enumerated twice: {:?}",
            continuation
        );
        contents.continuations[index][body] = true;
    }
    let joins = view
        .join_entries(u64::MAX, u64::MAX)
        .map_err(|error| failure("the durable joins did not enumerate", error))?;
    for (channel, joined, multiplicity) in joins {
        let member =
            position_of(&CONTINUATION_CHANNELS, &channel, "an unexpected durable join channel")?;
        let index = group_of(&joined)?;
        prop_assert!(
            member < GROUP_SIZES[index],
            "a durable join names a channel outside its group"
        );
        prop_assert_eq!(multiplicity.get(), 1, "a durable join has a multiplicity above one");
        prop_assert!(!contents.joins[index][member], "a durable join was enumerated twice");
        contents.joins[index][member] = true;
    }
    Ok(contents)
}

fn hot_contents(hot: &Store) -> Result<ModelContents, TestCaseError> {
    let mut contents = ModelContents::default();
    for (index, counts) in contents.data.iter_mut().enumerate() {
        for entry in hot.get_data(&data_channel(index)) {
            let value = position_of(&DATA_VALUES, &entry.a, "an unexpected hot datum value")?;
            prop_assert_eq!(&entry, &datum(index, value));
            counts[value] += 1;
        }
    }
    for (index, bodies) in contents.continuations.iter_mut().enumerate() {
        for continuation in hot.get_continuations(&group(index)) {
            let body = position_of(
                &CONTINUATION_BODIES,
                &continuation.continuation,
                "an unexpected hot continuation body",
            )?;
            prop_assert_eq!(&continuation, &waiting_continuation(index, body));
            prop_assert!(!bodies[body], "a hot continuation is held twice: {:?}", continuation);
            bodies[body] = true;
        }
    }
    for (member, channel) in CONTINUATION_CHANNELS.iter().enumerate() {
        for joined in hot.get_joins(&(*channel).to_owned()) {
            let index = group_of(&joined)?;
            prop_assert!(
                member < GROUP_SIZES[index],
                "a hot join names a channel outside its group"
            );
            prop_assert!(!contents.joins[index][member], "a hot join is held twice: {:?}", joined);
            contents.joins[index][member] = true;
        }
    }
    Ok(contents)
}

fn owner_step(edits: &[OwnerHistoryEdit], action: OwnerHistoryAction) -> OwnerHistoryStep {
    OwnerHistoryStep {
        edits: edits.to_vec(),
        action,
    }
}

fn owner_seed_histories() -> [Vec<OwnerHistoryStep>; 4] {
    use OwnerHistoryAction::{
        Cancel, Checkpoint, DecoyReconcile, Faulted, MinimalCheckpoint, PinOverflow, Reconcile,
        Restart, ZeroLimitReconcile,
    };
    use OwnerHistoryEdit::{
        ContinuationPut, ContinuationRemove, DataPut, DataRemove, JoinPut, JoinRemove,
    };
    use OwnerHistoryFault::{Hook, Io, Torn};
    [
        vec![
            owner_step(&[DataPut(0, 0), DataPut(0, 0)], Checkpoint),
            owner_step(&[ContinuationPut(1, 0)], Faulted(Hook(2))),
            owner_step(&[DataPut(1, 1)], Reconcile),
            owner_step(&[], Faulted(Io(IoFault::PhysicalSync))),
            owner_step(&[], Checkpoint),
            owner_step(&[DataRemove(0, 0)], Faulted(Hook(3))),
            owner_step(&[], Restart),
            owner_step(&[DataRemove(0, 0)], Faulted(Hook(4))),
            owner_step(&[], ZeroLimitReconcile),
            owner_step(&[], Checkpoint),
            owner_step(&[JoinPut(0, 0)], Faulted(Hook(5))),
            owner_step(&[], DecoyReconcile),
            owner_step(&[], Faulted(Hook(6))),
            owner_step(&[], Reconcile),
            owner_step(&[], Faulted(Hook(7))),
            owner_step(&[], Reconcile),
            owner_step(&[], Faulted(Hook(8))),
            owner_step(&[], Checkpoint),
        ],
        vec![
            owner_step(&[ContinuationPut(0, 1)], Checkpoint),
            owner_step(&[DataPut(1, 2)], Faulted(Hook(9))),
            owner_step(&[], Reconcile),
            owner_step(&[], Faulted(Hook(10))),
            owner_step(&[], Checkpoint),
            owner_step(&[ContinuationRemove(0, 1)], Faulted(Hook(11))),
            owner_step(&[DataPut(0, 1)], Checkpoint),
            owner_step(&[JoinRemove(0, 0)], Faulted(Hook(12))),
            owner_step(&[], Reconcile),
            owner_step(&[DataPut(0, 2)], Faulted(Hook(13))),
            owner_step(&[], DecoyReconcile),
            owner_step(&[DataPut(1, 0)], Faulted(Hook(14))),
            owner_step(&[], Restart),
            owner_step(&[], Checkpoint),
        ],
        vec![
            owner_step(&[DataPut(0, 0)], Checkpoint),
            owner_step(&[DataPut(0, 1)], Faulted(Io(IoFault::PhysicalSync))),
            owner_step(&[], Reconcile),
            owner_step(&[], Faulted(Io(IoFault::PhysicalWrite))),
            owner_step(&[], Checkpoint),
            owner_step(&[DataPut(1, 1)], Faulted(Io(IoFault::CatalogSlotWrite))),
            owner_step(&[], Reconcile),
            owner_step(&[], Faulted(Torn(128))),
            owner_step(&[], Checkpoint),
            owner_step(&[DataRemove(0, 1)], Faulted(Io(IoFault::CatalogSync))),
            owner_step(&[], Reconcile),
            owner_step(&[DataPut(0, 0)], Faulted(Io(IoFault::CatalogDirectorySync))),
            owner_step(&[], Checkpoint),
            owner_step(&[DataPut(1, 2)], Cancel),
            owner_step(&[], Restart),
            owner_step(&[], Restart),
        ],
        vec![
            owner_step(&[ContinuationPut(1, 2)], PinOverflow),
            owner_step(&[DataPut(0, 0)], Checkpoint),
            owner_step(&[JoinRemove(1, 0)], MinimalCheckpoint),
            owner_step(&[DataPut(1, 0)], PinOverflow),
            owner_step(&[], MinimalCheckpoint),
            owner_step(&[ContinuationRemove(1, 2)], PinOverflow),
            owner_step(&[], Reconcile),
            owner_step(&[JoinRemove(1, 0)], PinOverflow),
            owner_step(&[], ZeroLimitReconcile),
            owner_step(&[DataPut(0, 1)], PinOverflow),
            owner_step(&[], Restart),
        ],
    ]
}

fn edit_strategy() -> impl Strategy<Value = OwnerHistoryEdit> {
    prop_oneof![
        3 => (0_usize..2, 0_usize..3)
            .prop_map(|(channel, value)| OwnerHistoryEdit::DataPut(channel, value)),
        2 => (0_usize..2, 0_usize..3)
            .prop_map(|(channel, value)| OwnerHistoryEdit::DataRemove(channel, value)),
        2 => (0_usize..2, 0_usize..3)
            .prop_map(|(index, body)| OwnerHistoryEdit::ContinuationPut(index, body)),
        1 => (0_usize..2, 0_usize..3)
            .prop_map(|(index, body)| OwnerHistoryEdit::ContinuationRemove(index, body)),
        1 => (0_usize..2, 0_usize..2)
            .prop_map(|(index, member)| OwnerHistoryEdit::JoinPut(index, member)),
        1 => (0_usize..2, 0_usize..2)
            .prop_map(|(index, member)| OwnerHistoryEdit::JoinRemove(index, member)),
    ]
}

fn fault_strategy() -> impl Strategy<Value = OwnerHistoryFault> {
    prop_oneof![
        3 => (2_usize..=14).prop_map(OwnerHistoryFault::Hook),
        2 => prop::sample::select(IO_FAULTS.to_vec()).prop_map(OwnerHistoryFault::Io),
        1 => prop::sample::select(TORN_PREFIXES.to_vec()).prop_map(OwnerHistoryFault::Torn),
    ]
}

fn action_strategy() -> impl Strategy<Value = OwnerHistoryAction> {
    prop_oneof![
        4 => Just(OwnerHistoryAction::Checkpoint),
        4 => fault_strategy().prop_map(OwnerHistoryAction::Faulted),
        2 => Just(OwnerHistoryAction::Reconcile),
        1 => Just(OwnerHistoryAction::PinOverflow),
        1 => Just(OwnerHistoryAction::Cancel),
        1 => Just(OwnerHistoryAction::ZeroLimitReconcile),
        1 => Just(OwnerHistoryAction::MinimalCheckpoint),
        1 => Just(OwnerHistoryAction::DecoyReconcile),
        1 => Just(OwnerHistoryAction::Restart),
    ]
}

fn owner_history_strategy() -> impl Strategy<Value = Vec<OwnerHistoryStep>> {
    prop::collection::vec(
        (prop::collection::vec(edit_strategy(), 0..=3), action_strategy())
            .prop_map(|(edits, action)| OwnerHistoryStep { edits, action }),
        1..=8,
    )
}

fn run_owner_history(
    steps: &[OwnerHistoryStep],
    strict: bool,
) -> Result<OwnerHistoryCoverage, TestCaseError> {
    let mut trial = OwnerHistoryTrial::open(strict)?;
    let mut coverage = OwnerHistoryCoverage::default();
    for step in steps {
        trial.run_step(step, &mut coverage)?;
    }
    trial.finish()?;
    Ok(coverage)
}

pub(super) fn production_owner_histories(cases: u32) {
    let mut seeded = OwnerHistoryCoverage::default();
    for seed in owner_seed_histories() {
        let seen =
            isolated(|| run_owner_history(&seed, true)).expect("a seeded owner history passes");
        seeded.merge(seen);
    }
    println!("owner history production seeds: {seeded:?}");
    seeded.assert_authority();
    let generated = Cell::new(OwnerHistoryCoverage::default());
    trusted_runner(cases, file!())
        .run(&owner_history_strategy(), |steps| {
            let seen = isolated(|| run_owner_history(&steps, false))?;
            let mut total = generated.get();
            total.merge(seen);
            generated.set(total);
            Ok(())
        })
        .expect("generated owner histories pass");
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("owner history production: {merged:?}");
    merged.assert_authority();
    assert_authority_unwind_child("history", "owner history acknowledgement unwind");
}

pub(super) fn run_owner_history_unwind_child(case: &str, marker: PathBuf) {
    struct VerifyOnDrop {
        trial: OwnerHistoryTrial,
        coverage: OwnerHistoryCoverage,
        terminal: CatalogSlotV1,
        old_key: GenerationLeaseKeyV1,
        observed: Rc<Cell<bool>>,
        case: String,
        marker: PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            assert!(self.observed.get(), "the acknowledgement hook did not run");
            let new_key = GenerationLeaseKeyV1::from_catalog(self.terminal);
            let trial = &mut self.trial;
            assert_eq!(trial.owner().phase(trial.hot()), Phase::AwaitingAck);
            assert!(trial.gate.try_read().is_err());
            assert_empty(trial.hot());
            let registry = trial.owner().generation_leases();
            assert!(
                registry
                    .has_active(self.old_key)
                    .expect("read the predecessor pin")
            );
            assert!(
                registry
                    .has_active(new_key)
                    .expect("read the successor pin")
            );
            assert_eq!(trial.owner().pending_terminal(), Some(self.terminal));
            let files = directory_snapshot(&trial.live);
            let (gate, hot) = trial.handles();
            let (retried, effects) = observed_effects(|| {
                poll_checkpoint_with(trial.owner_mut(), &gate, &hot, publication_limits())
            });
            assert_eq!(
                retried.expect("the retry acknowledges the published successor"),
                self.terminal.state()
            );
            assert_eq!(effects, no_effects());
            assert_eq!(trial.owner().phase(trial.hot()), Phase::Ready);
            assert!(trial.gate.try_read().is_ok());
            assert!(
                !registry
                    .has_active(self.old_key)
                    .expect("read the predecessor pin")
            );
            assert!(
                registry
                    .has_active(new_key)
                    .expect("read the successor pin")
            );
            drop(registry);
            assert_eq!(directory_snapshot(&trial.live), files);
            trial.model.adopt(self.terminal);
            trial
                .judge()
                .expect("the acknowledged owner matches its byte copy");
            trial
                .run_step(
                    &owner_step(&[OwnerHistoryEdit::DataPut(1, 1)], OwnerHistoryAction::Checkpoint),
                    &mut self.coverage,
                )
                .expect("a checkpoint after the unwind publishes once");
            fs::write(&self.marker, self.case.as_bytes()).expect("write the unwind marker");
        }
    }
    let mut trial = OwnerHistoryTrial::open(true).expect("open the unwinding owner history");
    let mut coverage = OwnerHistoryCoverage::default();
    trial
        .run_step(
            &owner_step(&[OwnerHistoryEdit::DataPut(0, 0)], OwnerHistoryAction::Checkpoint),
            &mut coverage,
        )
        .expect("the first checkpoint publishes");
    trial
        .run_step(
            &owner_step(
                &[OwnerHistoryEdit::ContinuationPut(0, 0)],
                OwnerHistoryAction::Faulted(OwnerHistoryFault::Hook(13)),
            ),
            &mut coverage,
        )
        .expect("a published fault leaves the owner recovery-required");
    let terminal = trial
        .owner()
        .pending_terminal()
        .expect("a published fault retains its terminal");
    let old_key = trial.owner().generation_key();
    let new_key = GenerationLeaseKeyV1::from_catalog(terminal);
    let observed = Rc::new(Cell::new(false));
    let seen = Rc::clone(&observed);
    let hook_gate = Arc::clone(&trial.gate);
    let hook_hot = Arc::clone(trial.hot());
    let hook_registry = trial.owner().generation_leases();
    let mut verify = VerifyOnDrop {
        trial,
        coverage,
        terminal,
        old_key,
        observed,
        case: case.to_owned(),
        marker,
    };
    let _hook = journal_owner_hooks::arm(JournalOwnerHookPoint::AfterAcknowledgement, move || {
        seen.set(true);
        assert!(hook_gate.try_read().is_err());
        assert_empty(&hook_hot);
        assert!(
            hook_registry
                .has_active(old_key)
                .expect("read the predecessor pin")
        );
        assert!(
            hook_registry
                .has_active(new_key)
                .expect("read the successor pin")
        );
        panic!("owner history acknowledgement unwind");
    });
    let unexpected = verify.trial.owner_mut().reconcile(publication_limits());
    panic!("the acknowledgement hook did not unwind: {unexpected:?}");
}
