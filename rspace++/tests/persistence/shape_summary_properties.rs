use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

use super::checkpoint_wal::fixture as empty_checkpoint_source;
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{checkpoint_image_from_source, limits};
use super::path_exact_closure_properties::{
    CONTINUATION_BODIES, CONTINUATION_GROUPS, CONTINUATION_SLOTS, ModelState, continuation,
    install, join_key, staged_sources, transition_batch,
};
use super::support::{TestSemanticProfile, encode, synthetic_ref, trial_scratch};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::internal::Datum;
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableDirectoryV1, SemanticallyAdmittedDurableWriterV1,
    TrustedSuccessorPolicyV1, acquire_cold_checked, publish_incremental_from_lease_v1,
    publish_trusted_incremental_v1,
};
use crate::rspace::persistence::rewrite::DagRewriteReport;
use crate::rspace::persistence::semantic::{
    CurrentShapeSummaryV1, KeyLengthHistogramV1,
    verify_rspace_semantics_after_structural_with_shape,
};
use crate::rspace::persistence::{
    CanonicalFormatError, CanonicalObjectStore, DagTransactionLimits, LogPackDecodeLimits,
    MutableObjectStore, MutationBatchViewV1, ObjectAdmission, ObjectRefV1, OpenDagTransaction,
    RSpaceMutationBatchV1, RetainedUnionClosureLimitsV1, SemanticValidationLimits,
    SemanticValidationReport, StateVersionV1, StructuralClosureLedgerV1, VerifiedDagState,
    verify_rspace_semantics_after_structural, verify_structural_closure_in_ledger_with_limits,
};

const DATA_CHANNELS: [&str; 3] = ["alpha", "beta", "gamma"];
const DATUM_LENGTHS: [usize; 4] = [0, 5, 23, 61];
const DATA_SLOTS: usize = DATA_CHANNELS.len() * DATUM_LENGTHS.len();
const SHARED_DATA: [(&str, u64); 3] = [("left", 1), ("middle", 2), ("right", 1)];
const HUB_MEMBERS: [&str; 3] = ["xa", "xb", "y"];

impl ModelState {
    fn keys(&self) -> impl Iterator<Item = &Vec<u8>> {
        self.data
            .keys()
            .chain(self.continuations.keys())
            .chain(self.joins.keys())
    }

    fn endpoints(&self) -> usize {
        [self.data.len(), self.continuations.len(), self.joins.len()]
            .into_iter()
            .try_fold(0_usize, usize::checked_add)
            .expect("a model state counts its endpoints")
    }

    fn longest_key(&self) -> usize { self.keys().map(Vec::len).max().unwrap_or(0) }

    fn longest_key_multiplicity(&self) -> usize {
        let longest = self.longest_key();
        self.keys().filter(|key| key.len() == longest).count()
    }

    fn source_payload_bytes(&self) -> usize {
        self.continuations
            .values()
            .map(Vec::len)
            .try_fold(0_usize, usize::checked_add)
            .expect("a model state counts its source payload bytes")
    }
}

struct TrustedStep {
    rewrite: DagRewriteReport,
    report: SemanticValidationReport,
}

struct CornerChain {
    states: Vec<ModelState>,
    unique_longest: Vec<u8>,
    tied_longest: [Vec<u8>; 2],
}

fn count(value: usize) -> u64 { u64::try_from(value).expect("a model count fits u64") }

fn runner(cases: u32) -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

fn datum_key(channel: &str, holder: &str, text: &str, persist: bool) -> Vec<u8> {
    RSpaceDataIndex::datum_path(
        &channel.to_owned(),
        &Datum::create(&holder.to_owned(), text.to_owned(), persist),
    )
}

fn cold_shape(
    store: &CanonicalObjectStore,
    manifest_ref: ObjectRefV1,
) -> (SemanticValidationReport, CurrentShapeSummaryV1) {
    let limits = SemanticValidationLimits::unbounded();
    let mut ledger = StructuralClosureLedgerV1::new();
    let structural = verify_structural_closure_in_ledger_with_limits(
        store,
        manifest_ref,
        limits.structural,
        &mut ledger,
        RetainedUnionClosureLimitsV1::UNBOUNDED,
    )
    .expect("the installed state is structurally closed");
    let plain = verify_rspace_semantics_after_structural::<TestSemanticProfile, _>(
        store, structural, limits,
    )
    .expect("the installed state is semantically admitted");
    let (recorded, shape) = verify_rspace_semantics_after_structural_with_shape::<
        TestSemanticProfile,
        _,
    >(store, structural, limits)
    .expect("the key-length recording walk admits the same state");
    assert_eq!(recorded.manifest(), plain.manifest());
    assert_eq!(
        recorded.report(),
        plain.report(),
        "recording key lengths changed the cold semantic report"
    );
    (recorded.report(), shape)
}

fn assert_cold_walk_reads_the_state(
    report: &SemanticValidationReport,
    state: &ModelState,
) -> Result<(), TestCaseError> {
    prop_assert_eq!(report.data_entries, count(state.data.len()));
    prop_assert_eq!(report.continuation_entries, count(state.continuations.len()));
    prop_assert_eq!(report.join_entries, count(state.joins.len()));
    prop_assert_eq!(report.endpoints, count(state.endpoints()));
    prop_assert_eq!(report.source_payload_bytes, count(state.source_payload_bytes()));
    prop_assert_eq!(report.maximum_logical_path_bytes, count(state.longest_key()));
    Ok(())
}

fn assert_derived_matches_cold(
    derived: &CurrentShapeSummaryV1,
    report: &SemanticValidationReport,
    cold: &CurrentShapeSummaryV1,
) -> Result<(), TestCaseError> {
    prop_assert_eq!(derived, cold);
    prop_assert_eq!(derived.logical_nodes(), report.logical_nodes);
    prop_assert_eq!(derived.endpoints(), report.endpoints);
    prop_assert_eq!(derived.data_entries(), report.data_entries);
    prop_assert_eq!(derived.continuation_entries(), report.continuation_entries);
    prop_assert_eq!(derived.join_entries(), report.join_entries);
    prop_assert_eq!(derived.source_payload_bytes(), report.source_payload_bytes);
    prop_assert_eq!(derived.maximum_logical_path_bytes(), Ok(report.maximum_logical_path_bytes));
    Ok(())
}

fn check_in_memory_chain(states: &[ModelState]) -> Result<Vec<TrustedStep>, TestCaseError> {
    let store = CanonicalObjectStore::new();
    let initial = states.first().expect("a chain has an initial state");
    let mut version = StateVersionV1::new(1, install(&store, initial));
    let (report, mut shape) = cold_shape(&store, version.manifest());
    assert_cold_walk_reads_the_state(&report, initial)?;
    let mut steps = Vec::with_capacity(states.len().saturating_sub(1));
    for pair in states.windows(2) {
        let (from, to) = (&pair[0], &pair[1]);
        let Some(mutation) = transition_batch(version, from, to) else {
            continue;
        };
        let view = MutationBatchViewV1::parse(&mutation).expect("a model batch parses");
        let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
            &store,
            version,
            SemanticValidationLimits::unbounded(),
        )
        .expect("the chain base is admitted");
        let mut transaction =
            OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED)
                .expect("the batch opens over its exact base");
        for source in staged_sources(from, to) {
            transaction
                .stage_continuation_source(source)
                .expect("a changed continuation source stages");
        }
        let sealed = transaction
            .seal()
            .expect("a coherent model transition seals");
        let rewrite = sealed.diagnostics().rewrite;
        let derived = shape.successor(view, rewrite).map_err(|error| {
            TestCaseError::fail(format!("the trusted summary refused a sealed batch: {error:?}"))
        })?;
        for object in sealed.objects() {
            let admission = store
                .put_if_absent_or_equal(object.reference(), object.canonical_bytes_arc())
                .expect("a sealed object is admitted");
            assert!(matches!(
                admission,
                ObjectAdmission::Inserted | ObjectAdmission::AlreadyPresent
            ));
        }
        let target = sealed.receipt().target();
        let oracle = CanonicalObjectStore::new();
        prop_assert_eq!(target.manifest(), install(&oracle, to));
        let (report, cold) = cold_shape(&store, target.manifest());
        assert_cold_walk_reads_the_state(&report, to)?;
        assert_derived_matches_cold(&derived, &report, &cold)?;
        steps.push(TrustedStep { rewrite, report });
        version = target;
        shape = derived;
    }
    Ok(steps)
}

fn admit_durable(root: &Path) -> SemanticallyAdmittedDurableWriterV1<TestSemanticProfile> {
    DurableDirectoryV1::open(root)
        .expect("the durable trial directory opens")
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("the durable trial directory is admitted")
}

fn check_durable_chain(states: &[ModelState]) {
    assert_eq!(
        states.first(),
        Some(&ModelState::default()),
        "the durable fixture starts from the empty state"
    );
    let trial = trial_scratch("shape-summary-");
    install_checkpoint_image(
        trial.path(),
        &checkpoint_image_from_source(empty_checkpoint_source()),
    );
    let oracle = CanonicalObjectStore::new();
    let (_, mut expected) = cold_shape(&oracle, install(&oracle, &states[0]));
    for pair in states.windows(2) {
        let (from, to) = (&pair[0], &pair[1]);
        let writer = admit_durable(trial.path());
        assert_eq!(
            writer.shape(),
            &expected,
            "durable cold admission disagrees with the trusted summary"
        );
        let (mut lease, source) = writer.into_parts();
        let Some(mutation) = transition_batch(source.authority().slot().state(), from, to) else {
            continue;
        };
        let view = MutationBatchViewV1::parse(&mutation).expect("a model batch parses");
        let mut transaction = source
            .open_transaction(view, DagTransactionLimits::UNBOUNDED)
            .expect("the batch opens over the durable base");
        for staged in staged_sources(from, to) {
            transaction
                .stage_continuation_source(staged)
                .expect("a changed continuation source stages");
        }
        let sealed = transaction
            .seal()
            .expect("a coherent model transition seals over the durable base");
        let predicted = source
            .shape()
            .successor(view, sealed.diagnostics().rewrite)
            .expect("the trusted summary derives the durable successor");
        let bound = source
            .bind_durable_transaction(&lease, sealed, DurableAdmissionLimitsV1::UNBOUNDED)
            .expect("the sealed transaction binds to its writer");
        let sequence = source
            .authority()
            .slot()
            .commit_sequence()
            .successor()
            .expect("the commit sequence has a successor");
        let prepared = source
            .prepare_transaction(&lease, &bound, sequence, LogPackDecodeLimits::UNBOUNDED)
            .expect("the bound transaction prepares");
        let published = publish_incremental_from_lease_v1(
            &mut lease,
            &source,
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("the successor publishes");
        assert_eq!(
            published.shape(),
            &predicted,
            "the published cold admission disagrees with the trusted summary"
        );
        let (_, cold) = cold_shape(&oracle, install(&oracle, to));
        assert_eq!(predicted, cold, "the trusted durable summary disagrees with the model state");
        expected = predicted;
    }
    assert_eq!(
        admit_durable(trial.path()).shape(),
        &expected,
        "restart admission disagrees with the trusted summary"
    );
}

fn check_trusted_durable_chain(states: &[ModelState]) {
    assert_eq!(
        states.first(),
        Some(&ModelState::default()),
        "the durable fixture starts from the empty state"
    );
    let trial = trial_scratch("shape-summary-trusted-");
    install_checkpoint_image(
        trial.path(),
        &checkpoint_image_from_source(empty_checkpoint_source()),
    );
    let oracle = CanonicalObjectStore::new();
    let (_, mut expected) = cold_shape(&oracle, install(&oracle, &states[0]));
    let (mut lease, source) = admit_durable(trial.path()).into_parts();
    let mut core = source.into_core();
    for pair in states.windows(2) {
        let (from, to) = (&pair[0], &pair[1]);
        assert_eq!(
            acquire_cold_checked(
                &mut lease,
                &core,
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .expect("the trusted core's state is cold-admitted")
            .shape(),
            &expected,
            "cold admission of the trusted core disagrees with the trusted summary"
        );
        let Some(mutation) = transition_batch(core.authority().slot().state(), from, to) else {
            continue;
        };
        let view = MutationBatchViewV1::parse(&mutation).expect("a model batch parses");
        let mut transaction =
            OpenDagTransaction::new(core.verified_state(), view, DagTransactionLimits::UNBOUNDED)
                .expect("the batch opens over the trusted core");
        for staged in staged_sources(from, to) {
            transaction
                .stage_continuation_source(staged)
                .expect("a changed continuation source stages");
        }
        let sealed = transaction
            .seal()
            .expect("a coherent model transition seals over the trusted core");
        let predicted = core
            .shape()
            .successor(view, sealed.diagnostics().rewrite)
            .expect("the trusted summary derives the durable successor");
        let bound = core
            .bind_trusted_transaction(&lease, sealed, TrustedSuccessorPolicyV1::UNBOUNDED)
            .expect("the sealed transaction binds to the trusted core");
        let sequence = core
            .authority()
            .slot()
            .commit_sequence()
            .successor()
            .expect("the commit sequence has a successor");
        let prepared = core
            .prepare_transaction(&lease, &bound, sequence, LogPackDecodeLimits::UNBOUNDED)
            .expect("the bound transaction prepares");
        let successor = publish_trusted_incremental_v1(
            &mut lease,
            &core,
            &prepared,
            TrustedSuccessorPolicyV1::UNBOUNDED,
            |_| {},
        )
        .expect("the trusted successor publishes");
        assert_eq!(
            successor.shape(),
            &predicted,
            "the published trusted successor disagrees with the derived summary"
        );
        let (_, cold) = cold_shape(&oracle, install(&oracle, to));
        assert_eq!(predicted, cold, "the trusted durable summary disagrees with the model state");
        core = successor;
        expected = predicted;
    }
    drop((lease, core));
    assert_eq!(
        admit_durable(trial.path()).shape(),
        &expected,
        "restart admission disagrees with the trusted chain's summary"
    );
}

fn corner_chain() -> CornerChain {
    let unique_longest = datum_key("solo", "solo", &"s".repeat(240), false);
    let (solo_continuation, natural_source) = continuation(&["solo"], "solo-body", false);
    let (independent_continuation, independent_source) = continuation(&["solo"], "solo-body", true);
    assert_eq!(
        solo_continuation, independent_continuation,
        "a continuation identity excludes its source"
    );
    assert_ne!(natural_source, independent_source);
    let hub = |member: &str| join_key("hub", &["hub", member]);
    let tie = |channel: &str| datum_key(channel, "tie", &"t".repeat(300), false);

    let mut populated = ModelState::default();
    for channel in ["shared-a", "shared-b"] {
        for (text, multiplicity) in SHARED_DATA {
            populated
                .data
                .insert(datum_key(channel, "shared-a", text, true), multiplicity);
        }
    }
    populated.data.insert(unique_longest.clone(), 1);
    populated
        .continuations
        .insert(solo_continuation.clone(), natural_source);
    populated.joins.insert(join_key("solo", &["solo"]), 1);
    for member in HUB_MEMBERS {
        populated.joins.insert(hub(member), 1);
    }

    let mut absorbed = populated.clone();
    absorbed.data.remove(&unique_longest);
    absorbed
        .data
        .insert(datum_key("shared-b", "shared-a", "middle", true), 3);
    absorbed
        .continuations
        .insert(solo_continuation, independent_source);
    absorbed.joins.remove(&hub("y"));
    absorbed.joins.insert(hub("xa"), 2);

    let mut tied = absorbed.clone();
    tied.data.insert(tie("tie-a"), 1);
    tied.data.insert(tie("tie-b"), 1);
    let mut one_tie_left = tied.clone();
    one_tie_left.data.remove(&tie("tie-a"));
    let mut no_tie_left = one_tie_left.clone();
    no_tie_left.data.remove(&tie("tie-b"));

    CornerChain {
        states: vec![
            ModelState::default(),
            populated,
            absorbed,
            tied,
            one_tie_left,
            no_tie_left,
            ModelState::default(),
        ],
        unique_longest,
        tied_longest: [tie("tie-a"), tie("tie-b")],
    }
}

fn chain_strategy() -> impl Strategy<Value = Vec<([u8; DATA_SLOTS], [u8; CONTINUATION_SLOTS])>> {
    prop::collection::vec(
        (
            prop::array::uniform::<_, DATA_SLOTS>(0_u8..4),
            prop::array::uniform::<_, CONTINUATION_SLOTS>(0_u8..3),
        ),
        2..6,
    )
}

fn generated_state(
    data: &[u8; DATA_SLOTS],
    continuations: &[u8; CONTINUATION_SLOTS],
) -> ModelState {
    let mut state = ModelState::default();
    for (slot, multiplicity) in data.iter().enumerate() {
        let multiplicity = match *multiplicity {
            0 => continue,
            multiplicity => u64::from(multiplicity),
        };
        let channel = DATA_CHANNELS[slot / DATUM_LENGTHS.len()];
        let text = format!("datum-{}", "d".repeat(DATUM_LENGTHS[slot % DATUM_LENGTHS.len()]));
        state
            .data
            .insert(datum_key(channel, DATA_CHANNELS[0], &text, slot % 2 == 0), multiplicity);
    }
    for (group_index, group) in CONTINUATION_GROUPS.iter().enumerate() {
        let mut waiting = 0_u64;
        for (body_index, body) in CONTINUATION_BODIES.iter().enumerate() {
            let independent =
                match continuations[group_index * CONTINUATION_BODIES.len() + body_index] {
                    0 => continue,
                    1 => false,
                    _ => true,
                };
            let (key, payload) = continuation(group, body, independent);
            state.continuations.insert(key, payload);
            waiting = waiting
                .checked_add(1)
                .expect("a group holds at most three continuations");
        }
        if waiting == 0 {
            continue;
        }
        for channel in group.iter() {
            state.joins.insert(join_key(channel, group), waiting);
        }
    }
    state
}

#[test]
fn trusted_shape_summary_matches_cold_semantic_walk() {
    let corner = corner_chain();
    let states = &corner.states;
    assert_eq!(states[1].longest_key_multiplicity(), 1);
    assert_eq!(states[1].longest_key(), corner.unique_longest.len());
    assert!(states[1].data.contains_key(&corner.unique_longest));
    assert!(!states[2].data.contains_key(&corner.unique_longest));
    assert_eq!(states[3].longest_key_multiplicity(), 2);
    assert_eq!(states[3].longest_key(), corner.tied_longest[0].len());
    assert_eq!(corner.tied_longest[0].len(), corner.tied_longest[1].len());

    let steps = check_in_memory_chain(states)
        .expect("every trusted corner summary equals the cold semantic walk");
    assert_eq!(steps.len(), states.len() - 1, "every corner transition is nonempty");
    assert!(
        steps[0].report.structural.node_objects < steps[0].report.logical_nodes,
        "the populated corner state shares subtrees"
    );
    assert!(
        steps[1].rewrite.emitted_nodes > steps[1].rewrite.placed_nodes,
        "deleting one hub join absorbs an edited child into its parent"
    );
    assert!(
        steps[1].report.maximum_logical_path_bytes < steps[0].report.maximum_logical_path_bytes,
        "deleting the unique longest key lowers the maximum"
    );
    assert_eq!(
        steps[3].report.maximum_logical_path_bytes, steps[2].report.maximum_logical_path_bytes,
        "deleting one of two tied longest keys keeps the maximum"
    );
    assert!(
        steps[4].report.maximum_logical_path_bytes < steps[3].report.maximum_logical_path_bytes,
        "deleting the last longest key lowers the maximum"
    );

    check_durable_chain(states);
    check_trusted_durable_chain(states);

    runner(64)
        .run(&chain_strategy(), |chain| {
            let states = chain
                .iter()
                .map(|(data, continuations)| generated_state(data, continuations))
                .collect::<Vec<_>>();
            check_in_memory_chain(&states).map(|_| ())
        })
        .expect("every trusted generated summary equals the cold semantic walk");
}

#[test]
fn shape_summary_successor_refuses_inconsistent_batches_and_reports() {
    let store = CanonicalObjectStore::new();
    let empty = install(&store, &ModelState::default());
    let (_, summary) = cold_shape(&store, empty);
    let base = StateVersionV1::new(1, empty);
    let report = |mutations, loaded_nodes, placed_nodes| DagRewriteReport {
        mutations,
        loaded_nodes,
        placed_nodes,
        ..DagRewriteReport::default()
    };
    let key = datum_key("refusal", "refusal", "refused", false);

    let mut batch = RSpaceMutationBatchV1::new(base).expect("the base has a successor");
    batch
        .record_data(&key, 0, 1)
        .expect("an insertion is a valid mutation");
    let insertion = encode(&batch.plan().expect("the insertion plans"));
    let insertion = MutationBatchViewV1::parse(&insertion).expect("the insertion parses");
    assert!(matches!(
        summary.successor(insertion, report(2, 1, 1)),
        Err(CanonicalFormatError::SemanticCoherence { .. })
    ));
    assert!(matches!(
        summary.successor(insertion, report(1, 10, 0)),
        Err(CanonicalFormatError::SemanticCoherence { .. })
    ));
    let inserted = summary
        .successor(insertion, report(1, 1, 1))
        .expect("a consistent report derives the successor");
    assert_eq!(inserted.logical_nodes(), summary.logical_nodes());
    assert_eq!(inserted.endpoints(), 1);
    assert_eq!(inserted.data_entries(), 1);
    assert_eq!(inserted.maximum_logical_path_bytes(), Ok(count(key.len())));

    let mut batch = RSpaceMutationBatchV1::new(base).expect("the base has a successor");
    batch
        .record_data(&key, 1, 0)
        .expect("a deletion is a valid mutation");
    let deletion = encode(&batch.plan().expect("the deletion plans"));
    let deletion = MutationBatchViewV1::parse(&deletion).expect("the deletion parses");
    assert!(matches!(
        summary.successor(deletion, report(1, 1, 1)),
        Err(CanonicalFormatError::SemanticCoherence { .. })
    ));

    let (continuation_key, _) = continuation(&["refusal"], "refusal-body", false);
    let mut batch = RSpaceMutationBatchV1::new(base).expect("the base has a successor");
    batch
        .record_continuation(&continuation_key, None, Some(synthetic_ref(7)))
        .expect("a continuation insertion is a valid mutation");
    let short = encode(&batch.plan().expect("the continuation insertion plans"));
    let short = MutationBatchViewV1::parse(&short).expect("the continuation insertion parses");
    assert_eq!(
        summary.successor(short, report(1, 1, 1)),
        Err(CanonicalFormatError::ObjectLengthTooSmall(50))
    );

    let mut histogram = KeyLengthHistogramV1::new();
    assert_eq!(histogram.maximum(), Ok(0));
    assert!(matches!(histogram.release(4), Err(CanonicalFormatError::SemanticCoherence { .. })));
    for length in [4, 9, 9] {
        histogram
            .record(length)
            .expect("a recorded length is counted");
    }
    assert_eq!(histogram.keys(), 3);
    assert_eq!(histogram.maximum(), Ok(9));
    histogram.release(9).expect("a recorded length is released");
    assert_eq!(histogram.maximum(), Ok(9));
    histogram.release(9).expect("a recorded length is released");
    assert_eq!(histogram.maximum(), Ok(4));
    assert!(matches!(histogram.release(9), Err(CanonicalFormatError::SemanticCoherence { .. })));
    histogram.release(4).expect("a recorded length is released");
    assert_eq!(histogram.maximum(), Ok(0));
    assert_eq!(histogram, KeyLengthHistogramV1::new());
}
