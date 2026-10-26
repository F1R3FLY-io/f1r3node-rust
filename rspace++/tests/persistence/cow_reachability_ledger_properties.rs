use std::cell::Cell;
use std::collections::BTreeSet;
use std::sync::Arc;

use proptest::prelude::*;

use super::incremental_reachability_properties::{
    BUDGET_AXES, BudgetAxis, ComposedPredecessor, CompositionVariant, Graph, HistoryState,
    HistoryStep, Ledger, Predecessor, RetentionEntries, RetentionEntry, TrialCase,
    TrialEnvironment, TrialLimits, TrialOutcome, TrialRefusal, TrialResult, TrialUsage,
    TrialVariant, cold_graph_oracle, cold_predecessor, offset_limits, retained_delta_of,
    root_counts,
};
use super::support::{TEST_SEMANTIC_PROFILE_ID, TracingStore, admit, nonzero};
use crate::rspace::persistence::checkpoint::CheckpointObjectInventoryV1;
use crate::rspace::persistence::closure::ledger_trial::{
    LedgerTrialAxisV1, LedgerTrialLimitsV1, LedgerTrialRefusalV1, LedgerTrialUsageV1,
    PreparedLedgerTrialV1, RootDeltaV1, run_domain_trials, run_ledger_trial,
};
use crate::rspace::persistence::closure::{
    ClosureCreditV1, ClosureDomainV1, ClosureInventoryV1, RetainedUnionClosureReportV1,
    StructuralClosureLedgerV1, verify_structural_closure_in_ledger_with_credit,
};
use crate::rspace::persistence::{
    CanonicalBlob, CanonicalBlobKind, CanonicalDagNode, CanonicalFormatError, CanonicalObjectStore,
    ClosureError, DagChild, EndpointValue, ImmutableObjectStore, LegacyHistoryRoot, ObjectDigest,
    ObjectRefV1, RetainedUnionClosureLimitsV1, SemanticRoots, SemanticStateManifestV1,
    StructuralClosureReport, StructuralValidationLimits, verify_structural_closure_with_limits,
};

const NONFAITHFUL_TRIAL_VARIANTS: [TrialVariant; 6] = [
    TrialVariant::PartialCommit,
    TrialVariant::FinalOnUnion,
    TrialVariant::AllocationsAsScratch,
    TrialVariant::SkipContribution,
    TrialVariant::DuplicateContribution,
    TrialVariant::UnchargedInitialization,
];

const DOMAIN_MUTANTS: [CompositionVariant; 2] =
    [CompositionVariant::CurrentDeltaOnRetained, CompositionVariant::DeduplicatedRetainedRoots];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CreditRule {
    Faithful,
    EpochWide,
    Unexpanded,
}

#[derive(Clone, Copy, Debug)]
enum StepChoice {
    Commit,
    CommitReuse,
    Evict,
}

#[derive(Clone, Debug)]
struct ScenarioSpec {
    multiplicity: Vec<Vec<u8>>,
    content: Vec<Vec<u8>>,
    manifests: Vec<(u8, u8, u8)>,
}

struct Scenario {
    store: TracingStore,
    mirror: TrialEnvironment,
    references: Vec<ObjectRefV1>,
    manifests: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DomainObservation {
    counts: Vec<u64>,
    roots: Vec<u64>,
    inventory: ClosureInventoryV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LedgerObservation {
    objects: Vec<(ObjectRefV1, [u64; 3])>,
    roots: Vec<(ObjectDigest, [u64; 3])>,
    inventories: [ClosureInventoryV1; 3],
    union: Vec<ObjectRefV1>,
}

struct ColdBuild {
    ledger: StructuralClosureLedgerV1,
    reports: Vec<StructuralClosureReport>,
}

#[derive(Clone, Debug)]
struct TrialSpec {
    scenario: ScenarioSpec,
    current: u8,
    historical: Vec<u8>,
    retained: bool,
    added: Vec<u8>,
    removal_mask: Vec<bool>,
    unrooted: Option<u8>,
    offsets: [i64; 9],
}

struct TrialSetup {
    scenario: Scenario,
    predecessor: StructuralClosureLedgerV1,
    domain: ClosureDomainV1,
    case: TrialCase,
    limits: TrialLimits,
    unrooted_removal: bool,
}

#[derive(Clone, Debug)]
struct StepSpec {
    scenario: ScenarioSpec,
    current: u8,
    historical: Vec<u8>,
    choice: (StepChoice, u8),
}

#[derive(Clone, Debug)]
struct HistorySpec {
    scenario: ScenarioSpec,
    initial: u8,
    choices: Vec<(StepChoice, u8)>,
}

#[derive(Clone, Debug)]
struct IndependenceSpec {
    scenario: ScenarioSpec,
    current: u8,
    target: u8,
    unrelated: usize,
}

type TrialAttempt = Result<PreparedLedgerTrialV1, LedgerTrialRefusalV1<CanonicalFormatError>>;

impl Scenario {
    fn build(spec: &ScenarioSpec) -> Self {
        let multiplicity_count = spec.multiplicity.len();
        let content_count = spec.content.len();
        let vertex_count = 2 + multiplicity_count + content_count + spec.manifests.len();
        let store = TracingStore::default();
        let mut references = Vec::with_capacity(vertex_count);
        let mut graph: Graph = Vec::with_capacity(vertex_count);
        references.push(admit(&store.inner, &CanonicalDagNode::empty()));
        graph.push(Vec::new());
        let blob = CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, &b"cl-source"[..])
            .expect("the continuation source is canonical");
        references.push(admit(&store.inner, &blob));
        graph.push(Vec::new());
        for (index, seeds) in spec.multiplicity.iter().enumerate() {
            let targets = earlier_targets(seeds, 2, index);
            let tag = u8::try_from(index).expect("at most four multiplicity nodes");
            let node = CanonicalDagNode::new(
                vec![0x4D, tag],
                EndpointValue::Multiplicity(nonzero(u64::from(tag) + 1)),
                children_of(&references, &targets),
            )
            .expect("a generated multiplicity node is canonical");
            references.push(admit(&store.inner, &node));
            graph.push(targets);
        }
        let content_base = 2 + multiplicity_count;
        for (index, seeds) in spec.content.iter().enumerate() {
            let targets = earlier_targets(seeds, content_base, index);
            let tag = u8::try_from(index).expect("at most two content nodes");
            let node = CanonicalDagNode::new(
                vec![0x43, tag],
                EndpointValue::Content(references[1]),
                children_of(&references, &targets),
            )
            .expect("a generated content node is canonical");
            references.push(admit(&store.inner, &node));
            let mut outgoing = Vec::with_capacity(targets.len() + 1);
            outgoing.push(1);
            outgoing.extend_from_slice(&targets);
            graph.push(outgoing);
        }
        let mut manifests = Vec::with_capacity(spec.manifests.len());
        let bag_root = |seed: u8| match usize::from(seed) % (multiplicity_count + 1) {
            0 => 0,
            node => 1 + node,
        };
        for (index, &(data, continuation, join)) in spec.manifests.iter().enumerate() {
            let continuation_root = match usize::from(continuation) % (content_count + 1) {
                0 => 0,
                node => content_base + node - 1,
            };
            let roots = vec![bag_root(data), continuation_root, bag_root(join)];
            let tag = u8::try_from(index).expect("at most four manifests");
            let manifest = SemanticStateManifestV1::new(
                1,
                TEST_SEMANTIC_PROFILE_ID,
                SemanticRoots {
                    data_bag: references[roots[0]],
                    continuation_map: references[roots[1]],
                    join_bag: references[roots[2]],
                },
                Some(LegacyHistoryRoot::from_array([0xA0 + tag; 32])),
            )
            .expect("a generated manifest is valid");
            manifests.push(references.len());
            references.push(admit(&store.inner, &manifest));
            graph.push(roots);
        }
        let weights = references
            .iter()
            .map(|reference| reference.encoded_len())
            .collect::<Vec<_>>();
        Self {
            store,
            mirror: TrialEnvironment {
                graph,
                object_bytes: weights.clone(),
                quoted_read: weights.clone(),
                consumed_read: weights,
            },
            references,
            manifests,
        }
    }

    fn manifest_vertex(&self, seed: u8) -> usize {
        self.manifests[usize::from(seed) % self.manifests.len()]
    }

    fn entries(&self, current: u8, historical: &[u8]) -> RetentionEntries {
        RetentionEntries {
            current: RetentionEntry {
                generation: historical.len(),
                manifest: self.manifest_vertex(current),
            },
            historical: historical
                .iter()
                .enumerate()
                .map(|(generation, &seed)| RetentionEntry {
                    generation,
                    manifest: self.manifest_vertex(seed),
                })
                .collect(),
        }
    }

    fn refs(&self, vertices: &[usize]) -> Vec<ObjectRefV1> {
        vertices
            .iter()
            .map(|&vertex| self.references[vertex])
            .collect()
    }

    fn reads(&self, references: &[ObjectRefV1]) -> Vec<u64> {
        let reads = references
            .iter()
            .map(|reference| self.store.read_count(reference.digest()))
            .collect::<Vec<_>>();
        assert_eq!(
            reads.iter().sum::<u64>(),
            self.store.total_reads(),
            "a walk read an object outside the observed references"
        );
        reads
    }
}

fn earlier_targets(seeds: &[u8], base: usize, index: usize) -> Vec<usize> {
    match index {
        0 => Vec::new(),
        _ => seeds
            .iter()
            .map(|&seed| base + usize::from(seed) % index)
            .collect(),
    }
}

fn children_of(references: &[ObjectRefV1], targets: &[usize]) -> Vec<DagChild> {
    targets
        .iter()
        .enumerate()
        .map(|(edge, &target)| DagChild {
            edge: u8::try_from(edge).expect("generated fanout fits one byte"),
            target: references[target],
        })
        .collect()
}

fn wide(value: usize) -> u64 { u64::try_from(value).expect("test counts fit u64") }

fn narrow(value: u64) -> usize { usize::try_from(value).expect("test counts fit usize") }

fn others(domain: ClosureDomainV1) -> impl Iterator<Item = ClosureDomainV1> {
    ClosureDomainV1::ALL
        .into_iter()
        .filter(move |other| *other != domain)
}

fn cold_build_refs<S>(
    store: &S,
    current: ObjectRefV1,
    historical: &[ObjectRefV1],
    current_credit: ClosureCreditV1,
    union_limits: RetainedUnionClosureLimitsV1,
) -> Result<ColdBuild, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    let mut ledger = StructuralClosureLedgerV1::new();
    let mut reports = Vec::with_capacity(historical.len() + 1);
    let states = std::iter::once((current, current_credit)).chain(
        historical
            .iter()
            .map(|&manifest| (manifest, ClosureCreditV1::RETAINED)),
    );
    for (manifest, credit) in states {
        let admitted = verify_structural_closure_in_ledger_with_credit(
            store,
            manifest,
            StructuralValidationLimits::unbounded(),
            &mut ledger,
            union_limits,
            credit,
        )?;
        reports.push(admitted.report());
    }
    Ok(ColdBuild { ledger, reports })
}

fn cold_build(
    scenario: &Scenario,
    entries: &RetentionEntries,
    current_credit: ClosureCreditV1,
    union_limits: RetainedUnionClosureLimitsV1,
) -> Result<ColdBuild, ClosureError<CanonicalFormatError>> {
    let historical = entries
        .historical
        .iter()
        .map(|entry| scenario.references[entry.manifest])
        .collect::<Vec<_>>();
    cold_build_refs(
        &scenario.store,
        scenario.references[entries.current.manifest],
        &historical,
        current_credit,
        union_limits,
    )
}

fn credited_build(scenario: &Scenario, entries: &RetentionEntries) -> StructuralClosureLedgerV1 {
    cold_build(
        scenario,
        entries,
        ClosureCreditV1::CURRENT_AND_RETAINED,
        RetainedUnionClosureLimitsV1::UNBOUNDED,
    )
    .expect("generated entries are structurally admitted")
    .ledger
}

fn observe_domain(
    references: &[ObjectRefV1],
    ledger: &StructuralClosureLedgerV1,
    domain: ClosureDomainV1,
) -> DomainObservation {
    let known = references
        .iter()
        .map(|reference| reference.digest())
        .collect::<BTreeSet<_>>();
    for (reference, _) in ledger.counted_objects() {
        assert!(known.contains(&reference.digest()), "the ledger counts an unknown object");
    }
    for (digest, _) in ledger.root_multiplicities() {
        assert!(known.contains(&digest), "the ledger roots an unknown object");
    }
    DomainObservation {
        counts: references
            .iter()
            .map(|reference| ledger.count(domain, reference.digest()))
            .collect(),
        roots: references
            .iter()
            .map(|reference| ledger.root_count(domain, reference.digest()))
            .collect(),
        inventory: ledger.inventory(domain),
    }
}

fn observation_of(predecessor: &Predecessor) -> DomainObservation {
    DomainObservation {
        counts: predecessor
            .ledger
            .counts
            .iter()
            .map(|&count| wide(count))
            .collect(),
        roots: predecessor.roots.iter().map(|&count| wide(count)).collect(),
        inventory: ClosureInventoryV1 {
            distinct_objects: predecessor.objects,
            canonical_bytes: predecessor.bytes,
        },
    }
}

fn oracle_domain(env: &TrialEnvironment, root_events: &[usize]) -> DomainObservation {
    observation_of(&cold_predecessor(env, root_events))
}

fn retained_union(ledger: &StructuralClosureLedgerV1) -> Vec<ObjectRefV1> {
    let mut union = Vec::new();
    ledger
        .try_visit_retained_union(|reference| {
            union.push(reference);
            Ok::<(), std::convert::Infallible>(())
        })
        .expect("the union visitor is infallible");
    union
}

fn observe_ledger(ledger: &StructuralClosureLedgerV1) -> LedgerObservation {
    LedgerObservation {
        objects: ledger.counted_objects(),
        roots: ledger.root_multiplicities(),
        inventories: ClosureDomainV1::ALL.map(|domain| ledger.inventory(domain)),
        union: retained_union(ledger),
    }
}

fn model_axis(axis: LedgerTrialAxisV1) -> BudgetAxis {
    match axis {
        LedgerTrialAxisV1::Count => BudgetAxis::Count,
        LedgerTrialAxisV1::UnionObjects => BudgetAxis::UnionObjects,
        LedgerTrialAxisV1::UnionBytes => BudgetAxis::UnionBytes,
        LedgerTrialAxisV1::Scratch => BudgetAxis::Scratch,
        LedgerTrialAxisV1::References => BudgetAxis::References,
        LedgerTrialAxisV1::Expansions => BudgetAxis::Expansions,
        LedgerTrialAxisV1::QuotedBytes => BudgetAxis::QuotedBytes,
        LedgerTrialAxisV1::FinalObjects => BudgetAxis::FinalObjects,
        LedgerTrialAxisV1::FinalBytes => BudgetAxis::FinalBytes,
    }
}

fn model_refusal(refusal: &LedgerTrialRefusalV1<CanonicalFormatError>) -> Option<TrialRefusal> {
    match refusal {
        LedgerTrialRefusalV1::Budget(axis) => Some(TrialRefusal::Budget(model_axis(*axis))),
        LedgerTrialRefusalV1::UnauthorizedRemoval => Some(TrialRefusal::UnauthorizedRemoval),
        LedgerTrialRefusalV1::ZeroCountDecrement => Some(TrialRefusal::ZeroCountDecrement),
        LedgerTrialRefusalV1::InventoryUnderflow => Some(TrialRefusal::InventoryUnderflow),
        LedgerTrialRefusalV1::Closure(_) => None,
    }
}

fn model_usage(usage: LedgerTrialUsageV1) -> TrialUsage {
    TrialUsage {
        peak_count: usage.peak_count,
        peak_objects: usage.peak_objects,
        peak_bytes: usage.peak_bytes,
        peak_scratch: usage.peak_scratch,
        references: usage.references,
        expansions: usage.expansions,
        quoted_bytes: usage.quoted_bytes,
        consumed_bytes: usage.consumed_bytes,
    }
}

fn exact_limits(usage: LedgerTrialUsageV1, inventory: ClosureInventoryV1) -> [u64; 9] {
    [
        usage.peak_count,
        usage.peak_objects,
        usage.peak_bytes,
        usage.peak_scratch,
        usage.references,
        usage.expansions,
        usage.quoted_bytes,
        inventory.distinct_objects,
        inventory.canonical_bytes,
    ]
}

fn agrees_with_model(setup: &TrialSetup, model: &TrialResult, production: &TrialAttempt) -> bool {
    if model.retained != setup.case.base {
        return false;
    }
    match (&model.outcome, production) {
        (TrialOutcome::Refused(why), Err(refusal)) => model_refusal(refusal) == Some(*why),
        (TrialOutcome::Prepared(evidence), Ok(prepared)) => {
            let references = &setup.scenario.references;
            let expected = DomainObservation {
                counts: evidence
                    .ledger
                    .counts
                    .iter()
                    .map(|&count| wide(count))
                    .collect(),
                roots: evidence.roots.iter().map(|&count| wide(count)).collect(),
                inventory: ClosureInventoryV1 {
                    distinct_objects: evidence.objects,
                    canonical_bytes: evidence.bytes,
                },
            };
            observe_domain(references, &prepared.successor, setup.domain) == expected &&
                model_usage(prepared.usage) == evidence.usage &&
                others(setup.domain).all(|unchanged| {
                    observe_domain(references, &prepared.successor, unchanged) ==
                        observe_domain(references, &setup.predecessor, unchanged)
                })
        }
        _ => false,
    }
}

fn describe(setup: &TrialSetup, attempt: &TrialAttempt) -> String {
    match attempt {
        Ok(prepared) => format!(
            "prepared {:?} with {:?}",
            observe_domain(&setup.scenario.references, &prepared.successor, setup.domain),
            prepared.usage
        ),
        Err(refusal) => format!("refused {refusal:?}"),
    }
}

fn production_trial(setup: &TrialSetup, limits: LedgerTrialLimitsV1) -> TrialAttempt {
    run_ledger_trial(
        &setup.scenario.store,
        &setup.predecessor,
        setup.domain,
        &setup.scenario.refs(&setup.case.added),
        &setup.scenario.refs(&setup.case.removed),
        limits,
    )
}

fn setup_trial(spec: &TrialSpec) -> TrialSetup {
    let scenario = Scenario::build(&spec.scenario);
    let entries = scenario.entries(spec.current, &spec.historical);
    let predecessor = credited_build(&scenario, &entries);
    let (domain, base_roots) = match spec.retained {
        false => (ClosureDomainV1::Current, vec![entries.current.manifest]),
        true => (ClosureDomainV1::Retained, entries.manifests()),
    };
    let added = spec
        .added
        .iter()
        .map(|&seed| scenario.manifest_vertex(seed))
        .collect::<Vec<_>>();
    let mut removed = base_roots
        .iter()
        .chain(&added)
        .zip(
            spec.removal_mask
                .iter()
                .copied()
                .chain(std::iter::repeat(false)),
        )
        .filter_map(|(&vertex, remove)| remove.then_some(vertex))
        .collect::<Vec<_>>();
    let mut unrooted_removal = false;
    if let Some(seed) = spec.unrooted {
        let manifest = scenario.manifest_vertex(seed);
        if !base_roots.contains(&manifest) && !added.contains(&manifest) {
            removed.push(manifest);
            unrooted_removal = true;
        }
    }
    let case = TrialCase {
        env: scenario.mirror.clone(),
        base: cold_predecessor(&scenario.mirror, &base_roots),
        added,
        removed,
        schedule: vec![false],
    };
    let limits = match case
        .run(TrialVariant::Faithful, &TrialLimits::UNBOUNDED)
        .outcome
    {
        TrialOutcome::Prepared(evidence) => offset_limits(&evidence, spec.offsets),
        TrialOutcome::Refused(_) => TrialLimits::UNBOUNDED,
    };
    TrialSetup {
        scenario,
        predecessor,
        domain,
        case,
        limits,
        unrooted_removal,
    }
}

fn missing_manifest(scenario: &Scenario) -> ObjectRefV1 {
    let empty = scenario.references[0];
    SemanticStateManifestV1::new(
        1,
        TEST_SEMANTIC_PROFILE_ID,
        SemanticRoots {
            data_bag: empty,
            continuation_map: empty,
            join_bag: empty,
        },
        Some(LegacyHistoryRoot::from_array([0x5A; 32])),
    )
    .expect("the missing manifest is valid")
    .object_ref()
}

fn history_steps(
    scenario: &Scenario,
    initial: &HistoryState,
    choices: &[(StepChoice, u8)],
) -> Vec<HistoryStep> {
    let mut state = initial.clone();
    let mut steps = Vec::with_capacity(choices.len());
    let first_generation = state
        .entries
        .historical
        .len()
        .max(state.entries.current.generation) +
        1;
    for (generation, &(choice, seed)) in (first_generation..).zip(choices) {
        let seed = usize::from(seed);
        let step = match (choice, state.entries.historical.as_slice()) {
            (StepChoice::Commit, _) => Some(HistoryStep::Commit {
                next: RetentionEntry {
                    generation,
                    manifest: scenario.manifests[seed % scenario.manifests.len()],
                },
                metadata_root: 0,
            }),
            (StepChoice::CommitReuse, _) => {
                let manifests = state.entries.manifests();
                Some(HistoryStep::Commit {
                    next: RetentionEntry {
                        generation,
                        manifest: manifests[seed % manifests.len()],
                    },
                    metadata_root: 0,
                })
            }
            (StepChoice::Evict, []) => None,
            (StepChoice::Evict, historical) => Some(HistoryStep::Evict {
                victim: historical[seed % historical.len()],
                metadata_root: 0,
            }),
        };
        if let Some(step) = step {
            state = state.apply(step);
            steps.push(step);
        }
    }
    steps
}

fn mirror_predecessor(
    scenario: &Scenario,
    ledger: &StructuralClosureLedgerV1,
    domain: ClosureDomainV1,
) -> Predecessor {
    let observation = observe_domain(&scenario.references, ledger, domain);
    Predecessor {
        ledger: Ledger {
            counts: observation
                .counts
                .iter()
                .map(|&count| narrow(count))
                .collect(),
            active: observation.counts.iter().map(|&count| count > 0).collect(),
        },
        roots: observation
            .roots
            .iter()
            .map(|&count| narrow(count))
            .collect(),
        objects: observation.inventory.distinct_objects,
        bytes: observation.inventory.canonical_bytes,
    }
}

fn mirror_base(scenario: &Scenario, ledger: &StructuralClosureLedgerV1) -> ComposedPredecessor {
    ComposedPredecessor {
        current: mirror_predecessor(scenario, ledger, ClosureDomainV1::Current),
        retained: mirror_predecessor(scenario, ledger, ClosureDomainV1::Retained),
        metadata: Predecessor {
            ledger: Ledger {
                counts: Vec::new(),
                active: Vec::new(),
            },
            roots: Vec::new(),
            objects: 0,
            bytes: 0,
        },
    }
}

fn mirror_credited_epoch(
    graph: &Graph,
    counts: &mut [u64],
    root: usize,
    rule: CreditRule,
) -> Result<(), usize> {
    let mut expanded = vec![false; graph.len()];
    let mut stack = vec![(root, true)];
    while let Some((vertex, credited)) = stack.pop() {
        let crossed = credited && counts[vertex] == 0;
        counts[vertex] += u64::from(credited);
        match (expanded[vertex], crossed) {
            (true, true) => return Err(vertex),
            (true, false) => continue,
            (false, _) => expanded[vertex] = true,
        }
        let child_credit = match rule {
            CreditRule::Faithful => crossed,
            CreditRule::EpochWide => true,
            CreditRule::Unexpanded => false,
        };
        stack.extend(
            graph[vertex]
                .iter()
                .rev()
                .map(|&child| (child, child_credit)),
        );
    }
    Ok(())
}

fn mirror_retained_build(
    graph: &Graph,
    entries: &RetentionEntries,
    rule: CreditRule,
) -> Result<Vec<u64>, usize> {
    let mut counts = vec![0; graph.len()];
    let manifests = std::iter::once(entries.current.manifest)
        .chain(entries.historical.iter().map(|entry| entry.manifest));
    for manifest in manifests {
        mirror_credited_epoch(graph, &mut counts, manifest, rule)?;
    }
    Ok(counts)
}

fn required_count(graph: &Graph, counts: &[u64], roots: &[usize], vertex: usize) -> u64 {
    let incoming = graph
        .iter()
        .enumerate()
        .filter(|(parent, _)| counts[*parent] > 0)
        .map(|(_, children)| wide(children.iter().filter(|&&child| child == vertex).count()))
        .sum::<u64>();
    wide(root_counts(graph.len(), roots)[vertex]) + incoming
}

fn support(counts: &[u64]) -> Vec<bool> { counts.iter().map(|&count| count > 0).collect() }

fn runner() -> prop::test_runner::TestRunner {
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: 256,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

fn scenario_specs() -> impl Strategy<Value = ScenarioSpec> {
    (
        prop::collection::vec(prop::collection::vec(any::<u8>(), 0..4), 1..5),
        prop::collection::vec(prop::collection::vec(any::<u8>(), 0..3), 0..3),
        prop::collection::vec((any::<u8>(), any::<u8>(), any::<u8>()), 1..5),
    )
        .prop_map(|(multiplicity, content, manifests)| ScenarioSpec {
            multiplicity,
            content,
            manifests,
        })
}

fn step_choices() -> impl Strategy<Value = StepChoice> {
    prop_oneof![
        3 => Just(StepChoice::Commit),
        2 => Just(StepChoice::CommitReuse),
        2 => Just(StepChoice::Evict),
    ]
}

fn trial_specs() -> impl Strategy<Value = TrialSpec> {
    (
        scenario_specs(),
        any::<u8>(),
        prop::collection::vec(any::<u8>(), 0..4),
        any::<bool>(),
        prop::collection::vec(any::<u8>(), 0..5),
        prop::collection::vec(any::<bool>(), 0..10),
        prop::option::of(any::<u8>()),
        prop::array::uniform9(-2i64..=2),
    )
        .prop_map(
            |(scenario, current, historical, retained, added, removal_mask, unrooted, offsets)| {
                TrialSpec {
                    scenario,
                    current,
                    historical,
                    retained,
                    added,
                    removal_mask,
                    unrooted,
                    offsets,
                }
            },
        )
}

fn step_specs() -> impl Strategy<Value = StepSpec> {
    (
        scenario_specs(),
        any::<u8>(),
        prop::collection::vec(any::<u8>(), 0..4),
        (step_choices(), any::<u8>()),
    )
        .prop_map(|(scenario, current, historical, choice)| StepSpec {
            scenario,
            current,
            historical,
            choice,
        })
}

fn history_specs() -> impl Strategy<Value = HistorySpec> {
    (scenario_specs(), any::<u8>(), prop::collection::vec((step_choices(), any::<u8>()), 1..4))
        .prop_map(|(scenario, initial, choices)| HistorySpec {
            scenario,
            initial,
            choices,
        })
}

fn independence_specs() -> impl Strategy<Value = IndependenceSpec> {
    (scenario_specs(), any::<u8>(), any::<u8>(), 0usize..=8).prop_map(
        |(scenario, current, target, unrelated)| IndependenceSpec {
            scenario,
            current,
            target,
            unrelated,
        },
    )
}

#[test]
fn credited_cold_build_matches_independent_domain_oracles() {
    runner()
        .run(
            &(scenario_specs(), any::<u8>(), prop::collection::vec(any::<u8>(), 0..4)),
            |(spec, current, historical)| {
                let scenario = Scenario::build(&spec);
                let entries = scenario.entries(current, &historical);
                let vertex_count = scenario.references.len();
                scenario.store.clear_reads();
                let credited = cold_build(
                    &scenario,
                    &entries,
                    ClosureCreditV1::CURRENT_AND_RETAINED,
                    RetainedUnionClosureLimitsV1::UNBOUNDED,
                )
                .map_err(|error| TestCaseError::fail(format!("credited build refused: {error}")))?;
                let credited_reads = scenario.reads(&scenario.references);
                prop_assert_eq!(
                    observe_domain(
                        &scenario.references,
                        &credited.ledger,
                        ClosureDomainV1::Current
                    ),
                    oracle_domain(&scenario.mirror, &[entries.current.manifest])
                );
                prop_assert_eq!(
                    observe_domain(
                        &scenario.references,
                        &credited.ledger,
                        ClosureDomainV1::Retained
                    ),
                    oracle_domain(&scenario.mirror, &entries.manifests())
                );

                let states = std::iter::once(entries.current.manifest)
                    .chain(entries.historical.iter().map(|entry| entry.manifest))
                    .collect::<Vec<_>>();
                let mut union = vec![false; vertex_count];
                for (&manifest, report) in states.iter().zip(&credited.reports) {
                    let standalone = verify_structural_closure_with_limits(
                        &scenario.store,
                        scenario.references[manifest],
                        StructuralValidationLimits::unbounded(),
                    )
                    .map_err(|error| TestCaseError::fail(format!("standalone refused: {error}")))?;
                    prop_assert_eq!(*report, standalone);
                    let closure = cold_graph_oracle(
                        &scenario.mirror.graph,
                        &root_counts(vertex_count, &[manifest]),
                    );
                    for (member, reached) in union.iter_mut().zip(&closure.active) {
                        *member |= *reached;
                    }
                }
                let mut members = scenario
                    .references
                    .iter()
                    .zip(&union)
                    .filter_map(|(reference, member)| member.then_some(*reference))
                    .collect::<Vec<_>>();
                members.sort_by_key(|reference| reference.digest());
                let report = RetainedUnionClosureReportV1 {
                    distinct_objects: wide(members.len()),
                    canonical_bytes: members
                        .iter()
                        .map(|reference| reference.encoded_len())
                        .sum(),
                };
                prop_assert_eq!(credited.ledger.retained_union_report(), report);
                prop_assert_eq!(retained_union(&credited.ledger), members);

                scenario.store.clear_reads();
                let retained_only = cold_build(
                    &scenario,
                    &entries,
                    ClosureCreditV1::RETAINED,
                    RetainedUnionClosureLimitsV1::UNBOUNDED,
                )
                .map_err(|error| TestCaseError::fail(format!("retained build refused: {error}")))?;
                prop_assert_eq!(scenario.reads(&scenario.references), credited_reads);
                prop_assert_eq!(&retained_only.reports, &credited.reports);
                prop_assert_eq!(
                    retained_only.ledger.retained_union_report(),
                    credited.ledger.retained_union_report()
                );
                prop_assert_eq!(
                    retained_union(&retained_only.ledger),
                    retained_union(&credited.ledger)
                );

                for union_limits in [
                    RetainedUnionClosureLimitsV1 {
                        max_distinct_objects: report.distinct_objects - 1,
                        ..RetainedUnionClosureLimitsV1::UNBOUNDED
                    },
                    RetainedUnionClosureLimitsV1 {
                        max_canonical_bytes: report.canonical_bytes - 1,
                        ..RetainedUnionClosureLimitsV1::UNBOUNDED
                    },
                ] {
                    let refusal =
                        |credit| match cold_build(&scenario, &entries, credit, union_limits) {
                            Err(ClosureError::Format(error)) => Some(error),
                            _ => None,
                        };
                    let credited_refusal = refusal(ClosureCreditV1::CURRENT_AND_RETAINED);
                    prop_assert!(
                        matches!(
                            credited_refusal,
                            Some(CanonicalFormatError::ResourceLimit { .. })
                        ),
                        "one below the union did not refuse: {credited_refusal:?}"
                    );
                    prop_assert_eq!(credited_refusal, refusal(ClosureCreditV1::RETAINED));
                }
                Ok(())
            },
        )
        .expect("credited cold builds realize the independent current and retained oracles");
}

#[test]
fn production_ledger_trial_refines_the_model_trial() {
    let unrooted = Cell::new(0usize);
    let refusals = Cell::new(0usize);
    runner()
        .run(&trial_specs(), |spec| {
            let setup = setup_trial(&spec);
            let before = observe_ledger(&setup.predecessor);
            setup.scenario.store.clear_reads();
            let production = production_trial(&setup, LedgerTrialLimitsV1(setup.limits.0));
            let reads = setup.scenario.store.total_reads();
            let model = setup.case.run(TrialVariant::Faithful, &setup.limits);
            prop_assert!(
                agrees_with_model(&setup, &model, &production),
                "model {:?} but production {}",
                model.outcome,
                describe(&setup, &production)
            );
            prop_assert_eq!(&observe_ledger(&setup.predecessor), &before);
            if setup.unrooted_removal {
                prop_assert!(
                    matches!(production, Err(LedgerTrialRefusalV1::UnauthorizedRemoval)),
                    "an unrooted removal was not refused: {}",
                    describe(&setup, &production)
                );
                prop_assert_eq!(reads, 0);
                unrooted.set(unrooted.get() + 1);
            }
            if production.is_err() {
                refusals.set(refusals.get() + 1);
                let retry = production_trial(&setup, LedgerTrialLimitsV1::UNBOUNDED);
                let unbounded = setup
                    .case
                    .run(TrialVariant::Faithful, &TrialLimits::UNBOUNDED);
                prop_assert!(
                    agrees_with_model(&setup, &unbounded, &retry),
                    "retry {} differs from model {:?}",
                    describe(&setup, &retry),
                    unbounded.outcome
                );
                prop_assert_eq!(&observe_ledger(&setup.predecessor), &before);
            }
            let missing = missing_manifest(&setup.scenario);
            let refused = run_ledger_trial(
                &setup.scenario.store,
                &setup.predecessor,
                setup.domain,
                &[missing],
                &[],
                LedgerTrialLimitsV1::UNBOUNDED,
            );
            prop_assert!(
                matches!(
                    &refused,
                    Err(LedgerTrialRefusalV1::Closure(ClosureError::Format(
                        CanonicalFormatError::MissingObject(digest)
                    ))) if *digest == missing.digest()
                ),
                "a missing manifest was not refused as a load failure: {}",
                describe(&setup, &refused)
            );
            prop_assert_eq!(&observe_ledger(&setup.predecessor), &before);
            Ok(())
        })
        .expect("production ledger trials refine the model trial and keep their predecessor");
    assert!(unrooted.get() > 0, "no generated trial removed an unrooted manifest");
    assert!(refusals.get() > 0, "no generated trial was refused");
}

#[test]
fn production_trial_limits_are_exact_per_axis_and_domain() {
    let refused_axes = Cell::new(0usize);
    runner()
        .run(&step_specs(), |spec| {
            let scenario = Scenario::build(&spec.scenario);
            let state = HistoryState {
                entries: scenario.entries(spec.current, &spec.historical),
                metadata_root: 0,
                pins: Vec::new(),
            };
            let Some(&step) = history_steps(&scenario, &state, &[spec.choice]).first() else {
                return Ok(());
            };
            let delta = state.canonical_delta(step);
            let predecessor = credited_build(&scenario, &state.entries);
            let current =
                (scenario.refs(&delta.current.added), scenario.refs(&delta.current.removed));
            let retained =
                (scenario.refs(&delta.retained.added), scenario.refs(&delta.retained.removed));

            for (domain, (added, removed)) in
                [(ClosureDomainV1::Current, &current), (ClosureDomainV1::Retained, &retained)]
            {
                let trial = |limits: [u64; 9]| {
                    run_ledger_trial(
                        &scenario.store,
                        &predecessor,
                        domain,
                        added,
                        removed,
                        LedgerTrialLimitsV1(limits),
                    )
                };
                let unbounded = trial(LedgerTrialLimitsV1::UNBOUNDED.0).map_err(|refusal| {
                    TestCaseError::fail(format!("a canonical delta was refused: {refusal:?}"))
                })?;
                let exact = exact_limits(unbounded.usage, unbounded.successor.inventory(domain));
                let at_exact = trial(exact).map_err(|refusal| {
                    TestCaseError::fail(format!("exact limits refused: {refusal:?}"))
                })?;
                prop_assert_eq!(at_exact.usage, unbounded.usage);
                prop_assert_eq!(
                    observe_ledger(&at_exact.successor),
                    observe_ledger(&unbounded.successor)
                );
                for axis in BUDGET_AXES {
                    let index = axis as usize;
                    if exact[index] == 0 {
                        continue;
                    }
                    let mut tight = exact;
                    tight[index] -= 1;
                    let refused = trial(tight);
                    prop_assert!(
                        matches!(
                            &refused,
                            Err(LedgerTrialRefusalV1::Budget(refused_axis))
                                if model_axis(*refused_axis) == axis
                        ),
                        "{domain:?} one below {axis:?} gave {:?}",
                        refused.as_ref().err()
                    );
                    refused_axes.set(refused_axes.get() + 1);
                }
            }

            let composed = |limits: [[u64; 9]; 2]| {
                run_domain_trials(
                    &scenario.store,
                    &predecessor,
                    RootDeltaV1 {
                        added: &current.0,
                        removed: &current.1,
                    },
                    RootDeltaV1 {
                        added: &retained.0,
                        removed: &retained.1,
                    },
                    limits.map(LedgerTrialLimitsV1),
                )
            };
            let unbounded = composed([LedgerTrialLimitsV1::UNBOUNDED.0; 2]).map_err(|refusal| {
                TestCaseError::fail(format!("canonical deltas were refused: {refusal:?}"))
            })?;
            let exact = [
                exact_limits(
                    unbounded.usage[0],
                    unbounded.successor.inventory(ClosureDomainV1::Current),
                ),
                exact_limits(
                    unbounded.usage[1],
                    unbounded.successor.inventory(ClosureDomainV1::Retained),
                ),
            ];
            let at_exact = composed(exact).map_err(|refusal| {
                TestCaseError::fail(format!("exact composed limits refused: {refusal:?}"))
            })?;
            prop_assert_eq!(at_exact.usage, unbounded.usage);
            prop_assert_eq!(
                observe_ledger(&at_exact.successor),
                observe_ledger(&unbounded.successor)
            );
            for (position, domain) in ClosureDomainV1::SEMANTIC.into_iter().enumerate() {
                for axis in BUDGET_AXES {
                    let index = axis as usize;
                    if exact[position][index] == 0 {
                        continue;
                    }
                    let mut tight = exact;
                    tight[position][index] -= 1;
                    let refused = composed(tight);
                    prop_assert!(
                        matches!(
                            &refused,
                            Err((refused_domain, LedgerTrialRefusalV1::Budget(refused_axis)))
                                if *refused_domain == domain && model_axis(*refused_axis) == axis
                        ),
                        "composed {domain:?} one below {axis:?} gave {:?}",
                        refused.as_ref().err()
                    );
                }
            }
            let first_positive = |limits: &[u64; 9]| {
                BUDGET_AXES
                    .into_iter()
                    .find(|axis| limits[*axis as usize] > 0)
            };
            if let (Some(current_axis), Some(retained_axis)) =
                (first_positive(&exact[0]), first_positive(&exact[1]))
            {
                let mut tight = exact;
                tight[0][current_axis as usize] -= 1;
                tight[1][retained_axis as usize] -= 1;
                let refused = composed(tight);
                prop_assert!(
                    matches!(
                        &refused,
                        Err((ClosureDomainV1::Current, LedgerTrialRefusalV1::Budget(axis)))
                            if model_axis(*axis) == current_axis
                    ),
                    "tightening both domains gave {:?}",
                    refused.as_ref().err()
                );
            }
            Ok(())
        })
        .expect("production trial limits are exact on every axis of every domain");
    assert!(refused_axes.get() > 0, "no generated trial had a positive axis");
}

#[test]
fn composed_production_trials_match_cold_domains_and_checkpoint_inventory() {
    let alias_evictions = Cell::new(0usize);
    let steps_taken = Cell::new(0usize);
    let retention = ObjectRefV1::new(ObjectDigest::from_array([0xFF; 32]), 64)
        .expect("the synthetic retention manifest reference is complete");
    runner()
        .run(&history_specs(), |spec| {
            let scenario = Scenario::build(&spec.scenario);
            let mut state = HistoryState::first(scenario.manifest_vertex(spec.initial), 0);
            let steps = history_steps(&scenario, &state, &spec.choices);
            let mut base = credited_build(&scenario, &state.entries);
            for step in steps {
                if let HistoryStep::Evict { victim, .. } = step {
                    let aliases = state
                        .entries
                        .manifests()
                        .iter()
                        .filter(|&&manifest| manifest == victim.manifest)
                        .count();
                    if aliases > 1 {
                        alias_evictions.set(alias_evictions.get() + 1);
                    }
                }
                let delta = state.canonical_delta(step);
                let (current_added, current_removed) =
                    (scenario.refs(&delta.current.added), scenario.refs(&delta.current.removed));
                let (retained_added, retained_removed) =
                    (scenario.refs(&delta.retained.added), scenario.refs(&delta.retained.removed));
                let prepared = run_domain_trials(
                    &scenario.store,
                    &base,
                    RootDeltaV1 {
                        added: &current_added,
                        removed: &current_removed,
                    },
                    RootDeltaV1 {
                        added: &retained_added,
                        removed: &retained_removed,
                    },
                    [LedgerTrialLimitsV1::UNBOUNDED; 2],
                )
                .map_err(|refusal| {
                    TestCaseError::fail(format!("a canonical step was refused: {refusal:?}"))
                })?;
                state = state.apply(step);
                let cold = credited_build(&scenario, &state.entries);
                for domain in ClosureDomainV1::ALL {
                    prop_assert_eq!(
                        observe_domain(&scenario.references, &prepared.successor, domain),
                        observe_domain(&scenario.references, &cold, domain),
                        "{:?} after {:?}",
                        domain,
                        step
                    );
                }
                prop_assert_eq!(retained_union(&prepared.successor), retained_union(&cold));
                prop_assert_eq!(
                    CheckpointObjectInventoryV1::from_retained_union(
                        &prepared.successor,
                        retention
                    )
                    .expect("the successor has a checkpoint inventory"),
                    CheckpointObjectInventoryV1::from_retained_union(&cold, retention)
                        .expect("the cold build has a checkpoint inventory")
                );
                steps_taken.set(steps_taken.get() + 1);
                base = prepared.successor;
            }
            Ok(())
        })
        .expect("composed production trials match the credited cold build at every step");
    assert!(steps_taken.get() > 0, "no generated history took a step");
    assert!(alias_evictions.get() > 0, "no generated history evicted a shared manifest");
}

#[test]
fn production_trial_work_is_independent_of_unchanged_history() {
    let with_history = Cell::new(0usize);
    runner()
        .run(&independence_specs(), |spec| {
            let scenario = Scenario::build(&spec.scenario);
            let mut unrelated_manifests = Vec::with_capacity(spec.unrelated);
            let mut unrelated_objects = Vec::with_capacity(4 * spec.unrelated);
            for index in 0..spec.unrelated {
                let tag = u8::try_from(index).expect("at most eight unrelated states");
                let leaf = admit(
                    &scenario.store.inner,
                    &CanonicalDagNode::new(
                        vec![0x55, tag],
                        EndpointValue::Multiplicity(nonzero(u64::from(tag) + 1)),
                        Vec::<DagChild>::new(),
                    )
                    .expect("an unrelated leaf is canonical"),
                );
                let blob = admit(
                    &scenario.store.inner,
                    &CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, vec![0x75, tag])
                        .expect("an unrelated source is canonical"),
                );
                let content = admit(
                    &scenario.store.inner,
                    &CanonicalDagNode::new(
                        vec![0x63, tag],
                        EndpointValue::Content(blob),
                        Vec::<DagChild>::new(),
                    )
                    .expect("an unrelated content node is canonical"),
                );
                let manifest = admit(
                    &scenario.store.inner,
                    &SemanticStateManifestV1::new(
                        1,
                        TEST_SEMANTIC_PROFILE_ID,
                        SemanticRoots {
                            data_bag: leaf,
                            continuation_map: content,
                            join_bag: leaf,
                        },
                        Some(LegacyHistoryRoot::from_array([0xC0 + tag; 32])),
                    )
                    .expect("an unrelated manifest is valid"),
                );
                unrelated_objects.extend([leaf, blob, content, manifest]);
                unrelated_manifests.push(manifest);
            }
            let observed = [scenario.references.as_slice(), &unrelated_objects].concat();
            let unrelated = ClosureInventoryV1 {
                distinct_objects: wide(unrelated_objects.len()),
                canonical_bytes: unrelated_objects
                    .iter()
                    .map(|reference| reference.encoded_len())
                    .sum(),
            };
            let current = scenario.references[scenario.manifest_vertex(spec.current)];
            let target = scenario.references[scenario.manifest_vertex(spec.target)];
            let build = |historical: &[ObjectRefV1]| {
                cold_build_refs(
                    &scenario.store,
                    current,
                    historical,
                    ClosureCreditV1::CURRENT_AND_RETAINED,
                    RetainedUnionClosureLimitsV1::UNBOUNDED,
                )
                .expect("the predecessor states are admitted")
                .ledger
            };
            let alone = build(&[]);
            let extended = build(&unrelated_manifests);
            let commit = |predecessor: &StructuralClosureLedgerV1| {
                scenario.store.clear_reads();
                let prepared = run_domain_trials(
                    &scenario.store,
                    predecessor,
                    RootDeltaV1 {
                        added: &[target],
                        removed: &[current],
                    },
                    RootDeltaV1 {
                        added: &[target],
                        removed: &[],
                    },
                    [LedgerTrialLimitsV1::UNBOUNDED; 2],
                )
                .expect("a commit prepares without limits");
                (prepared, scenario.reads(&observed))
            };
            let (alone_commit, alone_reads) = commit(&alone);
            let (extended_commit, extended_reads) = commit(&extended);
            prop_assert_eq!(&alone_reads, &extended_reads);
            prop_assert!(
                alone_reads[scenario.references.len()..]
                    .iter()
                    .all(|&reads| reads == 0)
            );
            prop_assert_eq!(alone_commit.usage[0], extended_commit.usage[0]);
            let alone_retained = alone_commit.usage[1];
            prop_assert_eq!(
                LedgerTrialUsageV1 {
                    peak_objects: alone_retained.peak_objects + unrelated.distinct_objects,
                    peak_bytes: alone_retained.peak_bytes + unrelated.canonical_bytes,
                    ..alone_retained
                },
                extended_commit.usage[1]
            );
            prop_assert_eq!(
                alone_commit.successor.inventory(ClosureDomainV1::Current),
                extended_commit
                    .successor
                    .inventory(ClosureDomainV1::Current)
            );
            let alone_union = alone_commit.successor.inventory(ClosureDomainV1::Retained);
            prop_assert_eq!(
                ClosureInventoryV1 {
                    distinct_objects: alone_union.distinct_objects + unrelated.distinct_objects,
                    canonical_bytes: alone_union.canonical_bytes + unrelated.canonical_bytes,
                },
                extended_commit
                    .successor
                    .inventory(ClosureDomainV1::Retained)
            );
            let unrelated_counts = |ledger: &StructuralClosureLedgerV1| {
                observe_domain(&observed, ledger, ClosureDomainV1::Retained).counts
                    [scenario.references.len()..]
                    .to_vec()
            };
            prop_assert_eq!(
                unrelated_counts(&extended_commit.successor),
                unrelated_counts(&extended)
            );
            if spec.unrelated > 0 {
                with_history.set(with_history.get() + 1);
            }
            Ok(())
        })
        .expect("commit work is independent of unrelated retained history");
    assert!(with_history.get() > 0, "no generated case retained unrelated history");
}

#[test]
fn twenty_thousand_level_chain_is_credited_and_traded_on_a_small_stack() {
    const DEPTH: usize = 20_000;
    let store = Arc::new(CanonicalObjectStore::new());
    let mut references = Vec::with_capacity(2 * DEPTH + 3);
    let mut graph: Graph = Vec::with_capacity(2 * DEPTH + 3);
    let empty = admit(&store, &CanonicalDagNode::empty());
    references.push(empty);
    graph.push(Vec::new());
    let mut heads = Vec::with_capacity(2);
    for leaf_prefix in [&b"leaf-a"[..], &b"leaf-b"[..]] {
        let mut child = admit(
            &store,
            &CanonicalDagNode::new(
                leaf_prefix,
                EndpointValue::Multiplicity(nonzero(1)),
                Vec::<DagChild>::new(),
            )
            .expect("a chain leaf is canonical"),
        );
        references.push(child);
        graph.push(Vec::new());
        for depth in 1..DEPTH {
            child = admit(
                &store,
                &CanonicalDagNode::new(
                    Box::<[u8]>::default(),
                    EndpointValue::Multiplicity(nonzero(1)),
                    vec![DagChild {
                        edge: u8::try_from(depth % 251).expect("the edge fits one byte"),
                        target: child,
                    }],
                )
                .expect("an endpoint-bearing unary node is canonical"),
            );
            graph.push(vec![references.len() - 1]);
            references.push(child);
        }
        heads.push(references.len() - 1);
    }
    let mut manifests = Vec::with_capacity(2);
    for (tag, &head) in (0u8..).zip(&heads) {
        let manifest = SemanticStateManifestV1::new(
            1,
            TEST_SEMANTIC_PROFILE_ID,
            SemanticRoots {
                data_bag: references[head],
                continuation_map: empty,
                join_bag: empty,
            },
            Some(LegacyHistoryRoot::from_array([0xD0 + tag; 32])),
        )
        .expect("a chain manifest is valid");
        manifests.push(references.len());
        references.push(admit(&store, &manifest));
        graph.push(vec![head, 0, 0]);
    }
    let weights = references
        .iter()
        .map(|reference| reference.encoded_len())
        .collect::<Vec<_>>();
    let mirror = TrialEnvironment {
        graph,
        object_bytes: weights.clone(),
        quoted_read: weights.clone(),
        consumed_read: weights,
    };
    let (first, second) = (references[manifests[0]], references[manifests[1]]);
    let shared_references = Arc::new(references);
    let thread_references = Arc::clone(&shared_references);
    let thread_store = Arc::clone(&store);
    let [cold_current, cold_retained, traded_current, traded_retained] =
        std::thread::Builder::new()
            .name("credited-ledger-small-stack".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let store = thread_store.as_ref();
                let cold = cold_build_refs(
                    store,
                    first,
                    &[],
                    ClosureCreditV1::CURRENT_AND_RETAINED,
                    RetainedUnionClosureLimitsV1::UNBOUNDED,
                )
                .expect("the deep chain is credited")
                .ledger;
                let traded = run_domain_trials(
                    store,
                    &cold,
                    RootDeltaV1 {
                        added: &[second],
                        removed: &[first],
                    },
                    RootDeltaV1 {
                        added: &[second],
                        removed: &[],
                    },
                    [LedgerTrialLimitsV1::UNBOUNDED; 2],
                )
                .expect("the deep commit prepares")
                .successor;
                [
                    observe_domain(&thread_references, &cold, ClosureDomainV1::Current),
                    observe_domain(&thread_references, &cold, ClosureDomainV1::Retained),
                    observe_domain(&thread_references, &traded, ClosureDomainV1::Current),
                    observe_domain(&thread_references, &traded, ClosureDomainV1::Retained),
                ]
            })
            .expect("the small-stack thread starts")
            .join()
            .expect("the small-stack credited walk and trial do not overflow");
    assert_eq!(cold_current, oracle_domain(&mirror, &[manifests[0]]));
    assert_eq!(cold_retained, oracle_domain(&mirror, &[manifests[0]]));
    assert_eq!(traded_current, oracle_domain(&mirror, &[manifests[1]]));
    assert_eq!(traded_retained, oracle_domain(&mirror, &manifests));
    assert_eq!(traded_retained.inventory.distinct_objects, wide(2 * DEPTH + 3));
}

#[test]
fn cold_oracle_differential_detects_credit_trial_and_domain_mutants() {
    let chain: Graph = vec![vec![], vec![0], vec![1], vec![2]];
    let mut faithful = vec![1, 1, 0, 0];
    let mut epoch_wide = faithful.clone();
    mirror_credited_epoch(&chain, &mut faithful, 2, CreditRule::Faithful)
        .expect("the faithful epoch has no unexpanded crossing");
    mirror_credited_epoch(&chain, &mut epoch_wide, 2, CreditRule::EpochWide)
        .expect("the epoch-wide epoch has no unexpanded crossing");
    assert_eq!(
        (faithful[0], epoch_wide[0], required_count(&chain, &epoch_wide, &[1, 2], 0)),
        (1, 2, 1)
    );
    for (vertex, &count) in faithful.iter().enumerate() {
        assert_eq!(count, required_count(&chain, &faithful, &[1, 2], vertex));
    }
    let mut faithful = vec![0; 4];
    let mut unexpanded = vec![0; 4];
    mirror_credited_epoch(&chain, &mut faithful, 3, CreditRule::Faithful)
        .expect("the faithful epoch has no unexpanded crossing");
    mirror_credited_epoch(&chain, &mut unexpanded, 3, CreditRule::Unexpanded)
        .expect("the unexpanded epoch has no memoized crossing");
    assert_eq!(
        (faithful[2], unexpanded[2], required_count(&chain, &unexpanded, &[3], 2)),
        (1, 0, 1)
    );
    for (vertex, &count) in faithful.iter().enumerate() {
        assert_eq!(count, required_count(&chain, &faithful, &[3], vertex));
    }

    let credit_detections = Cell::new([0usize; 2]);
    runner()
        .run(
            &(scenario_specs(), any::<u8>(), prop::collection::vec(any::<u8>(), 0..4)),
            |(spec, current, historical)| {
                let scenario = Scenario::build(&spec);
                let entries = scenario.entries(current, &historical);
                let oracle = oracle_domain(&scenario.mirror, &entries.manifests()).counts;
                let faithful =
                    mirror_retained_build(&scenario.mirror.graph, &entries, CreditRule::Faithful);
                prop_assert_eq!(faithful, Ok(oracle.clone()));
                let mut detections = credit_detections.get();
                for (position, rule) in [CreditRule::EpochWide, CreditRule::Unexpanded]
                    .into_iter()
                    .enumerate()
                {
                    if mirror_retained_build(&scenario.mirror.graph, &entries, rule) !=
                        Ok(oracle.clone())
                    {
                        detections[position] += 1;
                    }
                }
                credit_detections.set(detections);
                Ok(())
            },
        )
        .expect("the faithful mirror credit rule realizes the cold oracle");
    assert!(
        credit_detections.get().iter().all(|&count| count > 0),
        "a crediting mutant escaped the cold oracle: {:?}",
        credit_detections.get()
    );

    let trial_detections = Cell::new([0usize; 6]);
    runner()
        .run(&trial_specs(), |spec| {
            let setup = setup_trial(&spec);
            let mut limit_sets = Vec::with_capacity(BUDGET_AXES.len() + 2);
            limit_sets.push(TrialLimits::UNBOUNDED);
            if let TrialOutcome::Prepared(evidence) = setup
                .case
                .run(TrialVariant::Faithful, &TrialLimits::UNBOUNDED)
                .outcome
            {
                let exact = offset_limits(&evidence, [0; 9]);
                limit_sets.push(exact);
                for (index, measured) in exact.0.into_iter().enumerate() {
                    if measured > 0 {
                        let mut offsets = [0; 9];
                        offsets[index] = -1;
                        limit_sets.push(offset_limits(&evidence, offsets));
                    }
                }
            }
            let mut detections = trial_detections.get();
            for limits in &limit_sets {
                let production = production_trial(&setup, LedgerTrialLimitsV1(limits.0));
                let faithful = setup.case.run(TrialVariant::Faithful, limits);
                prop_assert!(agrees_with_model(&setup, &faithful, &production));
                for (position, variant) in NONFAITHFUL_TRIAL_VARIANTS.into_iter().enumerate() {
                    if !agrees_with_model(&setup, &setup.case.run(variant, limits), &production) {
                        detections[position] += 1;
                    }
                }
            }
            trial_detections.set(detections);
            Ok(())
        })
        .expect("the faithful model trial agrees with production under every limit set");
    assert!(
        trial_detections.get().iter().all(|&count| count > 0),
        "a trial mutant escaped the production differential: {:?}",
        trial_detections.get()
    );

    let domain_detections = Cell::new([0usize; 2]);
    runner()
        .run(&history_specs(), |spec| {
            let scenario = Scenario::build(&spec.scenario);
            let initial = HistoryState::first(scenario.manifest_vertex(spec.initial), 0);
            let steps = history_steps(&scenario, &initial, &spec.choices);
            let mut detections = domain_detections.get();
            for (position, variant) in DOMAIN_MUTANTS.into_iter().enumerate() {
                let mut state = initial.clone();
                let mut base = credited_build(&scenario, &state.entries);
                for &step in &steps {
                    let delta = state.canonical_delta(step);
                    let retained_delta =
                        retained_delta_of(variant, &mirror_base(&scenario, &base), &delta);
                    let (current_added, current_removed) = (
                        scenario.refs(&delta.current.added),
                        scenario.refs(&delta.current.removed),
                    );
                    let (retained_added, retained_removed) = (
                        scenario.refs(&retained_delta.added),
                        scenario.refs(&retained_delta.removed),
                    );
                    state = state.apply(step);
                    let outcome = run_domain_trials(
                        &scenario.store,
                        &base,
                        RootDeltaV1 {
                            added: &current_added,
                            removed: &current_removed,
                        },
                        RootDeltaV1 {
                            added: &retained_added,
                            removed: &retained_removed,
                        },
                        [LedgerTrialLimitsV1::UNBOUNDED; 2],
                    );
                    let Ok(prepared) = outcome else {
                        detections[position] += 1;
                        break;
                    };
                    let observed = observe_domain(
                        &scenario.references,
                        &prepared.successor,
                        ClosureDomainV1::Retained,
                    );
                    let oracle = oracle_domain(&scenario.mirror, &state.entries.manifests());
                    if support(&observed.counts) != support(&oracle.counts) ||
                        observed.inventory != oracle.inventory
                    {
                        detections[position] += 1;
                        break;
                    }
                    base = prepared.successor;
                }
            }
            domain_detections.set(detections);
            Ok(())
        })
        .expect("mutant retained deltas run against the cold retained domain");
    assert!(
        domain_detections.get().iter().all(|&count| count > 0),
        "a retained-delta mutant escaped the cold retained domain: {:?}",
        domain_detections.get()
    );
}
