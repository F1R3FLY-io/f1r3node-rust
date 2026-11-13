#[allow(dead_code)]
mod reference_tuplespace;

use std::sync::{Arc, OnceLock};

use reference_tuplespace::{
    ConformanceReport, Guard, GuardedContinuation, GuardingMatch, ModeReport, Operation, Pattern,
    PropertyTally, SpacePair, StepKind, TestOperation, run_programs,
};
use rspace_plus_plus::rspace::history::history_repository::HistoryRepositoryInstances;
use rspace_plus_plus::rspace::hot_store::{HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const CHANNELS: [&str; 2] = ["a", "b"];
const VALUES: [&str; 2] = ["1", "2"];
const EXACT_VALUE: &str = "1";
const GUARDS: [Guard; 2] = [Guard::Unguarded, Guard::AtMost(1)];
const FLAGS: [bool; 2] = [false, true];
const ARRIVALS: [Arrival; 2] = [Arrival::DataFirst, Arrival::ReceiveFirst];
const FULL_PRODUCT_ARITIES: [usize; 2] = [1, 2];
const CORNER_ARITY: usize = 3;
const REISSUE_DEPTH_LIMIT: usize = 3;
const RECEIVE_NAME: &str = "receive";

#[derive(Clone, Copy, Debug)]
enum Arrival {
    DataFirst,
    ReceiveFirst,
}

#[derive(Clone, Copy, Debug)]
enum Scope {
    FullProduct,
    Corner,
}

impl Scope {
    fn values<T>(self, values: &[T]) -> &[T] {
        match self {
            Scope::FullProduct => values,
            Scope::Corner => &values[..1],
        }
    }
}

struct Shape<'s> {
    channels: &'s [&'static str],
    exact: &'s [bool],
    peeks: &'s [bool],
    receive_persistent: bool,
    guard: &'s Guard,
    values: &'s [&'static str],
    send_persistent: &'s [bool],
    order: &'s [usize],
    arrival: Arrival,
}

impl Shape<'_> {
    fn receive(&self) -> TestOperation {
        Operation::Consume {
            channels: self
                .channels
                .iter()
                .map(|channel| channel.to_string())
                .collect(),
            patterns: self
                .exact
                .iter()
                .map(|exact| match *exact {
                    true => Pattern::StringMatch(EXACT_VALUE.to_string()),
                    false => Pattern::Wildcard,
                })
                .collect(),
            continuation: GuardedContinuation {
                name: RECEIVE_NAME.to_string(),
                guard: self.guard.clone(),
            },
            persistent: self.receive_persistent,
            peeks: (0..)
                .zip(self.peeks)
                .filter_map(|(position, peek)| peek.then_some(position))
                .collect(),
        }
    }

    fn send(&self, position: usize) -> TestOperation {
        Operation::Produce {
            channel: self.channels[position].to_string(),
            datum: self.values[position].to_string(),
            persistent: self.send_persistent[position],
        }
    }

    fn program(&self) -> Vec<TestOperation> {
        let mut program = Vec::with_capacity(self.order.len() + 1);
        let sends = self.order.iter().map(|position| self.send(*position));
        match self.arrival {
            Arrival::DataFirst => {
                program.extend(sends);
                program.push(self.receive());
            }
            Arrival::ReceiveFirst => {
                program.push(self.receive());
                program.extend(sends);
            }
        }
        program
    }
}

fn words<T: Copy>(alphabet: &[T], length: usize) -> Vec<Vec<T>> {
    let mut words = vec![Vec::with_capacity(length)];
    for _ in 0..length {
        let mut longer = Vec::with_capacity(words.len() * alphabet.len());
        for word in &words {
            for letter in alphabet {
                let mut extended = Vec::with_capacity(length);
                extended.extend_from_slice(word);
                extended.push(*letter);
                longer.push(extended);
            }
        }
        words = longer;
    }
    words
}

fn orders(length: usize) -> Vec<Vec<usize>> {
    let mut orders = vec![Vec::with_capacity(length)];
    for item in 0..length {
        let mut longer = Vec::with_capacity(orders.len() * (item + 1));
        for order in &orders {
            for slot in (0..=item).rev() {
                let mut extended = Vec::with_capacity(length);
                extended.extend_from_slice(order);
                extended.insert(slot, item);
                longer.push(extended);
            }
        }
        orders = longer;
    }
    orders
}

fn program_count(arity: usize, scope: Scope) -> usize {
    let exponent = u32::try_from(arity).expect("a receive has few binds");
    let swept = (CHANNELS.len() * FLAGS.len() * FLAGS.len()).pow(exponent) * ARRIVALS.len();
    match scope {
        Scope::Corner => swept,
        Scope::FullProduct => {
            let orders: usize = (1..=arity).product();
            swept * FLAGS.len() * GUARDS.len() * (VALUES.len() * FLAGS.len()).pow(exponent) * orders
        }
    }
}

fn programs_of_arity(arity: usize, scope: Scope) -> Vec<Vec<TestOperation>> {
    let channel_lists = words(&CHANNELS, arity);
    let flag_words = words(&FLAGS, arity);
    let value_words = words(&VALUES, arity);
    let orders = orders(arity);
    let mut programs = Vec::with_capacity(program_count(arity, scope));
    for channels in &channel_lists {
        for exact in &flag_words {
            for peeks in &flag_words {
                for receive_persistent in scope.values(&FLAGS) {
                    for guard in scope.values(&GUARDS) {
                        for values in scope.values(&value_words) {
                            for send_persistent in scope.values(&flag_words) {
                                for order in scope.values(&orders) {
                                    for arrival in ARRIVALS {
                                        let shape = Shape {
                                            channels,
                                            exact,
                                            peeks,
                                            receive_persistent: *receive_persistent,
                                            guard,
                                            values,
                                            send_persistent,
                                            order,
                                            arrival,
                                        };
                                        programs.push(shape.program());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    programs
}

fn expected_program_count() -> usize {
    FULL_PRODUCT_ARITIES
        .into_iter()
        .map(|arity| program_count(arity, Scope::FullProduct))
        .sum::<usize>() +
        program_count(CORNER_ARITY, Scope::Corner)
}

fn enumerated_programs() -> Vec<Vec<TestOperation>> {
    let mut programs = Vec::with_capacity(expected_program_count());
    for arity in FULL_PRODUCT_ARITIES {
        programs.extend(programs_of_arity(arity, Scope::FullProduct));
    }
    programs.extend(programs_of_arity(CORNER_ARITY, Scope::Corner));
    programs
}

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

struct Run {
    programs: usize,
    report: ConformanceReport,
}

fn run() -> &'static Run {
    static RUN: OnceLock<Run> = OnceLock::new();
    RUN.get_or_init(|| {
        let programs = enumerated_programs();
        let report = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread tokio runtime for the permutation run")
            .block_on(run_programs(
                in_memory_space_pair,
                &GuardingMatch,
                &programs,
                REISSUE_DEPTH_LIMIT,
            ));
        Run {
            programs: programs.len(),
            report,
        }
    })
}

fn modes(report: &ConformanceReport) -> [(&'static str, &ModeReport); 2] {
    [("play", &report.play), ("replay", &report.replay)]
}

fn assert_property(name: &str, tally: &PropertyTally) {
    assert!(tally.checks > 0, "{name}: no check ran");
    assert_eq!(
        tally.violations, 0,
        "{name}: {} of {} checks failed; the first failures are {:#?}",
        tally.violations, tally.checks, tally.first_violations
    );
}

#[test]
fn every_enumerated_program_runs_to_its_end_in_play_and_replay() {
    let run = run();
    assert_eq!(
        run.programs,
        expected_program_count(),
        "the enumerator yields the product of the model's dimensions"
    );
    for (mode, report) in modes(&run.report) {
        println!(
            "{mode}: {} programs, {} completed, {} steps ({} issued again, {} dropped at depth \
             {REISSUE_DEPTH_LIMIT}), {} COMMs, {} rests, {} guard refusals",
            report.traces,
            report.completed_traces,
            report.steps,
            report.reissued_steps,
            report.dropped_reissues,
            report.comms,
            report.rests,
            report.rejected_by_guard
        );
        assert!(
            report.failures.is_empty(),
            "{mode}: the driver could not complete: {:#?}",
            report.failures
        );
        assert_eq!(report.traces, run.programs, "{mode}: every program is run");
        assert_eq!(report.completed_traces, run.programs, "{mode}: every program runs to its end");
    }
}

#[test]
fn the_enumeration_exercises_every_step_kind_guard_refusal_and_choice() {
    for (mode, report) in modes(&run().report) {
        for kind in StepKind::ALL {
            let counts = report.kinds.get(&kind).cloned().unwrap_or_default();
            match kind {
                StepKind::Install => assert!(
                    counts.fired == 0 && counts.rested == 0,
                    "{mode}: the model has no installs: {counts:?}"
                ),
                _ => assert!(
                    counts.fired > 0 && counts.rested > 0,
                    "{mode}: {kind:?} steps must both fire and rest: {counts:?}"
                ),
            }
        }
        assert!(report.rejected_by_guard > 0, "{mode}: no guard refused a candidate");
        let choices: usize = report
            .enabled_set_sizes
            .range(2..)
            .map(|(_, steps)| steps)
            .sum();
        assert!(choices > 0, "{mode}: no step met an enabled set with two or more members");
    }
}

#[test]
fn rc_01_every_comm_is_a_member_of_the_reference_enabled_set_and_rests_only_when_it_is_empty() {
    for (mode, report) in modes(&run().report) {
        assert_property(&format!("RC-01 {mode}"), &report.membership);
    }
}

#[test]
fn rc_02_enabled_rendezvous_equals_the_reference_set_and_is_empty_after_every_step() {
    for (mode, report) in modes(&run().report) {
        assert_property(&format!("RC-02 {mode}"), &report.enabled_sets);
    }
}

#[test]
fn rc_03_contents_agree_per_channel_and_ordered_channel_list_after_every_step() {
    for (mode, report) in modes(&run().report) {
        assert_property(&format!("RC-03 {mode}"), &report.contents);
    }
}
