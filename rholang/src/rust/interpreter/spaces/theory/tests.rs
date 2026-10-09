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
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;

use super::identity::TheoryIdentity;
use super::test_theories::{self, TestBehavior};
use super::TheoryUsage;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::agent::{
    run_op, ConsumeOp, JoinBind, JoinOp, ProduceOp, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp,
    SpaceOutcome,
};
use crate::rust::interpreter::spaces::errors::SpaceError;
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
        meter: ScanMeter::new(None),
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
    let family = SpaceFactoryUrn::all()
        .find(|urn| urn.outer == SpaceOuter::HashMap && urn.inner == inner)
        .expect("a factory family");
    let config = config_of(inner, identity);
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
    assert!(
        host.take_event_log().await.is_empty(),
        "the guard aborts before the commit"
    );
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
/// only. The metered config read, the creation of a space and every operation on a space whose
/// stored config names it fail with `TheoryUnavailable`, and they stage nothing.
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
            SpaceConfig::from_par_metered(&map, &ScanMeter::new(None)).err(),
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

/// Decision 74: the config record holds the identity as one `GByteArray` of its canonical bytes,
/// and a metered config read of a space with a builtin theory costs less than a tenth more than
/// the read of the same space without one. The TH-2 map in the record cost 169 % more, on every
/// operation.
#[test]
fn theory_config_slot_is_one_compact_par() {
    use models::rhoapi::expr::ExprInstance;
    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};

    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::spaces::types::config::{map_entries, THEORY_KEY};

    let read = |identity: Option<TheoryIdentity>| {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1 << 50)));
        let map = config_of(SpaceInner::Bag, identity);
        SpaceConfig::from_par_metered(&map, &ScanMeter::new(Some(budget.clone())))
            .expect("a config");
        (
            budget.usage(HostWorkDimension::VerificationBytes).get(),
            map,
        )
    };
    let (plain, _) = read(None);
    for name in super::builtin::BUILTIN_NAMES {
        let identity = TheoryIdentity::builtin(name);
        let (with_theory, map) = read(Some(identity.clone()));
        let entries = map_entries(&map).expect("a config map");
        let slot = entries.get(THEORY_KEY).expect("a theory slot");
        assert_eq!(
            slot.exprs.as_slice(),
            [models::rhoapi::Expr {
                expr_instance: Some(ExprInstance::GByteArray(identity.to_bytes())),
            }],
            "{name}"
        );
        assert!(
            (with_theory - plain) * 10 < plain,
            "{name}: {with_theory} against {plain}"
        );
    }
}
