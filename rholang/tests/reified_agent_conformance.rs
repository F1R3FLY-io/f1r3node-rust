//! The conformance kit of user agent-backed spaces (P8, plan decision 77, design v2 W8).
//!
//! A space agent answers the methods `gensym`, `produce` and `consume`, and it keeps the
//! obligations ASO-1 to ASO-3 (agent-api.md). The kit checks them through programs that use the
//! agent as a space. Each case is a sequence of deploys on one chain. The first deploy creates
//! the agent and publishes its reference on `kit-agent`, and the second one publishes names of
//! the space on `kit-names`. The later deploys send and receive on these names. After each
//! deploy, the kit compares all outputs on `rrs-native-out` with the expected outputs, so a
//! match that stays pending after a step fails the case (ASO-1). Every deploy plays natively at
//! the v6 limits and certifies by native replay (`support::native`).
//!
//! The kit runs each case against two agents: the reference agent
//! `examples/reified_rspaces/agent_hashmap_bag.rho`, and a wrapper agent that forwards every
//! call to the handle of a native `rho:space:hashmap:bag` space (W6). Another agent joins the
//! kit with one [`AgentUnderTest`]. The cases use only behavior that both agents share. A guard
//! on a join can differ: the reference agent filters each candidate in its matcher, and a
//! native space keeps the guard rule of decision 37 (design v2 A15).
//!
//! | Test | Property |
//! |---|---|
//! | `conformance_gensym_distinct_and_exactly_once` | ASO-3: a `new` of two names gives two distinct names of the agent, and a direct `gensym` call gets exactly one reply. |
//! | `conformance_produce_consume_roundtrip` | A send and a linear receive meet once, in either order. A second receive waits for a second send (ASO-2). |
//! | `conformance_persistent_consume_drains` | A persistent receive `<=` takes the stored message, stays, and takes each later message. |
//! | `conformance_peek_keeps_the_data` | A peek `<<-` keeps the message for a later receive, and a parked peek fires on a later send. |
//! | `conformance_join_of_two_channels` | A join waits for both channels and fires with one message of each. |
//! | `conformance_guard_filters_one_bind` | A guarded receive skips a message that its guard refuses, and fires on a later message that passes. |
//! | `conformance_repeated_channel_needs_distinct_data` | A join over one channel twice needs two distinct messages. |
//! | `conformance_no_pending_match_after_each_step` | ASO-1: of two waiting receives, each send fires exactly one. |
//! | `conformance_rejects_an_agent_that_leaves_a_match_pending` | Negative control: a faulty agent whose `consume` only stores the receive leaves the round trip pending, and the kit fails at that step. |
//! | `prop_reference_agent_keeps_no_pending_match` | `NoMatchRemains` (AgentSpaces.tla, A17): after each step of a generated scenario, the outputs of the reference agent equal those of the model of its algorithm, so no match stays pending (ASO-1). |
//! | `reference_agent_example_runs` | The example program runs: its demo join gives the sum 3 on `agentHashmapBag.demo`. |

#[path = "support/mod.rs"]
mod support;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::reified::route::agent_parts;
use support::native::{assert_reproduces, certify, host_work, NativeChain, AMPLE_LIMIT};
use support::props::{check, runtime_config, tokio_runtime};
use support::session::Injections;

// ─── The agents under test ────────────────────────────────────────────────────────────────────

/// The phlo limit of every deploy of the kit.
const LIMIT: u64 = AMPLE_LIMIT;

/// The reference agent: the example program of the repository.
const REFERENCE_AGENT: &str = include_str!("../../examples/reified_rspaces/agent_hashmap_bag.rho");

/// The comment that starts the demo of the reference agent. The kit runs the definitions before
/// it, without the demo.
const DEMO_MARKER: &str = "  // Demo:";

/// An agent under test: the deploy that creates one instance of the agent and publishes its
/// reference on the public channel `kit-agent`.
struct AgentUnderTest {
    label: &'static str,
    create: String,
}

/// The reference agent, created by its constructor `agentHashmapBag` with the qualifier
/// `"default"`.
fn reference_agent() -> AgentUnderTest {
    let (definitions, demo) = REFERENCE_AGENT
        .split_once(DEMO_MARKER)
        .expect("the reference agent marks its demo");
    assert!(
        definitions.trim_end().ends_with('|'),
        "a parallel bar separates the definitions from the demo"
    );
    assert!(demo.trim_end().ends_with('}'), "the demo ends the program");
    AgentUnderTest {
        label: "reference agent",
        create: format!(
            "{definitions}  for (space <- agentHashmapBag!?(\"default\")) {{ @\"kit-agent\"!(*space) }}\n}}\n"
        ),
    }
}

/// A wrapper agent that forwards every call to the handle of a native `rho:space:hashmap:bag`
/// space, which runs the receives of the translation (W6).
fn native_wrapper() -> AgentUnderTest {
    AgentUnderTest {
        label: "native wrapper",
        create: r#"new wrapper, factory(`rho:space:hashmap:bag`) in {
  contract wrapper(ret, @qualifier) = {
    for (h <- factory!?(qualifier)) {
      new instance in {
        for (reply, @"gensym" <= instance) { h!(*reply, "gensym") } |
        for (sink, @"produce", @raw, @msg <= instance) { h!(*sink, "produce", raw, msg) } |
        for (sink, @"consume", @pairs, @arrow, @body <= instance) { h!(*sink, "consume", pairs, arrow, body) } |
        ret!(bundle+{*instance})
      }
    }
  } |
  for (space <- wrapper!?("default")) { @"kit-agent"!(*space) }
}"#
        .to_string(),
    }
}

/// Every agent under test.
fn agents() -> [AgentUnderTest; 2] { [reference_agent(), native_wrapper()] }

/// The agent of the negative control: the reference agent whose `consume` stores each receive
/// without trying the stored messages. A receive after a matching send stays pending, so the
/// agent breaks ASO-1.
fn faulty_agent() -> AgentUnderTest {
    let reference = reference_agent();
    let tries = "            drain!(*private, messages, consumes, pairs, arrow, body)";
    let stores =
        "            @(*private, \"state\")!(messages, consumes ++ [(pairs, arrow, body)])";
    assert_eq!(
        reference.create.matches(tries).count(),
        1,
        "the consume method of the reference agent calls drain once"
    );
    AgentUnderTest {
        label: "faulty agent",
        create: reference.create.replace(tries, stores),
    }
}

// ─── The kit ──────────────────────────────────────────────────────────────────────────────────

/// One case on one chain: the deploys of the case, in order.
struct Kit {
    chain: NativeChain,
    label: String,
    steps: usize,
}

impl Kit {
    /// A chain that holds one instance of `agent` and the names `x` and `y` of its space, for
    /// the case `case`.
    async fn start(agent: &AgentUnderTest, case: &str) -> Self {
        let mut kit = Self {
            chain: NativeChain::new().await,
            label: format!("{case} with the {}", agent.label),
            steps: 0,
        };
        let out = kit.play(&agent.create).await;
        assert_eq!(out, Vec::<Par>::new(), "{}: the creation", kit.label);
        let names = r#"new names in {
  for (space <<- @"kit-agent") { use space { new x, y in { names!([*x, *y]) } } } |
  for (@ns <- names) { @"kit-names"!(ns) }
}"#;
        let out = kit.play(names).await;
        assert_eq!(out, Vec::<Par>::new(), "{}: the names", kit.label);
        kit
    }

    /// Plays `body` as the next deploy, with `x` and `y` bound to the names of the space.
    /// Checks that the deploy evaluates with no error, leaves no volatile channel and certifies
    /// by native replay. Returns every output on `rrs-native-out` after the deploy, sorted.
    async fn step(&mut self, body: &str) -> Vec<Par> {
        self.play(&format!("for (@[x, y] <<- @\"kit-names\") {{\n{body}\n}}"))
            .await
    }

    /// Plays `body` as the next step, like [`Kit::step`], and checks that the outputs after it
    /// are `expected`, in any order.
    async fn expect(&mut self, body: &str, expected: Vec<Par>) {
        let out = self.step(body).await;
        assert_eq!(
            out,
            sorted(expected),
            "{}, step {}: the outputs",
            self.label,
            self.steps
        );
    }

    async fn play(&mut self, term: &str) -> Vec<Par> {
        self.steps += 1;
        let label = format!("{}, step {}", self.label, self.steps);
        let played = self
            .chain
            .play_with(term, &label, LIMIT, host_work(), Injections::new())
            .await;
        assert!(
            played.result.errors.is_empty(),
            "{label}: {:?}",
            played.result.errors
        );
        assert!(played.residue.is_empty(), "{label}: {:?}", played.residue);
        let certification = certify(&played, term, LIMIT).await;
        assert_reproduces(&label, &played, &certification);
        let mut values: Vec<Par> = played
            .out
            .iter()
            .map(|pars| match pars.as_slice() {
                [value] => value.clone(),
                other => panic!("{label}: an output has one value, not {other:?}"),
            })
            .collect();
        values.sort_by_cached_key(|value| format!("{value:?}"));
        values
    }
}

/// The outputs `values`, sorted as [`Kit::step`] sorts them.
fn sorted(mut values: Vec<Par>) -> Vec<Par> {
    values.sort_by_cached_key(|value| format!("{value:?}"));
    values
}

fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

fn list(ps: Vec<Par>) -> Par { new_elist_par(ps, Vec::new(), false, None, Vec::new(), false) }

/// The output `[label, value]`.
fn labeled(label: &str, value: i64) -> Par { list(vec![string(label), int(value)]) }

/// The elements of the list `value`.
fn elements(value: &Par) -> Option<&[Par]> {
    match value.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
        }] => Some(ps),
        _ => None,
    }
}

/// The agent and the raw channel of the agent-minted name `name`: the read-write bundle of an
/// agent-tagged tuple (design v2 TN-3).
fn minted_parts(name: &Par) -> Option<(Par, Par)> {
    match name.bundles.as_slice() {
        [bundle] if bundle.write_flag && bundle.read_flag => bundle
            .body
            .as_ref()
            .and_then(agent_parts)
            .map(|(agent, raw)| (agent.clone(), raw.clone())),
        _ => None,
    }
}

// ─── The cases ────────────────────────────────────────────────────────────────────────────────

async fn gensym_distinct_and_exactly_once(agent: &AgentUnderTest) {
    let mut kit = Kit::start(agent, "gensym").await;
    let out = kit.step("@\"rrs-native-out\"!([x, y])").await;
    let [names] = out.as_slice() else {
        panic!("{}: one output, not {out:?}", kit.label);
    };
    let parts: Vec<(Par, Par)> = elements(names)
        .expect("a list of two names")
        .iter()
        .map(|name| minted_parts(name).expect("an agent-minted name"))
        .collect();
    assert_eq!(parts.len(), 2, "{}", kit.label);
    assert_eq!(parts[0].0, parts[1].0, "{}: one agent", kit.label);
    assert_ne!(parts[0].1, parts[1].1, "{}: two distinct names", kit.label);
    let out = kit
        .play(
            r#"new reply in {
  for (space <<- @"kit-agent") { space!(*reply, "gensym") } |
  for (@name <= reply) { @"rrs-native-out"!(["gensym", name]) }
}"#,
        )
        .await;
    let replies: Vec<&Par> = out
        .iter()
        .filter(|value| {
            elements(value).is_some_and(|parts| parts.first() == Some(&string("gensym")))
        })
        .collect();
    assert_eq!(
        replies.len(),
        1,
        "{}: exactly one reply: {out:?}",
        kit.label
    );
}

async fn produce_consume_roundtrip(agent: &AgentUnderTest) {
    let mut kit = Kit::start(agent, "round trip").await;
    kit.expect("@x!(1, 2)", Vec::new()).await;
    let first = list(vec![int(1), int(2)]);
    kit.expect("for (@a, @b <- @x) { @\"rrs-native-out\"!([a, b]) }", vec![
        first.clone(),
    ])
    .await;
    kit.expect(
        "for (@a, @b <- @x) { @\"rrs-native-out\"!([\"again\", a, b]) }",
        vec![first.clone()],
    )
    .await;
    let again = list(vec![string("again"), int(3), int(4)]);
    kit.expect("@x!(3, 4)", vec![first, again]).await;
}

async fn persistent_consume_drains(agent: &AgentUnderTest) {
    let mut kit = Kit::start(agent, "persistent consume").await;
    kit.expect("@x!(1)", Vec::new()).await;
    let mut expected = vec![labeled("drain", 1)];
    kit.expect(
        "for (@v <= @x) { @\"rrs-native-out\"!([\"drain\", v]) }",
        expected.clone(),
    )
    .await;
    for value in [2, 3] {
        expected.push(labeled("drain", value));
        kit.expect(&format!("@x!({value})"), expected.clone()).await;
    }
}

async fn peek_keeps_the_data(agent: &AgentUnderTest) {
    let mut kit = Kit::start(agent, "peek").await;
    kit.expect("@x!(1)", Vec::new()).await;
    let mut expected = vec![labeled("peek", 1)];
    kit.expect(
        "for (@v <<- @x) { @\"rrs-native-out\"!([\"peek\", v]) }",
        expected.clone(),
    )
    .await;
    expected.push(labeled("take", 1));
    kit.expect(
        "for (@v <- @x) { @\"rrs-native-out\"!([\"take\", v]) }",
        expected.clone(),
    )
    .await;
    kit.expect(
        "for (@v <<- @x) { @\"rrs-native-out\"!([\"late peek\", v]) }",
        expected.clone(),
    )
    .await;
    expected.push(labeled("late peek", 2));
    kit.expect("@x!(2)", expected.clone()).await;
    expected.push(labeled("take", 2));
    kit.expect(
        "for (@v <- @x) { @\"rrs-native-out\"!([\"take\", v]) }",
        expected,
    )
    .await;
}

async fn join_of_two_channels(agent: &AgentUnderTest) {
    let mut kit = Kit::start(agent, "join").await;
    kit.expect("@x!(1)", Vec::new()).await;
    kit.expect(
        "for (@a <- @x & @b <- @y) { @\"rrs-native-out\"!([a, b]) }",
        Vec::new(),
    )
    .await;
    kit.expect("@y!(2)", vec![list(vec![int(1), int(2)])]).await;
}

async fn guard_filters_one_bind(agent: &AgentUnderTest) {
    let mut kit = Kit::start(agent, "guard").await;
    kit.expect(
        "@x!(1) | for (@v <- @x where v > 3) { @\"rrs-native-out\"!([\"passed\", v]) }",
        Vec::new(),
    )
    .await;
    kit.expect("@x!(5)", vec![labeled("passed", 5)]).await;
    kit.expect(
        "for (@v <- @x) { @\"rrs-native-out\"!([\"rest\", v]) }",
        vec![labeled("passed", 5), labeled("rest", 1)],
    )
    .await;
}

async fn repeated_channel_needs_distinct_data(agent: &AgentUnderTest) {
    let mut kit = Kit::start(agent, "repeated channel").await;
    kit.expect(
        "match x { z => { @x!(1) | for (@a <- @x & @b <- @z) { @\"rrs-native-out\"!([a, b]) } } }",
        Vec::new(),
    )
    .await;
    let out = kit.step("@x!(2)").await;
    let [pair] = out.as_slice() else {
        panic!("{}: one output, not {out:?}", kit.label);
    };
    let mut values = elements(pair).expect("a pair").to_vec();
    values.sort_by_cached_key(|value| format!("{value:?}"));
    assert_eq!(values, sorted(vec![int(1), int(2)]), "{}", kit.label);
}

async fn no_pending_match_after_each_step(agent: &AgentUnderTest) {
    let mut kit = Kit::start(agent, "two waiters").await;
    for waiter in ["first", "second"] {
        kit.expect(
            &format!("for (@v <- @x) {{ @\"rrs-native-out\"!([\"{waiter}\", v]) }}"),
            Vec::new(),
        )
        .await;
    }
    let out = kit.step("@x!(1)").await;
    let fired = match out.as_slice() {
        [one] if *one == labeled("first", 1) => "first",
        [one] if *one == labeled("second", 1) => "second",
        _ => panic!("{}: one waiter fires with 1, not {out:?}", kit.label),
    };
    let other = if fired == "first" { "second" } else { "first" };
    kit.expect("@x!(2)", vec![labeled(fired, 1), labeled(other, 2)])
        .await;
}

// ─── The tests ────────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conformance_gensym_distinct_and_exactly_once() {
    for agent in agents() {
        gensym_distinct_and_exactly_once(&agent).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conformance_produce_consume_roundtrip() {
    for agent in agents() {
        produce_consume_roundtrip(&agent).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conformance_persistent_consume_drains() {
    for agent in agents() {
        persistent_consume_drains(&agent).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conformance_peek_keeps_the_data() {
    for agent in agents() {
        peek_keeps_the_data(&agent).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conformance_join_of_two_channels() {
    for agent in agents() {
        join_of_two_channels(&agent).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conformance_guard_filters_one_bind() {
    for agent in agents() {
        guard_filters_one_bind(&agent).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conformance_repeated_channel_needs_distinct_data() {
    for agent in agents() {
        repeated_channel_needs_distinct_data(&agent).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conformance_no_pending_match_after_each_step() {
    for agent in agents() {
        no_pending_match_after_each_step(&agent).await;
    }
}

/// The negative control of the kit: a faulty agent that stores each receive without trying the
/// stored messages leaves the receive of the round trip pending after its step, and the kit
/// fails at that step (ASO-1).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[should_panic(expected = "round trip with the faulty agent, step 4: the outputs")]
async fn conformance_rejects_an_agent_that_leaves_a_match_pending() {
    produce_consume_roundtrip(&faulty_agent()).await;
}

/// The example program runs as one deploy: its demo creates a space of the reference agent,
/// and the join of the demo sends the sum 3 to `agentHashmapBag.demo`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reference_agent_example_runs() {
    let mut chain = NativeChain::new().await;
    let label = "the reference agent example";
    let played = chain
        .play_with(
            REFERENCE_AGENT,
            label,
            LIMIT,
            host_work(),
            Injections::new(),
        )
        .await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    let certification = certify(&played, REFERENCE_AGENT, LIMIT).await;
    assert_reproduces(label, &played, &certification);
    assert_eq!(chain.values("agentHashmapBag.demo").await, vec![vec![int(
        3
    )]]);
}

// ─── The property of AgentSpaces.tla ─────────────────────────────────────────────────────────────

/// The arrows of a generated receive.
const ARROWS: [&str; 3] = ["<-", "<<-", "<="];

/// One step of a generated scenario on the name `x` of the reference agent: a send of a value,
/// or a receive whose pattern takes any value or only 1, with an arrow.
#[derive(Clone, Copy, Debug)]
enum Step {
    Send(i64),
    Receive { only_one: bool, arrow: usize },
}

/// The model of the reference agent on one channel (AgentSpaces.tla, the contracts offer and
/// drain of the example): the stored values and the stored receives, both in insertion order,
/// and the output `[step, value]` of each fire.
#[derive(Clone, Debug, Default)]
struct BagModel {
    values: Vec<i64>,
    receives: Vec<(i64, bool, usize)>,
    outputs: Vec<(i64, i64)>,
}

impl BagModel {
    fn accepts(only_one: bool, value: i64) -> bool { !only_one || value == 1 }

    /// The position of the first stored value that a receive accepts, in insertion order.
    fn first(&self, only_one: bool) -> Option<usize> {
        self.values
            .iter()
            .position(|value| Self::accepts(only_one, *value))
    }

    /// Runs the step with the label `index` and returns its number of fires.
    fn run(&mut self, index: i64, step: Step) -> usize {
        let before = self.outputs.len();
        match step {
            // offer: the stored receives get the value in insertion order, and a peek keeps it
            // for the next receive.
            Step::Send(value) => {
                self.values.push(value);
                let mut k = 0;
                while k < self.receives.len() {
                    let (label, only_one, arrow) = self.receives[k];
                    let Some(j) = self.first(only_one) else {
                        k += 1;
                        continue;
                    };
                    self.outputs.push((label, self.values[j]));
                    match ARROWS[arrow] {
                        "<<-" => {
                            self.receives.remove(k);
                        }
                        "<-" => {
                            self.values.remove(j);
                            self.receives.remove(k);
                            break;
                        }
                        _ => {
                            self.values.remove(j);
                            break;
                        }
                    }
                }
            }
            // drain: the receive fires at once on the stored values, a persistent one again,
            // or it is stored.
            Step::Receive { only_one, arrow } => loop {
                let Some(j) = self.first(only_one) else {
                    self.receives.push((index, only_one, arrow));
                    break;
                };
                self.outputs.push((index, self.values[j]));
                match ARROWS[arrow] {
                    "<<-" => break,
                    "<-" => {
                        self.values.remove(j);
                        break;
                    }
                    _ => {
                        self.values.remove(j);
                    }
                }
            },
        }
        self.outputs.len() - before
    }
}

fn arb_step() -> BoxedStrategy<Step> {
    prop_oneof![
        (1i64..3).prop_map(Step::Send),
        (any::<bool>(), 0usize..3).prop_map(|(only_one, arrow)| Step::Receive { only_one, arrow }),
    ]
    .boxed()
}

/// A scenario of two to five steps in which each step fires at most once: at the v6 limits,
/// native replay certifies about one fire of a Rholang agent in one deploy (decision 79).
fn arb_scenario() -> BoxedStrategy<Vec<Step>> {
    prop::collection::vec(arb_step(), 2..6)
        .prop_filter("each step fires at most once (decision 79)", |steps| {
            let mut model = BagModel::default();
            steps.iter().enumerate().all(|(index, step)| {
                model.run(i64::try_from(index).expect("a short scenario"), *step) <= 1
            })
        })
        .boxed()
}

/// The Rholang process of the step with the label `index` on the name `x`.
fn step_body(index: i64, step: Step) -> String {
    match step {
        Step::Send(value) => format!("@x!({value})"),
        Step::Receive {
            only_one: true,
            arrow,
        } => format!(
            "for (@1 {} @x) {{ @\"rrs-native-out\"!([{index}, 1]) }}",
            ARROWS[arrow]
        ),
        Step::Receive {
            only_one: false,
            arrow,
        } => format!(
            "for (@v {} @x) {{ @\"rrs-native-out\"!([{index}, v]) }}",
            ARROWS[arrow]
        ),
    }
}

/// `NoMatchRemains` (AgentSpaces.tla, A17): after each step of a generated scenario on the
/// reference agent, the outputs equal those of the model of its algorithm, which keeps no
/// stored receive with a stored value that it accepts. So no match stays pending in the agent
/// (ASO-1). Each step is a deploy that plays at the v6 limits and certifies by native replay.
#[test]
fn prop_reference_agent_keeps_no_pending_match() {
    check(runtime_config(), arb_scenario(), |steps| {
        tokio_runtime().block_on(scenario_case(steps))
    });
}

async fn scenario_case(steps: Vec<Step>) -> Result<(), TestCaseError> {
    let mut kit = Kit::start(&reference_agent(), "generated scenario").await;
    let mut model = BagModel::default();
    for (index, step) in steps.into_iter().enumerate() {
        let index = i64::try_from(index).expect("a short scenario");
        model.run(index, step);
        let expected = model
            .outputs
            .iter()
            .map(|(label, value)| list(vec![int(*label), int(*value)]))
            .collect();
        kit.expect(&step_body(index, step), expected).await;
    }
    Ok(())
}
