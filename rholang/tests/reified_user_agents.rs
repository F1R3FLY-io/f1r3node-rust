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

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeSet;

use models::rhoapi::Par;
use models::rust::utils::{
    new_bundle_par, new_elist_par, new_gbool_par, new_gint_par, new_gstring_par,
};
use rholang::rust::interpreter::spaces::namespace::{
    gprivate, op_channel, AGENT_TAG, REIFIED_TAG, SINK_OP,
};
use support::native::{assert_reproduces, certify, host_work, NativeChain, Played, AMPLE_LIMIT};
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
