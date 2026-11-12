mod reference_tuplespace;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use reference_tuplespace::{
    ConformancePlan, ConformanceReport, ContinuationEntry, DatumEntry, FiredBody, Guard,
    GuardedContinuation, GuardingMatch, ModeReport, Operation, Outcome, Pattern, PropertyTally,
    Rendezvous, Selected, SpacePair, StagedReport, StepKind, TestContents, TestOperation,
    TestOutcome, TestReference, TestRendezvous, generate_program, generate_staged_state,
    run_conformance,
};
use rspace_plus_plus::rspace::history::history_repository::HistoryRepositoryInstances;
use rspace_plus_plus::rspace::hot_store::{HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const TRACE_SEEDS: Range<u64> = 0..64;
const OPERATIONS_PER_TRACE: usize = 40;
const STAGED_SEEDS: Range<u64> = 1_000..1_064;
const REISSUE_DEPTH_LIMIT: usize = 3;

fn production_matcher() -> Arc<Box<dyn Match<Pattern, String, GuardedContinuation>>> {
    Arc::new(Box::new(GuardingMatch))
}

async fn in_memory_space_pair() -> SpacePair {
    let mut store_manager = InMemoryStoreManager::new();
    let store = store_manager
        .r_space_stores()
        .await
        .expect("in-memory rspace stores");
    let history_repository = Arc::new(
        HistoryRepositoryInstances::<String, Pattern, String, GuardedContinuation>::repository(
            store.authority(),
        )
        .expect("history repository over the in-memory store"),
    );
    let history_reader = history_repository
        .get_history_reader(&history_repository.root())
        .expect("history reader at the empty root");
    let play = RSpace::apply(
        history_repository.clone(),
        HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), history_reader.base()),
        production_matcher(),
    );
    let replay = ReplayRSpace::apply(
        history_repository,
        Arc::new(HotStoreInstances::create_from_hs_and_hr(
            HotStoreState::default(),
            history_reader.base(),
        )),
        production_matcher(),
    );
    SpacePair {
        play: Box::new(play),
        replay: Box::new(replay),
    }
}

fn conformance_report() -> &'static ConformanceReport {
    static REPORT: OnceLock<ConformanceReport> = OnceLock::new();
    REPORT.get_or_init(|| {
        let plan = ConformancePlan {
            trace_seeds: TRACE_SEEDS,
            operations_per_trace: OPERATIONS_PER_TRACE,
            staged_seeds: STAGED_SEEDS,
            reissue_depth_limit: REISSUE_DEPTH_LIMIT,
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread tokio runtime for the conformance run")
            .block_on(run_conformance(in_memory_space_pair, &GuardingMatch, &plan))
    })
}

fn modes(report: &ConformanceReport) -> [(&'static str, &ModeReport); 2] {
    [("play", &report.play), ("replay", &report.replay)]
}

fn staged_modes(report: &ConformanceReport) -> [(&'static str, &StagedReport); 2] {
    [("staged play", &report.staged_play), ("staged replay", &report.staged_replay)]
}

fn assert_property(name: &str, tally: &PropertyTally) {
    assert!(tally.checks > 0, "{name}: no check ran");
    assert_eq!(
        tally.violations, 0,
        "{name}: {} of {} checks failed; the first failures are {:#?}",
        tally.violations, tally.checks, tally.first_violations
    );
}

fn assert_every_trace_completed(report: &ConformanceReport) {
    for (mode, mode_report) in modes(report) {
        assert!(
            mode_report.failures.is_empty(),
            "{mode}: the driver could not complete: {:#?}",
            mode_report.failures
        );
        assert_eq!(
            mode_report.completed_traces,
            TRACE_SEEDS.count(),
            "{mode}: every seed runs to the end of its program"
        );
        assert_eq!(mode_report.traces, mode_report.completed_traces);
    }
}

#[test]
fn rc_01_every_comm_is_a_member_of_the_reference_enabled_set_and_rests_only_when_it_is_empty() {
    let report = conformance_report();
    assert_every_trace_completed(report);
    for (mode, mode_report) in modes(report) {
        assert_property(&format!("RC-01 {mode}"), &mode_report.membership);
        assert!(mode_report.comms > 0 && mode_report.rests > 0, "{mode}: {mode_report:#?}");
        let choices: usize = mode_report
            .enabled_set_sizes
            .range(2..)
            .map(|(_, steps)| steps)
            .sum();
        assert!(choices > 0, "{mode}: no operation met an enabled set with two or more members");
    }
}

#[test]
fn rc_02_enabled_rendezvous_equals_the_reference_set_and_is_empty_after_every_operation() {
    let report = conformance_report();
    assert_every_trace_completed(report);
    for (mode, mode_report) in modes(report) {
        assert_property(&format!("RC-02 {mode}"), &mode_report.enabled_sets);
    }
    for (mode, staged) in staged_modes(report) {
        assert_property(&format!("RC-02 {mode}"), &staged.enabled_sets);
        assert!(
            staged.non_empty_states > 0 && staged.members > staged.non_empty_states,
            "{mode}: the staged states enable too few rendezvous: {staged:#?}"
        );
        assert!(
            staged.repeated_occurrence_states > 0,
            "{mode}: no staged state enables rendezvous over repeated identical occurrences"
        );
    }
}

#[test]
fn rc_03_contents_agree_per_channel_and_ordered_channel_list_after_every_step() {
    let report = conformance_report();
    assert_every_trace_completed(report);
    for (mode, mode_report) in modes(report) {
        assert_property(&format!("RC-03 {mode}"), &mode_report.contents);
    }
    for (mode, staged) in staged_modes(report) {
        assert_property(&format!("RC-03 {mode}"), &staged.contents);
    }
}

#[test]
fn generated_traces_cover_every_operation_kind() {
    let report = conformance_report();
    println!("{report:#?}");
    assert_every_trace_completed(report);
    for (mode, mode_report) in modes(report) {
        for kind in StepKind::ALL {
            let counts = mode_report.kinds.get(&kind).cloned().unwrap_or_default();
            match kind {
                StepKind::Install => assert!(
                    counts.rested > 0 && counts.fired == 0,
                    "{mode}: installs rest on empty contents: {counts:?}"
                ),
                _ => assert!(
                    counts.fired > 0 && counts.rested > 0,
                    "{mode}: {kind:?} steps must both fire and rest: {counts:?}"
                ),
            }
        }
        assert!(mode_report.fired_installed > 0, "{mode}: no installed continuation fired");
        assert!(mode_report.rejected_by_guard > 0, "{mode}: no guard rejected a selection");
        assert!(mode_report.reissued_steps > 0, "{mode}: no operation was issued again");
    }
}

#[test]
fn generated_traces_are_deterministic_per_seed() {
    for seed in TRACE_SEEDS {
        assert_eq!(
            generate_program(seed, OPERATIONS_PER_TRACE),
            generate_program(seed, OPERATIONS_PER_TRACE),
            "seed {seed}"
        );
    }
    for seed in STAGED_SEEDS {
        assert_eq!(generate_staged_state(seed), generate_staged_state(seed), "seed {seed}");
    }
    let distinct_programs: BTreeSet<Vec<TestOperation>> = TRACE_SEEDS
        .map(|seed| generate_program(seed, OPERATIONS_PER_TRACE))
        .collect();
    assert_eq!(distinct_programs.len(), TRACE_SEEDS.count());
}

struct Receive {
    name: &'static str,
    channels: &'static [&'static str],
    patterns: Vec<Pattern>,
    guard: Guard,
    persistent: bool,
    peeks: &'static [i32],
}

impl Receive {
    fn new(name: &'static str, channels: &'static [&'static str], patterns: Vec<Pattern>) -> Self {
        Self {
            name,
            channels,
            patterns,
            guard: Guard::Unguarded,
            persistent: false,
            peeks: &[],
        }
    }

    fn guarded(self, guard: Guard) -> Self { Self { guard, ..self } }

    fn persistent(self) -> Self {
        Self {
            persistent: true,
            ..self
        }
    }

    fn peeking(self, peeks: &'static [i32]) -> Self { Self { peeks, ..self } }

    fn continuation(&self) -> GuardedContinuation {
        GuardedContinuation {
            name: self.name.to_string(),
            guard: self.guard.clone(),
        }
    }

    fn channel_list(&self) -> Vec<String> { strings(self.channels) }

    fn entry(&self) -> ContinuationEntry<Pattern, GuardedContinuation> {
        ContinuationEntry {
            patterns: self.patterns.clone(),
            continuation: self.continuation(),
            persistent: self.persistent,
            peeks: self.peeks.iter().copied().collect(),
        }
    }

    fn operation(&self) -> TestOperation {
        Operation::Consume {
            channels: self.channel_list(),
            patterns: self.patterns.clone(),
            continuation: self.continuation(),
            persistent: self.persistent,
            peeks: self.peeks.iter().copied().collect(),
        }
    }

    fn member(&self, selection: &[(&str, &str, bool)]) -> TestRendezvous {
        Rendezvous {
            channels: self.channel_list(),
            continuation: self.entry(),
            selection: selection
                .iter()
                .map(|(channel, value, persistent)| Selected {
                    channel: channel.to_string(),
                    datum: DatumEntry {
                        value: value.to_string(),
                        persistent: *persistent,
                    },
                    binding: value.to_string(),
                })
                .collect(),
        }
    }

    fn fired(&self, bindings: &[&str]) -> FiredBody<String, GuardedContinuation> {
        FiredBody {
            continuation: self.continuation(),
            bindings: strings(bindings),
        }
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn send(channel: &str, value: &str) -> TestOperation {
    Operation::Produce {
        channel: channel.to_string(),
        datum: value.to_string(),
        persistent: false,
    }
}

fn persistent_send(channel: &str, value: &str) -> TestOperation {
    Operation::Produce {
        channel: channel.to_string(),
        datum: value.to_string(),
        persistent: true,
    }
}

fn exact(value: &str) -> Pattern { Pattern::StringMatch(value.to_string()) }

fn outcome(
    data: &[(&str, &str, bool)],
    resting: &[&Receive],
    fired: &[FiredBody<String, GuardedContinuation>],
) -> TestOutcome {
    let mut contents = TestContents::default();
    for (channel, value, persistent) in data {
        contents.add_datum(channel.to_string(), DatumEntry {
            value: value.to_string(),
            persistent: *persistent,
        });
    }
    for receive in resting {
        contents.add_continuation(receive.channel_list(), receive.entry());
    }
    let mut fired_bodies = BTreeMap::new();
    for body in fired {
        *fired_bodies.entry(body.clone()).or_default() += 1;
    }
    Outcome {
        contents,
        fired: fired_bodies,
    }
}

struct CorpusEntry {
    name: &'static str,
    setup: Vec<TestOperation>,
    probe: TestOperation,
    enabled: BTreeSet<TestRendezvous>,
    outcomes: BTreeSet<TestOutcome>,
}

fn literal_corpus() -> Vec<CorpusEntry> {
    let receive_p = Receive::new("P", &["c"], vec![Pattern::Wildcard]);
    let receive_q = Receive::new("Q", &["c"], vec![Pattern::Wildcard]);
    let join_j = Receive::new("J", &["a", "b"], vec![Pattern::Wildcard, Pattern::Wildcard]);
    let receive_q_on_a = Receive::new("Q", &["a"], vec![Pattern::Wildcard]);
    let persistent_r = Receive::new("R", &["c"], vec![Pattern::Wildcard]).persistent();
    let peek_k = Receive::new("K", &["c"], vec![Pattern::Wildcard]).peeking(&[0]);
    let bounded_g = Receive::new("G", &["c"], vec![Pattern::Wildcard]).guarded(Guard::AtMost(45));
    let increasing_s = Receive::new("S", &["a", "b"], vec![Pattern::Wildcard, Pattern::Wildcard])
        .guarded(Guard::StrictlyIncreasing);
    let reversed_x = Receive::new("X", &["b", "a"], vec![exact("1"), exact("2")]);
    let ordered_y = Receive::new("Y", &["a", "b"], vec![exact("1"), exact("2")]);
    vec![
        CorpusEntry {
            name: "1 two data and one receive",
            setup: vec![send("c", "1"), send("c", "2")],
            probe: receive_p.operation(),
            enabled: BTreeSet::from([
                receive_p.member(&[("c", "1", false)]),
                receive_p.member(&[("c", "2", false)]),
            ]),
            outcomes: BTreeSet::from([
                outcome(&[("c", "2", false)], &[], &[receive_p.fired(&["1"])]),
                outcome(&[("c", "1", false)], &[], &[receive_p.fired(&["2"])]),
            ]),
        },
        CorpusEntry {
            name: "2 two receives and one datum",
            setup: vec![receive_p.operation(), receive_q.operation()],
            probe: send("c", "1"),
            enabled: BTreeSet::from([
                receive_p.member(&[("c", "1", false)]),
                receive_q.member(&[("c", "1", false)]),
            ]),
            outcomes: BTreeSet::from([
                outcome(&[], &[&receive_q], &[receive_p.fired(&["1"])]),
                outcome(&[], &[&receive_p], &[receive_q.fired(&["1"])]),
            ]),
        },
        CorpusEntry {
            name: "3 a join race",
            setup: vec![send("b", "2"), join_j.operation(), receive_q_on_a.operation()],
            probe: send("a", "1"),
            enabled: BTreeSet::from([
                join_j.member(&[("a", "1", false), ("b", "2", false)]),
                receive_q_on_a.member(&[("a", "1", false)]),
            ]),
            outcomes: BTreeSet::from([
                outcome(&[], &[&receive_q_on_a], &[join_j.fired(&["1", "2"])]),
                outcome(&[("b", "2", false)], &[&join_j], &[receive_q_on_a.fired(&["1"])]),
            ]),
        },
        CorpusEntry {
            name: "4 a persistent send",
            setup: vec![receive_p.operation(), receive_q.operation()],
            probe: persistent_send("c", "1"),
            enabled: BTreeSet::from([
                receive_p.member(&[("c", "1", true)]),
                receive_q.member(&[("c", "1", true)]),
            ]),
            outcomes: BTreeSet::from([outcome(&[("c", "1", true)], &[], &[
                receive_p.fired(&["1"]),
                receive_q.fired(&["1"]),
            ])]),
        },
        CorpusEntry {
            name: "5 a persistent receive",
            setup: vec![send("c", "1"), send("c", "2")],
            probe: persistent_r.operation(),
            enabled: BTreeSet::from([
                persistent_r.member(&[("c", "1", false)]),
                persistent_r.member(&[("c", "2", false)]),
            ]),
            outcomes: BTreeSet::from([outcome(&[], &[&persistent_r], &[
                persistent_r.fired(&["1"]),
                persistent_r.fired(&["2"]),
            ])]),
        },
        CorpusEntry {
            name: "6 a peek",
            setup: vec![send("c", "1")],
            probe: peek_k.operation(),
            enabled: BTreeSet::from([peek_k.member(&[("c", "1", false)])]),
            outcomes: BTreeSet::from([outcome(&[("c", "1", false)], &[], &[peek_k.fired(&["1"])])]),
        },
        CorpusEntry {
            name: "7 guard backtracking over a bound",
            setup: vec![send("c", "46"), send("c", "2")],
            probe: bounded_g.operation(),
            enabled: BTreeSet::from([bounded_g.member(&[("c", "2", false)])]),
            outcomes: BTreeSet::from([outcome(&[("c", "46", false)], &[], &[
                bounded_g.fired(&["2"])
            ])]),
        },
        CorpusEntry {
            name: "8 guard backtracking over a join",
            setup: vec![send("a", "3"), send("b", "1"), send("b", "5")],
            probe: increasing_s.operation(),
            enabled: BTreeSet::from([increasing_s.member(&[("a", "3", false), ("b", "5", false)])]),
            outcomes: BTreeSet::from([outcome(&[("b", "1", false)], &[], &[
                increasing_s.fired(&["3", "5"])
            ])]),
        },
        CorpusEntry {
            name: "9 a (b,a) receive against (a,b) data",
            setup: vec![send("a", "1"), send("b", "2")],
            probe: reversed_x.operation(),
            enabled: BTreeSet::new(),
            outcomes: BTreeSet::from([outcome(
                &[("a", "1", false), ("b", "2", false)],
                &[&reversed_x],
                &[],
            )]),
        },
        CorpusEntry {
            name: "10 an (a,b) receive against (a,b) data",
            setup: vec![send("a", "1"), send("b", "2")],
            probe: ordered_y.operation(),
            enabled: BTreeSet::from([ordered_y.member(&[("a", "1", false), ("b", "2", false)])]),
            outcomes: BTreeSet::from([outcome(&[], &[], &[ordered_y.fired(&["1", "2"])])]),
        },
        CorpusEntry {
            name: "11 (a,b) data arriving at a resting (b,a) receive",
            setup: vec![reversed_x.operation(), send("a", "1")],
            probe: send("b", "2"),
            enabled: BTreeSet::new(),
            outcomes: BTreeSet::from([outcome(
                &[("a", "1", false), ("b", "2", false)],
                &[&reversed_x],
                &[],
            )]),
        },
    ]
}

#[test]
fn rc_04_reference_enabled_and_outcome_sets_equal_the_written_corpus() {
    let corpus = literal_corpus();
    let mut disagreements = Vec::new();
    for entry in &corpus {
        let mut reference = TestReference::new(&GuardingMatch);
        for operation in &entry.setup {
            let enabled = reference.enabled_for(operation).members;
            if !enabled.is_empty() {
                disagreements.push(format!(
                    "{}: the setup operation {operation:?} enables {enabled:?}",
                    entry.name
                ));
            }
            reference.rest(operation);
        }
        let at_rest = reference.enabled_at_rest().members;
        if !at_rest.is_empty() {
            disagreements.push(format!("{}: the setup state enables {at_rest:?}", entry.name));
        }
        let enabled = reference.enabled_for(&entry.probe).members;
        if enabled != entry.enabled {
            disagreements.push(format!(
                "{}: the enabled set is {enabled:?}, the written set is {:?}",
                entry.name, entry.enabled
            ));
        }
        let outcomes = reference.outcomes(std::slice::from_ref(&entry.probe), REISSUE_DEPTH_LIMIT);
        if outcomes != entry.outcomes {
            disagreements.push(format!(
                "{}: the outcome set is {outcomes:?}, the written set is {:?}",
                entry.name, entry.outcomes
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "the reference disagrees with the written corpus:\n{}",
        disagreements.join("\n")
    );
    let enabled_sizes: BTreeSet<usize> = corpus.iter().map(|entry| entry.enabled.len()).collect();
    let outcome_sizes: BTreeSet<usize> = corpus.iter().map(|entry| entry.outcomes.len()).collect();
    assert_eq!(enabled_sizes, BTreeSet::from([0, 1, 2]));
    assert_eq!(outcome_sizes, BTreeSet::from([1, 2]));
}
