//! The VectorDB discipline of the native spaces (P6, design v3 4.2-4.6): the agent
//! `GenericSpace<HashMapOuter, VectorDbDiscipline>` runs directly over an in-memory play space
//! and its replay space.
//!
//! The example tests cover the selection rules of design v3 4.4 (single mode, list mode,
//! `scores`, the bounds, `ids`, the binary metrics, the payload-as-vector default), the error
//! codes of a send and of a query, the capacity of a collection, Rule E with the embedding of each
//! record, the LM-b counterexamples CE-1 and CE-2 of the arbitration, the carries at the round
//! cap (the restores of a list peek and LIST-CASCADE), joins over VectorDB collections and
//! `ListModeInJoin`, and replay.
//!
//! A collection with a join group runs in the join engine (`spaces::join`), and every other
//! collection runs in the driver of `disciplines::vectordb`. [`Rig::joined`] parks a join whose
//! patterns refuse every record, so every operation of its collection runs in the engine. The
//! properties run each program through both, and they check:
//!
//! | Property | Statement |
//! |---|---|
//! | `prop_vectordb_driver_and_engine_agree` | the driver and the engine fire the same continuations with the same values, and leave the same records and waiters |
//! | `prop_vectordb_no_pending_match` | after each operation of a program without guards, no parked waiter can fire on the records |
//! | `prop_vectordb_list_is_ordered_and_complete` | a list holds `min(k, |E|)` values, and its delivered scores follow the selection order |
//! | `prop_vectordb_replay_equal` | replay stages the transactions of play: equal outcomes, every COMM paired, the root of play |
//! | `prop_vectordb_transactions_are_bounded` | a transaction fires at most `FIRE_ROUND_CAP` times and re-issues at the round cap only after exactly that many fires, and a flood of `n` records or waiters drains in `n` fires and `n / FIRE_ROUND_CAP` round-cap re-issues |
//! | `prop_vectordb_records_are_conserved` | every record that leaves the collection was delivered by a fire of a waiter that does not peek, so no record leaves when every receive peeks (Rule E) |

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{BindPattern, CostAuthority, EEq, EVar, Expr, ListParWithRandom, Par, Var};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::{
    new_elist_par, new_freevar_par, new_gbigrat_expr, new_gbool_par, new_gbytearray_par,
    new_gfixedpoint_expr,
};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::reified::costs::{ReissueCause, FIRE_ROUND_CAP};
use rholang::rust::interpreter::spaces::agent::{
    ConsumeOp, JoinBind, JoinOp, ProduceOp, QueryParams, SpaceEnv, SpaceOp, SpaceOutcome,
};
use rholang::rust::interpreter::spaces::disciplines::vectordb::VectorDbDiscipline;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::generic::GenericSpace;
use rholang::rust::interpreter::spaces::namespace::{Durability, NativeSpace, SID_LEN};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::spaces::record::{self, WaiterPolicy};
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rspace_plus_plus::rspace::checkpoint::Checkpoint;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::trace::event::{Event, Produce, COMM};
use support::agent::{
    any, body_name, body_of, budget, create, creation, data_channel, env_over, exactly, fired, int,
    key_of, play, play_and_replay, receive_on, run, string, ParRSpace,
};
use support::props::{check, pure_config, tokio_runtime};
use support::session::{setup_source, with_space, Injections, Session};

// ─── The rig ─────────────────────────────────────────────────────────────────────────────

/// The VectorDB agent.
type VectorDb = GenericSpace<HashMapOuter, VectorDbDiscipline>;

/// The space of every rig.
const SPACE: NativeSpace = NativeSpace {
    sid: [26; SID_LEN],
    durability: Durability::Durable,
};

/// The family `hashmap:vectordb`.
fn vdb_family() -> SpaceFactoryUrn {
    SpaceFactoryUrn {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::VectorDb,
    }
}

/// The channel of the collection of every rig.
fn channel() -> Par { string("rrs-vdb") }

/// The channel of the second collection of a join.
fn other() -> Par { string("rrs-vdb-other") }

/// The data channel `D(sid,k)` of the collection.
fn data() -> Par { data_channel(SPACE, &channel()) }

/// The data channel of the second collection.
fn other_data() -> Par { data_channel(SPACE, &other()) }

/// The params of a VectorDB space.
#[derive(Clone, Copy, Debug)]
struct Shape {
    capacity: i64,
    dimensions: i64,
    metric: &'static str,
}

/// A cosine space of two dimensions with room for every program of the suite.
const COSINE: Shape = Shape {
    capacity: 100,
    dimensions: 2,
    metric: "cosine",
};

/// The config map of a VectorDB space with the params of `shape`.
fn config(shape: Shape) -> Par {
    let mut config = SpaceConfig::new(SPACE.sid, "hashmap", "vectordb", SpaceQualifier::Default);
    config
        .params
        .insert("capacity".to_string(), int(shape.capacity));
    config
        .params
        .insert("dimensions".to_string(), int(shape.dimensions));
    config
        .params
        .insert("metric".to_string(), string(shape.metric));
    config.to_par()
}

/// A VectorDB space over a fresh in-memory play space.
struct Rig {
    host: ParRSpace,
    env: SpaceEnv,
}

impl Rig {
    /// A rig of a space with the params of `shape`.
    async fn new(shape: Shape) -> Rig { Rig::over(play().await, shape).await }

    /// A rig over `host`, a fresh space.
    async fn over(host: ParRSpace, shape: Shape) -> Rig {
        let env = env_over(host.clone(), &budget());
        create(
            &VectorDb::default(),
            &env,
            creation(SPACE, config(shape), vdb_family()),
        )
        .await;
        host.take_event_log().await;
        Rig { host, env }
    }

    /// A rig whose collection runs in the join engine: a join over the collection and a second
    /// collection parks, and its patterns refuse every record of the suite (a payload of the
    /// suite is never the integer -1).
    async fn joined(shape: Shape) -> Rig {
        let rig = Rig::new(shape).await;
        let join = JoinOp {
            space: SPACE,
            binds: vec![
                bind(&channel(), exactly(-1), None),
                bind(&other(), exactly(-1), None),
            ],
            body: body_of("never"),
            persistent: false,
            peek: false,
            guard: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ords: None,
        };
        let outcome = rig.run(SpaceOp::Join(join)).await.expect("the join parks");
        assert!(outcome.fires.is_empty());
        rig.host.take_event_log().await;
        rig
    }

    async fn run(&self, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run(&VectorDb::default(), &self.env, op).await
    }

    /// Runs `op` and each re-issue that it leaves, and returns the outcome of each transaction
    /// in order.
    async fn run_chain(&self, op: SpaceOp) -> Result<Vec<SpaceOutcome>, SpaceError> {
        let mut chain = Vec::new();
        let mut next = Some(op);
        while let Some(op) = next.take() {
            assert!(chain.len() < 64, "a program of the suite settles");
            let outcome = self.run(op).await?;
            next = outcome.reissue.as_ref().map(|reissue| reissue.op.clone());
            chain.push(outcome);
        }
        Ok(chain)
    }

    /// Runs `op` and each re-issue that it leaves, and returns every fire in order and the
    /// causes of the re-issues.
    async fn run_all(
        &self,
        op: SpaceOp,
    ) -> Result<(Vec<(String, Vec<Par>)>, Vec<ReissueCause>), SpaceError> {
        let chain = self.run_chain(op).await?;
        let fires = chain.iter().flat_map(fired).collect();
        let causes = chain
            .iter()
            .filter_map(|outcome| outcome.reissue.as_ref().map(|reissue| reissue.cause))
            .collect();
        Ok((fires, causes))
    }

    async fn send(&self, op: ProduceOp) -> SpaceOutcome {
        self.run(SpaceOp::Produce(op))
            .await
            .expect("the send commits")
    }

    async fn receive(&self, op: ConsumeOp) -> SpaceOutcome {
        self.run(SpaceOp::Consume(op))
            .await
            .expect("the receive commits")
    }

    /// The records of the collection, by source hash.
    async fn records(&self) -> Vec<Stored> { records_of(&self.host, &data()).await }

    /// The records of the collection with the hash of their source produce, by that hash.
    async fn sourced_records(&self) -> Vec<(Blake2b256Hash, Stored)> {
        sourced_records_of(&self.host, &data()).await
    }

    /// The body names of the waiters of the collection, sorted.
    async fn waiters(&self) -> Vec<String> { waiter_names(&self.host, &data()).await }

    /// The events since the last call.
    async fn log(&self) -> Vec<Event> { self.host.take_event_log().await }
}

/// A stored record as a test reads it: its payload, its embedding and its persist flag.
type Stored = (Vec<Par>, Option<Par>, bool);

/// The records on the data channel `data`, by source hash.
async fn records_of(space: &ParRSpace, data: &Par) -> Vec<Stored> {
    sourced_records_of(space, data)
        .await
        .into_iter()
        .map(|(_, stored)| stored)
        .collect()
}

/// The records on the data channel `data` with the hash of their source produce, by that hash.
async fn sourced_records_of(space: &ParRSpace, data: &Par) -> Vec<(Blake2b256Hash, Stored)> {
    let mut records: Vec<(Blake2b256Hash, Stored)> = space
        .get_data(data)
        .await
        .into_iter()
        .map(|datum| {
            let record = record::unwrap_vdb_datum(&datum.a).expect("a VectorDB record");
            assert_eq!(
                record.ord, 0,
                "every record of a VectorDB collection has the ord 0"
            );
            (
                datum.source.hash.clone(),
                (record.datum.pars, record.embedding, datum.persist),
            )
        })
        .collect();
    records.sort_by(|left, right| left.0.cmp(&right.0));
    records
}

/// The body names of the waiters on the data channel `data`, sorted.
async fn waiter_names(space: &ParRSpace, data: &Par) -> Vec<String> {
    let mut names: Vec<String> = space
        .get_waiting_continuations(vec![data.clone()])
        .await
        .iter()
        .map(|waiter| {
            let parked = record::unwrap_policy_continuation(&waiter.continuation)
                .expect("a parked VectorDB waiter");
            assert_eq!(
                parked.ord, 0,
                "every waiter of a VectorDB collection has the ord 0"
            );
            body_name(&parked.continuation)
        })
        .collect();
    names.sort();
    names
}

// ─── Values, sends and receives ───────────────────────────────────────────────────────────

/// The float `value` as a `Par`.
fn double(value: f64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GDouble(value.to_bits())),
    }])
}

/// The Boolean `value` as a `Par`.
fn boolean(value: bool) -> Par { new_gbool_par(value, Vec::new(), false) }

/// The ground list of `values`.
fn list(values: Vec<Par>) -> Par {
    new_elist_par(values, Vec::new(), false, None, Vec::new(), false)
}

/// The vector of the floats `values`.
fn vector(values: &[f64]) -> Par { list(values.iter().map(|value| double(*value)).collect()) }

/// The vector of the integers `values`.
fn ints(values: &[i64]) -> Par { list(values.iter().map(|value| int(*value)).collect()) }

/// A send of `datum` with the params `params`, whose random state comes from `salt`.
fn send(datum: Par, params: &[(&str, Par)], salt: &str, persistent: bool) -> ProduceOp {
    ProduceOp {
        space: SPACE,
        key: key_of(&channel()),
        channel: channel(),
        data: ListParWithRandom {
            pars: vec![datum],
            random_state: Blake2b512Random::create_from_bytes(
                format!("rrs-vdb-send {salt}").as_bytes(),
            )
            .to_bytes(),
            cost_authority: None,
            cost_stack: None,
        },
        persistent,
        params: params
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
        authority: CostAuthority::default(),
        reissued_ord: None,
    }
}

/// A send of the vector `values` as its own embedding (decision 66).
fn vector_send(values: &[f64], salt: &str) -> ProduceOp { send(vector(values), &[], salt, false) }

/// A send of the string `name` with the embedding `embedding`.
fn doc_send(name: &str, embedding: &[f64], salt: &str) -> ProduceOp {
    send(
        string(name),
        &[("embedding", vector(embedding))],
        salt,
        false,
    )
}

/// `op` on the second collection.
fn on_other(op: ProduceOp) -> ProduceOp {
    ProduceOp {
        key: key_of(&other()),
        channel: other(),
        ..op
    }
}

/// The query with the vector `vector` and the named params `named`.
fn query(vector: Option<Par>, named: &[(&str, Par)]) -> QueryParams {
    QueryParams {
        payload: vector.into_iter().collect(),
        named: named
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    }
}

/// A receive whose body is the string `name`, with the pattern `pattern` and the query `query`.
fn receive(name: &str, pattern: BindPattern, query: Option<QueryParams>) -> ConsumeOp {
    ConsumeOp {
        query,
        ..receive_on(SPACE, &channel(), name, pattern)
    }
}

/// `op` as a peek (`<<-`).
fn peeking(op: ConsumeOp) -> ConsumeOp { ConsumeOp { peek: true, ..op } }

/// `op` with the guard `guard`.
fn guarded(op: ConsumeOp, guard: Par) -> ConsumeOp {
    ConsumeOp {
        guard: Some(guard),
        ..op
    }
}

/// `op` with the random state of its body taken from `salt`. The body stays the same.
fn resalted(op: ConsumeOp, salt: &str) -> ConsumeOp {
    let mut op = op;
    op.body.random_state =
        Blake2b512Random::create_from_bytes(format!("rrs-vdb-receive {salt}").as_bytes())
            .to_bytes();
    op
}

/// A bind of a join on `channel`.
fn bind(channel: &Par, pattern: BindPattern, query: Option<QueryParams>) -> JoinBind {
    JoinBind {
        key: key_of(channel),
        channel: channel.clone(),
        pattern,
        query,
    }
}

/// The pattern `[a, b]`: a list of two values.
fn pair() -> BindPattern {
    BindPattern {
        patterns: vec![new_elist_par(
            vec![
                new_freevar_par(0, Vec::new()),
                new_freevar_par(1, Vec::new()),
            ],
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        )],
        remainder: None,
        free_count: 2,
    }
}

/// The pattern that matches exactly the ground value `value`.
fn literal(value: Par) -> BindPattern {
    BindPattern {
        patterns: vec![value],
        remainder: None,
        free_count: 0,
    }
}

/// The guard `x == value` over the one bound value `x`.
fn equals(value: Par) -> Par {
    let bound = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::BoundVar(0)),
            }),
        })),
    }]);
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EEqBody(EEq {
            p1: Some(bound),
            p2: Some(value),
        })),
    }])
}

/// The id of the record of `op`: the hash of its source produce (design v3 4.4).
fn record_id(op: &ProduceOp) -> Blake2b256Hash {
    let embedding = op
        .params
        .get("embedding")
        .or(match op.data.pars.as_slice() {
            [single] if record_is_vector(single) => Some(single),
            _ => None,
        });
    let record = record::wrap_vdb_datum(0, embedding, &op.data).expect("a VectorDB record");
    Produce::create(&data(), &record, op.persistent).hash
}

/// True when `par` is a non-empty list of numbers, as the payload-as-vector rule reads it.
fn record_is_vector(par: &Par) -> bool {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EListBody(list)),
        }] => {
            !list.ps.is_empty()
                && list.ps.iter().all(|element| {
                    matches!(element.exprs.as_slice(), [Expr {
                        expr_instance: Some(
                            ExprInstance::GInt(_)
                                | ExprInstance::GDouble(_)
                                | ExprInstance::GBool(_)
                        ),
                    }])
                })
        }
        _ => false,
    }
}

/// The fire of the body `name` with the values `values`.
fn delivery(name: &str, values: Vec<Par>) -> (String, Vec<Par>) { (name.to_string(), values) }

/// The COMM events of a log.
fn comms(log: &[Event]) -> Vec<COMM> {
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => Some(comm.clone()),
            Event::IoEvent(_) => None,
        })
        .collect()
}

/// The error code of a failed operation.
async fn code_of(rig: &Rig, op: SpaceOp) -> String {
    rig.run(op)
        .await
        .expect_err("the operation fails")
        .code()
        .to_string()
}

// ─── Selection (design v3 4.4) ───────────────────────────────────────────────────────────

/// Single mode binds the record with the best score.
#[tokio::test]
async fn space_vectordb_single_mode_takes_the_best_record() {
    let rig = Rig::new(COSINE).await;
    rig.send(vector_send(&[0.0, 1.0], "a")).await;
    rig.send(vector_send(&[1.0, 0.0], "b")).await;
    rig.send(vector_send(&[1.0, 1.0], "c")).await;
    let outcome = rig
        .receive(receive(
            "r",
            any(),
            Some(query(Some(vector(&[1.0, 0.25])), &[])),
        ))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("r", vec![vector(&[
        1.0, 0.0
    ])])]);
    assert_eq!(rig.records().await.len(), 2);
    assert!(rig.waiters().await.is_empty());
}

/// `scores = true` delivers `[score, value]` with the correctly rounded exact score of each
/// metric (decision 54).
#[tokio::test]
async fn space_vectordb_scores_are_the_correctly_rounded_exact_scores() {
    for (metric, expected) in [
        ("cosine", 0.6),
        ("dot", 3.0),
        ("euclidean", 20f64.sqrt()),
        ("manhattan", 6.0),
    ] {
        let rig = Rig::new(Shape { metric, ..COSINE }).await;
        rig.send(vector_send(&[3.0, 4.0], "a")).await;
        let outcome = rig
            .receive(receive(
                "r",
                any(),
                Some(query(Some(vector(&[1.0, 0.0])), &[(
                    "scores",
                    boolean(true),
                )])),
            ))
            .await;
        assert_eq!(
            fired(&outcome),
            vec![delivery("r", vec![list(vec![
                double(expected),
                vector(&[3.0, 4.0])
            ])])],
            "{metric}"
        );
    }
}

/// A threshold bounds a similarity and a radius bounds a distance. A receive without an
/// eligible record parks with its query, and a later eligible send fires it.
#[tokio::test]
async fn space_vectordb_bounds_select_the_eligible_records() {
    let rig = Rig::new(COSINE).await;
    rig.send(vector_send(&[0.0, 1.0], "a")).await;
    rig.send(vector_send(&[1.0, 1.0], "b")).await;
    let near = || {
        Some(query(Some(vector(&[1.0, 0.0])), &[(
            "threshold",
            double(0.9),
        )]))
    };
    let outcome = rig.receive(receive("r", any(), near())).await;
    assert!(outcome.fires.is_empty(), "cos 0.707 and 0 are below 0.9");
    assert_eq!(rig.waiters().await, vec!["r".to_string()]);
    let outcome = rig.send(vector_send(&[2.0, 0.5], "c")).await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("r", vec![vector(&[2.0, 0.5])])],
        "cos 0.97 passes the threshold, and the parked waiter decodes after the commit"
    );
    assert!(rig.waiters().await.is_empty());
    assert_eq!(rig.records().await.len(), 2);

    let rig = Rig::new(Shape {
        metric: "euclidean",
        ..COSINE
    })
    .await;
    rig.send(vector_send(&[0.0, 0.0], "a")).await;
    rig.send(vector_send(&[3.0, 4.0], "b")).await;
    let within = |radius: f64| {
        Some(query(Some(vector(&[0.0, 0.0])), &[
            ("radius", double(radius)),
            ("results", string("all")),
        ]))
    };
    let outcome = rig.receive(peeking(receive("r", any(), within(5.0)))).await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("r", vec![list(vec![
            vector(&[0.0, 0.0]),
            vector(&[3.0, 4.0])
        ])])],
        "the radius 5 holds the distance 5"
    );
    let outcome = rig.receive(receive("s", any(), within(4.99))).await;
    assert_eq!(fired(&outcome), vec![delivery("s", vec![list(vec![
        vector(&[0.0, 0.0])
    ])])]);
}

/// A fixed-point threshold and a rational threshold are exact (decision 56, A9).
#[tokio::test]
async fn space_vectordb_fixed_point_and_rational_thresholds() {
    for threshold in [
        Par::default().with_exprs(vec![new_gfixedpoint_expr(vec![85], 2)]),
        Par::default().with_exprs(vec![new_gbigrat_expr(vec![17], vec![20])]),
    ] {
        let rig = Rig::new(COSINE).await;
        // cos([1, 0], [6, 5]) = 0.768 and cos([1, 0], [5, 1]) = 0.981.
        rig.send(vector_send(&[6.0, 5.0], "a")).await;
        rig.send(vector_send(&[5.0, 1.0], "b")).await;
        let outcome = rig
            .receive(receive(
                "r",
                any(),
                Some(query(Some(vector(&[1.0, 0.0])), &[
                    ("threshold", threshold.clone()),
                    ("k", int(2)),
                ])),
            ))
            .await;
        assert_eq!(fired(&outcome), vec![delivery("r", vec![list(vec![
            vector(&[5.0, 1.0])
        ])])]);
    }
}

/// List mode binds ONE value: the list of the first `k` eligible payload values in selection
/// order, in one COMM over `[D; m]` (design v3 4.4, A11).
#[tokio::test]
async fn space_vectordb_list_mode_delivers_one_list_in_selection_order() {
    let rig = Rig::new(COSINE).await;
    rig.send(vector_send(&[0.0, 1.0], "a")).await;
    rig.send(vector_send(&[1.0, 0.0], "b")).await;
    rig.send(vector_send(&[1.0, 1.0], "c")).await;
    rig.log().await;
    let outcome = rig
        .receive(receive(
            "r",
            any(),
            Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(2))])),
        ))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("r", vec![list(vec![
        vector(&[1.0, 0.0]),
        vector(&[1.0, 1.0])
    ])])]);
    let comms = comms(&rig.log().await);
    assert_eq!(comms.len(), 1, "one COMM");
    assert_eq!(comms[0].consume.channel_hashes.len(), 2, "over [D; 2]");
    assert_eq!(comms[0].produces.len(), 2);
    assert!(comms[0].peeks.is_empty());
    assert_eq!(rig.records().await, vec![(
        vec![vector(&[0.0, 1.0])],
        Some(vector(&[0.0, 1.0])),
        false
    )]);
    let outcome = rig
        .receive(receive(
            "s",
            any(),
            Some(query(Some(vector(&[1.0, 0.0])), &[
                ("results", string("all")),
                ("scores", boolean(true)),
            ])),
        ))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("s", vec![list(vec![
        list(vec![double(0.0), vector(&[0.0, 1.0])])
    ])])]);
    assert!(rig.records().await.is_empty());
}

/// Without a query vector, a receive reads the records in the canonical order of DR-75: a
/// plain receive binds the first record whose payload binds, and `k` lists the first records.
#[tokio::test]
async fn space_vectordb_reads_without_a_vector_follow_the_canonical_order() {
    let rig = Rig::new(COSINE).await;
    let sends = [
        vector_send(&[0.0, 1.0], "a"),
        vector_send(&[1.0, 0.0], "b"),
        vector_send(&[1.0, 1.0], "c"),
    ];
    for op in &sends {
        rig.send(op.clone()).await;
    }
    let mut canonical: Vec<&ProduceOp> = sends.iter().collect();
    canonical.sort_by_key(|op| record_id(op));
    let value = |op: &ProduceOp| op.data.pars[0].clone();
    let outcome = rig
        .receive(peeking(receive(
            "k",
            any(),
            Some(query(None, &[("k", int(2))])),
        )))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("k", vec![list(vec![
        value(canonical[0]),
        value(canonical[1])
    ])])]);
    let outcome = rig.receive(receive("plain", any(), None)).await;
    assert_eq!(fired(&outcome), vec![delivery("plain", vec![value(
        canonical[0]
    )])]);
}

/// The payload is the vector by default (decision 66): a send of one numeric list uses it as its
/// embedding, `embedding` overrides it, and any other payload has no vector, so a query with a
/// vector never selects it.
#[tokio::test]
async fn space_vectordb_payload_is_the_vector_by_default() {
    let rig = Rig::new(COSINE).await;
    rig.send(vector_send(&[3.0, 4.0], "a")).await;
    rig.send(send(ints(&[1, 2]), &[], "b", false)).await;
    rig.send(doc_send("doc", &[1.0, 0.0], "c")).await;
    rig.send(send(string("plain"), &[], "d", false)).await;
    let two = ListParWithRandom {
        pars: vec![vector(&[1.0, 1.0]), vector(&[2.0, 2.0])],
        ..send(int(0), &[], "e", false).data
    };
    rig.send(ProduceOp {
        data: two,
        ..send(int(0), &[], "e", false)
    })
    .await;
    let mut embeddings: Vec<Option<Par>> = rig
        .records()
        .await
        .into_iter()
        .map(|(_, embedding, _)| embedding)
        .collect();
    embeddings.sort_by_key(|embedding| format!("{embedding:?}"));
    let mut expected = vec![
        Some(vector(&[3.0, 4.0])),
        Some(ints(&[1, 2])),
        Some(vector(&[1.0, 0.0])),
        None,
        None,
    ];
    expected.sort_by_key(|embedding| format!("{embedding:?}"));
    assert_eq!(embeddings, expected);
    let outcome = rig
        .receive(receive(
            "all",
            any(),
            Some(query(Some(vector(&[1.0, 0.0])), &[(
                "results",
                string("all"),
            )])),
        ))
        .await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("all", vec![list(vec![
            string("doc"),
            vector(&[3.0, 4.0]),
            ints(&[1, 2])
        ])])],
        "cos 1, 0.6 and 0.447: the records without a vector are not eligible"
    );
    assert_eq!(rig.records().await.len(), 2);
}

/// `ids` keeps only the records whose source hashes it lists (design v3 4.4).
#[tokio::test]
async fn space_vectordb_ids_keep_only_the_listed_records() {
    let rig = Rig::new(COSINE).await;
    let best = vector_send(&[1.0, 0.0], "a");
    let worse = vector_send(&[1.0, 1.0], "b");
    rig.send(best).await;
    rig.send(worse.clone()).await;
    let ids = list(vec![new_gbytearray_par(
        record_id(&worse).bytes(),
        Vec::new(),
        false,
    )]);
    let outcome = rig
        .receive(receive(
            "r",
            any(),
            Some(query(Some(vector(&[1.0, 0.0])), &[("ids", ids)])),
        ))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("r", vec![vector(&[
        1.0, 1.0
    ])])]);
}

/// Hamming and jaccard read binary vectors only, and any other record is not eligible (decision
/// 66). Jaccard of two empty supports is undefined.
#[tokio::test]
async fn space_vectordb_binary_metrics_read_binary_vectors_only() {
    let shape = |metric| Shape {
        dimensions: 3,
        metric,
        ..COSINE
    };
    let rig = Rig::new(shape("hamming")).await;
    rig.send(vector_send(&[1.0, 1.0, 1.0], "a")).await;
    rig.send(vector_send(&[1.0, 0.0, 1.0], "b")).await;
    rig.send(vector_send(&[2.0, 0.0, 1.0], "c")).await;
    let all = |q: &[f64], name: &str| {
        receive(
            name,
            any(),
            Some(query(Some(vector(q)), &[
                ("results", string("all")),
                ("scores", boolean(true)),
            ])),
        )
    };
    let outcome = rig.receive(peeking(all(&[1.0, 0.0, 1.0], "hamming"))).await;
    assert_eq!(fired(&outcome), vec![delivery("hamming", vec![list(
        vec![
            list(vec![double(0.0), vector(&[1.0, 0.0, 1.0])]),
            list(vec![double(1.0), vector(&[1.0, 1.0, 1.0])])
        ]
    )])]);
    let outcome = rig.receive(all(&[2.0, 0.0, 1.0], "nonbinary")).await;
    assert!(
        outcome.fires.is_empty(),
        "a query that is not binary selects nothing"
    );
    assert_eq!(rig.waiters().await, vec!["nonbinary".to_string()]);

    let rig = Rig::new(shape("jaccard")).await;
    rig.send(vector_send(&[1.0, 1.0, 1.0], "a")).await;
    rig.send(vector_send(&[0.0, 0.0, 0.0], "b")).await;
    let outcome = rig.receive(peeking(all(&[1.0, 0.0, 1.0], "jaccard"))).await;
    assert_eq!(fired(&outcome), vec![delivery("jaccard", vec![list(
        vec![
            list(vec![double(2.0 / 3.0), vector(&[1.0, 1.0, 1.0])]),
            list(vec![double(0.0), vector(&[0.0, 0.0, 0.0])])
        ]
    )])]);
    let outcome = rig.receive(all(&[0.0, 0.0, 0.0], "empty")).await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("empty", vec![list(vec![list(vec![
            double(0.0),
            vector(&[1.0, 1.0, 1.0])
        ])])])],
        "the empty supports of the zero query and the zero record have no score"
    );
}

// ─── Errors and capacity ─────────────────────────────────────────────────────────────────

/// Each malformed send and query fails with its code before any staging (design v3 4.3, 4.4).
#[tokio::test]
async fn space_vectordb_errors_have_their_codes() {
    let rig = Rig::new(Shape {
        capacity: 3,
        ..COSINE
    })
    .await;
    rig.send(vector_send(&[1.0, 0.0], "a")).await;
    rig.log().await;
    let sends = [
        (vector_send(&[1.0, 2.0, 3.0], "x"), "DimensionMismatch"),
        (doc_send("doc", &[1.0], "x"), "DimensionMismatch"),
        (vector_send(&[f64::NAN, 0.0], "x"), "NonFiniteValue"),
        (vector_send(&[f64::INFINITY, 0.0], "x"), "NonFiniteValue"),
        (
            send(string("doc"), &[("embedding", string("v"))], "x", false),
            "InvalidParam",
        ),
    ];
    for (op, code) in sends {
        assert_eq!(code_of(&rig, SpaceOp::Produce(op)).await, code);
    }
    let q = || Some(vector(&[1.0, 0.0]));
    let queries: Vec<(QueryParams, &str)> = vec![
        (query(Some(vector(&[0.0, 0.0])), &[]), "ZeroQueryVector"),
        (
            query(Some(vector(&[1.0, 0.0, 0.0])), &[]),
            "DimensionMismatch",
        ),
        (query(Some(vector(&[f64::NAN, 1.0])), &[]), "NonFiniteValue"),
        (
            query(q(), &[("threshold", double(0.5)), ("radius", double(1.0))]),
            "ConflictingParams",
        ),
        (
            query(q(), &[
                ("metric", string("euclidean")),
                ("threshold", double(0.5)),
            ]),
            "ThresholdNeedsSimilarity",
        ),
        (
            query(q(), &[("radius", double(1.0))]),
            "RadiusNeedsDistance",
        ),
        (
            query(None, &[("threshold", double(0.5))]),
            "QueryNeedsVector",
        ),
        (
            query(None, &[("scores", boolean(true))]),
            "QueryNeedsVector",
        ),
        (
            query(None, &[("metric", string("dot"))]),
            "QueryNeedsVector",
        ),
        (query(q(), &[("threshold", double(1.5))]), "InvalidParam"),
        (query(q(), &[("threshold", double(-0.1))]), "InvalidParam"),
        (
            query(q(), &[
                ("metric", string("euclidean")),
                ("radius", double(-1.0)),
            ]),
            "InvalidParam",
        ),
        (query(q(), &[("k", int(0))]), "InvalidParam"),
        (query(q(), &[("k", int(4))]), "InvalidParam"),
        (
            query(q(), &[("k", int(1)), ("results", string("all"))]),
            "InvalidParam",
        ),
        (query(q(), &[("results", string("some"))]), "InvalidParam"),
        (query(q(), &[("scores", int(1))]), "InvalidParam"),
        (query(q(), &[("metric", string("l2"))]), "InvalidParam"),
        (
            query(q(), &[(
                "ids",
                list(vec![new_gbytearray_par(vec![1; 31], Vec::new(), false)]),
            )]),
            "InvalidParam",
        ),
        (
            QueryParams {
                payload: vec![vector(&[1.0, 0.0]), vector(&[0.0, 1.0])],
                named: BTreeMap::new(),
            },
            "PositionalParamsUnsupported",
        ),
    ];
    for (query, code) in queries {
        assert_eq!(
            code_of(
                &rig,
                SpaceOp::Consume(receive("r", any(), Some(query.clone())))
            )
            .await,
            code,
            "{query:?}"
        );
    }
    assert!(rig.log().await.is_empty(), "no failed operation commits");
    assert_eq!(rig.records().await.len(), 1);
    assert!(rig.waiters().await.is_empty());
}

/// The `capacity` bounds the records of a collection, not its waiters, and a send that fires a
/// waiter stores nothing (design v3 4.3).
#[tokio::test]
async fn space_vectordb_capacity_bounds_records_not_waiters() {
    let rig = Rig::new(Shape {
        capacity: 2,
        ..COSINE
    })
    .await;
    rig.send(vector_send(&[1.0, 0.0], "a")).await;
    rig.send(vector_send(&[0.0, 1.0], "b")).await;
    assert_eq!(
        code_of(&rig, SpaceOp::Produce(vector_send(&[1.0, 1.0], "c"))).await,
        "CapacityExceeded"
    );
    assert_eq!(rig.records().await.len(), 2);
    for index in 0..5 {
        let outcome = rig
            .receive(resalted(
                receive(
                    &format!("w{index}"),
                    any(),
                    Some(query(Some(vector(&[1.0, 1.0])), &[(
                        "threshold",
                        double(1.0),
                    )])),
                ),
                &format!("w{index}"),
            ))
            .await;
        assert!(outcome.fires.is_empty());
    }
    assert_eq!(rig.waiters().await.len(), 5);
    let outcome = rig.send(vector_send(&[2.0, 2.0], "d")).await;
    assert_eq!(
        outcome.fires.len(),
        1,
        "the full collection takes a send that fires"
    );
    assert_eq!(rig.records().await.len(), 2);
}

// ─── Rule E (decision 45) ────────────────────────────────────────────────────────────────

/// A peek removes the records that it binds and the transaction sends each one again, with its
/// embedding and its identity (design v3 4.5, A7).
#[tokio::test]
async fn space_vectordb_peek_restores_records_with_their_embedding() {
    let rig = Rig::new(COSINE).await;
    rig.send(doc_send("doc", &[1.0, 0.0], "a")).await;
    rig.send(vector_send(&[1.0, 1.0], "b")).await;
    let before = rig.records().await;
    rig.log().await;
    let outcome = rig
        .receive(peeking(receive(
            "p",
            any(),
            Some(query(Some(vector(&[1.0, 0.0])), &[])),
        )))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("p", vec![string("doc")])]);
    assert_eq!(
        rig.records().await,
        before,
        "the record is back with its embedding"
    );
    let log = rig.log().await;
    assert_eq!(comms(&log).len(), 1);
    assert!(!comms(&log)[0].peeks.is_empty(), "a peek COMM");
    let outcome = rig
        .receive(peeking(receive(
            "list",
            any(),
            Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(2))])),
        )))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("list", vec![list(vec![
        string("doc"),
        vector(&[1.0, 1.0])
    ])])]);
    assert_eq!(rig.records().await, before, "both records are back");
}

/// The restores of a list peek that the round cap leaves are carried by one settle job with the
/// pending records, and the job keeps their embeddings (arbitration A7).
#[tokio::test]
async fn space_vectordb_list_peek_restores_carry_at_the_cap() {
    let rig = Rig::new(COSINE).await;
    let sends = ordered_sends(&[
        (string("b"), &[0.5, 1.0]),
        (string("c"), &[0.0, 1.0]),
        (string("a"), &[1.0, 0.0]),
    ]);
    for op in &sends {
        rig.send(op.clone()).await;
    }
    let before = rig.records().await;
    // Each guarded peek waiter meets "b" first in canonical order and parks (decision 37).
    for index in 0..FIRE_ROUND_CAP - 1 {
        let outcome = rig
            .receive(resalted(
                peeking(guarded(
                    receive(&format!("p{index}"), any(), None),
                    equals(string("a")),
                )),
                &format!("p{index}"),
            ))
            .await;
        assert!(outcome.fires.is_empty());
    }
    let outcome = rig
        .receive(peeking(receive(
            "list",
            any(),
            Some(query(Some(vector(&[1.0, 0.0])), &[(
                "results",
                string("all"),
            )])),
        )))
        .await;
    assert_eq!(
        outcome.fires.len(),
        FIRE_ROUND_CAP,
        "the list fire and the restore of \"a\" to every guarded waiter share one cap"
    );
    let reissue = outcome.reissue.expect("the cap carries the restores");
    assert_eq!(reissue.cause, ReissueCause::RoundCap);
    let SpaceOp::Settle(settle) = &reissue.op else {
        panic!("a settle job carries the restores: {:?}", reissue.op);
    };
    let carried = settle.cascade.as_ref().expect("a cascade state");
    assert_eq!(
        carried.pending.len(),
        3,
        "\"a\" in flight, then \"b\" and \"c\""
    );
    assert!(carried.resume.is_none(), "the list receive is spent");
    let rest = rig.run(reissue.op).await.expect("the settle job commits");
    assert!(rest.fires.is_empty());
    assert_eq!(rest.reissue, None);
    assert_eq!(
        rig.records().await,
        before,
        "every record is back with its embedding"
    );
    assert!(rig.waiters().await.is_empty());
}

/// Sends of the payloads and embeddings `entries` whose records follow each other in canonical
/// order. The salts are searched in a fixed order, so every run makes the same sends.
fn ordered_sends(entries: &[(Par, &[f64])]) -> Vec<ProduceOp> {
    let mut sends: Vec<ProduceOp> = Vec::with_capacity(entries.len());
    let mut salt = 0;
    for (payload, embedding) in entries {
        loop {
            salt += 1;
            let candidate = send(
                payload.clone(),
                &[("embedding", vector(embedding))],
                &format!("ordered {salt}"),
                false,
            );
            if sends
                .last()
                .is_none_or(|last| record_id(last) < record_id(&candidate))
            {
                sends.push(candidate);
                break;
            }
        }
    }
    sends
}

// ─── LM-b (decision 66) ──────────────────────────────────────────────────────────────────

/// CE-1 of the arbitration: a list waiter whose pattern refuses a list of one parks, and a later
/// send that completes the list fires it with the incoming datum and retires the stored member.
async fn ce1(rig: &Rig) {
    let outcome = rig
        .receive(receive(
            "pair",
            pair(),
            Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(2))])),
        ))
        .await;
    assert!(outcome.fires.is_empty(), "no record: the waiter parks");
    let outcome = rig.send(vector_send(&[1.0, 0.0], "x1")).await;
    assert!(
        outcome.fires.is_empty(),
        "the list [x1] does not bind [a, b]"
    );
    assert_eq!(rig.records().await.len(), 1);
    assert_eq!(rig.waiters().await, vec!["pair".to_string()]);
    let outcome = rig.send(vector_send(&[1.0, 0.5], "x2")).await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("pair", vec![
            vector(&[1.0, 0.0]),
            vector(&[1.0, 0.5])
        ])],
        "the list [x1, x2] in selection order"
    );
    assert!(
        rig.records().await.is_empty(),
        "x2 is spent and x1 is retired"
    );
    assert!(rig.waiters().await.is_empty());
}

/// CE-2 of the arbitration: a list waiter refuses `[img, doc]` and parks. A receive that removes
/// the image exposes `[doc, doc]`, and LIST-CASCADE fires the waiter.
async fn ce2(rig: &Rig) {
    rig.send(doc_send("img", &[10.0, 1.0], "r1")).await;
    rig.send(doc_send("doc", &[10.0, 2.0], "r2")).await;
    rig.send(doc_send("doc", &[10.0, 3.0], "r3")).await;
    let docs = literal(list(vec![string("doc"), string("doc")]));
    let outcome = rig
        .receive(receive(
            "docs",
            docs,
            Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(2))])),
        ))
        .await;
    assert!(outcome.fires.is_empty(), "[img, doc] does not bind");
    let outcome = rig
        .receive(receive(
            "img",
            literal(string("img")),
            Some(query(Some(vector(&[1.0, 0.0])), &[])),
        ))
        .await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("img", Vec::new()), delivery("docs", Vec::new())],
        "the removal of the image fires the parked list waiter in the same transaction"
    );
    assert!(rig.records().await.is_empty());
    assert!(rig.waiters().await.is_empty());
}

#[tokio::test]
async fn space_vectordb_ce1_list_waiter_fires_with_the_incoming_datum() {
    ce1(&Rig::new(COSINE).await).await;
    ce1(&Rig::joined(COSINE).await).await;
}

#[tokio::test]
async fn space_vectordb_ce2_removal_cascade_fires_the_list_waiter() {
    ce2(&Rig::new(COSINE).await).await;
    ce2(&Rig::joined(COSINE).await).await;
}

/// LIST-CASCADE counts on the fire count of its transaction (decision 49). At the round cap the
/// transaction re-issues a settle job, which cascades on in the join engine.
async fn cascade_at_the_cap(rig: &Rig) {
    let waiters = FIRE_ROUND_CAP + 6;
    rig.send(doc_send("img", &[10.0, 0.0], "img")).await;
    for index in 1..=waiters {
        rig.send(doc_send(
            "doc",
            &[10.0, index as f64],
            &format!("doc{index}"),
        ))
        .await;
    }
    for index in 0..waiters {
        let outcome = rig
            .receive(resalted(
                receive(
                    &format!("w{index}"),
                    literal(list(vec![string("doc")])),
                    Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(1))])),
                ),
                &format!("w{index}"),
            ))
            .await;
        assert!(outcome.fires.is_empty(), "[img] does not bind [doc]");
    }
    let (fires, causes) = rig
        .run_all(SpaceOp::Consume(receive(
            "img",
            literal(string("img")),
            Some(query(Some(vector(&[1.0, 0.0])), &[])),
        )))
        .await
        .expect("the program commits");
    assert_eq!(
        fires.len(),
        1 + waiters,
        "the receive, then one cascade fire for each waiter"
    );
    assert_eq!(causes.first(), Some(&ReissueCause::RoundCap));
    assert!(rig.records().await.is_empty());
    assert!(rig.waiters().await.is_empty());
}

#[tokio::test]
async fn space_vectordb_list_cascade_carries_at_the_cap() {
    cascade_at_the_cap(
        &Rig::new(Shape {
            capacity: 200,
            ..COSINE
        })
        .await,
    )
    .await;
    cascade_at_the_cap(
        &Rig::joined(Shape {
            capacity: 200,
            ..COSINE
        })
        .await,
    )
    .await;
}

/// A cascade fire of a list waiter that peeks lifts the first member of its list (`K_lift`) and
/// sends it again in the same transaction (decisions 30 and 45). Three records make the list of
/// the waiter `[a, b]` too long, so it parks. A take of one record lets the cascade fire it, and
/// both members of its list stay stored.
async fn cascade_peek(rig: &Rig) {
    rig.send(vector_send(&[1.0, 0.0], "first")).await;
    rig.send(vector_send(&[0.0, 1.0], "taken")).await;
    rig.send(vector_send(&[1.0, 1.0], "second")).await;
    let taken = vector(&[0.0, 1.0]);
    let kept: Vec<Stored> = rig
        .records()
        .await
        .into_iter()
        .filter(|(payload, _, _)| payload.as_slice() != std::slice::from_ref(&taken))
        .collect();
    assert_eq!(kept.len(), 2);
    let outcome = rig
        .receive(peeking(receive(
            "list",
            pair(),
            Some(query(Some(vector(&[1.0, 0.0])), &[(
                "results",
                string("all"),
            )])),
        )))
        .await;
    assert!(
        outcome.fires.is_empty(),
        "a list of three does not bind [a, b]"
    );
    let (fires, causes) = rig
        .run_all(SpaceOp::Consume(receive("take", literal(taken), None)))
        .await
        .expect("the take commits");
    assert_eq!(fires, vec![
        delivery("take", vec![]),
        delivery("list", vec![vector(&[1.0, 0.0]), vector(&[1.0, 1.0])]),
    ]);
    assert!(causes.is_empty());
    assert_eq!(
        rig.records().await,
        kept,
        "the lifted member is stored again with its embedding"
    );
    assert!(rig.waiters().await.is_empty());
}

#[tokio::test]
async fn space_vectordb_cascade_peek_restores_the_lifted_member() {
    cascade_peek(&Rig::new(COSINE).await).await;
    cascade_peek(&Rig::joined(COSINE).await).await;
}

/// A waiter refused by its guard on a better record parks, and a later worse record that its
/// guard admits fires it (decision 37, until P12).
#[tokio::test]
async fn space_vectordb_guard_refusal_parks_until_a_worse_record_passes() {
    let rig = Rig::new(COSINE).await;
    rig.send(doc_send("x", &[1.0, 0.0], "a")).await;
    rig.send(doc_send("y", &[1.0, 1.0], "b")).await;
    let outcome = rig
        .receive(guarded(
            receive("g", any(), Some(query(Some(vector(&[1.0, 0.0])), &[]))),
            equals(string("y")),
        ))
        .await;
    assert!(
        outcome.fires.is_empty(),
        "the guard refuses the best record \"x\" once"
    );
    assert_eq!(rig.waiters().await, vec!["g".to_string()]);
    let outcome = rig.send(doc_send("y", &[0.0, 1.0], "c")).await;
    assert_eq!(fired(&outcome), vec![delivery("g", vec![string("y")])]);
    assert_eq!(rig.records().await.len(), 2);
}

// ─── Joins (design v3 4.6) ───────────────────────────────────────────────────────────────

/// Each bind of a join sees the records that its own query makes eligible, in its own
/// selection order.
#[tokio::test]
async fn space_vectordb_join_binds_each_position_by_its_query() {
    let rig = Rig::new(COSINE).await;
    rig.send(vector_send(&[1.0, 0.0], "a1")).await;
    rig.send(vector_send(&[0.0, 1.0], "a2")).await;
    rig.send(on_other(vector_send(&[1.0, 1.0], "b1"))).await;
    rig.send(on_other(vector_send(&[-1.0, 0.0], "b2"))).await;
    let join = JoinOp {
        space: SPACE,
        binds: vec![
            bind(
                &channel(),
                any(),
                Some(query(Some(vector(&[1.0, 0.0])), &[])),
            ),
            bind(&other(), any(), Some(query(Some(vector(&[0.0, 1.0])), &[]))),
        ],
        body: body_of("join"),
        persistent: false,
        peek: false,
        guard: None,
        authority: CostAuthority::default(),
        introduction_authority: CostAuthority::default(),
        reissued_ords: None,
    };
    let outcome = rig.run(SpaceOp::Join(join)).await.expect("the join fires");
    assert_eq!(fired(&outcome), vec![delivery("join", vec![
        vector(&[1.0, 0.0]),
        vector(&[1.0, 1.0])
    ])]);
    assert_eq!(rig.records().await.len(), 1);
    assert_eq!(records_of(&rig.host, &other_data()).await.len(), 1);
}

/// A join bind in list mode fails with `ListModeInJoin` before any staging (decision 66).
#[tokio::test]
async fn space_vectordb_list_mode_in_a_join_fails() {
    let rig = Rig::new(COSINE).await;
    rig.send(vector_send(&[1.0, 0.0], "a")).await;
    rig.log().await;
    let join = JoinOp {
        space: SPACE,
        binds: vec![
            bind(
                &channel(),
                any(),
                Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(1))])),
            ),
            bind(&other(), any(), None),
        ],
        body: body_of("join"),
        persistent: false,
        peek: false,
        guard: None,
        authority: CostAuthority::default(),
        introduction_authority: CostAuthority::default(),
        reissued_ords: None,
    };
    assert_eq!(code_of(&rig, SpaceOp::Join(join)).await, "ListModeInJoin");
    assert!(rig.log().await.is_empty());
}

/// A join waiter of VectorDB binds stores each bind's query in its slot, and a send fires it
/// only when the incoming record is eligible for the query of its position.
#[tokio::test]
async fn space_vectordb_join_waiter_fires_on_an_eligible_send() {
    let rig = Rig::new(COSINE).await;
    rig.send(on_other(vector_send(&[0.0, 1.0], "b"))).await;
    let join = JoinOp {
        space: SPACE,
        binds: vec![
            bind(
                &channel(),
                any(),
                Some(query(Some(vector(&[1.0, 0.0])), &[(
                    "threshold",
                    double(0.9),
                )])),
            ),
            bind(&other(), any(), None),
        ],
        body: body_of("join"),
        persistent: false,
        peek: false,
        guard: None,
        authority: CostAuthority::default(),
        introduction_authority: CostAuthority::default(),
        reissued_ords: None,
    };
    let outcome = rig.run(SpaceOp::Join(join)).await.expect("the join parks");
    assert!(outcome.fires.is_empty());
    let outcome = rig.send(vector_send(&[1.0, 1.0], "far")).await;
    assert!(outcome.fires.is_empty(), "cos 0.707 is below the threshold");
    let outcome = rig.send(vector_send(&[1.0, 0.1], "near")).await;
    assert_eq!(fired(&outcome), vec![delivery("join", vec![
        vector(&[1.0, 0.1]),
        vector(&[0.0, 1.0])
    ])]);
}

// ─── Replay ──────────────────────────────────────────────────────────────────────────────

/// A script with single mode, list mode, a peek, a guard and a cascade.
fn replay_script() -> Vec<SpaceOp> {
    vec![
        SpaceOp::Produce(doc_send("img", &[10.0, 1.0], "r1")),
        SpaceOp::Produce(doc_send("doc", &[10.0, 2.0], "r2")),
        SpaceOp::Produce(doc_send("doc", &[10.0, 3.0], "r3")),
        SpaceOp::Consume(receive(
            "docs",
            literal(list(vec![string("doc"), string("doc")])),
            Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(2))])),
        )),
        SpaceOp::Consume(peeking(receive(
            "peek",
            any(),
            Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(3))])),
        ))),
        SpaceOp::Consume(receive(
            "img",
            literal(string("img")),
            Some(query(Some(vector(&[1.0, 0.0])), &[])),
        )),
        SpaceOp::Produce(vector_send(&[1.0, 0.0], "x1")),
        SpaceOp::Consume(receive(
            "pair",
            pair(),
            Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(2))])),
        )),
        SpaceOp::Produce(vector_send(&[1.0, 0.5], "x2")),
    ]
}

/// Replay stages the transactions of play: equal outcomes, every recorded COMM paired, and the
/// root of play.
#[tokio::test]
async fn space_vectordb_replay_equal() {
    let (play, replay) = play_and_replay().await;
    let play_rig = Rig::over(play.clone(), COSINE).await;
    let start = play
        .create_checkpoint()
        .await
        .expect("a checkpoint after the creation")
        .root;
    let mut played = Vec::new();
    for op in replay_script() {
        played.push(play_rig.run(op).await.expect("the script plays"));
    }
    assert!(
        played.iter().any(|outcome| outcome.fires.len() == 2),
        "the cascade fires"
    );
    let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
    replay
        .rig_and_reset(start, log)
        .await
        .expect("the replay rigs the play log");
    let replay_rig = Rig {
        host: play.clone(),
        env: env_over(replay.clone(), &budget()),
    };
    let mut replayed = Vec::new();
    for op in replay_script() {
        replayed.push(replay_rig.run(op).await.expect("the script replays"));
    }
    assert_eq!(replayed, played);
    replay
        .check_replay_data()
        .await
        .expect("every recorded COMM is paired");
    assert_eq!(
        replay.create_checkpoint().await.expect("a checkpoint").root,
        root
    );
}

// ─── Source programs ─────────────────────────────────────────────────────────────────────

/// The factory of the suite and its constructor: a cosine space of two dimensions.
const VDB_URN: &str = "rho:space:hashmap:vectordb";

/// Rholang programs run on a play runtime through the whole stack: the normalizer, the factory
/// with its params, the handle, the send hook with `embedding`, and the receive hook with a
/// query. A list with scores and a parked waiter that a later send fires come back on public
/// channels, and the `config` reply holds the params of the factory.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_vectordb_source_programs_select_by_similarity() {
    let mut session = Session::new().await;
    session
        .setup(VDB_URN, "\"default\"; capacity=10, dimensions=2")
        .await;
    session
        .run(
            &with_space(r#"a!([1.0, 0.0]) | a!([0.0, 1.0]) | a!("doc"; embedding=[3.0, 4.0])"#),
            "rrs-vdb-sends",
        )
        .await;
    session
        .run(
            &with_space(
                r#"for (@list <- a ~ ([1.0, 0.0]; k=2, scores=true)) { @"rrs-vdb-list"!(list) }"#,
            ),
            "rrs-vdb-list",
        )
        .await;
    assert_eq!(
        session.sole("rrs-vdb-list").await,
        list(vec![
            list(vec![double(1.0), vector(&[1.0, 0.0])]),
            list(vec![double(0.6), string("doc")]),
        ]),
        "cos 1 and 3/5 in selection order, each [score, value]"
    );
    session
        .run(
            &with_space(r#"for (@x <- a ~ ([1.0, 0.0]; threshold=0.9)) { @"rrs-vdb-near"!(x) }"#),
            "rrs-vdb-park",
        )
        .await;
    assert!(
        session.values("rrs-vdb-near").await.is_empty(),
        "cos 0 is below the threshold, so the waiter parks"
    );
    session
        .run(&with_space(r#"a!([2.0, 0.1])"#), "rrs-vdb-fire")
        .await;
    assert_eq!(
        session.sole("rrs-vdb-near").await,
        vector(&[2.0, 0.1]),
        "the send fires the parked waiter, and its continuation decodes after the commit"
    );
    session
        .run(
            r#"for (space <<- @"rrs-handle") { for (@config <- space!?("config")) { @"rrs-vdb-config"!(config) } }"#,
            "rrs-vdb-config",
        )
        .await;
    let config = SpaceConfig::from_par(&session.sole("rrs-vdb-config").await)
        .expect("the config reply is a config record");
    assert_eq!(
        (config.outer.as_str(), config.inner.as_str()),
        ("hashmap", "vectordb")
    );
    assert_eq!(config.params.get("capacity"), Some(&int(10)));
    assert_eq!(config.params.get("dimensions"), Some(&int(2)));
}

/// The errors of a source program abort its deploy with their codes, and a failed deploy leaves
/// no space data: a vector of the wrong length, a zero cosine query, a list-mode bind in a join,
/// a full collection, and the factory params.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_vectordb_source_errors_have_their_codes() {
    let mut session = Session::new().await;
    session
        .setup(VDB_URN, "\"default\"; capacity=1, dimensions=2")
        .await;
    let none = Injections::new();
    session
        .fail_with(
            &with_space(r#"a!([1.0, 2.0, 3.0])"#),
            "rrs-vdb-dimension",
            &none,
            &["DimensionMismatch"],
        )
        .await;
    session
        .fail_with(
            &with_space(r#"for (@x <- a ~ ([0.0, 0.0])) { Nil }"#),
            "rrs-vdb-zero",
            &none,
            &["ZeroQueryVector"],
        )
        .await;
    session
        .fail_with(
            &with_space(r#"for (@x <- a ~ ([1.0, 0.0]; k=1) & @y <- b) { Nil }"#),
            "rrs-vdb-join",
            &none,
            &["ListModeInJoin"],
        )
        .await;
    session
        .run(&with_space(r#"a!([1.0, 0.0])"#), "rrs-vdb-first")
        .await;
    session
        .fail_with(&with_space(r#"a!([0.0, 1.0])"#), "rrs-vdb-full", &none, &[
            "CapacityExceeded",
        ])
        .await;
    for (ctor, code) in [
        ("\"default\"; capacity=0, dimensions=2", "InvalidParam"),
        ("\"default\"; capacity=5", "MissingParam"),
        ("\"default\"; capacity=5, dimensions=4097", "InvalidParam"),
        // A literal metric is checked by the normalizer. A computed one reaches the rule of the
        // factory.
        (
            "\"default\"; capacity=5, dimensions=2, metric=\"l\" ++ \"2\"",
            "InvalidParam",
        ),
        (
            "\"default\"; capacity=5, dimensions=2, metric=\"manhattan\", mirror=\"chroma\"",
            "InvalidParam",
        ),
    ] {
        session
            .fail_with(
                &setup_source(VDB_URN, ctor),
                &format!("rrs-vdb-factory {ctor}"),
                &none,
                &[code],
            )
            .await;
    }
}

// ─── Properties ──────────────────────────────────────────────────────────────────────────

/// The pattern of a generated receive.
#[derive(Clone, Copy, Debug)]
enum Pat {
    /// Binds any one value.
    Any,
    /// Binds a list of two values.
    Pair,
    /// Binds the string "x".
    Tag,
}

/// The mode of a generated receive.
#[derive(Clone, Debug)]
enum Mode {
    /// No query.
    Plain,
    /// Single mode with the query vector `q` and the threshold `t / 10`, or none.
    Single { q: [i64; 2], threshold: Option<u8> },
    /// List mode with the query vector `q`, or none, and the length `k`, or all.
    List { q: Option<[i64; 2]>, k: Option<u8> },
}

/// An operation of a generated program.
#[derive(Clone, Debug)]
enum VdbOp {
    /// A send of the vector `values` as its own embedding.
    Send {
        values: [i64; 2],
        persistent: bool,
        salt: u8,
    },
    /// A send of the string "x" or "y" with the embedding `values`.
    Doc { x: bool, values: [i64; 2], salt: u8 },
    /// A receive.
    Receive {
        mode: Mode,
        pattern: Pat,
        peek: bool,
        salt: u8,
    },
}

fn arb_vector() -> BoxedStrategy<[i64; 2]> {
    (-3i64..=3, -3i64..=3).prop_map(|(a, b)| [a, b]).boxed()
}

fn arb_mode() -> BoxedStrategy<Mode> {
    prop_oneof![
        1 => Just(Mode::Plain),
        3 => (arb_vector(), proptest::option::of(0u8..=10))
            .prop_map(|(q, threshold)| Mode::Single { q, threshold }),
        3 => (proptest::option::of(arb_vector()), proptest::option::of(1u8..=3))
            .prop_map(|(q, k)| Mode::List { q, k }),
    ]
    .boxed()
}

/// A send of a generated program: a vector as its own embedding, or a tagged string with an
/// embedding.
fn arb_send() -> BoxedStrategy<VdbOp> {
    prop_oneof![
        3 => (arb_vector(), proptest::bool::ANY, proptest::num::u8::ANY)
            .prop_map(|(values, persistent, salt)| VdbOp::Send { values, persistent, salt }),
        2 => (proptest::bool::ANY, arb_vector(), proptest::num::u8::ANY)
            .prop_map(|(x, values, salt)| VdbOp::Doc { x, values, salt }),
    ]
    .boxed()
}

/// A list waiter whose list a removal or a send can make acceptable: the pattern `[a, b]`
/// refuses a list of one or of three, and its length is 3 or all (LM-b, CE-1 and CE-2).
fn arb_list_waiter() -> BoxedStrategy<VdbOp> {
    (
        arb_vector(),
        prop_oneof![Just(Some(3u8)), Just(None)],
        proptest::bool::ANY,
        proptest::num::u8::ANY,
    )
        .prop_map(|(q, k, peek, salt)| VdbOp::Receive {
            mode: Mode::List { q: Some(q), k },
            pattern: Pat::Pair,
            peek,
            salt,
        })
        .boxed()
}

fn arb_op() -> BoxedStrategy<VdbOp> {
    prop_oneof![
        5 => arb_send(),
        4 => (
            arb_mode(),
            prop_oneof![Just(Pat::Any), Just(Pat::Pair), Just(Pat::Tag)],
            proptest::bool::ANY,
            proptest::num::u8::ANY,
        )
            .prop_map(|(mode, pattern, peek, salt)| VdbOp::Receive {
                mode,
                pattern,
                peek,
                salt,
            }),
    ]
    .boxed()
}

/// A program: a few sends, then list waiters that their lists can refuse, then any operations.
/// So the removal cascade and the offer to a list waiter occur in most programs.
fn arb_program() -> BoxedStrategy<Vec<VdbOp>> {
    (
        vec(arb_send(), 2..7),
        vec(arb_list_waiter(), 1..3),
        vec(arb_op(), 1..12),
    )
        .prop_map(|(sends, waiters, rest)| sends.into_iter().chain(waiters).chain(rest).collect())
        .boxed()
}

/// The integer vector `values` as a `Par`.
fn int_vector(values: [i64; 2]) -> Par { ints(&values) }

/// The space operation of the generated operation `op`, the `index`th of its program. Every
/// operation has a random state of its own.
fn space_op(op: &VdbOp, index: usize) -> SpaceOp {
    match op {
        VdbOp::Send {
            values,
            persistent,
            salt,
        } => SpaceOp::Produce(send(
            int_vector(*values),
            &[],
            &format!("{index} {salt}"),
            *persistent,
        )),
        VdbOp::Doc { x, values, salt } => SpaceOp::Produce(send(
            string(if *x { "x" } else { "y" }),
            &[("embedding", int_vector(*values))],
            &format!("{index} {salt}"),
            false,
        )),
        VdbOp::Receive {
            mode,
            pattern,
            peek,
            salt,
        } => {
            let pattern = match pattern {
                Pat::Any => any(),
                Pat::Pair => pair(),
                Pat::Tag => literal(string("x")),
            };
            let query = match mode {
                Mode::Plain => None,
                Mode::Single { q, threshold } => Some(query(
                    Some(int_vector(*q)),
                    &threshold
                        .map(|t| ("threshold", double(f64::from(t) / 10.0)))
                        .into_iter()
                        .collect::<Vec<_>>(),
                )),
                Mode::List { q, k } => Some(query(q.map(int_vector), &[match k {
                    Some(k) => ("k", int(i64::from(*k))),
                    None => ("results", string("all")),
                }])),
            };
            let name = format!("w{index}");
            let op = resalted(receive(&name, pattern, query), &format!("{index} {salt}"));
            SpaceOp::Consume(ConsumeOp { peek: *peek, ..op })
        }
    }
}

fn failure(message: impl Into<String>) -> TestCaseError { TestCaseError::fail(message.into()) }

/// The fires of an operation: the body name and the bound values of each fire, in order.
type Fires = Vec<(String, Vec<Par>)>;

/// The result of one generated operation: its fires and its causes, or its error code.
type Step = Result<(Fires, Vec<ReissueCause>), String>;

/// Runs the generated program `program` on `rig`, each operation to its end.
async fn run_program(rig: &Rig, program: &[VdbOp]) -> Vec<Step> {
    let mut steps = Vec::with_capacity(program.len());
    for (index, op) in program.iter().enumerate() {
        steps.push(
            rig.run_all(space_op(op, index))
                .await
                .map_err(|error| error.code().to_string()),
        );
    }
    steps
}

/// The driver and the engine fire the same continuations with the same values, and leave the
/// same records and waiters (design v3 4.6).
#[test]
fn prop_vectordb_driver_and_engine_agree() {
    check(pure_config(), arb_program(), |program| {
        tokio_runtime().block_on(async {
            let driver = Rig::new(COSINE).await;
            let engine = Rig::joined(COSINE).await;
            let by_driver = run_program(&driver, &program).await;
            let by_engine = run_program(&engine, &program).await;
            let fires = |steps: &[Step]| -> Vec<Result<Fires, String>> {
                steps
                    .iter()
                    .map(|step| step.clone().map(|(fires, _)| fires))
                    .collect()
            };
            if fires(&by_driver) != fires(&by_engine) {
                return Err(failure(format!(
                    "fires differ:\n  driver {by_driver:?}\n  engine {by_engine:?}"
                )));
            }
            if driver.records().await != engine.records().await {
                return Err(failure("the records differ"));
            }
            if driver.waiters().await != engine.waiters().await {
                return Err(failure("the waiters differ"));
            }
            Ok(())
        })
    });
}

/// The query of the policy `policy` of a parked waiter.
fn query_of_policy(policy: &WaiterPolicy) -> Option<QueryParams> {
    policy.query().map(|map| {
        let mut named = map.clone();
        let payload = named.remove("q").into_iter().collect();
        QueryParams { payload, named }
    })
}

/// After each operation of a program without guards, no parked waiter can fire: a fresh receive
/// with the pattern and the query of each parked waiter parks too (NoPendingMatch, LM-b).
#[test]
fn prop_vectordb_no_pending_match() {
    check(pure_config(), arb_program(), |program| {
        tokio_runtime().block_on(async {
            for engine in [false, true] {
                let rig = match engine {
                    false => Rig::new(COSINE).await,
                    true => Rig::joined(COSINE).await,
                };
                for (index, op) in program.iter().enumerate() {
                    let _ = rig.run_all(space_op(op, index)).await;
                    let parked: Vec<(BindPattern, Option<QueryParams>, String)> = rig
                        .host
                        .get_waiting_continuations(vec![data()])
                        .await
                        .iter()
                        .map(|waiter| {
                            let parked = record::unwrap_policy_continuation(&waiter.continuation)
                                .expect("a parked VectorDB waiter");
                            (
                                waiter.patterns[0].clone(),
                                query_of_policy(&parked.policy),
                                body_name(&parked.continuation),
                            )
                        })
                        .collect();
                    for (pattern, query, name) in parked {
                        let checkpoint = rig.host.create_soft_checkpoint().await;
                        let probe = receive(&format!("probe of {name}"), pattern, query);
                        let outcome = rig
                            .run(SpaceOp::Consume(probe))
                            .await
                            .map_err(|error| failure(format!("the probe fails: {error:?}")))?;
                        rig.host
                            .revert_to_soft_checkpoint(checkpoint)
                            .await
                            .expect("the probe reverts");
                        if !outcome.fires.is_empty() {
                            return Err(failure(format!(
                                "the parked waiter {name} has a pending match after op \
                                 {index} (engine {engine}): {:?}",
                                fired(&outcome)
                            )));
                        }
                    }
                }
            }
            Ok(())
        })
    });
}

/// The metrics of the list property, with their direction: true for a similarity.
const METRICS: [(&str, bool); 6] = [
    ("cosine", true),
    ("dot", true),
    ("euclidean", false),
    ("manhattan", false),
    ("hamming", false),
    ("jaccard", true),
];

/// True when the record `record` has a score for the query `q` under `metric`.
fn has_score(metric: &str, q: [i64; 2], record: [i64; 2]) -> bool {
    let binary = |v: [i64; 2]| v.iter().all(|x| *x == 0 || *x == 1);
    match metric {
        "cosine" => record != [0, 0],
        "hamming" => binary(q) && binary(record),
        "jaccard" => {
            binary(q) && binary(record) && q.iter().zip(&record).any(|(a, b)| *a == 1 || *b == 1)
        }
        _ => true,
    }
}

/// The delivered score of an entry `[score, value]` of a list.
fn score_of(entry: &Par) -> f64 {
    match entry.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EListBody(pair)),
        }] => match pair.ps[0].exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::GDouble(bits)),
            }] => f64::from_bits(*bits),
            _ => panic!("a score: {entry:?}"),
        },
        _ => panic!("an entry [score, value]: {entry:?}"),
    }
}

/// A list holds `min(k, |E|)` values, and its delivered scores follow the selection order: they
/// never increase for a similarity and never decrease for a distance (design v3 4.4).
#[test]
fn prop_vectordb_list_is_ordered_and_complete() {
    let strategy = (
        0usize..METRICS.len(),
        vec(arb_vector(), 0..8),
        arb_vector(),
        proptest::option::of(1u8..=8),
    );
    check(pure_config(), strategy, |(metric, records, q, k)| {
        let (metric, similarity) = METRICS[metric];
        if metric == "cosine" && q == [0, 0] {
            return Ok(());
        }
        tokio_runtime().block_on(async {
            let rig = Rig::new(Shape { metric, ..COSINE }).await;
            for (index, values) in records.iter().enumerate() {
                rig.send(send(int_vector(*values), &[], &format!("r{index}"), false))
                    .await;
            }
            let eligible = records
                .iter()
                .filter(|record| has_score(metric, q, **record))
                .count();
            let expected = k.map_or(eligible, |k| eligible.min(usize::from(k)));
            let named = [("scores", boolean(true)), match k {
                Some(k) => ("k", int(i64::from(k))),
                None => ("results", string("all")),
            }];
            let outcome = rig
                .receive(receive(
                    "list",
                    any(),
                    Some(query(Some(int_vector(q)), &named)),
                ))
                .await;
            let fires = fired(&outcome);
            if expected == 0 {
                return match fires.is_empty() {
                    true => Ok(()),
                    false => Err(failure(format!("an empty list fired: {fires:?}"))),
                };
            }
            let [(_, values)] = fires.as_slice() else {
                return Err(failure(format!("one list fire: {fires:?}")));
            };
            let entries = match values[0].exprs.as_slice() {
                [Expr {
                    expr_instance: Some(ExprInstance::EListBody(entries)),
                }] => entries.ps.clone(),
                _ => return Err(failure(format!("a list: {values:?}"))),
            };
            if entries.len() != expected {
                return Err(failure(format!(
                    "{metric}: {} entries, expected {expected}",
                    entries.len()
                )));
            }
            let scores: Vec<f64> = entries.iter().map(score_of).collect();
            let ordered = scores.windows(2).all(|pair| match similarity {
                true => pair[0] >= pair[1],
                false => pair[0] <= pair[1],
            });
            match ordered {
                true => Ok(()),
                false => Err(failure(format!("{metric}: scores out of order {scores:?}"))),
            }
        })
    });
}

/// Replay stages the transactions of play for every generated program: equal outcomes, every
/// recorded COMM paired, and the root of play.
#[test]
fn prop_vectordb_replay_equal() {
    check(pure_config(), arb_program(), |program| {
        tokio_runtime().block_on(async {
            let (play, replay) = play_and_replay().await;
            let play_rig = Rig::over(play.clone(), COSINE).await;
            let start = play
                .create_checkpoint()
                .await
                .expect("a checkpoint after the creation")
                .root;
            let played = run_program(&play_rig, &program).await;
            let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
            replay
                .rig_and_reset(start, log)
                .await
                .expect("the replay rigs the play log");
            let replay_rig = Rig {
                host: play.clone(),
                env: env_over(replay.clone(), &budget()),
            };
            let replayed = run_program(&replay_rig, &program).await;
            if replayed != played {
                return Err(failure(format!(
                    "replay differs:\n  play {played:?}\n  replay {replayed:?}"
                )));
            }
            replay
                .check_replay_data()
                .await
                .map_err(|error| failure(format!("an unpaired COMM: {error:?}")))?;
            let replay_root = replay.create_checkpoint().await.expect("a checkpoint").root;
            match replay_root == root {
                true => Ok(()),
                false => Err(failure("the roots differ")),
            }
        })
    });
}

/// The cause of the re-issue of `outcome`, or `None`.
fn cause_of(outcome: &SpaceOutcome) -> Option<ReissueCause> {
    outcome.reissue.as_ref().map(|reissue| reissue.cause)
}

/// The operation that each transaction of `chain` ran: `op`, then the re-issue of the
/// transaction before it.
fn inputs<'a>(op: &'a SpaceOp, chain: &'a [SpaceOutcome]) -> Vec<&'a SpaceOp> {
    let mut inputs = Vec::with_capacity(chain.len());
    let mut input = op;
    for outcome in chain {
        inputs.push(input);
        if let Some(reissue) = &outcome.reissue {
            input = &reissue.op;
        }
    }
    inputs
}

/// The rounds of the chain that `op` started (S24). Each transaction fires at most
/// [`FIRE_ROUND_CAP`] times. A transaction that runs an operation re-issues at the round cap only
/// after exactly [`FIRE_ROUND_CAP`] fires. A settle job starts its own count (decision 49): it
/// re-issues at the round cap after [`FIRE_ROUND_CAP`] fires of its own, or it re-issues the
/// operation that its cascade state resumes, with the cause of the stop of that operation.
fn check_rounds(op: &SpaceOp, chain: &[SpaceOutcome], context: &str) -> Result<(), TestCaseError> {
    for (index, (outcome, input)) in chain.iter().zip(inputs(op, chain)).enumerate() {
        let resumed = match input {
            SpaceOp::Settle(settle) => settle
                .cascade
                .as_ref()
                .and_then(|cascade| cascade.resume.as_deref()),
            SpaceOp::Produce(_) | SpaceOp::Consume(_) | SpaceOp::Join(_) => None,
        };
        match (outcome.fires.len(), cause_of(outcome)) {
            (fires, _) if fires > FIRE_ROUND_CAP => {
                return Err(failure(format!(
                    "{context}: transaction {index} fired {fires} times"
                )));
            }
            (fires, Some(ReissueCause::RoundCap)) if fires != FIRE_ROUND_CAP => {
                match (resumed, outcome.reissue.as_ref()) {
                    (Some(resume), Some(reissue)) if resume == reissue => {}
                    _ => {
                        return Err(failure(format!(
                            "{context}: transaction {index} re-issued at the round cap after \
                             {fires} fires"
                        )));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// A flood of `n` operations of one side, then one persistent operation of the other side that
/// meets each of them: `n` records and a persistent receive, or `n` parked receives and a
/// persistent send. Returns the persistent operation and its transactions.
async fn flood(rig: &Rig, n: usize, records: bool) -> (SpaceOp, Vec<SpaceOutcome>) {
    for index in 0..n {
        let salt = format!("flood {index}");
        match records {
            true => {
                rig.send(vector_send(&[1.0, index as f64], &salt)).await;
            }
            false => {
                let outcome = rig
                    .receive(resalted(receive(&format!("f{index}"), any(), None), &salt))
                    .await;
                assert!(outcome.fires.is_empty(), "the collection holds no record");
            }
        }
    }
    let drain = match records {
        true => SpaceOp::Consume(ConsumeOp {
            persistent: true,
            ..receive("drain", any(), None)
        }),
        false => SpaceOp::Produce(send(vector(&[1.0, 1.0]), &[], "drain", true)),
    };
    let chain = rig
        .run_chain(drain.clone())
        .await
        .expect("the drain commits");
    (drain, chain)
}

/// A transaction fires at most `FIRE_ROUND_CAP` times, and its operation re-issues at the round
/// cap only after exactly `FIRE_ROUND_CAP` fires (VectorDbDiscipline.v headline 4, S24). The
/// generated programs stay below the cap. So each case also floods a collection with `n` records
/// or `n` parked receives, and one persistent operation drains them: `n` fires, and
/// `n / FIRE_ROUND_CAP` stops of the operation at the round cap. A stop that owes a cascade
/// search continues in a settle job, which resumes the operation (the driver owes the search when
/// a record left, the engine after every fire).
#[test]
fn prop_vectordb_transactions_are_bounded() {
    let strategy = (arb_program(), 0usize..=150, proptest::bool::ANY);
    check(pure_config(), strategy, |(program, n, records)| {
        tokio_runtime().block_on(async {
            let flooded = Shape {
                capacity: 200,
                ..COSINE
            };
            for engine in [false, true] {
                let rig = match engine {
                    false => Rig::new(COSINE).await,
                    true => Rig::joined(COSINE).await,
                };
                for (index, op) in program.iter().enumerate() {
                    let op = space_op(op, index);
                    if let Ok(chain) = rig.run_chain(op.clone()).await {
                        check_rounds(&op, &chain, &format!("op {index} (engine {engine})"))?;
                    }
                }
                let rig = match engine {
                    false => Rig::new(flooded).await,
                    true => Rig::joined(flooded).await,
                };
                let (drain, chain) = flood(&rig, n, records).await;
                let context = format!("a flood of {n} (records {records}, engine {engine})");
                check_rounds(&drain, &chain, &context)?;
                let fires: usize = chain.iter().map(|outcome| outcome.fires.len()).sum();
                let stops = chain
                    .iter()
                    .zip(inputs(&drain, &chain))
                    .filter(|(outcome, input)| {
                        !matches!(input, SpaceOp::Settle(_))
                            && cause_of(outcome) == Some(ReissueCause::RoundCap)
                    })
                    .count();
                if (fires, stops) != (n, n / FIRE_ROUND_CAP) {
                    return Err(failure(format!(
                        "{context}: {fires} fires and {stops} stops at the round cap, expected \
                         {n} and {}",
                        n / FIRE_ROUND_CAP
                    )));
                }
            }
            Ok(())
        })
    });
}

/// The entries of `before` that `after` does not hold, as multisets.
fn removed<T: PartialEq + Clone>(before: &[T], after: &[T]) -> Vec<T> {
    let mut kept: Vec<&T> = after.iter().collect();
    let mut gone = Vec::with_capacity(before.len());
    for entry in before {
        match kept.iter().position(|stored| *stored == entry) {
            Some(position) => {
                kept.swap_remove(position);
            }
            None => gone.push(entry.clone()),
        }
    }
    gone
}

/// For each source produce that a COMM of `log` names, whether the last such COMM has no peeks.
/// A record that leaves its collection ends on a COMM without peeks: a fire of a waiter that does
/// not peek, or its `K_retire`. A `K_lift` has no peeks, and the fire of the lifted record follows
/// it. A COMM with peeks removes a record that Rule E sends again (decision 45).
fn last_comm_without_peeks(log: &[Event]) -> BTreeMap<Blake2b256Hash, bool> {
    let mut last = BTreeMap::new();
    for comm in comms(log) {
        let without_peeks = comm.peeks.is_empty();
        for produce in comm.produces {
            last.insert(produce.hash, without_peeks);
        }
    }
    last
}

/// `program` with each receive whose mode `select` accepts as a peek.
fn with_peeks(program: &[VdbOp], select: fn(&Mode) -> bool) -> Vec<VdbOp> {
    program
        .iter()
        .map(|op| match op {
            VdbOp::Receive {
                mode,
                pattern,
                peek,
                salt,
            } => VdbOp::Receive {
                mode: mode.clone(),
                pattern: *pattern,
                peek: *peek || select(mode),
                salt: *salt,
            },
            other => other.clone(),
        })
        .collect()
}

/// A drain of `program`: every send is not persistent, every list waiter peeks when
/// `list_peeks` holds, and one plain receive without a peek for each operation follows the
/// program. A plain receive takes the first record in canonical order. So a parked list waiter
/// whose list is too long fires in a cascade when two eligible records remain, and the cascade
/// lifts the first member of its list.
fn drained(program: &[VdbOp], list_peeks: bool) -> Vec<VdbOp> {
    let mut ops = Vec::with_capacity(2 * program.len());
    let select: fn(&Mode) -> bool = match list_peeks {
        true => |mode| matches!(mode, Mode::List { .. }),
        false => |_| false,
    };
    for op in with_peeks(program, select) {
        ops.push(match op {
            VdbOp::Send { values, salt, .. } => VdbOp::Send {
                values,
                persistent: false,
                salt,
            },
            other => other,
        });
    }
    ops.extend((0..program.len()).map(|index| VdbOp::Receive {
        mode: Mode::Plain,
        pattern: Pat::Any,
        peek: false,
        salt: u8::try_from(index).expect("a program has fewer than 256 operations"),
    }));
    ops
}

/// Rule E loses no record (VectorDbDiscipline.v headline 5, S25). After each operation of a
/// generated program, every record that left the collection, by the hash of its source produce
/// and its payload, embedding and persistence, ends on a COMM without peeks: the last COMM of the
/// operation that names the record is a fire of a waiter that does not peek, or its `K_retire`. A
/// peek fire removes its records and sends them again in the same transaction (decision 45). So
/// when every receive of the program peeks, no record leaves. Each program also runs as a drain
/// (`drained`), so a removal cascade fires peek waiters and lifts their first members.
#[test]
fn prop_vectordb_records_are_conserved() {
    check(pure_config(), arb_program(), |program| {
        let drain = drained(&program, true);
        let peeks = with_peeks(&program, |_| true);
        tokio_runtime().block_on(async {
            for (program, peeks_only) in [(&program, false), (&drain, false), (&peeks, true)] {
                for engine in [false, true] {
                    let rig = match engine {
                        false => Rig::new(COSINE).await,
                        true => Rig::joined(COSINE).await,
                    };
                    for (index, op) in program.iter().enumerate() {
                        let before = rig.sourced_records().await;
                        rig.log().await;
                        if rig.run_chain(space_op(op, index)).await.is_err() {
                            continue;
                        }
                        let gone = removed(&before, &rig.sourced_records().await);
                        let last = last_comm_without_peeks(&rig.log().await);
                        let context =
                            format!("op {index} (engine {engine}, peeks only {peeks_only})");
                        match (peeks_only, gone.as_slice()) {
                            (_, []) => {}
                            (true, gone) => {
                                return Err(failure(format!(
                                    "{context}: records left although every receive peeks: \
                                     {gone:?}"
                                )));
                            }
                            (false, gone) => {
                                if let Some((hash, record)) =
                                    gone.iter().find(|(hash, _)| last.get(hash) != Some(&true))
                                {
                                    return Err(failure(format!(
                                        "{context}: the record {record:?} ({hash:?}) left, and \
                                         the last COMM that names it peeks or does not exist"
                                    )));
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        })
    });
}

/// `program` with a distinct payload for each send: the send of index `i` stores the vector
/// `[a, b + 10 (i + 1)]` of its values `[a, b]`, and each doc becomes such a send. So the
/// payload of a record names it, and the values that a list fire binds name its members.
fn with_distinct_payloads(program: &[VdbOp]) -> Vec<VdbOp> {
    program
        .iter()
        .enumerate()
        .map(|(index, op)| {
            let offset = 10 * (i64::try_from(index).expect("a small program") + 1);
            match op {
                VdbOp::Send {
                    values: [a, b],
                    persistent,
                    salt,
                } => VdbOp::Send {
                    values: [*a, b + offset],
                    persistent: *persistent,
                    salt: *salt,
                },
                VdbOp::Doc {
                    values: [a, b],
                    salt,
                    ..
                } => VdbOp::Send {
                    values: [*a, b + offset],
                    persistent: false,
                    salt: *salt,
                },
                receive => receive.clone(),
            }
        })
        .collect()
}

/// The members of the list that a fire of a list waiter with the pattern `pattern` binds: the
/// two values of `[a, b]`, or the elements of the list that a variable binds.
fn list_members(pattern: Pat, bound: &[Par]) -> Vec<Par> {
    match pattern {
        Pat::Pair => bound.to_vec(),
        Pat::Any => bound
            .iter()
            .flat_map(|list| match list.exprs.as_slice() {
                [Expr {
                    expr_instance: Some(ExprInstance::EListBody(members)),
                }] => members.ps.clone(),
                _ => panic!("a list fire binds a list: {list:?}"),
            })
            .collect(),
        Pat::Tag => Vec::new(),
    }
}

/// A list fire without a peek spends each member of its list that is not persistent, in the
/// transaction of the fire (VectorDb.tla, ListAtomic). Each send of a program has a distinct
/// payload, so the values that a fire binds name the members of its list. After each operation,
/// a member of a list fire of a waiter that does not peek is stored only when it is persistent.
/// Each program also runs as a drain whose list waiters keep their peek flags, so the cascade
/// fires list waiters that do not peek and retires their members.
#[test]
fn prop_vectordb_list_fires_are_atomic() {
    check(pure_config(), arb_program(), |program| {
        let generated = with_distinct_payloads(&program);
        let drain = with_distinct_payloads(&drained(&program, false));
        tokio_runtime().block_on(async {
            for program in [&generated, &drain] {
                for engine in [false, true] {
                    let rig = match engine {
                        false => Rig::new(COSINE).await,
                        true => Rig::joined(COSINE).await,
                    };
                    for (index, op) in program.iter().enumerate() {
                        let Ok(chain) = rig.run_chain(space_op(op, index)).await else {
                            continue;
                        };
                        let after = rig.records().await;
                        for (name, bound) in chain.iter().flat_map(fired) {
                            let waiter = name
                                .strip_prefix('w')
                                .and_then(|index| index.parse::<usize>().ok())
                                .and_then(|index| program.get(index));
                            let Some(VdbOp::Receive {
                                mode: Mode::List { .. },
                                pattern,
                                peek: false,
                                ..
                            }) = waiter
                            else {
                                continue;
                            };
                            for member in list_members(*pattern, &bound) {
                                let stored = after
                                    .iter()
                                    .find(|(payload, _, _)| payload.as_slice() == [member.clone()]);
                                if let Some((_, _, false)) = stored {
                                    return Err(failure(format!(
                                        "op {index} (engine {engine}): the member {member:?} of \
                                         the list of {name} is still stored"
                                    )));
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        })
    });
}

// ─── The named tests of the P6 tracker leaves ────────────────────────────────────────────
//
// Each test carries the name of the leaf filter that it serves, as the named tests of P4 and P5
// do: `space_native_vectors`, `space_similarity_topk`, `space_vector_discipline`,
// `space_hash_collision_tie` and `rspace_list_random_state`.

/// The `Par` of the one expression `instance`.
fn number(instance: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }])
}

/// The rational `value` as a `GBigRat`.
fn rational_of(value: &num_rational::BigRational) -> Par {
    number(ExprInstance::GBigRat(models::rhoapi::GBigRational {
        numerator: value.numer().to_signed_bytes_be(),
        denominator: value.denom().to_signed_bytes_be(),
    }))
}

/// The rational `numerator / denominator` as a `GBigRat`, unreduced.
fn rational(numerator: i64, denominator: i64) -> Par {
    number(ExprInstance::GBigRat(models::rhoapi::GBigRational {
        numerator: num_bigint::BigInt::from(numerator).to_signed_bytes_be(),
        denominator: num_bigint::BigInt::from(denominator).to_signed_bytes_be(),
    }))
}

/// The `Par` of one element of a vector of `test-vectors/numeric-reference-v1.json`: an integer,
/// a rational `p/q`, `f64:<bits>` or `bool:<value>`.
fn reference_par(text: &str) -> Par {
    if let Some(bits) = text.strip_prefix("f64:") {
        let bits = u64::from_str_radix(bits, 16).expect("the hex bits of an f64");
        return double(f64::from_bits(bits));
    }
    if let Some(value) = text.strip_prefix("bool:") {
        return boolean(value == "true");
    }
    let value: num_rational::BigRational = text.parse().expect("an integer or a rational p/q");
    match (value.is_integer(), i64::try_from(value.numer())) {
        (true, Ok(integer)) => int(integer),
        _ => rational_of(&value),
    }
}

/// The vector of the reference elements `elements`.
fn reference_vector(elements: &serde_json::Value) -> Par {
    list(
        elements
            .as_array()
            .expect("a list of elements")
            .iter()
            .map(|element| reference_par(element.as_str().expect("an element")))
            .collect(),
    )
}

/// The value of a delivered entry `[score, value]`.
fn value_of(entry: &Par) -> Par {
    match entry.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EListBody(pair)),
        }] => pair.ps[1].clone(),
        _ => panic!("an entry [score, value]: {entry:?}"),
    }
}

/// The checkpoint root of `rig`.
async fn root_of(rig: &Rig) -> Blake2b256Hash {
    rig.host
        .create_checkpoint()
        .await
        .expect("a checkpoint")
        .root
}

/// Every numeric kind of the core enters a VectorDB space exactly (decision 54; tracker leaf
/// `rrs-spaces-determinism-native-vectors`). Equal values of different kinds have bitwise equal
/// scores and tie, the delivered payload keeps the kind and the order of its elements, `-0`
/// scores as `+0`, a string is no vector, and each stored record round-trips through its prost
/// bytes with its embedding.
#[tokio::test]
async fn space_native_vectors_every_kind_enters_exactly() {
    use prost::Message;

    let rig = Rig::new(Shape {
        capacity: 16,
        dimensions: 2,
        metric: "dot",
    })
    .await;
    let three_and_four = [
        ints(&[3, 4]),
        list(vec![
            number(ExprInstance::GBigInt(vec![3])),
            number(ExprInstance::GBigInt(vec![4])),
        ]),
        list(vec![rational(6, 2), rational(4, 1)]),
        list(vec![
            number(ExprInstance::GFixedPoint(models::rhoapi::GFixedPoint {
                unscaled: vec![30],
                scale: 1,
            })),
            number(ExprInstance::GFixedPoint(models::rhoapi::GFixedPoint {
                unscaled: vec![0x01, 0x90],
                scale: 2,
            })),
        ]),
        list(vec![
            double(3.0),
            number(ExprInstance::GFloat32(4.0f32.to_bits())),
        ]),
    ];
    for (index, vector) in three_and_four.iter().enumerate() {
        rig.send(send(vector.clone(), &[], &format!("kind {index}"), false))
            .await;
    }
    let swapped = ints(&[4, 3]);
    let binary = list(vec![boolean(true), boolean(false)]);
    let signed_zero = list(vec![double(-0.0), double(0.0)]);
    rig.send(send(swapped.clone(), &[], "swapped", false)).await;
    rig.send(send(binary.clone(), &[], "binary", false)).await;
    rig.send(send(signed_zero.clone(), &[], "signed zero", false))
        .await;
    rig.send(send(string("3,4"), &[], "text", false)).await;

    let all = query(Some(ints(&[1, 2])), &[
        ("results", string("all")),
        ("scores", boolean(true)),
    ]);
    let fires = fired(&rig.receive(peeking(receive("all", any(), Some(all)))).await);
    let [(name, values)] = fires.as_slice() else {
        panic!("one fire: {fires:?}");
    };
    assert_eq!(name, "all");
    let entries = match values.as_slice() {
        [Par { exprs, .. }] => match exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::EListBody(entries)),
            }] => entries.ps.clone(),
            _ => panic!("one list: {values:?}"),
        },
        _ => panic!("one value: {values:?}"),
    };
    let scores: Vec<u64> = entries
        .iter()
        .map(|entry| score_of(entry).to_bits())
        .collect();
    assert_eq!(
        scores,
        [11.0, 11.0, 11.0, 11.0, 11.0, 10.0, 1.0, 0.0].map(f64::to_bits),
        "3 + 2·4 for each kind, 4 + 2·3 for the swapped order, 1 for [true, false], and +0 for \
         [-0, 0]; the string has no vector"
    );
    let canonical: Vec<Par> = rig
        .sourced_records()
        .await
        .into_iter()
        .filter_map(|(_, (payload, _, _))| {
            three_and_four
                .contains(&payload[0])
                .then(|| payload[0].clone())
        })
        .collect();
    let tied: Vec<Par> = entries[..5].iter().map(value_of).collect();
    assert_eq!(
        tied, canonical,
        "the five equal values tie, and their order is the canonical order of the records"
    );
    assert_eq!(
        entries[5..].iter().map(value_of).collect::<Vec<_>>(),
        vec![swapped, binary, signed_zero],
        "each payload keeps the kind and the order of its elements"
    );

    let datums = rig.host.get_data(&data()).await;
    assert_eq!(datums.len(), 9, "the peek restores every record");
    for datum in &datums {
        let bytes = datum.a.encode_to_vec();
        let decoded = ListParWithRandom::decode(bytes.as_slice()).expect("the record decodes");
        assert_eq!(decoded, datum.a);
        let record = record::unwrap_vdb_datum(&decoded).expect("a VectorDB record");
        let payload = &record.datum.pars[0];
        match payload == &string("3,4") {
            true => assert_eq!(record.embedding, None, "a string is no vector"),
            false => assert_eq!(
                record.embedding.as_ref(),
                Some(payload),
                "the payload is the vector"
            ),
        }
    }
}

/// The 143 metric cases of the Sage reference vectors run through a VectorDB space (tracker leaf
/// `rrs-spaces-determinism-similarity-topk`): the space stores the record, a peek with
/// `scores=true` delivers the score of the reference bit for bit, an undefined score leaves the
/// record ineligible, the zero cosine query fails with `ZeroQueryVector`, and a score outside the
/// `f64` range fails with `ValueOutOfRange`. A bound inside the range of its param selects the
/// record exactly when the reference says `within`, and a bound outside that range (a `dot`
/// threshold outside `[0, 1]`, a negative radius) fails with `InvalidParam` (decision 66). The
/// values are fixed, so every process run must reproduce them.
#[tokio::test]
async fn space_similarity_topk_reference_cases_through_the_space() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-vectors/numeric-reference-v1.json"
    );
    let text = std::fs::read_to_string(path).expect("the reference vectors");
    let document: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let zero = num_rational::BigRational::from_integer(0.into());
    let one = num_rational::BigRational::from_integer(1.into());
    let mut counts = BTreeMap::<&str, usize>::new();
    for case in document["metric_cases"]
        .as_array()
        .expect("the metric cases")
    {
        let name = case["name"].as_str().expect("a name");
        let metric: &'static str = match case["metric"].as_str().expect("a metric") {
            "cosine" => "cosine",
            "dot" => "dot",
            "euclidean" => "euclidean",
            "manhattan" => "manhattan",
            "hamming" => "hamming",
            "jaccard" => "jaccard",
            other => panic!("{name}: the metric {other} is not a metric of the contract"),
        };
        let q = reference_vector(&case["query"]);
        let record = reference_vector(&case["record"]);
        let dimensions = case["query"].as_array().expect("a query").len();
        let rig = Rig::new(Shape {
            capacity: 4,
            dimensions: i64::try_from(dimensions).expect("a small dimension"),
            metric,
        })
        .await;
        rig.send(send(record.clone(), &[], name, false)).await;
        let scored = || {
            SpaceOp::Consume(peeking(receive(
                "scored",
                any(),
                Some(query(Some(q.clone()), &[("scores", boolean(true))])),
            )))
        };
        if case["query_refused"] == serde_json::json!(true) {
            assert_eq!(code_of(&rig, scored()).await, "ZeroQueryVector", "{name}");
            *counts.entry("refused").or_default() += 1;
            continue;
        }
        match (case["score_defined"].as_bool(), case["delivered"].as_str()) {
            (Some(false), _) => {
                let outcome = rig.run(scored()).await.expect("the receive parks");
                assert!(
                    outcome.fires.is_empty(),
                    "{name}: an undefined score is ineligible"
                );
                *counts.entry("undefined").or_default() += 1;
            }
            (Some(true), Some("ValueOutOfRange")) => {
                assert_eq!(code_of(&rig, scored()).await, "ValueOutOfRange", "{name}");
                *counts.entry("out of the f64 range").or_default() += 1;
            }
            (Some(true), Some(bits)) => {
                let bits = u64::from_str_radix(bits.trim_start_matches("0x"), 16)
                    .expect("the bits of a delivered score");
                let outcome = rig.run(scored()).await.expect("the peek commits");
                let fires = fired(&outcome);
                let [(_, values)] = fires.as_slice() else {
                    panic!("{name}: one fire, found {fires:?}");
                };
                let [entry] = values.as_slice() else {
                    panic!("{name}: one value [score, value], found {values:?}");
                };
                assert_eq!(
                    score_of(entry).to_bits(),
                    bits,
                    "{name}: the delivered score"
                );
                assert_eq!(value_of(entry), record, "{name}: the delivered record");
                *counts.entry("scored").or_default() += 1;
            }
            other => panic!("{name}: an unexpected reference outcome {other:?}"),
        }
        let bound = case["bound"].as_object().expect("a bound");
        assert_eq!(bound.len(), 1, "{name}: one bound");
        let (key, value) = bound.iter().next().expect("the bound");
        let value: num_rational::BigRational = value
            .as_str()
            .expect("a bound value")
            .parse()
            .expect("a rational bound");
        let in_range = match key.as_str() {
            "threshold" => zero <= value && value <= one,
            "radius" => zero <= value,
            other => panic!("{name}: the bound {other} is not a bound of the contract"),
        };
        let bounded = SpaceOp::Consume(receive(
            "bounded",
            any(),
            Some(query(Some(q.clone()), &[(
                key.as_str(),
                rational_of(&value),
            )])),
        ));
        if !in_range {
            assert_eq!(code_of(&rig, bounded).await, "InvalidParam", "{name}");
            *counts.entry("bound out of range").or_default() += 1;
            continue;
        }
        let within = case["within"].as_bool().unwrap_or(false);
        let outcome = rig.run(bounded).await.expect("the bounded receive commits");
        assert_eq!(
            fired(&outcome),
            match within {
                true => vec![delivery("bounded", vec![record.clone()])],
                false => Vec::new(),
            },
            "{name}: the bound {key} {value} selects the record exactly when it holds"
        );
        *counts
            .entry(match within {
                true => "within",
                false => "not within",
            })
            .or_default() += 1;
    }
    assert_eq!(
        counts,
        BTreeMap::from([
            ("bound out of range", 24),
            ("not within", 75),
            ("out of the f64 range", 1),
            ("refused", 1),
            ("scored", 136),
            ("undefined", 5),
            ("within", 43),
        ]),
        "every class of the reference cases is checked"
    );
}

/// The selection of a fresh store and the selection after a restart agree (tracker leaf
/// `rrs-spaces-port-core-vector-discipline`: native values, top-k ties, peek, rebuild in
/// canonical order, restart). Two rigs select from the records of their hot stores, one in the
/// order of the sends and one in the reverse order. A third rig makes the sends, makes a
/// checkpoint, resets to it as a restarted node does, and selects from the records of the
/// history, which keeps them in the order of their bytes. All three make the same selection,
/// ties included, and reach the same root, and the history keeps each record with its value,
/// its embedding and its persist flag.
#[tokio::test]
async fn space_vector_discipline_restart_keeps_values_ties_and_order() {
    let shape = Shape {
        capacity: 16,
        dimensions: 2,
        metric: "dot",
    };
    let sends = || {
        vec![
            send(ints(&[2, 7]), &[], "tie a", false),
            send(
                list(vec![rational(4, 2), double(-1.0)]),
                &[],
                "tie b",
                false,
            ),
            send(
                list(vec![
                    number(ExprInstance::GFloat32(2.0f32.to_bits())),
                    number(ExprInstance::GBigInt(vec![5])),
                ]),
                &[],
                "tie c",
                true,
            ),
            send(ints(&[3, 0]), &[], "best", false),
            send(ints(&[1, 1]), &[], "worst", false),
        ]
    };
    let peek = || {
        peeking(receive(
            "peek",
            any(),
            Some(query(Some(ints(&[1, 0])), &[("results", string("all"))])),
        ))
    };
    let top = || {
        receive(
            "top",
            any(),
            Some(query(Some(ints(&[1, 0])), &[
                ("k", int(3)),
                ("scores", boolean(true)),
            ])),
        )
    };
    let fresh = Rig::new(shape).await;
    let reversed = Rig::new(shape).await;
    let restarted = Rig::new(shape).await;
    for (rig, reverse) in [(&fresh, false), (&reversed, true), (&restarted, false)] {
        let mut ops = sends();
        if reverse {
            ops.reverse();
        }
        for op in ops {
            rig.send(op).await;
        }
        assert_eq!(
            fired(&rig.receive(peek()).await).len(),
            1,
            "the peek restores every record"
        );
    }
    let records = fresh.sourced_records().await;
    assert_eq!(reversed.sourced_records().await, records);
    let first = fired(&fresh.receive(top()).await);
    let after = root_of(&fresh).await;
    assert_eq!(
        fired(&reversed.receive(top()).await),
        first,
        "the order of the sends does not matter"
    );
    assert_eq!(root_of(&reversed).await, after);
    let start = root_of(&restarted).await;
    restarted
        .host
        .reset(&start)
        .await
        .expect("the reset to the checkpoint");
    assert_eq!(
        restarted.sourced_records().await,
        records,
        "the history holds each record with its value, its embedding and its persist flag"
    );
    let again = fired(&restarted.receive(top()).await);
    assert_eq!(
        again, first,
        "the selection does not depend on the order of the view"
    );
    assert_eq!(root_of(&restarted).await, after);
    let [(_, values)] = first.as_slice() else {
        panic!("one fire: {first:?}");
    };
    let entries = match values.as_slice() {
        [Par { exprs, .. }] => match exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::EListBody(entries)),
            }] => entries.ps.clone(),
            _ => panic!("one list: {values:?}"),
        },
        _ => panic!("one value: {values:?}"),
    };
    assert_eq!(
        entries.iter().map(score_of).collect::<Vec<_>>(),
        vec![3.0, 2.0, 2.0],
        "the best record, then two of the three tied records"
    );
}

/// Two byte-identical sends store two records with one source hash and one digest, so the order
/// of DR-75 keeps them by index, and a third record ties with them on its score (tracker leaf
/// `rrs-spaces-determinism-collision-ties`). In each of three orders of the sends, one rig
/// selects from its hot store, in the order of the sends, and a second rig selects after a
/// checkpoint and a reset, from the history. Every order and both views select the same value
/// and log the same COMM, the roots are equal, and replay reproduces play.
#[tokio::test]
async fn space_hash_collision_tie_duplicates_are_interchangeable() {
    let shape = Shape {
        capacity: 8,
        dimensions: 2,
        metric: "dot",
    };
    let duplicate = || send(ints(&[1, 5]), &[], "duplicate", false);
    let other = || send(ints(&[1, -5]), &[], "other", false);
    let orders: [[fn() -> ProduceOp; 3]; 3] = [
        [duplicate, duplicate, other],
        [duplicate, other, duplicate],
        [other, duplicate, duplicate],
    ];
    let take = || receive("take", any(), Some(query(Some(ints(&[1, 0])), &[])));
    let mut results = Vec::new();
    for order in orders {
        let hot = Rig::new(shape).await;
        let cold = Rig::new(shape).await;
        for op in order {
            hot.send(op()).await;
            cold.send(op()).await;
        }
        hot.log().await;
        let first = fired(&hot.receive(take()).await);
        let comm = comms(&hot.log().await);
        let after = root_of(&hot).await;
        let stored = root_of(&cold).await;
        cold.host
            .reset(&stored)
            .await
            .expect("the reset to the checkpoint");
        cold.log().await;
        let again = fired(&cold.receive(take()).await);
        assert_eq!(
            again, first,
            "the view of the history selects as the hot store does"
        );
        assert_eq!(comms(&cold.log().await), comm);
        assert_eq!(root_of(&cold).await, after);
        results.push((stored, first, comm, after));
    }
    for result in &results[1..] {
        assert_eq!(
            result, &results[0],
            "the order of the sends does not matter"
        );
    }
    let winner = match record_id(&duplicate()) < record_id(&other()) {
        true => ints(&[1, 5]),
        false => ints(&[1, -5]),
    };
    assert_eq!(
        results[0].1,
        vec![delivery("take", vec![winner])],
        "the tie goes to the lower source hash"
    );

    let (play, replay) = play_and_replay().await;
    let play_rig = Rig::over(play.clone(), shape).await;
    let start = play
        .create_checkpoint()
        .await
        .expect("a checkpoint after the creation")
        .root;
    let script = || {
        vec![
            SpaceOp::Produce(duplicate()),
            SpaceOp::Produce(other()),
            SpaceOp::Produce(duplicate()),
            SpaceOp::Consume(take()),
            SpaceOp::Consume(take()),
        ]
    };
    let mut played = Vec::new();
    for op in script() {
        played.push(play_rig.run(op).await.expect("the script plays"));
    }
    let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
    replay
        .rig_and_reset(start, log)
        .await
        .expect("the replay rigs the play log");
    let replay_rig = Rig {
        host: play.clone(),
        env: env_over(replay.clone(), &budget()),
    };
    let mut replayed = Vec::new();
    for op in script() {
        replayed.push(replay_rig.run(op).await.expect("the script replays"));
    }
    assert_eq!(replayed, played);
    replay
        .check_replay_data()
        .await
        .expect("every recorded COMM is paired");
    assert_eq!(
        replay.create_checkpoint().await.expect("a checkpoint").root,
        root
    );
}

/// Forced equal source hashes (tracker leaf `rrs-spaces-determinism-collision-ties`): the hash
/// of a source produce cannot collide in a test, so the records are built with one forced
/// source hash. Records with distinct digests are ordered by digest, byte-identical records are
/// kept by index, and with equal scores the selection order of design v3 4.4 (score, then the
/// canonical rank) picks the same value from every order of the view.
#[test]
fn space_hash_collision_tie_forced_equal_source_hashes_select_one_value() {
    use rholang::rust::interpreter::tensor::exact::{self, Candidate, Metric, Query};
    use rholang::rust::interpreter::tensor::num::vector_of;
    use rspace_plus_plus::rspace::internal::Datum;
    use rspace_plus_plus::rspace::space_store_handle::canonical_data_order;

    let forced = Blake2b256Hash::new(b"rrs-forced-source-hash");
    let record = |payload: Par, salt: &str| Datum {
        a: record::wrap_vdb_datum(
            0,
            Some(&ints(&[1, 0])),
            &send(payload, &[], salt, false).data,
        )
        .expect("a VectorDB record"),
        persist: false,
        source: Produce::new(Blake2b256Hash::new(b"rrs-channel"), forced.clone(), false),
    };
    let views = [
        record(string("first"), "a"),
        record(string("second"), "b"),
        record(string("first"), "a"),
    ];
    let q = Query::new(
        vector_of(&ints(&[1, 0]), Some(2)).expect("a query vector"),
        Metric::Dot,
    )
    .expect("a query")
    .expect("a query with a direction");
    let score = exact::score(&q, &vector_of(&ints(&[1, 0]), Some(2)).expect("a vector"))
        .expect("a score")
        .expect("a defined score");
    let selected = |view: &[Datum<ListParWithRandom>]| -> Par {
        let order = canonical_data_order(view);
        let mut candidates: Vec<Candidate> = order
            .iter()
            .enumerate()
            .map(|(rank, _)| Candidate {
                score: score.clone(),
                rank,
            })
            .collect();
        exact::rank(&mut candidates).expect("a rank");
        let best = order[candidates[0].rank];
        record::unwrap_vdb_datum(&view[best].a)
            .expect("a VectorDB record")
            .datum
            .pars[0]
            .clone()
    };
    let mut values = Vec::new();
    for permutation in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
        2, 1, 0,
    ]] {
        let view: Vec<Datum<ListParWithRandom>> = permutation
            .iter()
            .map(|index| views[*index].clone())
            .collect();
        values.push(selected(&view));
    }
    assert!(
        values.iter().all(|value| value == &values[0]),
        "every order of the view selects one value: {values:?}"
    );
}

/// The random state of a list delivery (tracker leaf `rrs-rspace-interface-list-random-state`,
/// design v3 4.4 and arbitration A11): with no eligible record a list waiter parks and the
/// space makes no datum, one member keeps its random state byte for byte, and two or more members
/// merge their random states in delivery order, also when the send of the last member fires a
/// parked list waiter.
#[tokio::test]
async fn rspace_list_random_state_zero_one_and_two_members() {
    let list_of_two = || {
        receive(
            "list",
            any(),
            Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(2))])),
        )
    };
    let random_of = |op: &ProduceOp| op.data.random_state.clone();
    let delivered_random = |outcome: &SpaceOutcome| -> Vec<u8> {
        match outcome.fires.as_slice() {
            [fire] => match fire.results.as_slice() {
                [result] => result.matched_datum.random_state.clone(),
                other => panic!("one result: {other:?}"),
            },
            other => panic!("one fire: {other:?}"),
        }
    };
    let merged = |states: &[Vec<u8>]| -> Vec<u8> {
        Blake2b512Random::merge(
            states
                .iter()
                .map(|state| Blake2b512Random::from_bytes(state))
                .collect(),
        )
        .to_bytes()
    };

    // Zero: the waiter parks, and the log holds its consume and no datum.
    let rig = Rig::new(COSINE).await;
    let outcome = rig.receive(list_of_two()).await;
    assert!(outcome.fires.is_empty());
    assert!(rig.records().await.is_empty(), "no datum is synthesized");
    let log = rig.log().await;
    assert!(comms(&log).is_empty());
    assert!(
        log.iter().all(|event| !matches!(event, Event::IoEvent(io) if matches!(io, rspace_plus_plus::rspace::trace::event::IOEvent::Produce(_)))),
        "no produce is logged: {log:?}"
    );

    // One: the member keeps its random state.
    let rig = Rig::new(COSINE).await;
    let only = vector_send(&[1.0, 0.0], "only");
    rig.send(only.clone()).await;
    let outcome = rig.receive(list_of_two()).await;
    assert_eq!(delivered_random(&outcome), random_of(&only));

    // Two: the random states merge in delivery order.
    let rig = Rig::new(COSINE).await;
    let near = vector_send(&[1.0, 0.0], "near");
    let far = vector_send(&[1.0, 1.0], "far");
    rig.send(far.clone()).await;
    rig.send(near.clone()).await;
    let outcome = rig.receive(list_of_two()).await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("list", vec![list(vec![
            vector(&[1.0, 0.0]),
            vector(&[1.0, 1.0])
        ])])],
        "the delivery order is the selection order"
    );
    assert_eq!(
        delivered_random(&outcome),
        merged(&[random_of(&near), random_of(&far)])
    );
    assert_ne!(
        delivered_random(&outcome),
        merged(&[random_of(&far), random_of(&near)]),
        "the merge follows the delivery order"
    );

    // Two, on the side of a send: the pattern refuses a list of one, so the waiter parks; the
    // second send completes the list and fires it with a stored and an incoming member.
    let rig = Rig::new(COSINE).await;
    rig.send(far.clone()).await;
    let waiting = receive(
        "pair",
        literal(list(vec![vector(&[1.0, 0.0]), vector(&[1.0, 1.0])])),
        Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(2))])),
    );
    assert!(
        rig.receive(waiting).await.fires.is_empty(),
        "a list of one parks"
    );
    let outcome = rig.send(near.clone()).await;
    assert_eq!(
        delivered_random(&outcome),
        merged(&[random_of(&near), random_of(&far)])
    );
}

/// Replay reproduces the list deliveries of play with their random states (tracker leaf
/// `rrs-rspace-interface-list-random-state`, the replay case): a parked list waiter, a list of
/// one and a list of two, with equal outcomes, every COMM paired and the root of play.
#[tokio::test]
async fn rspace_list_random_state_replay_reproduces_the_merged_states() {
    let (play, replay) = play_and_replay().await;
    let play_rig = Rig::over(play.clone(), COSINE).await;
    let start = play
        .create_checkpoint()
        .await
        .expect("a checkpoint after the creation")
        .root;
    let list_of_two = |name: &str| {
        receive(
            name,
            any(),
            Some(query(Some(vector(&[1.0, 0.0])), &[("k", int(2))])),
        )
    };
    let script = || {
        vec![
            SpaceOp::Consume(list_of_two("zero")),
            SpaceOp::Produce(vector_send(&[1.0, 0.0], "one")),
            SpaceOp::Produce(vector_send(&[1.0, 1.0], "far")),
            SpaceOp::Produce(vector_send(&[1.0, 0.5], "near")),
            SpaceOp::Consume(list_of_two("two")),
        ]
    };
    let mut played = Vec::new();
    for op in script() {
        played.push(play_rig.run(op).await.expect("the script plays"));
    }
    assert_eq!(
        played
            .iter()
            .map(|outcome| outcome.fires.len())
            .collect::<Vec<_>>(),
        vec![0, 1, 0, 0, 1],
        "the first send fires the parked waiter with one member, the last receive takes two"
    );
    let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
    replay
        .rig_and_reset(start, log)
        .await
        .expect("the replay rigs the play log");
    let replay_rig = Rig {
        host: play.clone(),
        env: env_over(replay.clone(), &budget()),
    };
    let mut replayed = Vec::new();
    for op in script() {
        replayed.push(replay_rig.run(op).await.expect("the script replays"));
    }
    assert_eq!(
        replayed, played,
        "the outcomes hold the random states of the deliveries"
    );
    replay
        .check_replay_data()
        .await
        .expect("every recorded COMM is paired");
    assert_eq!(
        replay.create_checkpoint().await.expect("a checkpoint").root,
        root
    );
}
