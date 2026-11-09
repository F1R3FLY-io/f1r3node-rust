//! Library tests of the theory seam (P7, TH-5 to TH-10). They run whole space operations
//! through the family agent over spaces whose config names a theory. The test checkers of
//! [`super::test_theories`] exist only in test builds (TH-10), so the tests of a lease, of a
//! refusal at a match and of a starved limit profile are library tests.

use std::collections::BTreeMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::{new_freevar_par, new_freevar_var, new_gint_par, new_gstring_par};
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

use super::identity::TheoryIdentity;
use super::test_theories::{self, TestBehavior};
use super::TheoryUsage;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::matcher::r#match::Matcher;
// Added by decision 124 (L6): the round cap and its cause, for the tests of the lease at the fire.
use crate::rust::interpreter::reified::costs::{ReissueCause, FIRE_ROUND_CAP};
use crate::rust::interpreter::spaces::agent::{
    run_op, ConsumeOp, JoinBind, JoinOp, ProduceOp, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp,
    SpaceOutcome,
};
use crate::rust::interpreter::spaces::errors::SpaceError;
// Added by decision 124 (L6): the two count params of a VectorDB config (`vdb_config_of`).
use crate::rust::interpreter::spaces::factory::defaults::{CAPACITY_PARAM, DIMENSIONS_PARAM};
use crate::rust::interpreter::spaces::families::FamilyAgent;
use crate::rust::interpreter::spaces::namespace::{
    self, CollectionKey, Durability, NativeSpace, SID_LEN,
};
use crate::rust::interpreter::spaces::record;
use crate::rust::interpreter::spaces::types::config::SpaceConfig;
use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

const SPACE: NativeSpace = NativeSpace {
    sid: [13; SID_LEN],
    durability: Durability::Durable,
};

fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

fn key_of(channel: &Par) -> CollectionKey {
    CollectionKey::of_channel(channel).expect("a channel has a key")
}

fn data_of(channel: &Par) -> Par {
    namespace::gprivate(namespace::data_channel(SPACE, &key_of(channel)))
}

fn body_of(name: &str) -> ParWithRandom {
    ParWithRandom {
        body: Some(string(name)),
        random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
    }
}

fn any() -> BindPattern {
    BindPattern {
        patterns: vec![new_freevar_par(0, Vec::new())],
        remainder: None,
        free_count: 1,
    }
}

fn nat() -> TheoryIdentity { TheoryIdentity::builtin("Nat") }

/// A fresh play space and an environment over it.
async fn fresh() -> (ParRSpace, SpaceEnv) {
    let mut stores = InMemoryStoreManager::new();
    let host = ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("an in-memory space");
    let env = SpaceEnv {
        store: SpaceStoreHandle::new(host.clone()),
        budget: RuntimeBudget::new(Cost::create(10_000_000, "theory seam test")),
        volatile: Default::default(),
        origin: Default::default(),
    };
    (host, env)
}

/// The config map of a space of the family `hashmap:inner` whose theory is `identity`.
fn config_of(inner: SpaceInner, identity: Option<TheoryIdentity>) -> Par {
    SpaceConfig {
        theory: identity,
        ..SpaceConfig::new(
            SPACE.sid,
            SpaceOuter::HashMap.as_str(),
            inner.as_str(),
            SpaceQualifier::Default,
        )
    }
    .to_par()
}

/// A space of the family `hashmap:inner` over a fresh play space. Its config names the theory
/// `identity`, or no theory.
async fn space_of(inner: SpaceInner, identity: Option<TheoryIdentity>) -> (ParRSpace, SpaceEnv) {
    let (host, env) = fresh().await;
    create(&env, inner, identity)
        .await
        .expect("the creation commits");
    host.take_event_log().await;
    (host, env)
}

/// The creation of the space of `space_of` in `env`.
async fn create(
    env: &SpaceEnv,
    inner: SpaceInner,
    identity: Option<TheoryIdentity>,
) -> Result<(), SpaceError> {
    // Changed by decision 124 (L6): the body moved to `create_with`, which takes the config map
    // as an input. The text before:
    // let family = SpaceFactoryUrn::all()
    //     .find(|urn| urn.outer == SpaceOuter::HashMap && urn.inner == inner)
    //     .expect("a factory family");
    // let config = config_of(inner, identity);
    // FamilyAgent
    //     .prepare_create(env, SpaceCreation {
    //         space: SPACE,
    //         config,
    //         handle_pattern: BindPattern {
    //             patterns: Vec::new(),
    //             remainder: Some(new_freevar_var(0)),
    //             free_count: 1,
    //         },
    //         handle_continuation: TaggedContinuation {
    //             tagged_cont: Some(TaggedCont::ParBody(body_of("handle"))),
    //             guard: None,
    //             cost_authority: None,
    //         },
    //         random: Blake2b512Random::create_from_bytes(b"theory seam factory"),
    //         authority: CostAuthority::default(),
    //         family,
    //     })
    //     .await?
    //     .commit()
    //     .await?;
    // Ok(())
    create_with(env, inner, config_of(inner, identity)).await
}

/// Added by decision 124 (L6): the creation in `env` of a space of the family `hashmap:inner`
/// whose config map is `config`.
async fn create_with(env: &SpaceEnv, inner: SpaceInner, config: Par) -> Result<(), SpaceError> {
    let family = SpaceFactoryUrn::all()
        .find(|urn| urn.outer == SpaceOuter::HashMap && urn.inner == inner)
        .expect("a factory family");
    FamilyAgent
        .prepare_create(env, SpaceCreation {
            space: SPACE,
            config,
            handle_pattern: BindPattern {
                patterns: Vec::new(),
                remainder: Some(new_freevar_var(0)),
                free_count: 1,
            },
            handle_continuation: TaggedContinuation {
                tagged_cont: Some(TaggedCont::ParBody(body_of("handle"))),
                guard: None,
                cost_authority: None,
            },
            random: Blake2b512Random::create_from_bytes(b"theory seam factory"),
            authority: CostAuthority::default(),
            family,
        })
        .await?
        .commit()
        .await?;
    Ok(())
}

/// Added by decision 124 (L6): the config map of a VectorDB space whose theory is `identity`,
/// with the two count params that every VectorDB operation reads (vectordb.rs:314-316).
fn vdb_config_of(identity: Option<TheoryIdentity>) -> Par {
    let mut params = BTreeMap::<String, Par>::new();
    params.insert(CAPACITY_PARAM.to_string(), int(64));
    params.insert(DIMENSIONS_PARAM.to_string(), int(2));
    SpaceConfig {
        params,
        theory: identity,
        ..SpaceConfig::new(
            SPACE.sid,
            SpaceOuter::HashMap.as_str(),
            SpaceInner::VectorDb.as_str(),
            SpaceQualifier::Default,
        )
    }
    .to_par()
}

fn send(channel: &Par, values: Vec<Par>, tag: &str) -> SpaceOp {
    SpaceOp::Produce(ProduceOp {
        space: SPACE,
        key: key_of(channel),
        channel: channel.clone(),
        data: ListParWithRandom {
            pars: values,
            random_state: Blake2b512Random::create_from_bytes(tag.as_bytes()).to_bytes(),
            cost_authority: None,
            cost_stack: None,
        },
        persistent: false,
        params: BTreeMap::new(),
        authority: CostAuthority::default(),
        reissued_ord: None,
    })
}

fn receive(channel: &Par, name: &str) -> SpaceOp {
    SpaceOp::Consume(ConsumeOp {
        space: SPACE,
        key: key_of(channel),
        channel: channel.clone(),
        pattern: any(),
        body: body_of(name),
        persistent: false,
        peek: false,
        guard: None,
        query: None,
        authority: CostAuthority::default(),
        introduction_authority: CostAuthority::default(),
        reissued_ord: None,
    })
}

fn join(channels: &[&Par], name: &str) -> SpaceOp {
    SpaceOp::Join(JoinOp {
        space: SPACE,
        binds: channels
            .iter()
            .map(|channel| JoinBind {
                key: key_of(channel),
                channel: (*channel).clone(),
                pattern: any(),
                query: None,
            })
            .collect(),
        body: body_of(name),
        persistent: false,
        peek: false,
        guard: None,
        authority: CostAuthority::default(),
        introduction_authority: CostAuthority::default(),
        reissued_ords: None,
    })
}

async fn run(env: &SpaceEnv, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
    run_op(&FamilyAgent, env, op).await
}

async fn records(host: &ParRSpace, channel: &Par) -> usize {
    host.get_data(&data_of(channel)).await.len()
}

async fn waiters(host: &ParRSpace, channels: &[&Par]) -> usize {
    host.get_waiting_continuations(channels.iter().map(|channel| data_of(channel)).collect())
        .await
        .len()
}

/// The waiters of the join group of `a` and `b`, in either order: the store keeps a group in
/// its canonical order (FR-1').
async fn join_waiters(host: &ParRSpace, a: &Par, b: &Par) -> usize {
    waiters(host, &[a, b]).await + waiters(host, &[b, a]).await
}

/// TH-1 and TH-5: a limit profile without one work unit makes every check `Undetermined`, so a
/// send aborts with `TheoryUndetermined` before any staging, bump or event, and a receive that
/// matches nothing parks as in a space without a theory.
#[tokio::test]
async fn theory_undetermined_budget0_aborts() {
    let starved = TheoryIdentity {
        limit_profile: TheoryUsage::new(0, 0),
        ..nat()
    };
    for inner in [SpaceInner::Bag, SpaceInner::Queue] {
        let (host, env) = space_of(inner, Some(starved.clone())).await;
        let channel = string("c");
        assert_eq!(
            run(&env, send(&channel, vec![int(1)], "s1")).await,
            Err(SpaceError::TheoryUndetermined {
                theory: "Nat".to_string(),
                index: 0,
            }),
            "{inner:?}"
        );
        assert_eq!(records(&host, &channel).await, 0, "{inner:?}");
        assert!(
            host.take_event_log().await.is_empty(),
            "{inner:?}: no event"
        );
        run(&env, receive(&channel, "w"))
            .await
            .expect("a receive that matches nothing parks");
        assert_eq!(waiters(&host, &[&channel]).await, 1, "{inner:?}");
    }
}

/// TH-5: the first value of a send that its theory refutes aborts the send with
/// `TheoryRefuted` and the index of the value, before any staging, bump or event.
#[tokio::test]
async fn theory_refuted_value_aborts_before_staging() {
    for inner in [SpaceInner::Bag, SpaceInner::Queue] {
        let (host, env) = space_of(inner, Some(nat())).await;
        let channel = string("c");
        for (values, index) in [
            (vec![int(-1)], 0),
            (vec![int(1), string("2")], 1),
            (vec![int(0), int(1), int(-2)], 2),
        ] {
            assert_eq!(
                run(&env, send(&channel, values, "refused")).await,
                Err(SpaceError::TheoryRefuted {
                    theory: "Nat".to_string(),
                    index,
                }),
                "{inner:?}"
            );
        }
        assert_eq!(records(&host, &channel).await, 0, "{inner:?}");
        assert!(
            host.take_event_log().await.is_empty(),
            "{inner:?}: no event"
        );
        run(&env, send(&channel, vec![int(0), int(7)], "admitted"))
            .await
            .expect("two naturals are admitted");
        assert_eq!(records(&host, &channel).await, 1, "{inner:?}");
    }
}

/// TH-9: each `Proven` verdict of a leasing checker gives one lease, and a transaction with two
/// or more leases aborts with `NestedCommitGuard` before its commit. The admission of a send
/// counts one lease per value. A receive counts one per stored datum that it matches, and a
/// send that fires a waiter counts none for its datum in flight.
#[tokio::test]
async fn theory_nested_commit_guard() {
    let leasing = test_theories::identity(TestBehavior::Leasing);
    let (host, env) = space_of(SpaceInner::Bag, Some(leasing)).await;
    let (a, b) = (string("a"), string("b"));
    assert_eq!(
        run(&env, send(&a, vec![int(1), int(2)], "two values")).await,
        Err(SpaceError::NestedCommitGuard),
        "two values give two leases"
    );
    assert_eq!(records(&host, &a).await, 0);
    // Changed by decision 92: the guard discards the staged operations and commits the anchor of
    // the job alone, a consume of the config record on `C(sid)` and its COMM.
    // assert!(
    //     host.take_event_log().await.is_empty(),
    //     "the guard aborts before the commit"
    // );
    let config = stable_hash_provider::hash(&namespace::gprivate(namespace::config_channel(SPACE)));
    match host.take_event_log().await.as_slice() {
        [Event::IoEvent(IOEvent::Consume(consume)), Event::Comm(comm)] => {
            assert_eq!(
                consume.channel_hashes,
                vec![config],
                "the anchor consumes C(sid)"
            );
            assert_eq!(comm.consume, *consume, "the anchor fires");
        }
        log => panic!("the guard commits the anchor alone: {log:?}"),
    }
    run(&env, send(&a, vec![int(1)], "a1"))
        .await
        .expect("one value gives one lease");
    let fired = run(&env, receive(&a, "single"))
        .await
        .expect("one stored datum gives one lease");
    assert_eq!(fired.fires.len(), 1);
    assert_eq!(records(&host, &a).await, 0);
    run(&env, send(&a, vec![int(1)], "a2"))
        .await
        .expect("one lease");
    run(&env, send(&b, vec![int(2)], "b1"))
        .await
        .expect("one lease");
    assert_eq!(
        run(&env, join(&[&a, &b], "joined")).await,
        Err(SpaceError::NestedCommitGuard),
        "a join over two stored data gives two leases"
    );
    assert_eq!(records(&host, &a).await, 1);
    assert_eq!(records(&host, &b).await, 1);
    assert_eq!(join_waiters(&host, &a, &b).await, 0);
    let c = string("c");
    run(&env, receive(&c, "waiting"))
        .await
        .expect("the receive parks");
    let fired = run(&env, send(&c, vec![int(3)], "c1"))
        .await
        .expect("a fire checks no datum in flight");
    assert_eq!(fired.fires.len(), 1);
}

/// Added by decision 124 (L6): the pattern of the integer `value` alone.
fn exactly(value: i64) -> BindPattern {
    BindPattern {
        patterns: vec![int(value)],
        remainder: None,
        free_count: 0,
    }
}

/// Added by decision 124 (L6): `receive` with the pattern `exactly(value)`. It peeks when `peek`.
fn receive_exactly(channel: &Par, value: i64, peek: bool, name: &str) -> SpaceOp {
    let SpaceOp::Consume(op) = receive(channel, name) else {
        panic!("receive gives a consume")
    };
    SpaceOp::Consume(ConsumeOp {
        pattern: exactly(value),
        peek,
        ..op
    })
}

/// Added by decision 124 (L6): `join` with the pattern `exactly(value)` on each channel.
fn join_exactly(binds: &[(&Par, i64)], name: &str) -> SpaceOp {
    let mut channels = Vec::<&Par>::with_capacity(binds.len());
    channels.extend(binds.iter().map(|(channel, _)| *channel));
    let SpaceOp::Join(mut op) = join(&channels, name) else {
        panic!("join gives a join")
    };
    for (bind, (_, value)) in op.binds.iter_mut().zip(binds) {
        bind.pattern = exactly(*value);
    }
    SpaceOp::Join(op)
}

/// Added by decision 124 (L6): parks the receive `w` of 7 on `channel` and 63 peeks of 7 above
/// it. A send of 7 then makes 63 peek fires, and `w` spends it with fire 64 (FIRE_ROUND_CAP).
async fn park_peeks_over_a_receive(env: &SpaceEnv, channel: &Par) {
    run(env, receive_exactly(channel, 7, false, "w"))
        .await
        .expect("w parks");
    for index in 1..FIRE_ROUND_CAP {
        let name = format!("k{index}");
        run(env, receive_exactly(channel, 7, true, &name))
            .await
            .expect("a peek parks");
    }
}

/// Added by decision 124 (L6, verify-124-J.md:135): after fire 64 the J cascade checks the join `z`
/// on the stored 8 and 5, and the cap stops the round before `z` fires (the second cap check of
/// `drain_and_cascade`). The send keeps the one lease of its value. Before L6 it failed with
/// `NestedCommitGuard`.
#[tokio::test]
async fn cascade_qualification_takes_no_lease() {
    let leasing = test_theories::identity(TestBehavior::Leasing);
    let (host, env) = space_of(SpaceInner::Stack, Some(leasing)).await;
    let (b, m) = (string("b"), string("m"));
    run(&env, join_exactly(&[(&b, 8), (&m, 5)], "z"))
        .await
        .expect("z parks");
    park_peeks_over_a_receive(&env, &b).await;
    run(&env, send(&b, vec![int(8)], "b8"))
        .await
        .expect("the head refuses 8");
    run(&env, send(&m, vec![int(5)], "m5"))
        .await
        .expect("z is not the head of b");
    let outcome = run(&env, send(&b, vec![int(7)], "b7"))
        .await
        .expect("no lease at the cap");
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    assert_eq!(
        outcome.reissue.map(|reissue| reissue.cause),
        Some(ReissueCause::RoundCap)
    );
    assert_eq!((records(&host, &b).await, records(&host, &m).await), (1, 1));
    assert_eq!(join_waiters(&host, &b, &m).await, 1);
}

/// Added by decision 124 (L6, VJ V11): after fire 64 the stack settle checks the waiter `v` on
/// the stored 8, and the cap stops the round before `v` fires (SET:415-418). The send keeps the
/// one lease of its value. Before L6 it failed with `NestedCommitGuard`.
#[tokio::test]
async fn settle_round_stopped_at_the_cap_keeps_no_lease() {
    let leasing = test_theories::identity(TestBehavior::Leasing);
    let (host, env) = space_of(SpaceInner::Stack, Some(leasing)).await;
    let b = string("b");
    run(&env, receive_exactly(&b, 8, false, "v"))
        .await
        .expect("v parks");
    park_peeks_over_a_receive(&env, &b).await;
    run(&env, send(&b, vec![int(8)], "b8"))
        .await
        .expect("the head refuses 8");
    let outcome = run(&env, send(&b, vec![int(7)], "b7"))
        .await
        .expect("no lease at the cap");
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    assert_eq!(
        outcome.reissue.map(|reissue| reissue.cause),
        Some(ReissueCause::RoundCap)
    );
    assert_eq!(
        (records(&host, &b).await, waiters(&host, &[&b]).await),
        (1, 1)
    );
}

/// Added by decision 124 (L6): parks `count` peeks of `value` on `channel`, with the names
/// `prefix` 1 to `count`. In a priority queue they take the lane `Any`, first in, first out.
async fn park_peeks(env: &SpaceEnv, channel: &Par, value: i64, count: usize, prefix: &str) {
    for index in 1..=count {
        let name = format!("{prefix}{index}");
        run(env, receive_exactly(channel, value, true, &name))
            .await
            .expect("a peek parks");
    }
}

/// Added by decision 124 (L6): `op` with the one named param `key = value`, by struct update of
/// the params of a send or of the query of a receive (agent.rs:208, 221-226, 251).
fn with_param(op: SpaceOp, key: &str, value: Par) -> SpaceOp {
    use crate::rust::interpreter::spaces::agent::QueryParams;
    let mut named = BTreeMap::<String, Par>::new();
    named.insert(key.to_string(), value);
    match op {
        SpaceOp::Produce(op) => SpaceOp::Produce(ProduceOp {
            params: named,
            ..op
        }),
        SpaceOp::Consume(op) => SpaceOp::Consume(ConsumeOp {
            query: Some(QueryParams {
                payload: Vec::new(),
                named,
            }),
            ..op
        }),
        SpaceOp::Join(_) | SpaceOp::Settle(_) => panic!("only a send or a receive takes a param"),
    }
}

/// Added by decision 124 (L6): the priority-queue view at the cap (row 8, SET:415-418). The
/// lanes are first in, first out, so the 63 peeks park before `w`, and `w` before `v`. The send
/// of 7 makes 63 peek fires, and `w` spends it with fire 64. The settle then checks `v` on the
/// stored 8 of level 0, and the cap stops the round before `v` fires. The send keeps the one
/// lease of its value. Before L6 it failed with `NestedCommitGuard`.
#[tokio::test]
async fn priority_queue_round_stopped_at_the_cap_keeps_no_lease() {
    let leasing = test_theories::identity(TestBehavior::Leasing);
    let (host, env) = space_of(SpaceInner::PriorityQueue, Some(leasing)).await;
    let b = string("b");
    park_peeks(&env, &b, 7, FIRE_ROUND_CAP - 1, "k").await;
    run(&env, receive_exactly(&b, 7, false, "w"))
        .await
        .expect("w parks");
    run(&env, receive_exactly(&b, 8, false, "v"))
        .await
        .expect("v parks");
    let eight = with_param(send(&b, vec![int(8)], "b8"), record::PRIORITY_KEY, int(0));
    run(&env, eight).await.expect("the head refuses 8");
    let outcome = run(&env, send(&b, vec![int(7)], "b7"))
        .await
        .expect("no lease at the cap");
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    assert_eq!(
        outcome.reissue.map(|reissue| reissue.cause),
        Some(ReissueCause::RoundCap)
    );
    assert_eq!(
        (records(&host, &b).await, waiters(&host, &[&b]).await),
        (1, 1)
    );
}

/// Added by decision 124 (L6): the heads that the priority-queue view does not choose (row 8,
/// `next_fire` of priority_queue.rs), at the cap (the cap check of `settle`). The send of 7 makes
/// 64 peek fires, and the cap re-issues it before its settle, so `z` heads the lane `Any` and the
/// space owes its fire. The receive `r` settles first. `z` lifts the stored 8 of level 0, which
/// takes the one lease of the receive. 62 peeks fire 8 in flight, and `c` spends it with fire 64.
/// The stored 5 is then the head of level 0, and two waiter-lane heads admit it: `l` of the lane of
/// level 0 and `a` of the lane `Any`. The view keeps the older one, and the cap stops the round
/// before it fires. Before L6 each of the two checks kept a lease, and the receive failed with
/// `NestedCommitGuard`.
#[tokio::test]
async fn priority_queue_heads_that_the_view_does_not_choose_keep_no_lease() {
    let leasing = test_theories::identity(TestBehavior::Leasing);
    let (host, env) = space_of(SpaceInner::PriorityQueue, Some(leasing)).await;
    let b = string("b");
    park_peeks(&env, &b, 7, FIRE_ROUND_CAP, "k").await;
    run(&env, receive_exactly(&b, 8, true, "z"))
        .await
        .expect("z parks");
    park_peeks(&env, &b, 8, FIRE_ROUND_CAP - 2, "p").await;
    run(&env, receive_exactly(&b, 8, false, "c"))
        .await
        .expect("c parks");
    run(&env, receive_exactly(&b, 5, false, "a"))
        .await
        .expect("a parks");
    let lane = with_param(
        receive_exactly(&b, 5, false, "l"),
        record::PRIORITY_KEY,
        int(0),
    );
    run(&env, lane)
        .await
        .expect("l parks in the lane of level 0");
    for (value, tag) in [(8, "b8"), (5, "b5")] {
        let stored = with_param(
            send(&b, vec![int(value)], tag),
            record::PRIORITY_KEY,
            int(0),
        );
        run(&env, stored).await.expect("no lane head takes it");
    }
    let capped = run(&env, send(&b, vec![int(7)], "b7"))
        .await
        .expect("64 peek fires");
    assert_eq!(capped.fires.len(), FIRE_ROUND_CAP);
    assert_eq!(
        capped.reissue.map(|reissue| reissue.cause),
        Some(ReissueCause::RoundCap)
    );
    let outcome = run(&env, receive_exactly(&b, 9, false, "r"))
        .await
        .expect("no lease for a head that the view does not choose");
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    assert_eq!(
        outcome.reissue.map(|reissue| reissue.cause),
        Some(ReissueCause::RoundCap)
    );
    assert_eq!(
        (records(&host, &b).await, waiters(&host, &[&b]).await),
        (1, 2)
    );
}

/// Added by decision 124 (L6): the re-plan of the priority-queue view (row 8, SET:409-414), in a
/// regression test, not a red-first test. A round that re-plans makes two stored fires, so it
/// holds two leases after L6 too. The send of 7 makes 64 peek fires, and the cap re-issues it
/// before its settle, so `z` heads the lane `Any`. The receive `r` settles first. `z` lifts the
/// stored 8 of level 1. With 8 in flight the view plans `x` on the stored 5 of level 0, so the
/// settle lands 8 and plans again, and `x` fires on 5. The lift and that fire each hold a lease,
/// so the receive fails with `NestedCommitGuard` before and after L6.
#[tokio::test]
async fn priority_queue_replan_holds_the_leases_of_its_two_fires() {
    let leasing = test_theories::identity(TestBehavior::Leasing);
    let (host, env) = space_of(SpaceInner::PriorityQueue, Some(leasing)).await;
    let b = string("b");
    park_peeks(&env, &b, 7, FIRE_ROUND_CAP, "k").await;
    run(&env, receive_exactly(&b, 8, true, "z"))
        .await
        .expect("z parks");
    run(&env, receive_exactly(&b, 5, false, "x"))
        .await
        .expect("x parks");
    for (value, level, tag) in [(8, 1, "b8"), (5, 0, "b5")] {
        let stored = with_param(
            send(&b, vec![int(value)], tag),
            record::PRIORITY_KEY,
            int(level),
        );
        run(&env, stored).await.expect("no lane head takes it");
    }
    run(&env, send(&b, vec![int(7)], "b7"))
        .await
        .expect("64 peek fires");
    assert_eq!(
        run(&env, receive_exactly(&b, 9, false, "r")).await,
        Err(SpaceError::NestedCommitGuard),
        "the lift of 8 and the fire of x on 5"
    );
    assert_eq!(
        (records(&host, &b).await, waiters(&host, &[&b]).await),
        (2, 2)
    );
}

/// Added by decision 124 (L6): a VectorDB space whose theory is `identity`, over a fresh play
/// space (`create_with`, `vdb_config_of`).
async fn vdb_space_of(identity: Option<TheoryIdentity>) -> (ParRSpace, SpaceEnv) {
    let (host, env) = fresh().await;
    create_with(&env, SpaceInner::VectorDb, vdb_config_of(identity))
        .await
        .expect("the creation commits");
    host.take_event_log().await;
    (host, env)
}

/// Added by decision 124 (L6): the pattern of a list of `count` free variables. It binds the
/// list data of a VectorDB list fire with exactly `count` members (vectordb.rs `list_data`).
fn list_of_free(count: usize) -> BindPattern {
    use models::rust::utils::new_elist_par;
    let mut items = Vec::<Par>::with_capacity(count);
    for level in 0..count {
        let level = i32::try_from(level).expect("a small level");
        items.push(new_freevar_par(level, Vec::new()));
    }
    BindPattern {
        patterns: vec![new_elist_par(
            items,
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        )],
        remainder: None,
        free_count: i32::try_from(count).expect("a small count"),
    }
}

/// Added by decision 124 (L6): a list receive of a VectorDB space on `channel`, with the pattern
/// `pattern` and the query `key = value`, where `key` is `k` or `results` (VDB:395-399).
fn list_receive(channel: &Par, pattern: BindPattern, key: &str, value: Par, name: &str) -> SpaceOp {
    let SpaceOp::Consume(op) = with_param(receive(channel, name), key, value) else {
        panic!("receive gives a consume")
    };
    SpaceOp::Consume(ConsumeOp { pattern, ..op })
}

/// Added by decision 124 (L6): a regression test of row 16, the check of the stored members of a
/// VectorDB offer (`first_offer_target`), which drops no proven check. The list waiter `w` binds
/// two members, so the send of 1 is stored. The send of 2 fires `w` on 1 and 2, with the stored 1
/// as the other member. Its lease joins the lease of the value of the send: before L6 at the check,
/// and after L6 at the fire. The send fails with `NestedCommitGuard` both times.
#[tokio::test]
async fn vdb_offer_holds_the_lease_of_its_stored_member() {
    use models::rust::reified::validation::K_KEY;
    let leasing = test_theories::identity(TestBehavior::Leasing);
    let (host, env) = vdb_space_of(Some(leasing)).await;
    let b = string("b");
    run(&env, list_receive(&b, list_of_free(2), K_KEY, int(2), "w"))
        .await
        .expect("w parks");
    run(&env, send(&b, vec![int(1)], "b1"))
        .await
        .expect("one member does not bind w");
    assert_eq!(
        run(&env, send(&b, vec![int(2)], "b2")).await,
        Err(SpaceError::NestedCommitGuard),
        "the value of the send and the stored member"
    );
    assert_eq!(
        (records(&host, &b).await, waiters(&host, &[&b]).await),
        (1, 1)
    );
}

/// Added by decision 124 (L6): a regression test of row 17, the check of the members of a VectorDB
/// list receive (`receive_plan`), which drops no proven check. A receive of one stored member holds
/// one lease and commits. A receive of two stored members holds two leases and fails with
/// `NestedCommitGuard`: before L6 at the check, and after L6 at the fire.
#[tokio::test]
async fn vdb_list_receive_holds_one_lease_for_each_member() {
    use models::rust::reified::validation::K_KEY;
    let leasing = test_theories::identity(TestBehavior::Leasing);
    let (host, env) = vdb_space_of(Some(leasing)).await;
    let b = string("b");
    run(&env, send(&b, vec![int(1)], "b1"))
        .await
        .expect("one lease");
    let fired = run(&env, list_receive(&b, any(), K_KEY, int(1), "one"))
        .await
        .expect("one member gives one lease");
    assert_eq!(fired.fires.len(), 1);
    for (value, tag) in [(2, "b2"), (3, "b3")] {
        run(&env, send(&b, vec![int(value)], tag))
            .await
            .expect("one lease");
    }
    assert_eq!(
        run(&env, list_receive(&b, any(), K_KEY, int(2), "two")).await,
        Err(SpaceError::NestedCommitGuard),
        "two members give two leases"
    );
    assert_eq!(
        (records(&host, &b).await, waiters(&host, &[&b]).await),
        (2, 0)
    );
}

/// Added by decision 124 (L6): a regression test of row 18, the check of the cascade of a VectorDB
/// space (`first_cascade_target`). Its red-first form is out of reach: a send reaches the cascade
/// only after a fire retires a stored member (`tail.removed |= retired` in `offer_vdb`, and the
/// `tail.removed` test of `run_tail`), and that fire holds a lease too (row 16). Here the receive
/// `taker` takes one of the stored 1 and 2, and its removal lets the list waiter `w` of exactly one
/// member fire on the other one in the cascade. The receive and the cascade each hold a lease:
/// before L6 at their checks, and after L6 at their fires. The receive fails with
/// `NestedCommitGuard` both times.
#[tokio::test]
async fn vdb_cascade_holds_the_lease_of_its_fire() {
    use models::rust::reified::validation::{RESULTS_ALL, RESULTS_KEY};
    let leasing = test_theories::identity(TestBehavior::Leasing);
    let (host, env) = vdb_space_of(Some(leasing)).await;
    let b = string("b");
    for (value, tag) in [(1, "b1"), (2, "b2")] {
        run(&env, send(&b, vec![int(value)], tag))
            .await
            .expect("one lease");
    }
    let all = string(RESULTS_ALL);
    run(
        &env,
        list_receive(&b, list_of_free(1), RESULTS_KEY, all, "w"),
    )
    .await
    .expect("two members do not bind w");
    assert_eq!(
        run(&env, receive(&b, "taker")).await,
        Err(SpaceError::NestedCommitGuard),
        "the receive and the cascade"
    );
    assert_eq!(
        (records(&host, &b).await, waiters(&host, &[&b]).await),
        (2, 1)
    );
}

/// Added by decision 124 (L6, verify-join-l6.md Q3): the pattern of a send of no values.
fn nothing() -> BindPattern {
    BindPattern {
        patterns: Vec::new(),
        remainder: None,
        free_count: 0,
    }
}

/// Added by decision 124 (L6, verify-join-l6.md Q3): a regression test of row 15, the lease of
/// the List arm of `drain_and_cascade` (`Fireable::List`), which no other test runs. The join `j`
/// of `d` and `b` puts both collections in the join engine. The list waiter `w` binds a list of
/// `members` values, so it refuses the list of the members of `b` while the stored 5 is there.
/// The persistent send of no values on `d` takes no admission lease. It fires the 63 receives of
/// no values and `j`, in any order, and `j` takes the stored 5 with the one lease of the
/// transaction. Fire 64 stops the transaction before its cascade, so the space owes the fire of
/// `w`. The settle job holds no admission lease, and its cascade fires `w` through the List arm.
/// One stored member gives one lease and commits. Two stored members give two leases and fail
/// with `NestedCommitGuard`: before L6 at the check, and after L6 at the fire.
#[tokio::test]
async fn vdb_join_cascade_list_holds_one_lease_for_each_member() {
    use models::rust::reified::validation::{RESULTS_ALL, RESULTS_KEY};
    let leasing = test_theories::identity(TestBehavior::Leasing);
    for members in [1_usize, 2] {
        let (host, env) = vdb_space_of(Some(leasing.clone())).await;
        let (b, d) = (string("b"), string("d"));
        run(&env, send(&b, vec![int(5)], "b5"))
            .await
            .expect("one lease");
        for value in 1..=members {
            let tag = format!("b{value}");
            let value = i64::try_from(value).expect("a small value");
            run(&env, send(&b, vec![int(value)], &tag))
                .await
                .expect("one lease");
        }
        let all = string(RESULTS_ALL);
        run(
            &env,
            list_receive(&b, list_of_free(members), RESULTS_KEY, all, "w"),
        )
        .await
        .expect("the list of every member does not bind w");
        let SpaceOp::Join(mut op) = join(&[&d, &b], "j") else {
            panic!("join gives a join")
        };
        for (bind, pattern) in op.binds.iter_mut().zip([nothing(), exactly(5)]) {
            bind.pattern = pattern;
        }
        run(&env, SpaceOp::Join(op)).await.expect("j parks");
        for index in 1..FIRE_ROUND_CAP {
            let SpaceOp::Consume(op) = receive(&d, &format!("r{index}")) else {
                panic!("receive gives a consume")
            };
            let op = SpaceOp::Consume(ConsumeOp {
                pattern: nothing(),
                ..op
            });
            run(&env, op).await.expect("a receive parks");
        }
        let SpaceOp::Produce(op) = send(&d, Vec::new(), "d") else {
            panic!("send gives a produce")
        };
        let forever = SpaceOp::Produce(ProduceOp {
            persistent: true,
            ..op
        });
        let capped = run(&env, forever).await.expect("the one lease of j");
        assert_eq!(capped.fires.len(), FIRE_ROUND_CAP);
        let reissue = capped.reissue.expect("fire 64 stops the transaction");
        assert_eq!(reissue.cause, ReissueCause::RoundCap);
        assert!(
            matches!(&reissue.op, SpaceOp::Settle(settle) if settle.cascade.is_some()),
            "the owed cascade re-issues a settle job"
        );
        assert_eq!(
            (records(&host, &b).await, waiters(&host, &[&b]).await),
            (members, 1)
        );
        let settled = run(&env, reissue.op).await;
        match members {
            1 => {
                let outcome = settled.expect("one member gives one lease");
                assert_eq!(outcome.fires.len(), 1, "the List fire of w");
                assert_eq!(
                    (records(&host, &b).await, waiters(&host, &[&b]).await),
                    (0, 0)
                );
            }
            _ => {
                assert_eq!(
                    settled,
                    Err(SpaceError::NestedCommitGuard),
                    "two members give two leases"
                );
                assert_eq!(
                    (records(&host, &b).await, waiters(&host, &[&b]).await),
                    (2, 1)
                );
            }
        }
    }
}

/// TH-6: a stored datum that the theory refutes at a match is a guard refusal. A bag receive
/// parks, and a later send fires the parked waiter, because its datum is in flight. The head
/// of a queue blocks. A join parks, and a send to one of its channels stays stored, because the
/// stored datum of the other channel is refused.
#[tokio::test]
async fn space_guard_match_refusal_is_a_guard_refusal() {
    let refuting = test_theories::identity(TestBehavior::RefuteMatch);
    let channel = string("c");

    let (host, env) = space_of(SpaceInner::Bag, Some(refuting.clone())).await;
    run(&env, send(&channel, vec![int(1)], "stored"))
        .await
        .expect("admitted");
    let parked = run(&env, receive(&channel, "w"))
        .await
        .expect("the receive parks");
    assert!(parked.fires.is_empty());
    assert_eq!(records(&host, &channel).await, 1);
    assert_eq!(waiters(&host, &[&channel]).await, 1);
    let fired = run(&env, send(&channel, vec![int(2)], "in flight"))
        .await
        .expect("the datum in flight fires the waiter");
    assert_eq!(fired.fires.len(), 1);
    assert_eq!(records(&host, &channel).await, 1, "the refused datum stays");
    assert_eq!(waiters(&host, &[&channel]).await, 0);

    let (host, env) = space_of(SpaceInner::Queue, Some(refuting.clone())).await;
    run(&env, send(&channel, vec![int(1)], "head"))
        .await
        .expect("admitted");
    run(&env, receive(&channel, "w"))
        .await
        .expect("the receive parks");
    run(&env, send(&channel, vec![int(2)], "tail"))
        .await
        .expect("stored behind the head");
    assert_eq!(records(&host, &channel).await, 2, "the refused head blocks");
    assert_eq!(waiters(&host, &[&channel]).await, 1);

    let (a, b) = (string("a"), string("b"));
    let (host, env) = space_of(SpaceInner::Bag, Some(refuting)).await;
    run(&env, send(&b, vec![int(2)], "b stored"))
        .await
        .expect("admitted");
    let parked = run(&env, join(&[&a, &b], "joined"))
        .await
        .expect("the join parks");
    assert!(parked.fires.is_empty());
    let stored = run(&env, send(&a, vec![int(1)], "a in flight"))
        .await
        .expect("the send is stored");
    assert!(stored.fires.is_empty(), "the stored datum of b is refused");
    assert_eq!(records(&host, &a).await, 1);
    assert_eq!(records(&host, &b).await, 1);
    assert_eq!(join_waiters(&host, &a, &b).await, 1);

    let (host, env) = space_of(SpaceInner::Bag, None).await;
    run(&env, send(&b, vec![int(2)], "b stored"))
        .await
        .expect("stored");
    run(&env, join(&[&a, &b], "joined"))
        .await
        .expect("the join parks");
    let fired = run(&env, send(&a, vec![int(1)], "a in flight"))
        .await
        .expect("the join fires");
    assert_eq!(fired.fires.len(), 1, "without a theory the same join fires");
    assert_eq!(records(&host, &b).await, 0);
}

/// TH-3 and TH-7: each config read resolves the identity of the theory. An identity that names
/// no checker of this build is a valid identity map for the strict decoder, which reads content
/// only. The config read (`SpaceConfig::from_par_with_theory`), the creation of a space and every
/// operation on a space whose stored config names it fail with `TheoryUnavailable`. They stage
/// nothing.
#[tokio::test]
async fn theory_unavailable_fails_each_config_read() {
    for identity in [
        TheoryIdentity::builtin("Real"),
        TheoryIdentity {
            checker_abi: "rrs:structural:v2".to_string(),
            ..nat()
        },
        TheoryIdentity {
            checker_abi: test_theories::TEST_ABI.to_string(),
            ..nat()
        },
    ] {
        let expected = SpaceError::TheoryUnavailable(identity.theorem_id.clone());
        let map = config_of(SpaceInner::Bag, Some(identity.clone()));
        assert_eq!(
            SpaceConfig::from_par(&map).map(|config| config.theory),
            Ok(Some(identity.clone()))
        );
        assert_eq!(
            SpaceConfig::from_par_with_theory(&map).err(),
            Some(expected.clone())
        );
        let (host, env) = fresh().await;
        assert_eq!(
            create(&env, SpaceInner::Bag, Some(identity.clone())).await,
            Err(expected.clone())
        );
        assert!(
            host.take_event_log().await.is_empty(),
            "the creation stages nothing"
        );
        // A config that another build stored: the record is in the store, and each operation
        // reads it first.
        host.produce(
            namespace::gprivate(namespace::config_channel(SPACE)),
            record::config_record(map, &Blake2b512Random::create_from_bytes(b"stored config")),
            true,
        )
        .await
        .expect("the config record is stored");
        host.take_event_log().await;
        let channel = string("c");
        for op in [send(&channel, vec![int(1)], "s"), receive(&channel, "w")] {
            assert_eq!(run(&env, op).await, Err(expected.clone()));
        }
        assert_eq!(records(&host, &channel).await, 0);
        assert_eq!(waiters(&host, &[&channel]).await, 0);
        assert!(
            host.take_event_log().await.is_empty(),
            "no operation stages anything"
        );
    }
}

/// Decision 74: the config record holds the identity as one `GByteArray` of its canonical bytes.
#[test]
fn theory_config_slot_is_one_compact_par() {
    use models::rhoapi::expr::ExprInstance;

    use crate::rust::interpreter::spaces::types::config::{map_entries, THEORY_KEY};

    for name in super::builtin::BUILTIN_NAMES {
        let identity = TheoryIdentity::builtin(name);
        let map = config_of(SpaceInner::Bag, Some(identity.clone()));
        SpaceConfig::from_par_with_theory(&map).expect("a config");
        let entries = map_entries(&map).expect("a config map");
        let slot = entries.get(THEORY_KEY).expect("a theory slot");
        assert_eq!(
            slot.exprs.as_slice(),
            [models::rhoapi::Expr {
                expr_instance: Some(ExprInstance::GByteArray(identity.to_bytes())),
            }],
            "{name}"
        );
    }
}
