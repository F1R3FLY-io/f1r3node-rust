use std::cell::Cell;
use std::collections::{HashSet, VecDeque};

use proptest::prelude::*;

use super::incremental_reachability_properties::Domain;
use super::trusted_successor_properties::{
    AuthorityCore, BASE_RUN_ACTIONS, CatalogImage, ColdDemand, HookPhase, NotPublishedStage,
    OwnerEvent, OwnerJournal, OwnerLimits, OwnerOutcome, OwnerPhase, OwnerProbe, OwnerRequest,
    PUBLICATION_FAULTS, PackEntry, PublicationEvent, PublicationFault, PublicationMode,
    PublicationOutcome, PublicationRun, PublicationScenario, RunAction, RunFailure, RunOutcome,
    RunPhase, SealedReceipt, TrustedHistory, TrustedPolicy, TrustedVariant, TrustedWorld,
    adopted_successor_is_exact_catalog, assert_exact_authority, catalog_images, cold_admitted_core,
    fault_outcome, hook_failure_behaviors, initial_records, interrupted_images,
    measure_cold_demand, mutation_free, owner_checkpoint, owner_state, receipt_for,
    recover_appointed, tla_behavior, transaction_entries, trusted_histories, trusted_hook_phases,
    trusted_outcome_boundary_is_exact, trusted_runner,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HistoryBounds {
    max_steps: usize,
    max_restarts: usize,
}

const CHECKED_BOUNDS: HistoryBounds = HistoryBounds {
    max_steps: 3,
    max_restarts: 1,
};

const HISTORY_REACHABLE_STATES: usize = 6841;

const ORPHAN: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum HistoryMode {
    Idle,
    Publishing,
    Recovery,
    Damaged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum HistoryTag {
    Init,
    Begun,
    Ready,
    Refused,
    NotPublished,
    RecoveryRefused,
    RestartRefused,
    Replaced,
    Repaired,
    Indeterminate,
    PublishedError,
    RecoveredPredecessorAfterAmbiguity,
    RecoveredSuccessorAfterAmbiguity,
    Recovered,
    Restarted,
    RestartedMid,
}

impl HistoryTag {
    fn ambiguous(self) -> bool { matches!(self, Self::Indeterminate | Self::PublishedError) }

    fn recovered(self) -> bool {
        matches!(
            self,
            Self::RecoveredPredecessorAfterAmbiguity |
                Self::RecoveredSuccessorAfterAmbiguity |
                Self::Recovered
        )
    }

    fn selected(self) -> bool {
        self.recovered() || matches!(self, Self::Restarted | Self::RestartedMid)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BeginImage {
    log: Vec<usize>,
    suffix: bool,
    durable: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct HistoryRecord {
    durable: usize,
    log: Vec<usize>,
    suffix: bool,
    core: Option<usize>,
    mode: HistoryMode,
    pre: usize,
    pinned: Option<usize>,
    pinned_log: Vec<usize>,
    restarts: usize,
    decoy: bool,
    foreign: bool,
    at_begin: BeginImage,
    last: HistoryTag,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct HistoryState {
    run: PublicationRun,
    record: HistoryRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HistoryAction {
    Begin(PublicationScenario),
    Publish(RunAction),
    Settle,
    Recover,
    RecoverRefused,
    ReplaceCatalog,
    Repair,
    Restart(CatalogImage),
    PinReader,
    UnpinReader,
    PlaceDecoy,
    RemoveDecoy,
    AdoptPreparedOnIndeterminate,
    RecoverReplayingSuffix,
    RecoverThroughPathname,
    AdoptThroughPathname,
    SettleKeepingStaleCore,
    SettleAfterOrphan,
    RetryFromStaleCore,
    TruncateCommittedEntry,
    SettleRefusalAfterTruncation,
    MisclassifyCatalogWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HistoryMutant {
    AdoptPreparedOnIndeterminate,
    SuffixReplay,
    PathnameRecovery,
    PathnameReread,
    StaleWriterRestoration,
    AppendWithoutTruncation,
    StaleCoreRetry,
    TruncateBelowAppointed,
    TruncateOnRefusal,
    MisclassifiedCatalogWrite,
}

impl HistoryMutant {
    fn action(self) -> HistoryAction {
        match self {
            Self::AdoptPreparedOnIndeterminate => HistoryAction::AdoptPreparedOnIndeterminate,
            Self::SuffixReplay => HistoryAction::RecoverReplayingSuffix,
            Self::PathnameRecovery => HistoryAction::RecoverThroughPathname,
            Self::PathnameReread => HistoryAction::AdoptThroughPathname,
            Self::StaleWriterRestoration => HistoryAction::SettleKeepingStaleCore,
            Self::AppendWithoutTruncation => HistoryAction::SettleAfterOrphan,
            Self::StaleCoreRetry => HistoryAction::RetryFromStaleCore,
            Self::TruncateBelowAppointed => HistoryAction::TruncateCommittedEntry,
            Self::TruncateOnRefusal => HistoryAction::SettleRefusalAfterTruncation,
            Self::MisclassifiedCatalogWrite => HistoryAction::MisclassifyCatalogWrite,
        }
    }
}

const fn history_scenario(
    call_mode: PublicationMode,
    exact_margin: bool,
    sealed_call_policy: bool,
    assembly_succeeds: bool,
) -> PublicationScenario {
    PublicationScenario {
        sealed_mode: PublicationMode::Trusted,
        call_mode,
        exact_margin,
        sealed_call_policy,
        assembly_succeeds,
        loose_cold_limits: false,
    }
}

const IDLE_SCENARIO: PublicationScenario =
    history_scenario(PublicationMode::Trusted, true, true, true);

const CHANGED_POLICY_SCENARIO: PublicationScenario =
    history_scenario(PublicationMode::Trusted, true, false, true);

const HISTORY_SCENARIOS: [PublicationScenario; 5] = [
    IDLE_SCENARIO,
    history_scenario(PublicationMode::Trusted, false, true, true),
    CHANGED_POLICY_SCENARIO,
    history_scenario(PublicationMode::ColdChecked, true, true, true),
    history_scenario(PublicationMode::Trusted, true, true, false),
];

const RESTART_IMAGES: [CatalogImage; 4] =
    [CatalogImage::Old, CatalogImage::Torn, CatalogImage::New, CatalogImage::Other];

const PERSISTED_IMAGES: [CatalogImage; 3] =
    [CatalogImage::Old, CatalogImage::Torn, CatalogImage::New];

fn idle_run() -> PublicationRun { PublicationRun::initial(IDLE_SCENARIO) }

fn terminal(phase: RunPhase) -> bool {
    matches!(phase, RunPhase::Ready | RunPhase::Refused | RunPhase::Failed)
}

fn appended(log: &[usize], commits: &[usize]) -> Vec<usize> {
    let mut next = Vec::with_capacity(log.len() + commits.len());
    next.extend_from_slice(log);
    next.extend_from_slice(commits);
    next
}

fn ambiguous_tag(outcome: RunOutcome) -> Option<HistoryTag> {
    match outcome {
        RunOutcome::Indeterminate => Some(HistoryTag::Indeterminate),
        RunOutcome::PublishedError => Some(HistoryTag::PublishedError),
        RunOutcome::Pending | RunOutcome::DefinitelyNotPublished | RunOutcome::Ready => None,
    }
}

fn recovered_tag(record: &HistoryRecord) -> HistoryTag {
    match (record.last.ambiguous(), record.durable == record.pre + 1) {
        (true, true) => HistoryTag::RecoveredSuccessorAfterAmbiguity,
        (true, false) => HistoryTag::RecoveredPredecessorAfterAmbiguity,
        (false, _) => HistoryTag::Recovered,
    }
}

impl HistoryState {
    fn initial() -> Self {
        Self {
            run: idle_run(),
            record: HistoryRecord {
                durable: 0,
                log: Vec::new(),
                suffix: false,
                core: Some(0),
                mode: HistoryMode::Idle,
                pre: 0,
                pinned: None,
                pinned_log: Vec::new(),
                restarts: 0,
                decoy: false,
                foreign: false,
                at_begin: BeginImage {
                    log: Vec::new(),
                    suffix: false,
                    durable: 0,
                },
                last: HistoryTag::Init,
            },
        }
    }

    fn with_record(&self, record: HistoryRecord) -> Self {
        Self {
            run: self.run,
            record,
        }
    }

    fn settled_with(record: HistoryRecord) -> Self {
        Self {
            run: idle_run(),
            record,
        }
    }

    fn restart_allows(&self, image: CatalogImage) -> bool {
        match (self.record.mode, self.run.phase, self.record.foreign) {
            (HistoryMode::Publishing, RunPhase::SlotWriting, _) => {
                matches!(image, CatalogImage::Old | CatalogImage::Torn | CatalogImage::New)
            }
            (HistoryMode::Publishing, _, _) => image == self.run.catalog,
            (HistoryMode::Idle | HistoryMode::Recovery | HistoryMode::Damaged, _, true) => {
                image == CatalogImage::Other
            }
            (HistoryMode::Idle | HistoryMode::Recovery | HistoryMode::Damaged, _, false) => {
                image == CatalogImage::Old
            }
        }
    }

    fn restart_images(&self) -> Vec<CatalogImage> {
        RESTART_IMAGES
            .into_iter()
            .filter(|&image| self.restart_allows(image))
            .collect()
    }

    fn settled(&self) -> Option<HistoryRecord> {
        let record = &self.record;
        let run = self.run;
        let successor = record.pre + 1;
        let next = match (run.outcome, run.catalog) {
            (RunOutcome::Ready, _) => HistoryRecord {
                durable: successor,
                log: appended(&record.log, &[successor]),
                suffix: false,
                core: Some(successor),
                mode: HistoryMode::Idle,
                foreign: false,
                last: HistoryTag::Ready,
                ..record.clone()
            },
            (RunOutcome::DefinitelyNotPublished, catalog) => HistoryRecord {
                suffix: record.suffix || run.mutated,
                core: Some(record.pre),
                mode: HistoryMode::Idle,
                foreign: catalog == CatalogImage::Other,
                last: match run.refusal {
                    Some(_) => HistoryTag::Refused,
                    None => HistoryTag::NotPublished,
                },
                ..record.clone()
            },
            (RunOutcome::Indeterminate, CatalogImage::New) => HistoryRecord {
                durable: successor,
                log: appended(&record.log, &[successor]),
                suffix: false,
                core: None,
                mode: HistoryMode::Recovery,
                last: HistoryTag::Indeterminate,
                ..record.clone()
            },
            (
                RunOutcome::Indeterminate,
                CatalogImage::Old | CatalogImage::Torn | CatalogImage::Other,
            ) => HistoryRecord {
                suffix: true,
                core: None,
                mode: HistoryMode::Recovery,
                last: HistoryTag::Indeterminate,
                ..record.clone()
            },
            (RunOutcome::PublishedError, catalog) => HistoryRecord {
                durable: successor,
                log: appended(&record.log, &[successor]),
                suffix: false,
                core: None,
                mode: match catalog {
                    CatalogImage::Other => HistoryMode::Damaged,
                    CatalogImage::Old | CatalogImage::Torn | CatalogImage::New => {
                        HistoryMode::Recovery
                    }
                },
                foreign: catalog == CatalogImage::Other,
                last: HistoryTag::PublishedError,
                ..record.clone()
            },
            (RunOutcome::Pending, _) => return None,
        };
        Some(next)
    }

    fn restarted_from(&self, image: CatalogImage) -> HistoryRecord {
        let record = &self.record;
        let publishing = record.mode == HistoryMode::Publishing;
        let successor = record.pre + 1;
        let written = match image {
            CatalogImage::New if publishing => successor,
            CatalogImage::Other if publishing && self.run.catalog_file_durable => successor,
            CatalogImage::Old | CatalogImage::Torn | CatalogImage::New | CatalogImage::Other => {
                record.durable
            }
        };
        let appended_commit = publishing && written == successor;
        let foreign = image == CatalogImage::Other;
        HistoryRecord {
            durable: written,
            log: if appended_commit {
                appended(&record.log, &[successor])
            } else {
                record.log.clone()
            },
            suffix: !appended_commit && (record.suffix || (publishing && self.run.mutated)),
            foreign,
            core: if foreign { None } else { Some(written) },
            mode: if foreign {
                HistoryMode::Damaged
            } else {
                HistoryMode::Idle
            },
            pinned: None,
            pinned_log: Vec::new(),
            restarts: record.restarts + 1,
            last: match (foreign, publishing) {
                (true, _) => HistoryTag::RestartRefused,
                (false, true) => HistoryTag::RestartedMid,
                (false, false) => HistoryTag::Restarted,
            },
            ..record.clone()
        }
    }

    fn step(&self, bounds: HistoryBounds, action: HistoryAction) -> Option<Self> {
        let record = &self.record;
        let run = self.run;
        let mode = record.mode;
        let publishing = mode == HistoryMode::Publishing;
        let settling = publishing && terminal(run.phase);
        let idle_or_recovering = matches!(mode, HistoryMode::Idle | HistoryMode::Recovery);
        let next = match action {
            HistoryAction::Begin(scenario)
                if mode == HistoryMode::Idle &&
                    record.core == Some(record.durable) &&
                    record.durable < bounds.max_steps &&
                    !record.foreign =>
            {
                Self {
                    run: PublicationRun::initial(scenario),
                    record: HistoryRecord {
                        mode: HistoryMode::Publishing,
                        pre: record.durable,
                        at_begin: BeginImage {
                            log: record.log.clone(),
                            suffix: record.suffix,
                            durable: record.durable,
                        },
                        last: HistoryTag::Begun,
                        ..record.clone()
                    },
                }
            }
            HistoryAction::Publish(run_action) if publishing && !terminal(run.phase) => Self {
                run: run.step(run_action)?,
                record: record.clone(),
            },
            HistoryAction::Settle if settling => Self::settled_with(self.settled()?),
            HistoryAction::Recover if mode == HistoryMode::Recovery && !record.foreign => self
                .with_record(HistoryRecord {
                    core: Some(record.durable),
                    mode: HistoryMode::Idle,
                    last: recovered_tag(record),
                    ..record.clone()
                }),
            HistoryAction::RecoverRefused if mode == HistoryMode::Recovery && record.foreign => {
                self.with_record(HistoryRecord {
                    mode: HistoryMode::Damaged,
                    last: HistoryTag::RecoveryRefused,
                    ..record.clone()
                })
            }
            HistoryAction::ReplaceCatalog if idle_or_recovering && !record.foreign => self
                .with_record(HistoryRecord {
                    foreign: true,
                    last: HistoryTag::Replaced,
                    ..record.clone()
                }),
            HistoryAction::Repair if record.foreign && !publishing => {
                self.with_record(HistoryRecord {
                    foreign: false,
                    mode: match mode {
                        HistoryMode::Damaged => HistoryMode::Recovery,
                        HistoryMode::Idle | HistoryMode::Publishing | HistoryMode::Recovery => mode,
                    },
                    last: HistoryTag::Repaired,
                    ..record.clone()
                })
            }
            HistoryAction::Restart(image)
                if record.restarts < bounds.max_restarts &&
                    !record.decoy &&
                    self.restart_allows(image) =>
            {
                Self::settled_with(self.restarted_from(image))
            }
            HistoryAction::PinReader if mode == HistoryMode::Idle && record.pinned.is_none() => {
                self.with_record(HistoryRecord {
                    pinned: record.core,
                    pinned_log: record.log.clone(),
                    ..record.clone()
                })
            }
            HistoryAction::UnpinReader if !publishing && record.pinned.is_some() => self
                .with_record(HistoryRecord {
                    pinned: None,
                    pinned_log: Vec::new(),
                    ..record.clone()
                }),
            HistoryAction::PlaceDecoy if idle_or_recovering && !record.decoy => {
                self.with_record(HistoryRecord {
                    decoy: true,
                    ..record.clone()
                })
            }
            HistoryAction::RemoveDecoy if idle_or_recovering && record.decoy => {
                self.with_record(HistoryRecord {
                    decoy: false,
                    ..record.clone()
                })
            }
            HistoryAction::AdoptPreparedOnIndeterminate
                if settling &&
                    run.outcome == RunOutcome::Indeterminate &&
                    matches!(run.catalog, CatalogImage::Old | CatalogImage::Torn) =>
            {
                Self::settled_with(HistoryRecord {
                    suffix: true,
                    core: Some(record.pre + 1),
                    mode: HistoryMode::Idle,
                    last: HistoryTag::Indeterminate,
                    ..record.clone()
                })
            }
            HistoryAction::RecoverReplayingSuffix
                if mode == HistoryMode::Recovery && !record.foreign && record.suffix =>
            {
                self.with_record(HistoryRecord {
                    core: Some(record.durable + 1),
                    mode: HistoryMode::Idle,
                    last: recovered_tag(record),
                    ..record.clone()
                })
            }
            HistoryAction::RecoverThroughPathname
                if mode == HistoryMode::Recovery && !record.foreign && record.decoy =>
            {
                self.with_record(HistoryRecord {
                    core: Some(record.pre),
                    mode: HistoryMode::Idle,
                    last: recovered_tag(record),
                    ..record.clone()
                })
            }
            HistoryAction::AdoptThroughPathname
                if publishing &&
                    record.decoy &&
                    run.phase == RunPhase::Resolved &&
                    run.scenario.call_mode == PublicationMode::Trusted &&
                    run.catalog == CatalogImage::Other =>
            {
                Self {
                    run: run.adopt(CatalogImage::New),
                    record: record.clone(),
                }
            }
            HistoryAction::SettleKeepingStaleCore if settling => {
                Self::settled_with(HistoryRecord {
                    core: Some(record.pre),
                    mode: HistoryMode::Idle,
                    last: ambiguous_tag(run.outcome)?,
                    ..record.clone()
                })
            }
            HistoryAction::SettleAfterOrphan
                if settling && run.outcome == RunOutcome::Ready && record.at_begin.suffix =>
            {
                Self::settled_with(HistoryRecord {
                    durable: record.pre + 1,
                    log: appended(&record.log, &[ORPHAN, record.pre + 1]),
                    suffix: false,
                    core: Some(record.pre + 1),
                    mode: HistoryMode::Idle,
                    last: HistoryTag::Ready,
                    ..record.clone()
                })
            }
            HistoryAction::RetryFromStaleCore
                if mode == HistoryMode::Recovery &&
                    record.last.ambiguous() &&
                    record.durable == record.pre + 1 =>
            {
                self.with_record(HistoryRecord {
                    log: appended(&record.log, &[record.pre + 1]),
                    core: Some(record.durable),
                    mode: HistoryMode::Idle,
                    last: HistoryTag::Ready,
                    ..record.clone()
                })
            }
            HistoryAction::TruncateCommittedEntry if mode == HistoryMode::Idle && record.suffix => {
                let (_, kept) = record.log.split_last()?;
                self.with_record(HistoryRecord {
                    log: kept.to_vec(),
                    suffix: false,
                    ..record.clone()
                })
            }
            HistoryAction::SettleRefusalAfterTruncation
                if settling && run.refusal.is_some() && record.suffix =>
            {
                Self::settled_with(HistoryRecord {
                    suffix: false,
                    core: Some(record.pre),
                    mode: HistoryMode::Idle,
                    last: HistoryTag::Refused,
                    ..record.clone()
                })
            }
            HistoryAction::MisclassifyCatalogWrite
                if publishing && run.phase == RunPhase::SlotWriting =>
            {
                Self {
                    run: PublicationRun {
                        catalog: CatalogImage::New,
                        ..run.fail(RunFailure::SlotWrite, RunOutcome::DefinitelyNotPublished)
                    },
                    record: record.clone(),
                }
            }
            _ => return None,
        };
        Some(next)
    }
}

fn history_actions(mutant: Option<HistoryMutant>) -> Vec<HistoryAction> {
    let mut actions = Vec::with_capacity(
        HISTORY_SCENARIOS.len() + BASE_RUN_ACTIONS.len() + RESTART_IMAGES.len() + 10,
    );
    actions.extend(HISTORY_SCENARIOS.map(HistoryAction::Begin));
    actions.extend(BASE_RUN_ACTIONS.map(HistoryAction::Publish));
    actions.extend([
        HistoryAction::Settle,
        HistoryAction::Recover,
        HistoryAction::RecoverRefused,
        HistoryAction::ReplaceCatalog,
        HistoryAction::Repair,
    ]);
    actions.extend(RESTART_IMAGES.map(HistoryAction::Restart));
    actions.extend([
        HistoryAction::PinReader,
        HistoryAction::UnpinReader,
        HistoryAction::PlaceDecoy,
        HistoryAction::RemoveDecoy,
    ]);
    actions.extend(mutant.map(HistoryMutant::action));
    actions
}

#[derive(Clone, Copy)]
struct HistoryProperty {
    tla: &'static str,
    holds: fn(&HistoryState) -> bool,
}

fn history_type_invariant_within(bounds: HistoryBounds, state: &HistoryState) -> bool {
    let record = &state.record;
    let generation = |value: &usize| *value <= bounds.max_steps;
    generation(&record.durable) &&
        record.log.iter().all(generation) &&
        record.core.is_none_or(|core| generation(&core)) &&
        generation(&record.pre) &&
        record.pinned.is_none_or(|pinned| generation(&pinned)) &&
        record.pinned_log.iter().all(generation) &&
        record.restarts <= bounds.max_restarts &&
        record.at_begin.log.iter().all(generation) &&
        generation(&record.at_begin.durable)
}

fn history_type_invariant(state: &HistoryState) -> bool {
    history_type_invariant_within(CHECKED_BOUNDS, state)
}

fn history_sole_authority(state: &HistoryState) -> bool {
    state.record.mode != HistoryMode::Idle || state.record.core == Some(state.record.durable)
}

fn history_authority_is_catalog_selected(state: &HistoryState) -> bool {
    let record = &state.record;
    !record.last.selected() ||
        (record.core == Some(record.durable) &&
            (record.durable == record.pre || record.durable == record.pre + 1))
}

fn history_recovery_required_after_ambiguity(state: &HistoryState) -> bool {
    let record = &state.record;
    !record.last.ambiguous() ||
        (matches!(record.mode, HistoryMode::Recovery | HistoryMode::Damaged) &&
            record.core.is_none())
}

fn history_appointed_prefix_is_committed(state: &HistoryState) -> bool {
    state
        .record
        .log
        .iter()
        .copied()
        .eq(1..=state.record.durable)
}

fn history_commits_at_most_once(state: &HistoryState) -> bool {
    let log = &state.record.log;
    log.iter()
        .enumerate()
        .all(|(position, commit)| !log[position + 1..].contains(commit))
}

fn history_pinned_prefix_stable(state: &HistoryState) -> bool {
    let record = &state.record;
    match record.pinned {
        None => true,
        Some(pinned) => record
            .log
            .get(..pinned)
            .is_some_and(|prefix| prefix == record.pinned_log.as_slice()),
    }
}

fn history_refusal_preserves_files(state: &HistoryState) -> bool {
    let record = &state.record;
    record.last != HistoryTag::Refused ||
        (record.log == record.at_begin.log &&
            record.suffix == record.at_begin.suffix &&
            record.durable == record.at_begin.durable)
}

fn run_outcome_boundary_is_exact(state: &HistoryState) -> bool {
    trusted_outcome_boundary_is_exact(&state.run)
}

fn run_adopted_successor_is_exact_catalog(state: &HistoryState) -> bool {
    adopted_successor_is_exact_catalog(&state.run)
}

fn never_recovered_successor_after_ambiguity(state: &HistoryState) -> bool {
    state.record.last != HistoryTag::RecoveredSuccessorAfterAmbiguity
}

fn never_recovered_predecessor_after_ambiguity(state: &HistoryState) -> bool {
    state.record.last != HistoryTag::RecoveredPredecessorAfterAmbiguity
}

fn never_retried_after_suffix(state: &HistoryState) -> bool {
    !(state.record.last == HistoryTag::Ready && state.record.at_begin.suffix)
}

fn never_reader_across_fault(state: &HistoryState) -> bool {
    let record = &state.record;
    !(record.pinned.is_some() &&
        (record.last == HistoryTag::NotPublished || record.last.ambiguous()))
}

fn never_restarted_mid_publication(state: &HistoryState) -> bool {
    state.record.last != HistoryTag::RestartedMid
}

fn never_recovered_under_decoy(state: &HistoryState) -> bool {
    !(state.record.decoy && state.record.last.recovered())
}

fn never_refused_with_suffix(state: &HistoryState) -> bool {
    !(state.record.last == HistoryTag::Refused && state.record.suffix)
}

fn never_repaired_foreign_catalog(state: &HistoryState) -> bool {
    state.record.last != HistoryTag::Repaired
}

const HISTORY_TYPE_INVARIANT: HistoryProperty = HistoryProperty {
    tla: "HistoryTypeInvariant",
    holds: history_type_invariant,
};

const HISTORY_SOLE_AUTHORITY: HistoryProperty = HistoryProperty {
    tla: "HistorySoleAuthority",
    holds: history_sole_authority,
};

const HISTORY_AUTHORITY_IS_CATALOG_SELECTED: HistoryProperty = HistoryProperty {
    tla: "HistoryAuthorityIsCatalogSelected",
    holds: history_authority_is_catalog_selected,
};

const HISTORY_RECOVERY_REQUIRED_AFTER_AMBIGUITY: HistoryProperty = HistoryProperty {
    tla: "HistoryRecoveryRequiredAfterAmbiguity",
    holds: history_recovery_required_after_ambiguity,
};

const HISTORY_APPOINTED_PREFIX_IS_COMMITTED: HistoryProperty = HistoryProperty {
    tla: "HistoryAppointedPrefixIsCommitted",
    holds: history_appointed_prefix_is_committed,
};

const HISTORY_COMMITS_AT_MOST_ONCE: HistoryProperty = HistoryProperty {
    tla: "HistoryCommitsAtMostOnce",
    holds: history_commits_at_most_once,
};

const HISTORY_PINNED_PREFIX_STABLE: HistoryProperty = HistoryProperty {
    tla: "HistoryPinnedPrefixStable",
    holds: history_pinned_prefix_stable,
};

const HISTORY_REFUSAL_PRESERVES_FILES: HistoryProperty = HistoryProperty {
    tla: "HistoryRefusalPreservesFiles",
    holds: history_refusal_preserves_files,
};

const RUN_OUTCOME_BOUNDARY_IS_EXACT: HistoryProperty = HistoryProperty {
    tla: "RunOutcomeBoundaryIsExact",
    holds: run_outcome_boundary_is_exact,
};

const RUN_ADOPTED_SUCCESSOR_IS_EXACT_CATALOG: HistoryProperty = HistoryProperty {
    tla: "RunAdoptedSuccessorIsExactCatalog",
    holds: run_adopted_successor_is_exact_catalog,
};

const NEVER_RECOVERED_SUCCESSOR_AFTER_AMBIGUITY: HistoryProperty = HistoryProperty {
    tla: "NeverRecoveredSuccessorAfterAmbiguity",
    holds: never_recovered_successor_after_ambiguity,
};

const NEVER_RECOVERED_PREDECESSOR_AFTER_AMBIGUITY: HistoryProperty = HistoryProperty {
    tla: "NeverRecoveredPredecessorAfterAmbiguity",
    holds: never_recovered_predecessor_after_ambiguity,
};

const NEVER_RETRIED_AFTER_SUFFIX: HistoryProperty = HistoryProperty {
    tla: "NeverRetriedAfterSuffix",
    holds: never_retried_after_suffix,
};

const NEVER_READER_ACROSS_FAULT: HistoryProperty = HistoryProperty {
    tla: "NeverReaderAcrossFault",
    holds: never_reader_across_fault,
};

const NEVER_RESTARTED_MID_PUBLICATION: HistoryProperty = HistoryProperty {
    tla: "NeverRestartedMidPublication",
    holds: never_restarted_mid_publication,
};

const NEVER_RECOVERED_UNDER_DECOY: HistoryProperty = HistoryProperty {
    tla: "NeverRecoveredUnderDecoy",
    holds: never_recovered_under_decoy,
};

const NEVER_REFUSED_WITH_SUFFIX: HistoryProperty = HistoryProperty {
    tla: "NeverRefusedWithSuffix",
    holds: never_refused_with_suffix,
};

const NEVER_REPAIRED_FOREIGN_CATALOG: HistoryProperty = HistoryProperty {
    tla: "NeverRepairedForeignCatalog",
    holds: never_repaired_foreign_catalog,
};

const BOUND_FREE_INVARIANTS: [HistoryProperty; 9] = [
    HISTORY_SOLE_AUTHORITY,
    HISTORY_AUTHORITY_IS_CATALOG_SELECTED,
    HISTORY_RECOVERY_REQUIRED_AFTER_AMBIGUITY,
    HISTORY_APPOINTED_PREFIX_IS_COMMITTED,
    HISTORY_COMMITS_AT_MOST_ONCE,
    HISTORY_PINNED_PREFIX_STABLE,
    HISTORY_REFUSAL_PRESERVES_FILES,
    RUN_OUTCOME_BOUNDARY_IS_EXACT,
    RUN_ADOPTED_SUCCESSOR_IS_EXACT_CATALOG,
];

fn explore_history(mutant: Option<HistoryMutant>) -> HashSet<HistoryState> {
    let actions = history_actions(mutant);
    let initial = HistoryState::initial();
    let mut seen = HashSet::with_capacity(2 * HISTORY_REACHABLE_STATES);
    let mut frontier = VecDeque::with_capacity(HISTORY_REACHABLE_STATES);
    seen.insert(initial.clone());
    frontier.push_back(initial);
    while let Some(state) = frontier.pop_front() {
        for &action in &actions {
            let Some(next) = state.step(CHECKED_BOUNDS, action) else {
                continue;
            };
            if !seen.contains(&next) {
                seen.insert(next.clone());
                frontier.push_back(next);
            }
        }
    }
    seen
}

fn assert_history_model(
    invariants: &[HistoryProperty],
    witnesses: &[HistoryProperty],
    mutants: &[(HistoryMutant, HistoryProperty)],
) {
    let reachable = explore_history(None);
    assert_eq!(
        reachable.len(),
        HISTORY_REACHABLE_STATES,
        "the history test model left the TLA+ state space"
    );
    for invariant in invariants {
        assert!(
            reachable.iter().all(invariant.holds),
            "history invariant {} fails on a reachable state",
            invariant.tla,
        );
    }
    for witness in witnesses {
        assert!(
            reachable.iter().any(|state| !(witness.holds)(state)),
            "history witness {} is unreachable",
            witness.tla,
        );
    }
    for (mutant, invariant) in mutants {
        assert!(
            explore_history(Some(*mutant))
                .iter()
                .any(|state| !(invariant.holds)(state)),
            "history mutant {mutant:?} kept {}",
            invariant.tla,
        );
    }
}

fn advance(
    state: &HistoryState,
    bounds: HistoryBounds,
    action: HistoryAction,
) -> Result<HistoryState, TestCaseError> {
    let next = state.step(bounds, action).ok_or_else(|| {
        TestCaseError::fail(format!("{action:?} is not a history step from {state:?}"))
    })?;
    prop_assert!(history_type_invariant_within(bounds, &next), "{action:?} left the history types");
    for invariant in BOUND_FREE_INVARIANTS {
        prop_assert!((invariant.holds)(&next), "{} fails after {action:?}", invariant.tla);
    }
    Ok(next)
}

fn replay(
    state: &HistoryState,
    bounds: HistoryBounds,
    actions: Vec<HistoryAction>,
) -> Result<HistoryState, TestCaseError> {
    actions
        .into_iter()
        .try_fold(state.clone(), |current, action| advance(&current, bounds, action))
}

fn publication_actions(
    scenario: PublicationScenario,
    behavior: &[RunAction],
) -> Vec<HistoryAction> {
    let mut actions = Vec::with_capacity(behavior.len() + 2);
    actions.push(HistoryAction::Begin(scenario));
    actions.extend(behavior.iter().copied().map(HistoryAction::Publish));
    actions
}

fn bump(counter: &Cell<usize>) { counter.set(counter.get() + 1); }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FaultChoice {
    fault: PublicationFault,
    persisted: CatalogImage,
}

fn every_fault_choice() -> Vec<FaultChoice> {
    let mut choices = Vec::with_capacity(PUBLICATION_FAULTS.len() + PERSISTED_IMAGES.len() - 1);
    for fault in PUBLICATION_FAULTS {
        match fault {
            PublicationFault::NoFault |
            PublicationFault::DirectorySyncFault |
            PublicationFault::SidecarFault => choices.push(FaultChoice {
                fault,
                persisted: CatalogImage::New,
            }),
            PublicationFault::PinFault | PublicationFault::CatalogMismatch => {
                choices.push(FaultChoice {
                    fault,
                    persisted: CatalogImage::Old,
                })
            }
            PublicationFault::CatalogInterrupted => {
                choices.extend(PERSISTED_IMAGES.map(|persisted| FaultChoice { fault, persisted }))
            }
        }
    }
    choices
}

#[derive(Clone, Debug)]
struct FaultedHistory {
    history: TrustedHistory,
    faults: Vec<FaultChoice>,
    pinned_step: Option<usize>,
}

fn faulted_histories() -> impl Strategy<Value = FaultedHistory> {
    trusted_histories().prop_flat_map(|history| {
        let steps = history.steps.len();
        (
            Just(history),
            prop::collection::vec(prop::sample::select(every_fault_choice()), steps),
            prop::option::of(0..steps),
        )
            .prop_map(|(history, faults, pinned_step)| FaultedHistory {
                history,
                faults,
                pinned_step,
            })
    })
}

fn chain_bounds(history: &TrustedHistory) -> HistoryBounds {
    HistoryBounds {
        max_steps: history.steps.len(),
        max_restarts: CHECKED_BOUNDS.max_restarts,
    }
}

fn next_receipt(core: &AuthorityCore, history: &TrustedHistory, position: usize) -> SealedReceipt {
    receipt_for(core, core.entries.current.generation + 1, &history.steps[position])
}

fn reference_successor(
    world: &TrustedWorld,
    core: &AuthorityCore,
    receipt: &SealedReceipt,
) -> Result<AuthorityCore, TestCaseError> {
    match world
        .publish(
            TrustedVariant::Faithful,
            core,
            receipt,
            &TrustedPolicy::UNBOUNDED,
            &TrustedPolicy::UNBOUNDED,
            PublicationFault::NoFault,
        )
        .0
    {
        PublicationOutcome::TrustedReady(successor) => Ok(*successor),
        other => Err(TestCaseError::fail(format!(
            "an unbounded faithful step did not publish: {other:?}"
        ))),
    }
}

fn assert_cold_admission(world: &TrustedWorld, core: &AuthorityCore) -> Result<(), TestCaseError> {
    assert_exact_authority(world, core)?;
    let cold = cold_admitted_core(
        world,
        core.entries.clone(),
        core.metadata_roots.clone(),
        core.shape,
        core.profile,
        core.index_records.clone(),
    )
    .map_err(|refusal| {
        TestCaseError::fail(format!("cold admission refused a recovered authority: {refusal:?}"))
    })?;
    prop_assert_eq!(core, &cold);
    Ok(())
}

struct SettledStep {
    recovered: AuthorityCore,
    settled: HistoryState,
    resolved: HistoryState,
    trace: Vec<PublicationEvent>,
}

fn settle_faulted_step(
    world: &TrustedWorld,
    core: &AuthorityCore,
    receipt: &SealedReceipt,
    successor: &AuthorityCore,
    choice: FaultChoice,
    state: &HistoryState,
    bounds: HistoryBounds,
) -> Result<SettledStep, TestCaseError> {
    let (outcome, trace) = world.publish(
        TrustedVariant::Faithful,
        core,
        receipt,
        &TrustedPolicy::UNBOUNDED,
        &TrustedPolicy::UNBOUNDED,
        choice.fault,
    );
    prop_assert_eq!(&outcome, &fault_outcome(choice.fault, successor.clone()));
    let behavior = tla_behavior(&trace, &outcome, choice.persisted)
        .ok_or_else(|| TestCaseError::fail(format!("{choice:?} has no publication behavior")))?;
    let mut actions = publication_actions(IDLE_SCENARIO, &behavior);
    actions.push(HistoryAction::Settle);
    let settled = replay(state, bounds, actions)?;
    let resolved = match (settled.record.mode, settled.record.foreign) {
        (HistoryMode::Recovery, false) => advance(&settled, bounds, HistoryAction::Recover)?,
        (HistoryMode::Idle, true) => advance(&settled, bounds, HistoryAction::Repair)?,
        (HistoryMode::Idle, false) => settled.clone(),
        (mode, foreign) => {
            return Err(TestCaseError::fail(format!(
                "{choice:?} settled the writer {mode:?} with a foreign catalog {foreign}"
            )));
        }
    };
    let committed = resolved.record.durable == state.record.durable + 1;
    prop_assert_eq!(committed, choice.persisted == CatalogImage::New, "{:?}", choice);
    prop_assert_eq!(resolved.record.core, Some(resolved.record.durable));
    let recovered = match choice.persisted {
        CatalogImage::New => successor.clone(),
        CatalogImage::Old | CatalogImage::Torn | CatalogImage::Other => core.clone(),
    };
    prop_assert_eq!(owner_state(&recovered), resolved.record.durable);
    Ok(SettledStep {
        recovered,
        settled,
        resolved,
        trace,
    })
}

#[derive(Debug, Default)]
struct AuthorityCoverage {
    successor_recoveries: Cell<usize>,
    predecessor_recoveries: Cell<usize>,
    readers_across_faults: Cell<usize>,
    recoveries_under_decoy: Cell<usize>,
    repaired_catalogs: Cell<usize>,
    adoption_controls: Cell<usize>,
}

fn faulted_authority_chain_property() {
    let coverage = AuthorityCoverage::default();
    trusted_runner(256, file!())
        .run(&faulted_histories(), |case| {
            let history = &case.history;
            let world = &history.world;
            let bounds = chain_bounds(history);
            let mut core = history.initial.clone();
            let mut state = HistoryState::initial();
            assert_cold_admission(world, &core)?;
            for (position, &drawn) in case.faults.iter().enumerate() {
                if case.pinned_step == Some(position) {
                    state = advance(&state, bounds, HistoryAction::PinReader)?;
                }
                let receipt = next_receipt(&core, history, position);
                let successor = reference_successor(world, &core, &receipt)?;
                prop_assert_ne!(&successor, &core);
                let observed = match state.record.pinned {
                    Some(_) => state.clone(),
                    None => advance(&state, bounds, HistoryAction::PinReader)?,
                };
                let observed = advance(&observed, bounds, HistoryAction::PlaceDecoy)?;
                for sweep in every_fault_choice() {
                    let step = settle_faulted_step(
                        world, &core, &receipt, &successor, sweep, &observed, bounds,
                    )?;
                    if !never_reader_across_fault(&step.settled) {
                        bump(&coverage.readers_across_faults);
                    }
                    match step.resolved.record.last {
                        HistoryTag::RecoveredSuccessorAfterAmbiguity => {
                            bump(&coverage.successor_recoveries)
                        }
                        HistoryTag::RecoveredPredecessorAfterAmbiguity => {
                            bump(&coverage.predecessor_recoveries)
                        }
                        HistoryTag::Repaired => bump(&coverage.repaired_catalogs),
                        _ => {}
                    }
                    if !never_recovered_under_decoy(&step.resolved) {
                        bump(&coverage.recoveries_under_decoy);
                    }
                    advance(&step.resolved, bounds, HistoryAction::RemoveDecoy)?;
                    assert_cold_admission(world, &step.recovered)?;
                    if sweep.fault == PublicationFault::CatalogInterrupted &&
                        sweep.persisted != CatalogImage::New
                    {
                        prop_assert_ne!(&successor, &step.recovered);
                        bump(&coverage.adoption_controls);
                    }
                }
                let step =
                    settle_faulted_step(world, &core, &receipt, &successor, drawn, &state, bounds)?;
                assert_cold_admission(world, &step.recovered)?;
                core = step.recovered;
                state = step.resolved;
            }
            Ok(())
        })
        .expect("faulted trusted histories keep one exact authority equal to cold admission");
    for (name, count) in [
        ("successor recoveries", &coverage.successor_recoveries),
        ("predecessor recoveries", &coverage.predecessor_recoveries),
        ("readers across faults", &coverage.readers_across_faults),
        ("recoveries under a decoy", &coverage.recoveries_under_decoy),
        ("repaired catalogs", &coverage.repaired_catalogs),
        ("adoption controls", &coverage.adoption_controls),
    ] {
        assert!(count.get() > 0, "the faulted chain never reached {name}");
    }
}

#[derive(Debug, Default)]
struct PackCoverage {
    retries_after_suffix: Cell<usize>,
    refusals_with_suffix: Cell<usize>,
    restarts_mid_publication: Cell<usize>,
    orphaned_attempts: Cell<usize>,
}

fn refusal_preserves_files(
    world: &TrustedWorld,
    core: &AuthorityCore,
    receipt: &SealedReceipt,
    state: &HistoryState,
    bounds: HistoryBounds,
    coverage: &PackCoverage,
) -> Result<(), TestCaseError> {
    let (outcome, trace) = world.publish(
        TrustedVariant::Faithful,
        core,
        receipt,
        &TrustedPolicy::UNBOUNDED,
        &TrustedPolicy::GENEROUS,
        PublicationFault::NoFault,
    );
    prop_assert!(
        matches!(
            outcome,
            PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Preflight(_))
        ),
        "{outcome:?}"
    );
    prop_assert!(mutation_free(&trace));
    let behavior = tla_behavior(&trace, &outcome, CatalogImage::Old)
        .ok_or_else(|| TestCaseError::fail("a policy refusal has no publication behavior"))?;
    let mut actions = publication_actions(CHANGED_POLICY_SCENARIO, &behavior);
    actions.push(HistoryAction::Settle);
    let refused = replay(state, bounds, actions)?;
    prop_assert_eq!(refused.record.last, HistoryTag::Refused);
    prop_assert_eq!(
        (&refused.record.log, refused.record.suffix, refused.record.durable),
        (&state.record.log, state.record.suffix, state.record.durable)
    );
    if state.record.suffix {
        bump(&coverage.refusals_with_suffix);
    }
    Ok(())
}

fn restart_selects_the_recovered_prefix(
    state: &HistoryState,
    bounds: HistoryBounds,
    hook: HookPhase,
    committed: &[PackEntry],
    transaction: &[PackEntry],
    coverage: &PackCoverage,
) -> Result<(), TestCaseError> {
    let behaviors = hook_failure_behaviors(hook);
    let (_, prefix) = behaviors
        .first()
        .and_then(|behavior| behavior.split_last())
        .ok_or_else(|| TestCaseError::fail(format!("{hook:?} has no failure behavior")))?;
    let interrupted = replay(state, bounds, publication_actions(IDLE_SCENARIO, prefix))?;
    prop_assert_eq!(interrupted.restart_images(), catalog_images(hook).to_vec(), "{:?}", hook);
    let successor_entries = committed.len() + transaction.len();
    for (pack, catalog) in interrupted_images(committed, transaction, hook) {
        let restarted = advance(&interrupted, bounds, HistoryAction::Restart(catalog))?;
        let recovered = recover_appointed(&pack, catalog, committed.len(), successor_entries);
        prop_assert_eq!(restarted.record.last, HistoryTag::RestartedMid);
        prop_assert_eq!(
            restarted.record.durable == state.record.durable + 1,
            recovered.len() == successor_entries,
            "{:?} {:?}",
            hook,
            catalog
        );
        prop_assert!(pack.len() == recovered.len() || restarted.record.suffix, "{hook:?}");
        bump(&coverage.restarts_mid_publication);
    }
    Ok(())
}

fn faulted_pack_chain_property() {
    let coverage = PackCoverage::default();
    trusted_runner(256, file!())
        .run(&faulted_histories(), |case| {
            let history = &case.history;
            let world = &history.world;
            let bounds = chain_bounds(history);
            let mut core = history.initial.clone();
            let mut state = HistoryState::initial();
            let mut committed = initial_records(core.index_records.len())
                .into_iter()
                .map(PackEntry::Record)
                .collect::<Vec<_>>();
            let mut pack = committed.clone();
            for (position, &drawn) in case.faults.iter().enumerate() {
                let receipt = next_receipt(&core, history, position);
                let successor = reference_successor(world, &core, &receipt)?;
                let transaction = transaction_entries(&receipt);
                let appointed = committed.len();
                prop_assert_eq!(&pack[..appointed], committed.as_slice());
                prop_assert_eq!(state.record.suffix, pack.len() > appointed);
                refusal_preserves_files(world, &core, &receipt, &state, bounds, &coverage)?;
                for &hook in trusted_hook_phases() {
                    restart_selects_the_recovered_prefix(
                        &state,
                        bounds,
                        hook,
                        &committed,
                        &transaction,
                        &coverage,
                    )?;
                }
                let step =
                    settle_faulted_step(world, &core, &receipt, &successor, drawn, &state, bounds)?;
                prop_assert!(
                    step.trace
                        .iter()
                        .any(|event| matches!(event, PublicationEvent::PackAppended(_)))
                );
                let orphaned = pack.len() > appointed;
                let unpruned = [pack.as_slice(), &transaction].concat();
                pack.truncate(appointed);
                pack.extend_from_slice(&transaction);
                let recovered = recover_appointed(&pack, drawn.persisted, appointed, pack.len());
                match drawn.persisted {
                    CatalogImage::New => {
                        committed.extend_from_slice(&transaction);
                        if orphaned {
                            prop_assert_ne!(
                                &recover_appointed(
                                    &unpruned,
                                    CatalogImage::New,
                                    appointed,
                                    unpruned.len()
                                ),
                                &committed
                            );
                            bump(&coverage.retries_after_suffix);
                        }
                    }
                    CatalogImage::Old | CatalogImage::Torn | CatalogImage::Other => {
                        bump(&coverage.orphaned_attempts)
                    }
                }
                prop_assert_eq!(&recovered, &committed);
                prop_assert_eq!(step.resolved.record.suffix, pack.len() > committed.len());
                core = step.recovered;
                state = step.resolved;
            }
            Ok(())
        })
        .expect(
            "every faulted step recovers to the appointed prefix and appoints each commit once",
        );
    for (name, count) in [
        ("retries after a suffix", &coverage.retries_after_suffix),
        ("refusals with a suffix", &coverage.refusals_with_suffix),
        ("restarts mid-publication", &coverage.restarts_mid_publication),
        ("orphaned attempts", &coverage.orphaned_attempts),
    ] {
        assert!(count.get() > 0, "the faulted pack chain never reached {name}");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnerInterleave {
    Uninterrupted,
    TightenedRecovery,
    UnwindAfterAcknowledgement,
    Restart,
}

const OWNER_INTERLEAVES: [OwnerInterleave; 4] = [
    OwnerInterleave::Uninterrupted,
    OwnerInterleave::TightenedRecovery,
    OwnerInterleave::UnwindAfterAcknowledgement,
    OwnerInterleave::Restart,
];

#[derive(Clone, Debug)]
struct OwnerHistory {
    history: TrustedHistory,
    choices: Vec<(FaultChoice, OwnerInterleave)>,
}

fn owner_histories() -> impl Strategy<Value = OwnerHistory> {
    trusted_histories().prop_flat_map(|history| {
        let steps = history.steps.len();
        (
            Just(history),
            prop::collection::vec(
                (
                    prop::sample::select(every_fault_choice()),
                    prop::sample::select(OWNER_INTERLEAVES.to_vec()),
                ),
                steps,
            ),
        )
            .prop_map(|(history, choices)| OwnerHistory { history, choices })
    })
}

#[derive(Debug, Default)]
struct OwnerCoverage {
    successor_reconciliations: Cell<usize>,
    predecessor_retries: Cell<usize>,
    awaiting_retries: Cell<usize>,
    tightened_refusals: Cell<usize>,
    discarded_journals: Cell<usize>,
}

fn publisher<'a>(
    world: &'a TrustedWorld,
    core: &'a AuthorityCore,
    receipt: &'a SealedReceipt,
    fault: PublicationFault,
) -> impl Fn(&TrustedPolicy) -> PublicationOutcome + 'a {
    move |policy| {
        world
            .publish(TrustedVariant::Faithful, core, receipt, policy, policy, fault)
            .0
    }
}

fn unpublished(_: &TrustedPolicy) -> PublicationOutcome {
    PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Pin)
}

fn recovery_demand(world: &TrustedWorld, core: &AuthorityCore) -> ColdDemand {
    measure_cold_demand(&world.env.semantic, &core.ledger.project(Domain::Current).ledger)
}

fn reopened_owner_discards_the_journal(
    world: &TrustedWorld,
    durable: &AuthorityCore,
    coverage: &OwnerCoverage,
) -> Result<(), TestCaseError> {
    assert_cold_admission(world, durable)?;
    let state = owner_state(durable);
    let reopened = OwnerRequest {
        current: state,
        pending: OwnerPhase::Ready,
        journal: OwnerJournal::Empty,
        limits: OwnerLimits {
            cold: ColdDemand::default(),
            trusted: TrustedPolicy::UNBOUNDED,
        },
        recovery_demand: ColdDemand::default(),
        recovered: state,
    };
    let probe = OwnerProbe::default();
    prop_assert_eq!(
        owner_checkpoint(&reopened, &probe, &unpublished),
        (OwnerOutcome::Accepted(state), vec![OwnerEvent::NoChange])
    );
    prop_assert_eq!((probe.cold_checks.get(), probe.publications.get()), (0, 0));
    bump(&coverage.discarded_journals);
    Ok(())
}

fn completed_acknowledgement(
    world: &TrustedWorld,
    core: &AuthorityCore,
    receipt: &SealedReceipt,
    successor: &AuthorityCore,
    interleave: OwnerInterleave,
    coverage: &OwnerCoverage,
) -> Result<usize, TestCaseError> {
    let state = owner_state(successor);
    match interleave {
        OwnerInterleave::UnwindAfterAcknowledgement => {
            let awaiting = OwnerRequest {
                current: owner_state(core),
                pending: OwnerPhase::AwaitingAck(state),
                journal: OwnerJournal::Captured,
                limits: OwnerLimits {
                    cold: ColdDemand::default(),
                    trusted: TrustedPolicy::UNBOUNDED,
                },
                recovery_demand: recovery_demand(world, successor),
                recovered: state,
            };
            let probe = OwnerProbe::default();
            let republish = publisher(world, core, receipt, PublicationFault::NoFault);
            prop_assert_eq!(
                owner_checkpoint(&awaiting, &probe, &republish),
                (OwnerOutcome::Accepted(state), vec![OwnerEvent::AcknowledgedOnce(state)])
            );
            prop_assert_eq!((probe.cold_checks.get(), probe.publications.get()), (0, 0));
            bump(&coverage.awaiting_retries);
        }
        OwnerInterleave::Restart => {
            reopened_owner_discards_the_journal(world, successor, coverage)?
        }
        OwnerInterleave::Uninterrupted | OwnerInterleave::TightenedRecovery => {}
    }
    Ok(state)
}

fn owner_step(
    world: &TrustedWorld,
    core: &AuthorityCore,
    receipt: &SealedReceipt,
    successor: &AuthorityCore,
    choice: FaultChoice,
    interleave: OwnerInterleave,
    coverage: &OwnerCoverage,
) -> Result<(AuthorityCore, Option<usize>), TestCaseError> {
    let current = owner_state(core);
    let state = owner_state(successor);
    prop_assert_eq!(state, current + 1);
    let terminal = match choice.persisted {
        CatalogImage::New => successor,
        CatalogImage::Old | CatalogImage::Torn | CatalogImage::Other => core,
    };
    let limits = OwnerLimits {
        cold: ColdDemand::LOOSE,
        trusted: TrustedPolicy::UNBOUNDED,
    };
    let captured = OwnerRequest {
        current,
        pending: OwnerPhase::Ready,
        journal: OwnerJournal::Captured,
        limits,
        recovery_demand: recovery_demand(world, terminal),
        recovered: owner_state(terminal),
    };
    let attempt = publisher(world, core, receipt, choice.fault);
    let retry = publisher(world, core, receipt, PublicationFault::NoFault);
    let probe = OwnerProbe::default();
    let (outcome, events) = owner_checkpoint(&captured, &probe, &attempt);
    prop_assert_eq!((probe.cold_checks.get(), probe.publications.get()), (0, 1));
    match outcome {
        OwnerOutcome::Accepted(accepted) => {
            prop_assert_eq!(choice.fault, PublicationFault::NoFault);
            prop_assert_eq!(accepted, state);
            prop_assert_eq!(events, vec![
                OwnerEvent::TrustedPublished,
                OwnerEvent::AcknowledgedOnce(state)
            ]);
        }
        OwnerOutcome::Refused(OwnerPhase::RecoveryRequired) => {
            let published = match choice.fault {
                PublicationFault::PinFault | PublicationFault::CatalogMismatch => Vec::new(),
                PublicationFault::NoFault |
                PublicationFault::CatalogInterrupted |
                PublicationFault::DirectorySyncFault |
                PublicationFault::SidecarFault => vec![OwnerEvent::TrustedPublished],
            };
            prop_assert_eq!(events, published);
            match interleave {
                OwnerInterleave::Restart => {
                    reopened_owner_discards_the_journal(world, terminal, coverage)?;
                    return Ok((terminal.clone(), None));
                }
                OwnerInterleave::TightenedRecovery => {
                    let tightened = OwnerRequest {
                        pending: OwnerPhase::RecoveryRequired,
                        limits: OwnerLimits {
                            cold: ColdDemand::default(),
                            ..limits
                        },
                        ..captured
                    };
                    let probe = OwnerProbe::default();
                    prop_assert_eq!(
                        owner_checkpoint(&tightened, &probe, &retry),
                        (OwnerOutcome::Refused(OwnerPhase::RecoveryRequired), Vec::new())
                    );
                    prop_assert_eq!((probe.cold_checks.get(), probe.publications.get()), (1, 0));
                    bump(&coverage.tightened_refusals);
                }
                OwnerInterleave::Uninterrupted | OwnerInterleave::UnwindAfterAcknowledgement => {}
            }
            let reconciling = OwnerRequest {
                pending: OwnerPhase::RecoveryRequired,
                ..captured
            };
            let probe = OwnerProbe::default();
            let (outcome, events) = owner_checkpoint(&reconciling, &probe, &retry);
            prop_assert_eq!(outcome, OwnerOutcome::Accepted(state));
            prop_assert_eq!(probe.cold_checks.get(), 1);
            match choice.persisted {
                CatalogImage::New => {
                    prop_assert_eq!(probe.publications.get(), 0);
                    prop_assert_eq!(events, vec![
                        OwnerEvent::ColdRecovered,
                        OwnerEvent::AcknowledgedOnce(state)
                    ]);
                    bump(&coverage.successor_reconciliations);
                }
                CatalogImage::Old | CatalogImage::Torn | CatalogImage::Other => {
                    prop_assert_eq!(probe.publications.get(), 1);
                    prop_assert_eq!(events, vec![
                        OwnerEvent::ColdRecovered,
                        OwnerEvent::TrustedPublished,
                        OwnerEvent::AcknowledgedOnce(state)
                    ]);
                    bump(&coverage.predecessor_retries);
                }
            }
        }
        OwnerOutcome::Refused(phase) => {
            return Err(TestCaseError::fail(format!(
                "{choice:?} returned the owner to {phase:?} under an unbounded policy"
            )));
        }
    }
    let acknowledged =
        completed_acknowledgement(world, core, receipt, successor, interleave, coverage)?;
    Ok((successor.clone(), Some(acknowledged)))
}

fn owner_history_property() {
    let coverage = OwnerCoverage::default();
    trusted_runner(128, file!())
        .run(&owner_histories(), |case| {
            let history = &case.history;
            let world = &history.world;
            let mut core = history.initial.clone();
            let mut acknowledged = Vec::with_capacity(case.choices.len());
            for (position, &(drawn, interleave)) in case.choices.iter().enumerate() {
                let receipt = next_receipt(&core, history, position);
                let successor = reference_successor(world, &core, &receipt)?;
                for sweep in every_fault_choice() {
                    for sweep_interleave in OWNER_INTERLEAVES {
                        owner_step(
                            world,
                            &core,
                            &receipt,
                            &successor,
                            sweep,
                            sweep_interleave,
                            &coverage,
                        )?;
                    }
                }
                let (next, acknowledgement) =
                    owner_step(world, &core, &receipt, &successor, drawn, interleave, &coverage)?;
                match acknowledgement {
                    Some(state) => {
                        prop_assert_eq!(&next, &successor);
                        acknowledged.push(state);
                    }
                    None => prop_assert_eq!(interleave, OwnerInterleave::Restart),
                }
                core = next;
            }
            for (position, state) in acknowledged.iter().enumerate() {
                prop_assert!(
                    !acknowledged[position + 1..].contains(state),
                    "generation {state} was acknowledged twice"
                );
            }
            Ok(())
        })
        .expect(
            "generated owner histories resolve ambiguity, acknowledge once and never replay the \
             journal",
        );
    for (name, count) in [
        ("successor reconciliations", &coverage.successor_reconciliations),
        ("predecessor retries", &coverage.predecessor_retries),
        ("awaiting-acknowledgement retries", &coverage.awaiting_retries),
        ("tightened recovery refusals", &coverage.tightened_refusals),
        ("discarded journals", &coverage.discarded_journals),
    ] {
        assert!(count.get() > 0, "the owner histories never reached {name}");
    }
}

#[test]
fn faulted_trusted_histories_match_independent_cold_admission_at_every_step() {
    assert_history_model(
        &[
            HISTORY_TYPE_INVARIANT,
            HISTORY_SOLE_AUTHORITY,
            HISTORY_AUTHORITY_IS_CATALOG_SELECTED,
            HISTORY_PINNED_PREFIX_STABLE,
            RUN_OUTCOME_BOUNDARY_IS_EXACT,
            RUN_ADOPTED_SUCCESSOR_IS_EXACT_CATALOG,
        ],
        &[
            NEVER_RECOVERED_SUCCESSOR_AFTER_AMBIGUITY,
            NEVER_RECOVERED_PREDECESSOR_AFTER_AMBIGUITY,
            NEVER_READER_ACROSS_FAULT,
            NEVER_RECOVERED_UNDER_DECOY,
            NEVER_REPAIRED_FOREIGN_CATALOG,
        ],
        &[
            (HistoryMutant::AdoptPreparedOnIndeterminate, HISTORY_SOLE_AUTHORITY),
            (HistoryMutant::SuffixReplay, HISTORY_AUTHORITY_IS_CATALOG_SELECTED),
            (HistoryMutant::PathnameRecovery, HISTORY_AUTHORITY_IS_CATALOG_SELECTED),
            (HistoryMutant::PathnameReread, RUN_ADOPTED_SUCCESSOR_IS_EXACT_CATALOG),
            (HistoryMutant::TruncateBelowAppointed, HISTORY_PINNED_PREFIX_STABLE),
            (HistoryMutant::MisclassifiedCatalogWrite, RUN_OUTCOME_BOUNDARY_IS_EXACT),
        ],
    );
    faulted_authority_chain_property();
}

#[test]
fn every_failpoint_recovers_to_the_appointed_prefix_at_later_history_steps() {
    assert_history_model(
        &[
            HISTORY_APPOINTED_PREFIX_IS_COMMITTED,
            HISTORY_COMMITS_AT_MOST_ONCE,
            HISTORY_REFUSAL_PRESERVES_FILES,
            HISTORY_RECOVERY_REQUIRED_AFTER_AMBIGUITY,
        ],
        &[NEVER_RETRIED_AFTER_SUFFIX, NEVER_REFUSED_WITH_SUFFIX, NEVER_RESTARTED_MID_PUBLICATION],
        &[
            (HistoryMutant::AppendWithoutTruncation, HISTORY_APPOINTED_PREFIX_IS_COMMITTED),
            (HistoryMutant::StaleCoreRetry, HISTORY_COMMITS_AT_MOST_ONCE),
            (HistoryMutant::TruncateOnRefusal, HISTORY_REFUSAL_PRESERVES_FILES),
            (HistoryMutant::StaleWriterRestoration, HISTORY_RECOVERY_REQUIRED_AFTER_AMBIGUITY),
        ],
    );
    faulted_pack_chain_property();
}

#[test]
fn owner_histories_resolve_ambiguity_acknowledge_once_and_never_replay_the_journal() {
    owner_history_property();
}
