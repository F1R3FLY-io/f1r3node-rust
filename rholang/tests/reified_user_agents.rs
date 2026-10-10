//! The spaces that user agents implement (P8, plan decision 77, design v2).
//!
//! Every deploy plays natively at the v6 limits and certifies through its committed evidence,
//! as proposer self-certification and validator replay do (`support::native`). The translation
//! of `new` arrives in P8 step 4, so these programs name agent-minted names with the agent tag,
//! which the tests inject as `rho:rrs:test:tag`. A program cannot forge the tag: it is the
//! unforgeable name `GPrivate([0xE0, 0xFF])`.
//!
//! | Test | Property |
//! |---|---|
//! | `user_agent_send_calls_the_agent` | TS-1 and AG-4 (step 2): a send on an agent-tagged name becomes the call `A!(sink, "produce", raw, [d…])`, which an ordinary contract on the agent receives. The sink absorbs a reply of any arity. A call on an agent whose reference is itself an agent-minted name becomes a call on the inner agent. |
//! | `user_agent_reply_reaches_the_agent` | R7A-1 (step 2): the reply of a system process on an agent-tagged name, or on the bundle with both flags around one, becomes the call on the agent. A reply on a write-only bundle stays in the default space. |
//! | `agent_xlate_bang_bang_rejected` | TS-3 (step 2): a persistent send on an agent space aborts with `PersistentSendOnAgentSpace`. |
//! | `agent_reference_not_writable_rejected` | AG-4 (step 2): a call on an agent reference under a bundle without the write flag aborts with `HandleNotWritable`. |
//! | `user_agent_errors_params` | TS-3 (step 2): params on an agent space abort with `ParamsUnsupportedOnAgentSpace`. |
//! | `user_agent_send_seq_names_abort` | A5 (step 2): a send on an agent space whose data or raw channel holds a name of a `seq` space aborts with `SeqSent`. |
//! | `user_agent_produce_consume_roundtrip` | TR-1 to TR-7 (step 3): a send and a receive on one agent-tagged name meet through a memory agent, which asks the matcher of the receive and sends the env map to the body capability. |
//! | `agent_xlate_body_equivalence_vs_native` | TR-6 (step 3): the body of a translated receive binds what a native receive with the same pattern binds, with a list remainder and a map pattern. |
//! | `agent_xlate_join_two_matchers` | TR-3, TR-7 (step 3): a join of two channels of one agent gives two matchers with disjoint env maps. |
//! | `agent_xlate_guard_single_bind` | TR-4, MO-3 (step 3): the matcher of a guarded bind checks the guard. |
//! | `agent_xlate_guard_span_rejected` | TR-4 (step 3): a guard over two binds aborts with `AgentGuardSpansJoin`. |
//! | `agent_xlate_cross_space_join_rejected` | TR-1 (step 3): a join of an agent channel and a default channel aborts with `CrossSpaceJoin`. |
//! | `user_agent_errors_query` | TR-2 (step 3): a query on an agent space aborts with `QuerySpecUnsupportedOnAgentSpace`. |
//! | `user_agent_errors_pattern_too_deep` | TR-5 (step 3): a pattern whose matcher state is nested deeper than the decode limit of prost aborts with `AgentPatternTooDeep`. The boundary is exact: 30 nested lists abort, and 29 translate. A pattern of three nested lists fires. |
//! | `agent_wrapper_forwards_to_native` | W6 (step 3): an agent that forwards its calls to a native handle serves the space through the native receive. |
//! | `agent_never_replies_pending_replay_equal` | TN-5 (step 3): a silent agent leaves the receive pending, and native replay reproduces the play. |
//! | `reified_p8_synthesizes_no_join` | Consensus checklist item 18 and I-16 (step 3): every consume and COMM of a translated receive names one channel, and the post-state holds no join of two channels. An ordinary join is the control. |
//! | `prop_body_equivalence` | `body_equivalence` (AgentTranslation.v, A5): for generated binds and data, the translated receive binds what a native receive binds. |
//! | `prop_matcher_sound_complete` | `matcher_sound_complete` (A7): the matcher of `[a, b]` accepts a generated list exactly when a native receive fires, with the same values. |
//! | `prop_guard_fold_sound` | `guard_fold_sound` (A6): a guard over one bind of a join goes to that matcher, and the join fires exactly when a native guarded join fires. |

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeSet;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::utils::{
    new_bundle_par, new_elist_par, new_gbool_par, new_gint_par, new_gstring_par,
};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::spaces::namespace::{
    gprivate, op_channel, AGENT_TAG, REIFIED_TAG, SINK_OP,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use support::native::{assert_reproduces, certify, host_work, NativeChain, Played, AMPLE_LIMIT};
use support::props::{check, runtime_config, tokio_runtime};
use support::session::{error_codes, injected, Injections};

// ─── Helpers ───────────────────────────────────────────────────────────────────────────────────

/// The phlo limit of the deploys of this suite.
const LIMIT: u64 = AMPLE_LIMIT;

/// The injected ids of every deploy: the agent tag as `rho:rrs:test:tag`.
fn injections() -> Injections {
    Injections::from([(injected("tag"), gprivate(vec![REIFIED_TAG, AGENT_TAG]))])
}

/// A deploy that binds the agent tag to `tag`, the names `names` to fresh names, and runs
/// `body`.
fn program(names: &[&str], body: &str) -> String {
    let mut binders = vec![format!("tag(`{}`)", injected("tag"))];
    binders.extend(names.iter().map(|name| name.to_string()));
    format!("new {} in {{\n{body}\n}}", binders.join(", "))
}

/// The contract of an agent on `name`: it forwards each call `[method, raw, msg]` to the public
/// channel `rrs-native-out` and replies to the return name with three values, which the sink
/// absorbs.
fn forwarding_agent(name: &str) -> String {
    format!(
        r#"contract {name}(sink, @method, @raw, @msg) = {{
  @"rrs-native-out"!([method, raw, msg]) | sink!(method, raw, msg)
}}"#
    )
}

/// Plays `term` on a fresh chain with the injected tag, checks that the hot store holds no
/// volatile channel after it, and certifies it. Returns the play and the chain.
async fn play_and_certify(label: &str, term: &str) -> (Played, NativeChain) {
    let mut chain = NativeChain::new().await;
    let played = chain
        .play_with(term, label, LIMIT, host_work(), injections())
        .await;
    assert!(
        played.result.native_budget_recording.is_some(),
        "{label}: the deploy evaluated: {:?}",
        played.result.errors
    );
    assert!(played.residue.is_empty(), "{label}: {:?}", played.residue);
    let certification = certify(&played, term, LIMIT).await;
    assert_reproduces(label, &played, &certification);
    (played, chain)
}

/// The distinct error codes of `played`.
fn codes(played: &Played) -> BTreeSet<String> {
    error_codes(&played.result.errors).into_iter().collect()
}

/// The single code `code`.
fn only(code: &str) -> BTreeSet<String> { BTreeSet::from([code.to_string()]) }

fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

fn boolean(value: bool) -> Par { new_gbool_par(value, Vec::new(), false) }

fn list(ps: Vec<Par>) -> Par { new_elist_par(ps, Vec::new(), false, None, Vec::new(), false) }

/// The return channel of a call: the write-only bundle of the sink op (TS-2).
fn sink() -> Par { new_bundle_par(gprivate(op_channel(SINK_OP)), true, false) }

/// The values that a forwarding agent sent to `rrs-native-out`, as `[method, raw, msg]` lists.
fn forwarded(played: &Played) -> Vec<Par> {
    let mut values: Vec<Par> = played
        .out
        .iter()
        .map(|pars| match pars.as_slice() {
            [value] => value.clone(),
            other => panic!("a forwarded call has one value, not {other:?}"),
        })
        .collect();
    values.sort_by_cached_key(|value| format!("{value:?}"));
    values
}

/// The forwarded form of a call with the raw channel `raw` and the data `data`.
fn call(raw: Par, data: Vec<Par>) -> Par { list(vec![string("produce"), raw, list(data)]) }

// ─── TS-1: the call of a send ────────────────────────────────────────────────────────────────────

/// TS-1 and AG-4 (design v2): a send on an agent-tagged name is a method call on the agent, and
/// a call on an agent whose reference is an agent-minted name is a send on that name in turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_agent_send_calls_the_agent() {
    let term = program(
        &["agent"],
        &format!(
            "{} |\n@(*tag, *agent, \"chan\")!(1, 2)",
            forwarding_agent("agent")
        ),
    );
    let (played, chain) = play_and_certify("send", &term).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert_eq!(forwarded(&played), vec![call(string("chan"), vec![
        int(1),
        int(2)
    ])]);
    assert!(
        chain.data(&gprivate(op_channel(SINK_OP))).await.is_empty(),
        "the sink absorbs the reply"
    );

    let nested = program(
        &["inner"],
        &format!(
            "{} |\n@(*tag, bundle{{(*tag, *inner, 5)}}, 9)!(3)",
            forwarding_agent("inner")
        ),
    );
    let (played, _) = play_and_certify("nested", &nested).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert_eq!(forwarded(&played), vec![call(int(5), vec![
        sink(),
        string("produce"),
        int(9),
        list(vec![int(3)])
    ])]);
}

// ─── R7A-1: replies of system processes ──────────────────────────────────────────────────────────

/// R7A-1 (design v2): `assertEqual` replies through the R7 backend. A reply on an agent-tagged
/// name, or on the bundle with both flags that `new` binds to such a name, reaches the agent as
/// a call. A reply on a write-only bundle stays on the bundle in the default space, as in HEAD.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_agent_reply_reaches_the_agent() {
    let term = program(
        &["agent", "assert(`rho:assert:assertEqual`)"],
        &format!(
            r#"{} |
assert!(bundle{{(*tag, *agent, "rw")}}, 1, 1) |
assert!((*tag, *agent, "bare"), 1, 2) |
assert!(bundle+{{(*tag, *agent, "write-only")}}, 1, 1)"#,
            forwarding_agent("agent")
        ),
    );
    let (played, _) = play_and_certify("replies", &term).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    let mut expected = vec![
        call(string("bare"), vec![boolean(false)]),
        call(string("rw"), vec![boolean(true)]),
    ];
    expected.sort_by_cached_key(|value| format!("{value:?}"));
    assert_eq!(forwarded(&played), expected);
}

// ─── TS-3 and AG-4: the checks of a send ─────────────────────────────────────────────────────────

/// Plays a deploy whose send `send` on an agent space must abort with `code`, and checks that
/// the agent receives no call.
async fn rejected_send(label: &str, send: &str, code: &str) {
    let term = program(
        &["agent"],
        &format!("{} |\n{send}", forwarding_agent("agent")),
    );
    let (played, _) = play_and_certify(label, &term).await;
    assert_eq!(codes(&played), only(code), "{label}");
    assert!(forwarded(&played).is_empty(), "{label}");
}

/// TS-3 (design v2): `!!` on an agent space aborts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_xlate_bang_bang_rejected() {
    rejected_send(
        "persistent",
        r#"@(*tag, *agent, "chan")!!(1)"#,
        "PersistentSendOnAgentSpace",
    )
    .await;
}

/// AG-4 (design v2): a call needs a reference with the write flag, as `eval_send` does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_reference_not_writable_rejected() {
    for (label, reference) in [
        ("read-only", "bundle-{*agent}"),
        ("no flag", "bundle0{*agent}"),
    ] {
        rejected_send(
            label,
            &format!(r#"@(*tag, {reference}, "chan")!(1)"#),
            "HandleNotWritable",
        )
        .await;
    }
}

/// TS-3 (design v2): params on an agent space abort.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_agent_errors_params() {
    rejected_send(
        "params",
        r#"@(*tag, *agent, "chan")!(1; k=2)"#,
        "ParamsUnsupportedOnAgentSpace",
    )
    .await;
}

// ─── A5: the Seq scan ────────────────────────────────────────────────────────────────────────────

/// A5 (design v2): a name of a `seq` space never reaches a user agent, neither in the data of a
/// send nor as its raw channel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_agent_send_seq_names_abort() {
    for (label, send) in [
        ("data", r#"@(*tag, *agent, "chan")!(*x)"#),
        ("raw", "@(*tag, *agent, *x)!(1)"),
    ] {
        let term = program(
            &["agent", "factory(`rho:space:hashmap:queue`)"],
            &format!(
                r#"{} |
for (S <- factory!?("seq")) {{
  for (x <- S!?("gensym")) {{
    {send}
  }}
}}"#,
                forwarding_agent("agent")
            ),
        );
        let (played, _) = play_and_certify(label, &term).await;
        assert_eq!(codes(&played), only("SeqSent"), "{label}");
        assert!(forwarded(&played).is_empty(), "{label}");
    }
}

// ─── T_recv: receives on agent spaces (step 3) ───────────────────────────────────────────────────

/// A minimal agent on the name `agent` that keeps each message on a private channel per raw
/// channel. A consume of one bind asks its matcher about the first message of its channel, and
/// a consume of two binds asks both matchers and joins the two env maps. On `(true, env)` the
/// agent sends the env map to the body capability. The agent never replies to the sink.
fn memory_agent() -> &'static str {
    r#"new stored in {
  for (_, @"produce", @raw, @msg <= agent) { @(*stored, raw)!(msg) } |
  for (_, @"consume", @[(m, c)], @arrow, body <= agent) {
    for (@msg <- @(*stored, c)) {
      new ret in { @m!(*ret, msg) | for (@(true, env) <- ret) { body!(env) } }
    }
  } |
  for (_, @"consume", @[(m0, c0), (m1, c1)], @arrow, body <= agent) {
    for (@msg0 <- @(*stored, c0) & @msg1 <- @(*stored, c1)) {
      new ret0, ret1 in {
        @m0!(*ret0, msg0) | @m1!(*ret1, msg1) |
        for (@(true, env0) <- ret0 & @(true, env1) <- ret1) { body!(env0.union(env1)) }
      }
    }
  }
}"#
}

/// A deploy with the memory agent on `agent` and the processes `body`.
fn with_memory_agent(body: &str) -> String {
    program(&["agent"], &format!("{} |\n{body}", memory_agent()))
}

/// TR-1 to TR-7 (design v2): a send and a receive on one agent-tagged name meet through the
/// agent. The agent asks the matcher of the receive, the matcher replies the env of the bind,
/// and the body of the receive runs with the bound values.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_agent_produce_consume_roundtrip() {
    let term = with_memory_agent(
        r#"@(*tag, *agent, "chan")!(1, 2) |
for (@x, @y <- @(*tag, *agent, "chan")) { @"rrs-native-out"!([x, y]) }"#,
    );
    let (played, _) = play_and_certify("round trip", &term).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert_eq!(forwarded(&played), vec![list(vec![int(1), int(2)])]);
}

/// TR-6 (design v2): the body of a translated receive binds the values that a native receive
/// with the same pattern binds, a list remainder and a map pattern included.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_xlate_body_equivalence_vs_native() {
    let term = with_memory_agent(
        r#"@(*tag, *agent, "chan")!(1, [2, 3, 4], {"k": 5}) |
@"native"!(1, [2, 3, 4], {"k": 5}) |
for (@x, @[y, ...z], @{"k": w} <- @(*tag, *agent, "chan")) { @"rrs-native-out"!(["agent", x, y, z, w]) } |
for (@x, @[y, ...z], @{"k": w} <- @"native") { @"rrs-native-out"!(["native", x, y, z, w]) }"#,
    );
    let (played, _) = play_and_certify("equivalence", &term).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    let bound = |side: &str| {
        list(vec![
            string(side),
            int(1),
            int(2),
            list(vec![int(3), int(4)]),
            int(5),
        ])
    };
    assert_eq!(forwarded(&played), vec![bound("agent"), bound("native")]);
}

/// TR-3 and TR-7 (design v2): a join of two channels of one agent gives the agent two
/// matchers, whose env maps are disjoint and join into the env of the body.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_xlate_join_two_matchers() {
    let term = with_memory_agent(
        r#"@(*tag, *agent, "a")!(1) | @(*tag, *agent, "b")!(2) |
for (@x <- @(*tag, *agent, "a") & @y <- @(*tag, *agent, "b")) { @"rrs-native-out"!([x, y]) }"#,
    );
    let (played, _) = play_and_certify("join", &term).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert_eq!(forwarded(&played), vec![list(vec![int(1), int(2)])]);
}

/// TR-4 and MO-3 (design v2): the matcher of a guarded bind checks the guard, so a message
/// that fails the guard gets `(false, Nil)` and the body does not run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_xlate_guard_single_bind() {
    for (value, expected) in [(7, vec![int(7)]), (3, Vec::new())] {
        let term = with_memory_agent(&format!(
            r#"@(*tag, *agent, "chan")!({value}) |
for (@x <- @(*tag, *agent, "chan") where x > 5) {{ @"rrs-native-out"!(x) }}"#
        ));
        let (played, _) = play_and_certify("guard", &term).await;
        assert!(
            played.result.errors.is_empty(),
            "{:?}",
            played.result.errors
        );
        assert_eq!(forwarded(&played), expected, "{value}");
    }
}

/// Plays a deploy whose receive on an agent space must abort with `code`.
async fn rejected_receive(label: &str, receive: &str, code: &str) {
    let term = with_memory_agent(receive);
    let (played, _) = play_and_certify(label, &term).await;
    assert_eq!(codes(&played), only(code), "{label}");
    assert!(forwarded(&played).is_empty(), "{label}");
}

/// TR-4 (design v2): a guard over the binders of two binds aborts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_xlate_guard_span_rejected() {
    rejected_receive(
        "guard span",
        r#"for (@x <- @(*tag, *agent, "a") & @y <- @(*tag, *agent, "b") where x == y) { @"rrs-native-out"!(x) }"#,
        "AgentGuardSpansJoin",
    )
    .await;
}

/// TR-1 (design v2): a join of an agent channel and a channel of the default space aborts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_xlate_cross_space_join_rejected() {
    rejected_receive(
        "cross space",
        r#"for (@x <- @(*tag, *agent, "a") & @y <- @"host") { @"rrs-native-out"!(x) }"#,
        "CrossSpaceJoin",
    )
    .await;
}

/// TR-2 (design v2): a query on an agent space aborts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_agent_errors_query() {
    rejected_receive(
        "query",
        r#"for (@x <- @(*tag, *agent, "a") ~ 5) { @"rrs-native-out"!(x) }"#,
        "QuerySpecUnsupportedOnAgentSpace",
    )
    .await;
}

/// The pattern `[[…[x]…]]` with `depth` nested lists.
fn nested_pattern(depth: usize) -> String { format!("{}x{}", "[".repeat(depth), "]".repeat(depth)) }

/// The value `[[…[7]…]]` with `depth` nested lists.
fn nested_value(depth: usize) -> String { format!("{}7{}", "[".repeat(depth), "]".repeat(depth)) }

/// The deepest chain of nested lists in the pattern of one bind whose matcher state prost
/// decodes: one more list puts the state past the recursion limit of 100 (measured).
const DEEPEST_DECODABLE_LISTS: usize = 29;

/// TR-5 (design v2): a pattern whose matcher state prost cannot decode, because it is nested
/// deeper than the recursion limit of 100, aborts with `AgentPatternTooDeep` before any receive.
/// So no node holds a matcher id that it cannot decode. The boundary is exact: a chain of 30
/// nested lists aborts, and a chain of 29 translates, and its receive waits on the memory agent.
/// A receive with a pattern of three nested lists fires through the agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_agent_errors_pattern_too_deep() {
    rejected_receive(
        "too deep",
        &format!(
            r#"for (@{} <- @(*tag, *agent, "a")) {{ @"rrs-native-out"!(x) }}"#,
            nested_pattern(DEEPEST_DECODABLE_LISTS + 1)
        ),
        "AgentPatternTooDeep",
    )
    .await;
    let deepest = with_memory_agent(&format!(
        r#"for (@{} <- @(*tag, *agent, "a")) {{ @"rrs-native-out"!(x) }}"#,
        nested_pattern(DEEPEST_DECODABLE_LISTS)
    ));
    let (played, _) = play_and_certify("deepest", &deepest).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert!(forwarded(&played).is_empty());
    let fired = with_memory_agent(&format!(
        r#"@(*tag, *agent, "a")!({}) |
for (@{} <- @(*tag, *agent, "a")) {{ @"rrs-native-out"!(x) }}"#,
        nested_value(3),
        nested_pattern(3)
    ));
    let (played, _) = play_and_certify("fired", &fired).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert_eq!(forwarded(&played), vec![int(7)]);
}

/// W6 (design v2, HC-1 to HC-3): an agent that forwards each call to the handle of a native
/// space serves the space. The native handle decodes the matchers of the translation, runs the
/// receive on its own space, and sends the env map to the body capability.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_wrapper_forwards_to_native() {
    let term = program(
        &["agent", "ignore", "factory(`rho:space:hashmap:queue`)"],
        r#"for (h <- factory!?("default")) {
  for (_, @"produce", @raw, @msg <= agent) { h!(*ignore, "produce", raw, msg) } |
  for (_, @"consume", @pairs, @arrow, body <= agent) { h!(*ignore, "consume", pairs, arrow, *body) } |
  @(*tag, *agent, "chan")!(5) |
  for (@x <- @(*tag, *agent, "chan")) { @"rrs-native-out"!(x) }
}"#,
    );
    let (played, _) = play_and_certify("wrapper", &term).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert_eq!(forwarded(&played), vec![int(5)]);
}

/// ASO-2 (design v2, TN-5): an agent that never answers leaves the receive pending: the
/// matcher and body receives stay parked, nothing runs, and native replay reproduces the play.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_never_replies_pending_replay_equal() {
    let term = program(
        &["agent"],
        r#"for (_, @"produce", @raw, @msg <= agent) { Nil } |
for (_, @"consume", @pairs, @arrow, body <= agent) { Nil } |
@(*tag, *agent, "chan")!(1) |
for (@x <- @(*tag, *agent, "chan")) { @"rrs-native-out"!(x) }"#,
    );
    let (played, _) = play_and_certify("silent agent", &term).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert!(forwarded(&played).is_empty());
}

/// The channels that the events of `played` name. Panics when a consume or a COMM names two or
/// more channels.
fn single_channel_events(played: &Played, label: &str) -> BTreeSet<Blake2b256Hash> {
    let mut channels = BTreeSet::new();
    for event in &played.log {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => {
                channels.insert(produce.channel_hash.clone());
            }
            Event::IoEvent(IOEvent::Consume(consume)) => {
                assert_eq!(
                    consume.channel_hashes.len(),
                    1,
                    "{label}: a consume names two or more channels"
                );
                channels.extend(consume.channel_hashes.iter().cloned());
            }
            Event::Comm(comm) => {
                assert_eq!(
                    comm.consume.channel_hashes.len(),
                    1,
                    "{label}: a COMM consumes on two or more channels"
                );
                channels.extend(comm.consume.channel_hashes.iter().cloned());
                channels.extend(
                    comm.produces
                        .iter()
                        .map(|produce| produce.channel_hash.clone()),
                );
            }
        }
    }
    channels
}

/// The widths of the joins of `channel` in the post-state of `played`.
fn join_widths(played: &Played, channel: &Blake2b256Hash) -> Vec<usize> {
    played
        .history
        .get_history_reader(&played.root)
        .expect("read the history at the post-state root")
        .get_joins(channel)
        .expect("read the joins of a channel")
        .iter()
        .map(Vec::len)
        .collect()
}

/// Consensus checklist item 18 at P8 step 3 (I-16, design v2 TR-8): the translation of a
/// receive uses no join of two channels. Each matcher receive and the body receive name one
/// channel, and the call is a send. The programs park translated receives on a silent agent
/// (one bind, a join of two binds, a guard, the arrows `<=` and `<<-`), fire one through the
/// memory agent, and run one on a native handle through a wrapper agent. Every consume and COMM
/// names one channel, and the post-state holds no join of two channels. An ordinary join is the
/// control: the check sees its join.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_p8_synthesizes_no_join() {
    let silent = r#"for (_, @"produce", @raw, @msg <= agent) { Nil } |
for (_, @"consume", @pairs, @arrow, body <= agent) { Nil }"#;
    let cases = [
        (
            "rrs-p8-no-join-one-bind",
            program(
                &["agent"],
                &format!("{silent} |\nfor (@x <- @(*tag, *agent, \"a\")) {{ Nil }}"),
            ),
        ),
        (
            "rrs-p8-no-join-two-binds",
            program(
                &["agent"],
                &format!(
                    "{silent} |\nfor (@x <- @(*tag, *agent, \"a\") & @y <- @(*tag, *agent, \"b\")) {{ Nil }}"
                ),
            ),
        ),
        (
            "rrs-p8-no-join-guard",
            program(
                &["agent"],
                &format!(
                    "{silent} |\nfor (@x <- @(*tag, *agent, \"a\") & @y <- @(*tag, *agent, \"b\") where x > 1) {{ Nil }}"
                ),
            ),
        ),
        (
            "rrs-p8-no-join-persistent",
            program(
                &["agent"],
                &format!("{silent} |\nfor (@x <= @(*tag, *agent, \"a\")) {{ Nil }}"),
            ),
        ),
        (
            "rrs-p8-no-join-peek",
            program(
                &["agent"],
                &format!("{silent} |\nfor (@x <<- @(*tag, *agent, \"a\")) {{ Nil }}"),
            ),
        ),
        (
            "rrs-p8-no-join-fired",
            with_memory_agent(
                r#"@(*tag, *agent, "chan")!(1) |
for (@x <- @(*tag, *agent, "chan")) { @"rrs-native-out"!(x) }"#,
            ),
        ),
        (
            "rrs-p8-no-join-wrapper",
            program(
                &["agent", "ignore", "factory(`rho:space:hashmap:queue`)"],
                r#"for (h <- factory!?("default")) {
  for (_, @"produce", @raw, @msg <= agent) { h!(*ignore, "produce", raw, msg) } |
  for (_, @"consume", @pairs, @arrow, body <= agent) { h!(*ignore, "consume", pairs, arrow, *body) } |
  @(*tag, *agent, "chan")!(5) |
  for (@x <- @(*tag, *agent, "chan")) { @"rrs-native-out"!(x) }
}"#,
            ),
        ),
    ];
    for (label, term) in cases {
        let (played, _) = play_and_certify(label, &term).await;
        assert!(
            played.result.errors.is_empty(),
            "{label}: {:?}",
            played.result.errors
        );
        let channels = single_channel_events(&played, label);
        assert!(!channels.is_empty(), "{label}: the check reads the events");
        for channel in &channels {
            let widths = join_widths(&played, channel);
            assert!(
                widths.iter().all(|width| *width < 2),
                "{label}: the state holds a join of {widths:?} channels"
            );
        }
    }
    // The control: an ordinary join on two public channels leaves a join of two channels, and
    // the reader of the check sees it.
    let (played, _) = play_and_certify(
        "rrs-p8-no-join-control",
        &program(
            &[],
            r#"for (@x <- @"rrs-p8-join-left" & @y <- @"rrs-p8-join-right") { Nil }"#,
        ),
    )
    .await;
    let widest = played
        .log
        .iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Consume(consume)) => Some(consume.channel_hashes.clone()),
            Event::IoEvent(IOEvent::Produce(_)) | Event::Comm(_) => None,
        })
        .flatten()
        .flat_map(|channel| join_widths(&played, &channel))
        .max();
    assert_eq!(widest, Some(2), "the control leaves a join of two channels");
}

// ─── Native properties of AgentTranslation.v (step 3) ────────────────────────────────────────────

/// Plays `term` and certifies it, for a property: the play must evaluate, leave no volatile
/// channel and reproduce under native replay. Returns the forwarded values.
async fn played_values(label: &str, term: &str) -> Result<Vec<Par>, TestCaseError> {
    let (played, _) = play_and_certify(label, term).await;
    prop_assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    Ok(forwarded(&played))
}

/// The values of a forwarded list whose first value is the tag `side`.
fn side_values(values: &[Par], side: &str) -> Vec<Vec<Par>> {
    values
        .iter()
        .filter_map(|value| match value.exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
            }] => match ps.split_first() {
                Some((head, rest)) if *head == string(side) => Some(rest.to_vec()),
                Some(_) | None => None,
            },
            _ => None,
        })
        .collect()
}

/// `body_equivalence` (AgentTranslation.v, A5): for generated binds of one or two channels with
/// generated data, the body of the translated receive binds exactly the values that a native
/// receive with the same patterns binds, in binder order.
#[test]
fn prop_body_equivalence() {
    check(
        runtime_config(),
        prop::collection::vec(prop::collection::vec(-5i64..5, 1..4), 1..3),
        |binds| tokio_runtime().block_on(body_equivalence_case(binds)),
    );
}

async fn body_equivalence_case(binds: Vec<Vec<i64>>) -> Result<(), TestCaseError> {
    let mut sends = Vec::new();
    let (mut agent_binds, mut native_binds, mut vars) = (Vec::new(), Vec::new(), Vec::new());
    for (bind, data) in binds.iter().enumerate() {
        let names: Vec<String> = (0..data.len()).map(|j| format!("x{bind}_{j}")).collect();
        let pattern = names
            .iter()
            .map(|name| format!("@{name}"))
            .collect::<Vec<_>>()
            .join(", ");
        let values = data
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        sends.push(format!(r#"@(*tag, *agent, "c{bind}")!({values})"#));
        sends.push(format!(r#"@"n{bind}"!({values})"#));
        agent_binds.push(format!(r#"{pattern} <- @(*tag, *agent, "c{bind}")"#));
        native_binds.push(format!(r#"{pattern} <- @"n{bind}""#));
        vars.extend(names);
    }
    let vars = vars.join(", ");
    let term = with_memory_agent(&format!(
        r#"{} |
for ({}) {{ @"rrs-native-out"!(["agent", {vars}]) }} |
for ({}) {{ @"rrs-native-out"!(["native", {vars}]) }}"#,
        sends.join(" | "),
        agent_binds.join(" & "),
        native_binds.join(" & "),
    ));
    let values = played_values("body equivalence", &term).await?;
    let expected: Vec<Par> = binds.iter().flatten().map(|value| int(*value)).collect();
    prop_assert_eq!(side_values(&values, "native"), vec![expected.clone()]);
    prop_assert_eq!(side_values(&values, "agent"), vec![expected]);
    Ok(())
}

/// `matcher_sound_complete` (AgentTranslation.v, A7): the matcher of a bind with the pattern
/// `[a, b]` accepts a generated list exactly when the native receive with the same pattern
/// fires, and then the body gets the same values.
#[test]
fn prop_matcher_sound_complete() {
    check(
        runtime_config(),
        prop::collection::vec(-5i64..5, 1..4),
        |data| tokio_runtime().block_on(matcher_sound_complete_case(data)),
    );
}

async fn matcher_sound_complete_case(data: Vec<i64>) -> Result<(), TestCaseError> {
    let list = data
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let term = with_memory_agent(&format!(
        r#"@(*tag, *agent, "chan")!([{list}]) | @"native"!([{list}]) |
for (@[a, b] <- @(*tag, *agent, "chan")) {{ @"rrs-native-out"!(["agent", a, b]) }} |
for (@[a, b] <- @"native") {{ @"rrs-native-out"!(["native", a, b]) }}"#
    ));
    let values = played_values("matcher", &term).await?;
    let fires = data.len() == 2;
    let expected: Vec<Vec<Par>> = match fires {
        true => vec![data.iter().map(|value| int(*value)).collect()],
        false => Vec::new(),
    };
    prop_assert_eq!(side_values(&values, "native"), expected.clone());
    prop_assert_eq!(side_values(&values, "agent"), expected);
    Ok(())
}

/// `guard_fold_sound` (AgentTranslation.v, A6): a guard over one bind of a join of two agent
/// channels goes to the matcher of that bind, and the translated join fires exactly when the
/// native join with the same guard fires, with the same values.
#[test]
fn prop_guard_fold_sound() {
    check(
        runtime_config(),
        (0i64..10, 0i64..10, 0i64..10, any::<bool>()),
        |(x, y, bound, on_x)| tokio_runtime().block_on(guard_fold_case(x, y, bound, on_x)),
    );
}

async fn guard_fold_case(x: i64, y: i64, bound: i64, on_x: bool) -> Result<(), TestCaseError> {
    let guarded = match on_x {
        true => "x",
        false => "y",
    };
    let term = with_memory_agent(&format!(
        r#"@(*tag, *agent, "a")!({x}) | @(*tag, *agent, "b")!({y}) | @"na"!({x}) | @"nb"!({y}) |
for (@x <- @(*tag, *agent, "a") & @y <- @(*tag, *agent, "b") where {guarded} > {bound}) {{ @"rrs-native-out"!(["agent", x, y]) }} |
for (@x <- @"na" & @y <- @"nb" where {guarded} > {bound}) {{ @"rrs-native-out"!(["native", x, y]) }}"#
    ));
    let values = played_values("guard fold", &term).await?;
    let passes = match on_x {
        true => x > bound,
        false => y > bound,
    };
    let expected: Vec<Vec<Par>> = match passes {
        true => vec![vec![int(x), int(y)]],
        false => Vec::new(),
    };
    prop_assert_eq!(side_values(&values, "native"), expected.clone());
    prop_assert_eq!(side_values(&values, "agent"), expected);
    Ok(())
}
