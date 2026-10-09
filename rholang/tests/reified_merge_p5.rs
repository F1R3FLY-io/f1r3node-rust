//! P5 integration tests of the merge of space operations across branches. The sources are:
//!
//! * the P5 design v3 (sections A.4, A.7 and A.13) and v2 (sections 2.8.4 and 3.1),
//! * plan revision M-1, and decisions 31 and 45,
//! * items 18 and 19 of the consensus checklist.
//!
//! The harness builds the merge indices as Casper builds them for a block
//! (`casper/src/rust/merging/block_index.rs`, `create_event_log_index`). The pre-state closures
//! read the history at the pre-state root, and `EventLogIndex::new` indexes the event log of each
//! deploy. The tests call the merger functions `conflicts`, `conflict_reason` and `depends` of
//! rspace++ on these indices. The harness computes a merged state as `dag_merger.rs` computes
//! it, so a test can compare the merge with each serial order of the branches.
//!
//! A branch is one deploy that a play runtime evaluates from the pre-state root. Two branches
//! conflict when `conflicts` is not empty. Each counter race test also runs a negative control:
//! the same logs without the events of the counter channels. Without the counters the branches
//! merge, so the counters are necessary (M-1).
//!
//! Scope of wave W2: the families `hashmap:{queue,bag,stack,cell}`, `array:queue` and
//! `vector:queue`. P6 adds the VectorDB rows in native execution (decision 69): writes merge
//! with no counter, and `capacity` is a soft bound under merges (decision 71). Wave W3 adds
//! these rows:
//!
//! * the set rows (F24 `set_retirements_race`, F25 `unordered_set_writes_commute`),
//! * the priority-queue rows, and item 18 for the class fire `[D; n]`,
//! * the rows of the same-space joins (P5c, joins plan section 5).
//!
//! A join bumps the counter of each of its collections. So it races with every branch that uses
//! one of them. Only the join waiters of user programs persist as join entries. The formal counterparts are
//! `CounterConflict.v` and `CounterConflict.tla`. Each property names the headline or the
//! invariant that it tests.

#[path = "support/mod.rs"]
mod support;

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, ListParWithRandom, Par};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use reified_strategies::{gint, gstring, tuple};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::merging::mergeable_tags::non_negative_mergeable_tag_name;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::{RhoHistoryRepository, RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::allocator;
use rholang::rust::interpreter::spaces::namespace::{self, NativeSpace, REIFIED_TAG};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{
    conflict_reason, conflicts, depends, MergeType, NumberChannelsDiff,
};
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::merger::state_change_merger::compute_trie_actions;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};
use support::props::{check, runtime_config, tokio_runtime, RUNTIME_CASES};
use support::session::{
    counter_channel, data_channel, error_codes, native, space_counter_channel, DEPLOY_SIGNATURE,
    FUNDING_SIGNATURE, PHLO,
};

/// The factory of the bag family.
const HASHMAP_BAG: &str = "rho:space:hashmap:bag";

/// The factory of the cell family.
const HASHMAP_CELL: &str = "rho:space:hashmap:cell";

/// The factory of the stack family.
const HASHMAP_STACK: &str = "rho:space:hashmap:stack";

/// The factory of the queue family.
const HASHMAP_QUEUE: &str = "rho:space:hashmap:queue";

/// The factory of the set family.
const HASHMAP_SET: &str = "rho:space:hashmap:set";

/// The factory of the priority-queue family.
const HASHMAP_PRIORITY_QUEUE: &str = "rho:space:hashmap:priorityqueue";

/// The factory of the Array family with the queue discipline.
const ARRAY_QUEUE: &str = "rho:space:array:queue";

/// The factory of the Vector family with the queue discipline.
const VECTOR_QUEUE: &str = "rho:space:vector:queue";

/// The constructor arguments of a family without params.
const DEFAULT: &str = "\"default\"";

/// The constructor arguments of a fixed Array space with eight index names.
const FIXED_ARRAY: &str = "\"default\"; size=8";

/// The constructor arguments of a cyclic Array space with four index names.
const CYCLIC_ARRAY: &str = "\"default\"; cyclic=true, size=4";

/// The public channel of the deliveries of the receive bodies of a test.
const OUT: &str = "rrs-p5-merge-out";

/// The public channel of the names that a `new` inside `use` makes.
const MINTED: &str = "rrs-p5-merge-minted";

/// The names of the three collection slots of a test space, as the deploys bind them.
const NAMES: [&str; 3] = ["a", "b", "c"];

// ─── The harness ─────────────────────────────────────────────────────────────────────────────

/// The source of the deploy that creates a space with the factory `urn` and the constructor
/// arguments `ctor`, and three names of it. The handle goes to `@"rrs-handle"` and the names go
/// to `@"rrs-names"`. An Array or a Vector space allocates the names from its allocator.
fn setup_program(urn: &str, ctor: &str) -> String {
    format!(
        r#"
new factory(`{urn}`) in {{
  for (space <- factory!?({ctor})) {{
    @"rrs-handle"!(*space) |
    for (a <- space!?("gensym")) {{
      for (b <- space!?("gensym")) {{
        for (c <- space!?("gensym")) {{
          @"rrs-names"!(*a, *b, *c)
        }}
      }}
    }}
  }}
}}
"#
    )
}

/// `body` with the handle of the setup space bound to `space` and its names bound to `a`, `b`
/// and `c`.
fn with_names(body: &str) -> String {
    format!("for (space <<- @\"rrs-handle\") {{ for (a, b, c <<- @\"rrs-names\") {{\n{body}\n}} }}")
}

/// One deploy as Casper processes it: the evaluation and its event log.
struct Deployed {
    result: EvaluateResult,
    log: Vec<Event>,
}

impl Deployed {
    /// The error codes of the deploy, in error order.
    fn codes(&self) -> Vec<String> { error_codes(&self.result.errors) }

    /// True when the deploy succeeded.
    fn ok(&self) -> bool { self.result.errors.is_empty() }
}

/// The execution model of the deploys of a node: the legacy model of the P5 cases, or the
/// native funded model (protocol v6) at the v6 host-work limits, which the P6 cases use
/// (decision 69).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Execution {
    Legacy,
    Native,
}

/// A play runtime and the history of one in-memory store, as a validator evaluates a block.
struct Node {
    play: RhoRuntimeImpl,
    history: RhoHistoryRepository,
    execution: Execution,
}

impl Node {
    /// A node of the legacy model over a fresh in-memory store, with a funded deploy signature.
    async fn new() -> Node { Node::with(Execution::Legacy).await }

    /// A node of the model `execution` over a fresh in-memory store, with the funded deploy
    /// signature of that model.
    async fn with(execution: Execution) -> Node {
        let mut manager = InMemoryStoreManager::new();
        let stores = manager
            .r_space_stores()
            .await
            .expect("create the in-memory RSpace stores");
        let (play, _replay, history) = create_runtimes(stores, false, &mut Vec::new()).await;
        match execution {
            Execution::Legacy => play.cost.set_deploy_signature_funded(
                DEPLOY_SIGNATURE,
                Sig::Ground(FUNDING_SIGNATURE.to_vec()),
            ),
            Execution::Native => play
                .cost
                .set_deploy_signature_funded(support::native::DEPLOY, support::native::funding()),
        }
        Node {
            play,
            history,
            execution,
        }
    }

    /// Evaluates one deploy as Casper processes a user deploy. It takes a soft checkpoint,
    /// evaluates `source` in the model of the node, takes the event log, and reverts a failed
    /// deploy. The label seeds the random state, so one label gives the same deploy in every run.
    async fn deploy(&mut self, source: &str, label: &str) -> Deployed {
        let fallback = self.play.create_soft_checkpoint().await;
        let random = Blake2b512Random::create_from_bytes(label.as_bytes());
        let result = match self.execution {
            Execution::Legacy => {
                self.play
                    .evaluate(
                        source,
                        Cost::create(PHLO, label.to_owned()),
                        HashMap::new(),
                        random,
                    )
                    .await
            }
            Execution::Native => {
                self.play
                    .evaluate_with_native_phlo(
                        source,
                        HashMap::new(),
                        random,
                        None,
                        support::native::config(
                            support::native::AMPLE_LIMIT,
                            support::native::host_work(),
                        ),
                    )
                    .await
            }
        }
        .unwrap_or_else(|error| panic!("{label}: the evaluation returns no result: {error:?}"));
        let log = self.play.take_event_log().await;
        if !result.errors.is_empty() {
            self.play.revert_to_soft_checkpoint(fallback).await;
        }
        Deployed { result, log }
    }

    /// Evaluates one deploy that must succeed, and returns its event log.
    async fn run(&mut self, source: &str, label: &str) -> Vec<Event> {
        let deployed = self.deploy(source, label).await;
        assert!(
            deployed.ok(),
            "{label}: the deploy fails: {:?}",
            deployed.result.errors
        );
        deployed.log
    }

    /// Checkpoints the play runtime and returns the root.
    async fn checkpoint(&mut self) -> Blake2b256Hash { self.play.create_checkpoint().await.root }

    /// Resets the play runtime to `root`.
    async fn reset(&mut self, root: &Blake2b256Hash) {
        self.play
            .reset(root)
            .await
            .expect("reset the play runtime to a checkpoint root");
    }

    /// The pars of each datum on the public channel `channel`.
    async fn values(&self, channel: &str) -> Vec<Vec<Par>> {
        self.play
            .get_data(&gstring(channel))
            .await
            .into_iter()
            .map(|datum| datum.a.pars)
            .collect()
    }

    /// The deliveries of the receive bodies on `OUT`, as sorted `(label, value)` pairs.
    async fn deliveries(&self) -> Vec<(String, i64)> {
        let mut deliveries: Vec<(String, i64)> = self
            .values(OUT)
            .await
            .iter()
            .map(|pars| delivery(pars.as_slice()))
            .collect();
        deliveries.sort();
        deliveries
    }

    /// The merge index of a deploy log over the pre-state `root`, with the pre-state closures
    /// of `block_index.rs::create_event_log_index`.
    fn index(&self, root: &Blake2b256Hash, log: &[Event]) -> EventLogIndex {
        let reader = self
            .history
            .get_history_reader(root)
            .expect("read the history at the pre-state root");
        let produce_exists_in_pre_state = |produce: &Produce| {
            reader
                .get_data(&produce.channel_hash)
                .is_ok_and(|data| data.iter().any(|datum| datum.source == *produce))
        };
        let produce_touches_pre_state_join = |produce: &Produce| {
            reader
                .get_joins(&produce.channel_hash)
                .is_ok_and(|joins| joins.iter().any(|join| join.len() > 1))
        };
        EventLogIndex::new(
            log.to_vec(),
            produce_exists_in_pre_state,
            produce_touches_pre_state_join,
            NumberChannelsDiff::new(),
        )
    }

    /// The merged state of branches that the merger accepts, as `dag_merger.rs` computes it.
    /// The state changes of the branches are combined, turned into trie actions over the base,
    /// and applied.
    fn merged_root(
        &self,
        base: &Blake2b256Hash,
        branches: &[(&Blake2b256Hash, &EventLogIndex)],
    ) -> Blake2b256Hash {
        let mut combined = StateChange::empty();
        let mut writers: HashMap<Blake2b256Hash, usize> = HashMap::new();
        for (post, index) in branches {
            let change = StateChange::new(
                self.history
                    .get_history_reader_struct(base)
                    .expect("the base reader"),
                self.history
                    .get_history_reader_struct(post)
                    .expect("the post-state reader"),
                index,
            )
            .expect("the state change of a branch");
            for entry in change.datums_changes.iter() {
                if !entry.value().added.is_empty() {
                    *writers.entry(entry.key().clone()).or_insert(0) += 1;
                }
            }
            combined = combined.combine(change);
        }
        let multi_writer: BTreeSet<Blake2b256Hash> = writers
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(channel, _)| channel)
            .collect();
        let reader = self
            .history
            .get_history_reader(base)
            .expect("the base reader");
        let actions =
            compute_trie_actions(&combined, &reader, &BTreeMap::new(), |hash, changes, _| {
                if !changes.added.is_empty() {
                    let base_data = reader.get_data(hash)?;
                    if !base_data.is_empty() || multi_writer.contains(hash) {
                        let base_binary = reader.get_data_proj_binary(hash)?;
                        RholangMergingLogic::check_single_value_cell_not_overfilled(
                            hash,
                            &base_data,
                            &base_binary,
                            changes,
                        )?;
                    }
                }
                Ok(None)
            })
            .expect("the trie actions of the merge");
        self.history
            .reset(base)
            .expect("reset the history to the base")
            .do_checkpoint(actions)
            .root()
    }

    /// The records on the channel with the hash `channel` at the state `root`.
    fn data_at(
        &self,
        root: &Blake2b256Hash,
        channel: &Blake2b256Hash,
    ) -> Vec<Datum<ListParWithRandom>> {
        self.history
            .get_history_reader(root)
            .expect("read the history at a root")
            .get_data(channel)
            .expect("read the records of a channel")
    }

    /// The joins of the channel with the hash `channel` at the state `root`.
    fn joins_at(&self, root: &Blake2b256Hash, channel: &Blake2b256Hash) -> Vec<Vec<Par>> {
        self.history
            .get_history_reader(root)
            .expect("read the history at a root")
            .get_joins(channel)
            .expect("read the joins of a channel")
    }
}

/// A branch: its deploy, its post-state root and the deliveries on `OUT` after it.
struct Branch {
    deployed: Deployed,
    post: Blake2b256Hash,
    out: Vec<(String, i64)>,
}

impl Branch {
    /// The event log of the branch.
    fn log(&self) -> &[Event] { &self.deployed.log }
}

/// A space of one family at a checkpointed pre-state. The setup deploy creates the space and
/// the three names `a`, `b` and `c`, and a seed deploy stores the first records.
struct Rig {
    node: Node,
    space: NativeSpace,
    names: [Par; 3],
    root: Blake2b256Hash,
    /// The logs of the setup deploy and of the seed deploy.
    setup_logs: Vec<Vec<Event>>,
}

impl Rig {
    /// Creates a space with the factory `urn` and the constructor arguments `ctor`. Then it runs
    /// the seed `seed` on the names, if the seed is not empty, and checkpoints the pre-state.
    /// The label `label` seeds the random states of the setup and of the seed. The deploys run
    /// in the legacy model.
    async fn new(urn: &str, ctor: &str, seed: &str, label: &str) -> Rig {
        Rig::with(Execution::Legacy, urn, ctor, seed, label).await
    }

    /// [`Rig::new`] with the deploys in the native funded model (decision 69).
    async fn native(urn: &str, ctor: &str, seed: &str, label: &str) -> Rig {
        Rig::with(Execution::Native, urn, ctor, seed, label).await
    }

    /// [`Rig::new`] with the deploys in the model `execution`.
    async fn with(execution: Execution, urn: &str, ctor: &str, seed: &str, label: &str) -> Rig {
        let mut node = Node::with(execution).await;
        let mut setup_logs = Vec::with_capacity(2);
        setup_logs.push(
            node.run(&setup_program(urn, ctor), &format!("{label}-setup"))
                .await,
        );
        let space = match <[Vec<Par>; 1]>::try_from(node.values("rrs-handle").await) {
            Ok([pars]) => match <[Par; 1]>::try_from(pars) {
                Ok([handle]) => native(&handle),
                Err(pars) => panic!("the handle datum has {} values", pars.len()),
            },
            Err(data) => panic!("@\"rrs-handle\" holds {} data", data.len()),
        };
        let names = match <[Vec<Par>; 1]>::try_from(node.values("rrs-names").await) {
            Ok([pars]) => <[Par; 3]>::try_from(pars).expect("the setup makes three names"),
            Err(data) => panic!("@\"rrs-names\" holds {} data", data.len()),
        };
        if !seed.is_empty() {
            setup_logs.push(node.run(&with_names(seed), &format!("{label}-seed")).await);
        }
        let root = node.checkpoint().await;
        Rig {
            node,
            space,
            names,
            root,
            setup_logs,
        }
    }

    /// The name of the slot `slot`.
    fn name(&self, slot: usize) -> &Par { &self.names[slot] }

    /// One branch: `body` evaluated from the pre-state root, with the label `label`.
    async fn branch(&mut self, body: &str, label: &str) -> Branch {
        self.node.reset(&self.root).await;
        let deployed = self.node.deploy(&with_names(body), label).await;
        let post = self.node.checkpoint().await;
        let out = self.node.deliveries().await;
        Branch {
            deployed,
            post,
            out,
        }
    }

    /// One branch that must succeed.
    async fn branch_ok(&mut self, body: &str, label: &str) -> Branch {
        let branch = self.branch(body, label).await;
        assert!(
            branch.deployed.ok(),
            "{label}: the branch fails: {:?}",
            branch.deployed.result.errors
        );
        branch
    }

    /// The serial run of `deploys` from the pre-state root, one deploy for each `(body, label)`
    /// pair, in order. It returns the deploys, the post-state root and the deliveries.
    async fn serial(
        &mut self,
        deploys: &[(&str, &str)],
    ) -> (Vec<Deployed>, Blake2b256Hash, Vec<(String, i64)>) {
        self.node.reset(&self.root).await;
        let mut deployed = Vec::with_capacity(deploys.len());
        for (body, label) in deploys {
            deployed.push(self.node.deploy(&with_names(body), label).await);
        }
        let post = self.node.checkpoint().await;
        let out = self.node.deliveries().await;
        (deployed, post, out)
    }

    /// The merge index of a log over the pre-state.
    fn index(&self, log: &[Event]) -> EventLogIndex { self.node.index(&self.root, log) }

    /// The channels on which two branch logs conflict.
    fn conflicts(&self, left: &[Event], right: &[Event]) -> BTreeSet<Blake2b256Hash> {
        conflicts(&self.index(left), &self.index(right))
            .0
            .into_iter()
            .collect()
    }

    /// The reason that the merger gives for the first conflict of two branch logs.
    fn reason(&self, left: &[Event], right: &[Event]) -> Option<String> {
        conflict_reason(&self.index(left), &self.index(right))
    }

    /// The merged state of the branches, each one with the log `log` that its index takes.
    fn merged(&self, branches: &[(&Branch, &[Event])]) -> Blake2b256Hash {
        let indices: Vec<(Blake2b256Hash, EventLogIndex)> = branches
            .iter()
            .map(|(branch, log)| (branch.post.clone(), self.index(log)))
            .collect();
        let refs: Vec<(&Blake2b256Hash, &EventLogIndex)> =
            indices.iter().map(|(post, index)| (post, index)).collect();
        self.node.merged_root(&self.root, &refs)
    }

    /// The hash of the counter channel `S(sid,k)` of the slot `slot`.
    fn counter(&self, slot: usize) -> Blake2b256Hash {
        hash(&counter_channel(self.space, self.name(slot)))
    }

    /// The hash of the data channel `D(sid,k)` of the slot `slot`.
    fn data(&self, slot: usize) -> Blake2b256Hash {
        hash(&data_channel(self.space, self.name(slot)))
    }

    /// The hash of the space counter `S(sid)`.
    fn space_counter(&self) -> Blake2b256Hash { hash(&space_counter_channel(self.space)) }

    /// The hash of the allocator `A(sid)` of an Array or a Vector space.
    fn allocator(&self) -> Blake2b256Hash { hash(&allocator::allocator_channel(self.space)) }

    /// The hashes of every counter channel of the space.
    fn counters(&self) -> Vec<Blake2b256Hash> {
        let mut counters = Vec::with_capacity(NAMES.len() + 1);
        counters.push(self.space_counter());
        counters.extend((0..NAMES.len()).map(|slot| self.counter(slot)));
        counters
    }
}

/// `log` without the events on the channels `channels`: the log that the space would make
/// without these channels. It generalizes `without_counters` of `reified_merge.rs`, because a
/// negative control can also strip only some counters or the allocator.
fn without_channels(log: &[Event], channels: &[Blake2b256Hash]) -> Vec<Event> {
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => !channels.contains(&produce.channel_hash),
            Event::IoEvent(IOEvent::Consume(consume)) => !consume
                .channel_hashes
                .iter()
                .any(|channel| channels.contains(channel)),
            Event::Comm(comm) => !comm
                .consume
                .channel_hashes
                .iter()
                .any(|channel| channels.contains(channel)),
        })
        .cloned()
        .collect()
}

/// The number of events of `log` that touch the channel with the hash `channel`.
fn touching(log: &[Event], channel: &Blake2b256Hash) -> usize {
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => produce.channel_hash == *channel,
            Event::IoEvent(IOEvent::Consume(consume)) => consume.channel_hashes.contains(channel),
            Event::Comm(comm) => comm.consume.channel_hashes.contains(channel),
        })
        .count()
}

/// The number of consume events of `log` on the channel with the hash `channel`. A consume
/// event on a data channel that a send makes is a synthesized consume: `K_lift` or `K_retire`.
fn consumes_on(log: &[Event], channel: &Blake2b256Hash) -> usize {
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Consume(consume)) => {
                consume.channel_hashes.as_slice() == std::slice::from_ref(channel)
            }
            Event::IoEvent(IOEvent::Produce(_)) | Event::Comm(_) => false,
        })
        .count()
}

/// The number of COMMs of `log` whose consume is on the channel with the hash `channel` alone.
fn comms_on(log: &[Event], channel: &Blake2b256Hash) -> usize {
    log.iter()
        .filter(|event| match event {
            Event::Comm(comm) => {
                comm.consume.channel_hashes.as_slice() == std::slice::from_ref(channel)
            }
            Event::IoEvent(_) => false,
        })
        .count()
}

/// The text of a `GString` par.
fn string_of(par: &Par) -> Option<&str> {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GString(value)),
        }] => Some(value),
        _ => None,
    }
}

/// The number of a `GInt` par.
fn number_of(par: &Par) -> Option<i64> {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }] => Some(*value),
        _ => None,
    }
}

/// A delivery `[label, value]` that a receive body sent to `OUT`.
fn delivery(pars: &[Par]) -> (String, i64) {
    match pars {
        [par] => match par.exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
            }] => match ps.as_slice() {
                [label, value] => (
                    string_of(label)
                        .expect("the label of a delivery is a string")
                        .to_owned(),
                    number_of(value).expect("the value of a delivery is a number"),
                ),
                other => panic!("a delivery has {} elements", other.len()),
            },
            other => panic!("a delivery is not a list: {other:?}"),
        },
        other => panic!("a delivery datum has {} values", other.len()),
    }
}

/// Two branches race on exactly the channels `expected` in merge check #1 (P4 design D.2), and
/// the same logs without the counter events merge. The branches run with the labels
/// `{label}-left` and `{label}-right`.
async fn assert_counter_race(
    rig: &mut Rig,
    left: &str,
    right: &str,
    expected: BTreeSet<Blake2b256Hash>,
    label: &str,
) -> (Branch, Branch) {
    let left = rig.branch_ok(left, &format!("{label}-left")).await;
    let right = rig.branch_ok(right, &format!("{label}-right")).await;
    assert_eq!(
        rig.conflicts(left.log(), right.log()),
        expected,
        "{label}: the branches race on the counters"
    );
    let reason = rig.reason(left.log(), right.log());
    assert!(
        reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("racesForSameIOEvent")),
        "{label}: merge check #1 finds the race: {reason:?}"
    );
    let counters = rig.counters();
    assert!(
        rig.conflicts(
            &without_channels(left.log(), &counters),
            &without_channels(right.log(), &counters)
        )
        .is_empty(),
        "{label}: without the counters the branches would merge"
    );
    (left, right)
}

// ─── Bag: no counter, so writes merge as on a host channel ───────────────────────────────────

/// Two branches that store records in one bag merge (M-1: a bag uses no counter). The merged
/// bag holds the records of both branches, and it is the state of each serial order. Two takes
/// of one record still race on the record itself, as on a host channel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_bag_writes_merge() {
    let label = "rrs-p5-merge-bag";
    let mut rig = Rig::new(HASHMAP_BAG, DEFAULT, "a!(10)", label).await;
    let left_label = format!("{label}-left");
    let right_label = format!("{label}-right");
    let left = rig.branch_ok("a!(1)", &left_label).await;
    let right = rig.branch_ok("a!(2)", &right_label).await;
    for branch in [&left, &right] {
        for counter in rig.counters() {
            assert_eq!(
                touching(branch.log(), &counter),
                0,
                "a bag operation touches no counter"
            );
        }
    }
    assert!(
        rig.conflicts(left.log(), right.log()).is_empty(),
        "bag writes conflict: {:?}",
        rig.reason(left.log(), right.log())
    );
    let (left_index, right_index) = (rig.index(left.log()), rig.index(right.log()));
    assert!(!depends(&left_index, &right_index));
    assert!(!depends(&right_index, &left_index));
    let merged = rig.merged(&[(&left, left.log()), (&right, right.log())]);
    let (_, serial, _) = rig
        .serial(&[("a!(1)", &left_label), ("a!(2)", &right_label)])
        .await;
    let (_, reversed, _) = rig
        .serial(&[("a!(2)", &right_label), ("a!(1)", &left_label)])
        .await;
    assert_eq!(merged, serial, "the merge is the serial state");
    assert_eq!(merged, reversed, "the merge is the reversed serial state");
    assert_eq!(
        rig.node.data_at(&merged, &rig.data(0)).len(),
        3,
        "the merged bag holds the seed record and both writes"
    );
    // The control: both takes consume the one pre-state record. So check #1 sees the race on
    // the data channel, as for two takes on a host channel.
    let take_left = rig
        .branch_ok("for (@x <- a) { Nil }", &format!("{label}-take-left"))
        .await;
    let take_right = rig
        .branch_ok("for (@y <- a) { Nil }", &format!("{label}-take-right"))
        .await;
    assert_eq!(
        rig.conflicts(take_left.log(), take_right.log()),
        BTreeSet::from([rig.data(0)]),
        "two takes of one record race on the record"
    );
}

// ─── Cell: one counter for every operation (decision 31) and a capacity of one (F15) ─────────

/// Two branches that each write one empty cell race on its counter (F23). Without the counter
/// events they merge, and the merged cell holds two records, which breaks the capacity of one
/// (F15). No serial order gives that state: the second write of each order fails with
/// `CellOccupied`, and the merge drops a failed deploy (`block_index.rs`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_cell_writes_race() {
    let label = "rrs-p5-merge-cell-writes";
    // The seed opens the cell `a` and leaves it empty: the receive takes the record, or the
    // send fires the parked receive.
    let mut rig = Rig::new(
        HASHMAP_CELL,
        DEFAULT,
        "a!(10) | for (@x <- a) { Nil }",
        label,
    )
    .await;
    let expected = BTreeSet::from([rig.counter(0)]);
    let (left, right) = assert_counter_race(&mut rig, "a!(1)", "a!(2)", expected, label).await;
    let counters = rig.counters();
    let left_free = without_channels(left.log(), &counters);
    let right_free = without_channels(right.log(), &counters);
    let merged = rig.merged(&[(&left, &left_free), (&right, &right_free)]);
    assert_eq!(
        rig.node.data_at(&merged, &rig.data(0)).len(),
        2,
        "without the counters a merge puts two records into one cell"
    );
    let left_label = format!("{label}-left");
    let right_label = format!("{label}-right");
    for order in [
        [
            ("a!(1)", left_label.as_str()),
            ("a!(2)", right_label.as_str()),
        ],
        [
            ("a!(2)", right_label.as_str()),
            ("a!(1)", left_label.as_str()),
        ],
    ] {
        let (deployed, post, _) = rig.serial(&order).await;
        assert!(deployed[0].ok(), "{:?}", deployed[0].result.errors);
        assert_eq!(deployed[1].codes(), vec!["CellOccupied".to_string()]);
        assert_eq!(
            rig.node.data_at(&post, &rig.data(0)).len(),
            1,
            "a serial order keeps one record in the cell"
        );
    }
}

/// Two branches whose first operations open cells of one space race on the space counter
/// `S(sid)` (A25), for two cells and for one cell.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_cell_first_ops_race() {
    let label = "rrs-p5-merge-cell-first";
    let mut rig = Rig::new(HASHMAP_CELL, DEFAULT, "", label).await;
    let expected = BTreeSet::from([rig.space_counter()]);
    assert_counter_race(
        &mut rig,
        "a!(1)",
        "b!(2)",
        expected.clone(),
        &format!("{label}-two-cells"),
    )
    .await;
    assert_counter_race(
        &mut rig,
        "a!(1)",
        "a!(2)",
        expected,
        &format!("{label}-one-cell"),
    )
    .await;
}

// ─── Stack: removals must bump the counter (M-1, crossed stacks) ─────────────────────────────

/// The crossed stacks of M-1 ("Why removals must touch it"). Branch A pushes 1 onto `a` and pops
/// `b`. Branch B pushes 2 onto `b` and pops `a`. Each operation bumps its counter, so the
/// branches race on both counters (`crossed_collections_race`). Without the counter events of
/// the pops, checks #1 to #3 find nothing. But no serial order gives the merged outcome. After
/// A, B pops 1 instead of 10, and after B, A pops 2 instead of 20
/// (`crossed_stacks_without_removal_counters_fabricate`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_crossed_stacks() {
    let label = "rrs-p5-merge-crossed";
    let mut rig = Rig::new(HASHMAP_STACK, DEFAULT, "a!(10) | b!(20)", label).await;
    let left_body = format!("a!(1) | for (@x <- b) {{ @\"{OUT}\"!([\"left\", x]) }}");
    let right_body = format!("b!(2) | for (@y <- a) {{ @\"{OUT}\"!([\"right\", y]) }}");
    let expected = BTreeSet::from([rig.counter(0), rig.counter(1)]);
    let (left, right) =
        assert_counter_race(&mut rig, &left_body, &right_body, expected, label).await;
    assert_eq!(left.out, vec![("left".to_string(), 20)]);
    assert_eq!(right.out, vec![("right".to_string(), 10)]);
    // RemovalCounterFree: the pushes keep their counter events, and the pops lose theirs.
    let left_free = without_channels(left.log(), &[rig.counter(1)]);
    let right_free = without_channels(right.log(), &[rig.counter(0)]);
    assert!(
        rig.conflicts(&left_free, &right_free).is_empty(),
        "without the removal counters the crossed stacks merge: {:?}",
        rig.reason(&left_free, &right_free)
    );
    let merged = rig.merged(&[(&left, &left_free), (&right, &right_free)]);
    let mut merged_out = left.out.clone();
    merged_out.extend(right.out.clone());
    merged_out.sort();
    let left_label = format!("{label}-left");
    let right_label = format!("{label}-right");
    let (serial, serial_root, serial_out) = rig
        .serial(&[(&left_body, &left_label), (&right_body, &right_label)])
        .await;
    let (reversed, reversed_root, reversed_out) = rig
        .serial(&[(&right_body, &right_label), (&left_body, &left_label)])
        .await;
    assert!(serial.iter().chain(reversed.iter()).all(Deployed::ok));
    assert_eq!(serial_out, vec![
        ("left".to_string(), 20),
        ("right".to_string(), 1)
    ]);
    assert_eq!(reversed_out, vec![
        ("left".to_string(), 2),
        ("right".to_string(), 10)
    ]);
    assert_ne!(
        merged_out, serial_out,
        "the merge is not the serial outcome"
    );
    assert_ne!(
        merged_out, reversed_out,
        "the merge is not the reversed outcome"
    );
    assert_ne!(merged, serial_root);
    assert_ne!(merged, reversed_root);
}

/// A push and a pop of one stack race on its counter. Two pops of one stack race on its counter
/// and on the top record. Operations on two stacks merge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_stack_push_pop_race() {
    let label = "rrs-p5-merge-push-pop";
    let mut rig = Rig::new(HASHMAP_STACK, DEFAULT, "a!(10) | b!(20)", label).await;
    let expected = BTreeSet::from([rig.counter(0)]);
    assert_counter_race(&mut rig, "a!(1)", "for (@x <- a) { Nil }", expected, label).await;
    let left = rig
        .branch_ok("for (@x <- a) { Nil }", &format!("{label}-pop-left"))
        .await;
    let right = rig
        .branch_ok(
            &format!("for (@y <- a) {{ @\"{OUT}\"!([\"right\", y]) }}"),
            &format!("{label}-pop-right"),
        )
        .await;
    assert_eq!(
        rig.conflicts(left.log(), right.log()),
        BTreeSet::from([rig.counter(0), rig.data(0)]),
        "two pops race on the counter and on the top record"
    );
    let other = rig
        .branch_ok("for (@z <- b) { Nil }", &format!("{label}-other"))
        .await;
    assert!(
        rig.conflicts(left.log(), other.log()).is_empty(),
        "operations on two stacks merge"
    );
    let (left_index, other_index) = (rig.index(left.log()), rig.index(other.log()));
    assert!(!depends(&left_index, &other_index));
    assert!(!depends(&other_index, &left_index));
}

// ─── Array and Vector: the allocator A(sid) (F11, NoDuplicateIndex) ──────────────────────────

/// Two branches that run `new` inside `use` of one Array or Vector space each run one allocation
/// job. Both jobs consume the pre-state produce of the allocator `A(sid)`, so merge check #1
/// sees the race. Without the allocator events the branches merge, and both hold the same index
/// name, which `NoDuplicateIndex` of `CounterConflict.tla` forbids.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_array_merge_new_in_use() {
    let body = format!(
        "new out in {{ use space {{ new n in {{ out!(*n) }} }} | for (@n <- out) {{ @\"{MINTED}\"!(n) }} }}"
    );
    for (store, urn, ctor) in [
        ("array-fixed", ARRAY_QUEUE, FIXED_ARRAY),
        ("array-cyclic", ARRAY_QUEUE, CYCLIC_ARRAY),
        ("vector", VECTOR_QUEUE, DEFAULT),
    ] {
        let label = format!("rrs-p5-merge-new-in-use-{store}");
        let mut rig = Rig::new(urn, ctor, "", &label).await;
        let left = rig.branch_ok(&body, &format!("{label}-left")).await;
        let left_minted = rig.node.values(MINTED).await;
        let right = rig.branch_ok(&body, &format!("{label}-right")).await;
        let right_minted = rig.node.values(MINTED).await;
        // The setup allocated the indices 0, 1 and 2, so each branch makes the index name 3.
        let third = vec![vec![allocator::index_name(rig.space, 3)]];
        assert_eq!(
            left_minted, third,
            "{store}: the left branch makes the index 3"
        );
        assert_eq!(
            right_minted, third,
            "{store}: the right branch makes the index 3"
        );
        for branch in [&left, &right] {
            assert_eq!(
                comms_on(branch.log(), &rig.allocator()),
                1,
                "{store}: the allocation job takes the allocator once"
            );
        }
        assert_eq!(
            rig.conflicts(left.log(), right.log()),
            BTreeSet::from([rig.allocator()]),
            "{store}: the branches race on the allocator"
        );
        let reason = rig.reason(left.log(), right.log());
        assert!(
            reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("racesForSameIOEvent")),
            "{store}: merge check #1 finds the race: {reason:?}"
        );
        // LazyAllocator: without the events of the allocator, the branches merge, and both hold
        // the index name 3.
        let allocator = [rig.allocator()];
        assert!(
            rig.conflicts(
                &without_channels(left.log(), &allocator),
                &without_channels(right.log(), &allocator)
            )
            .is_empty(),
            "{store}: without the allocator the branches would merge"
        );
    }
}

// ─── Consensus checklist item 18: no synthesized join ────────────────────────────────────────

/// The operations of one family for item 18: its label, its factory and constructor arguments,
/// and the body of each step deploy, in order.
struct JoinProgram {
    family: &'static str,
    urn: &'static str,
    ctor: &'static str,
    steps: Vec<String>,
}

/// A receive body that delivers `value` with the label `label`.
fn out(label: &str, value: &str) -> String { format!("@\"{OUT}\"!([\"{label}\", {value}])") }

/// The programs of item 18: every bag, cell, stack and allocator operation shape of wave W2.
fn join_programs() -> Vec<JoinProgram> {
    let use_new = format!(
        "new o in {{ use space {{ new n in {{ o!(*n) | n!(1) }} }} | for (@n <- o) {{ @\"{MINTED}\"!(n) }} }}"
    );
    let index_ops = format!(
        "a!(1) | for (@x <- a) {{ {} }} | for (@y <<- b) {{ {} }}",
        out("take", "x"),
        out("peek", "y")
    );
    vec![
        JoinProgram {
            family: "bag",
            urn: HASHMAP_BAG,
            ctor: DEFAULT,
            steps: vec![
                "a!(1) | a!(2) | a!!(3)".to_string(),
                format!("for (@x <- a) {{ {} }}", out("take", "x")),
                format!("for (@x <<- a) {{ {} }}", out("peek", "x")),
                format!(
                    "for (@9 <- b) {{ {} }} | for (@y <= c) {{ {} }}",
                    out("nine", "9"),
                    out("each", "y")
                ),
                "b!(9) | c!(4) | c!(5)".to_string(),
                format!("for (@x <<- b) {{ {} }}", out("peek-waiter", "x")),
                "b!(6)".to_string(),
            ],
        },
        JoinProgram {
            family: "cell",
            urn: HASHMAP_CELL,
            ctor: DEFAULT,
            steps: vec![
                "a!(1)".to_string(),
                format!("for (@x <<- a) {{ {} }}", out("peek", "x")),
                format!("for (@x <- a) {{ {} }}", out("take", "x")),
                format!("for (@7 <- a) {{ {} }}", out("seven", "7")),
                "a!(7)".to_string(),
                format!("for (@x <= b) {{ {} }}", out("each", "x")),
                "b!(3)".to_string(),
            ],
        },
        JoinProgram {
            family: "stack",
            urn: HASHMAP_STACK,
            ctor: DEFAULT,
            steps: vec![
                format!("for (@x <- a) {{ {} }}", out("w2", "x")),
                format!("for (@2 <- a) {{ {} }}", out("w", "2")),
                "a!(1)".to_string(),
                "a!(2)".to_string(),
                format!("b!(5) | for (@x <<- b) {{ {} }}", out("peek", "x")),
                format!("for (@x <- b) {{ {} }}", out("pop", "x")),
            ],
        },
        JoinProgram {
            family: "set",
            urn: HASHMAP_SET,
            ctor: DEFAULT,
            steps: vec![
                "a!(1) | a!(1) | a!(2)".to_string(),
                format!("for (@1 <- a) {{ {} }}", out("class", "1")),
                format!("for (@x <<- a) {{ {} }}", out("peek", "x")),
                "b!(3) | b!!(3)".to_string(),
                format!("for (@3 <- b) {{ {} }}", out("three", "3")),
                format!("for (@x <= c) {{ {} }} | c!(4) | c!(5)", out("each", "x")),
            ],
        },
        JoinProgram {
            family: "priorityqueue",
            urn: HASHMAP_PRIORITY_QUEUE,
            ctor: DEFAULT,
            steps: vec![
                "a!(1; priority=1) | a!(2; priority=0) | a!(3)".to_string(),
                format!("for (@x <- a) {{ {} }}", out("take", "x")),
                format!("for (@x <- a ~ (; priority=1)) {{ {} }}", out("lane", "x")),
                format!("for (@x <<- a) {{ {} }}", out("peek", "x")),
                format!(
                    "for (@x <- b) {{ {} }} | for (@9 <- b) {{ {} }}",
                    out("w", "x"),
                    out("nine", "9")
                ),
                "b!(9) | b!(4)".to_string(),
            ],
        },
        JoinProgram {
            family: "array",
            urn: ARRAY_QUEUE,
            ctor: FIXED_ARRAY,
            steps: vec![use_new.clone(), index_ops.clone()],
        },
        JoinProgram {
            family: "vector",
            urn: VECTOR_QUEUE,
            ctor: DEFAULT,
            steps: vec![use_new, index_ops],
        },
    ]
}

/// True when the channels `channels` of a consume name one collection. They are one channel, or
/// the `n` copies of one data channel of the class fire `[D; n]` of a set.
fn one_collection(channels: &[Blake2b256Hash]) -> bool {
    channels
        .split_first()
        .is_some_and(|(first, rest)| rest.iter().all(|channel| channel == first))
}

/// Item 18 at P5: every consume and every COMM of `logs` names one collection. That is one
/// channel, or the `n` copies of one channel of the class fire `[D; n]` of a set (wave W3). The
/// state at `root` holds no join of two or more channels on a channel of the logs. So the space machinery
/// adds no pre-state join for merge check #3. It returns the number of channels that it checked.
fn assert_no_join(node: &Node, root: &Blake2b256Hash, logs: &[Vec<Event>], label: &str) -> usize {
    let mut channels: BTreeSet<Blake2b256Hash> = BTreeSet::new();
    for event in logs.iter().flatten() {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => {
                channels.insert(produce.channel_hash.clone());
            }
            Event::IoEvent(IOEvent::Consume(consume)) => {
                assert!(
                    one_collection(&consume.channel_hashes),
                    "{label}: a consume names {} channels",
                    consume.channel_hashes.len()
                );
                channels.extend(consume.channel_hashes.iter().cloned());
            }
            Event::Comm(comm) => {
                assert!(
                    one_collection(&comm.consume.channel_hashes),
                    "{label}: a COMM consumes on {} channels",
                    comm.consume.channel_hashes.len()
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
    for channel in &channels {
        let joins = node.joins_at(root, channel);
        assert!(
            joins.iter().all(|join| join.len() < 2),
            "{label}: the state holds a join of {:?} channels",
            joins.iter().map(Vec::len).collect::<Vec<_>>()
        );
    }
    channels.len()
}

/// Consensus checklist item 18 at P5: the bag, cell, stack, set, priority-queue and allocator
/// operations use no join of two channels (I-16). This includes the `K_lift` of a settle and the
/// `K_alloc` of an allocation. It also includes the class fire `[D; n]` of a set, which consumes
/// on `n` copies of one channel. An ordinary host join is the control: the check sees it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_p5_synthesizes_no_join() {
    for program in join_programs() {
        let label = format!("rrs-p5-merge-no-join-{}", program.family);
        let mut rig = Rig::new(program.urn, program.ctor, "", &label).await;
        rig.node.reset(&rig.root).await;
        // The setup makes the space and its names. On an Array or a Vector space each gensym
        // call runs an allocation job with K_alloc.
        let mut logs = rig.setup_logs.clone();
        let checked = assert_no_join(&rig.node, &rig.root, &logs, &format!("{label} setup"));
        assert!(
            checked > 0,
            "{label}: the check reads the channels of the logs"
        );
        // Item 18 holds after every operation: the check runs at the root after each step.
        let mut steps = Vec::with_capacity(program.steps.len());
        for (step, body) in program.steps.iter().enumerate() {
            let log = rig
                .node
                .run(&with_names(body), &format!("{label}-step-{step}"))
                .await;
            logs.push(log.clone());
            steps.push(log);
            let root = rig.node.checkpoint().await;
            assert_no_join(&rig.node, &root, &logs, &format!("{label} step {step}"));
        }
        match program.family {
            "stack" => {
                // Step 3 sends 2 and fires the top waiter. The settle then lifts the record 1
                // with K_lift, the only consume of the send on `D(a)`, and fires the waiter
                // below.
                let send = &steps[3];
                assert_eq!(
                    consumes_on(send, &rig.data(0)),
                    1,
                    "the settle lifts the record with K_lift"
                );
                assert_eq!(comms_on(send, &rig.data(0)), 3, "w, K_lift and w2 fire");
            }
            "array" | "vector" => {
                // The setup deploy and the step with `new` inside `use` run allocation jobs.
                let allocations: usize =
                    logs.iter().map(|log| comms_on(log, &rig.allocator())).sum();
                assert_eq!(allocations, 4, "three gensym calls and one new run K_alloc");
            }
            "set" => {
                // Step 1 takes the class of 1 with one class fire `[D; 2]`: a consume on two
                // copies of `D(a)`.
                let class_fires = steps[1]
                    .iter()
                    .filter(|event| matches!(
                        event,
                        Event::Comm(comm) if comm.consume.channel_hashes == vec![rig.data(0), rig.data(0)]
                    ))
                    .count();
                assert_eq!(
                    class_fires, 1,
                    "the class fire binds both members of the class of 1"
                );
            }
            _ => {}
        }
    }
    // The control: an ordinary join on two public channels leaves a join of two channels, and
    // the reader of the check sees it.
    let mut node = Node::new().await;
    node.run(
        r#"for (@x <- @"rrs-p5-join-left" & @y <- @"rrs-p5-join-right") { Nil }"#,
        "rrs-p5-merge-no-join-control",
    )
    .await;
    let root = node.checkpoint().await;
    assert!(node
        .joins_at(&root, &hash(&gstring("rrs-p5-join-left")))
        .iter()
        .any(|join| join.len() == 2));
}

// ─── Consensus checklist item 19: namespaced channels are never mergeable ────────────────────

/// Consensus checklist item 19 at P5: the space operations of the wave W2 families add no
/// `0xE0` channel to `EvaluateResult.mergeable`. The control sends on a tuple channel headed by
/// the IntegerAdd tag outside every use block. Inside a use block of a HashMap space, the same
/// kind of tuple channel routes to the space. So it is a collection, not a mergeable channel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_p5_namespaced_channels_never_mergeable() {
    for (family, urn, ctor) in [
        ("bag", HASHMAP_BAG, DEFAULT),
        ("cell", HASHMAP_CELL, DEFAULT),
        ("stack", HASHMAP_STACK, DEFAULT),
        ("array", ARRAY_QUEUE, FIXED_ARRAY),
        ("vector", VECTOR_QUEUE, DEFAULT),
    ] {
        let label = format!("rrs-p5-merge-mergeable-{family}");
        let mut rig = Rig::new(urn, ctor, "", &label).await;
        rig.node.reset(&rig.root).await;
        let hashmap = !matches!(family, "array" | "vector");
        let in_use = match hashmap {
            true => "@(*tag, 2)!(6) | for (@v <- @(*tag, 2)) { Nil }",
            false => "new n in { n!(6) | for (@v <- n) { Nil } }",
        };
        let source = format!(
            "new tag(`rho:system:integerAddMergeableTag`) in {{ {} }}",
            with_names(&format!(
                "@(*tag, 1)!(5) | use space {{ {in_use} }} | a!(1) | for (@x <- a) {{ Nil }} | for (@y <<- b) {{ Nil }}"
            ))
        );
        let deployed = rig.node.deploy(&source, &label).await;
        assert!(deployed.ok(), "{family}: {:?}", deployed.result.errors);
        let tag = non_negative_mergeable_tag_name();
        let control = tuple(vec![tag.clone(), gint(1)]);
        let routed = tuple(vec![tag, gint(2)]);
        let mergeable = &deployed.result.mergeable;
        assert_eq!(
            mergeable.keys().cloned().collect::<Vec<_>>(),
            vec![control.clone()],
            "{family}: only the control channel is mergeable"
        );
        assert_eq!(mergeable.get(&control), Some(&MergeType::IntegerAdd));
        for channel in mergeable.keys() {
            assert!(
                namespace::sole_gprivate_id(channel)
                    .is_none_or(|id| id.first() != Some(&REIFIED_TAG)),
                "{family}: no 0xE0 channel is mergeable"
            );
        }
        if hashmap {
            assert_eq!(
                comms_on(&deployed.log, &hash(&data_channel(rig.space, &routed))),
                1,
                "{family}: the routed send and receive meet in the space"
            );
            assert_eq!(touching(&deployed.log, &hash(&routed)), 0);
        }
    }
}

// ─── Properties: the generated operations ────────────────────────────────────────────────────

/// The pattern of a generated receive: any value, or one literal value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pat {
    Any,
    Lit(i64),
}

/// The arrow of a generated receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arrow {
    Consume,
    Peek,
    Persistent,
}

impl Arrow {
    /// The Rholang arrow of the receive.
    fn symbol(self) -> &'static str {
        match self {
            Arrow::Consume => "<-",
            Arrow::Peek => "<<-",
            Arrow::Persistent => "<=",
        }
    }
}

/// One generated operation on the collection of the slot `slot`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SlotOp {
    Send {
        slot: usize,
        value: i64,
    },
    Receive {
        slot: usize,
        pat: Pat,
        arrow: Arrow,
        label: String,
    },
}

impl SlotOp {
    /// The slot of the operation.
    fn slot(&self) -> usize {
        match self {
            SlotOp::Send { slot, .. } | SlotOp::Receive { slot, .. } => *slot,
        }
    }

    /// The same operation on the slot `slot`.
    fn on(self, slot: usize) -> SlotOp {
        match self {
            SlotOp::Send { value, .. } => SlotOp::Send { slot, value },
            SlotOp::Receive {
                pat, arrow, label, ..
            } => SlotOp::Receive {
                slot,
                pat,
                arrow,
                label,
            },
        }
    }

    /// The process of the operation. A receive body delivers its label and its value to `OUT`.
    fn process(&self) -> String {
        match self {
            SlotOp::Send { slot, value } => format!("{}!({value})", NAMES[*slot]),
            SlotOp::Receive {
                slot,
                pat,
                arrow,
                label,
            } => {
                let (pattern, value) = match pat {
                    Pat::Any => ("x".to_string(), "x".to_string()),
                    Pat::Lit(value) => (value.to_string(), value.to_string()),
                };
                format!(
                    "for (@{pattern} {} {}) {{ {} }}",
                    arrow.symbol(),
                    NAMES[*slot],
                    out(label, &value)
                )
            }
        }
    }
}

/// The deploy body that runs `ops` in parallel.
fn program(ops: &[SlotOp]) -> String {
    match ops.is_empty() {
        true => "Nil".to_string(),
        false => ops
            .iter()
            .map(SlotOp::process)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

/// The seed body that opens the slots. `Some(records)` stores `records`, and an empty list opens
/// the collection and leaves it empty. `None` leaves the collection fresh.
fn seed(opened: &[Option<Vec<i64>>]) -> String {
    let mut parts = Vec::with_capacity(opened.len());
    for (slot, records) in opened.iter().enumerate() {
        match records.as_deref() {
            None => {}
            Some([]) => parts.push(format!(
                "{name}!(0) | for (@x <- {name}) {{ Nil }}",
                name = NAMES[slot]
            )),
            Some(records) => parts.extend(
                records
                    .iter()
                    .map(|value| format!("{}!({value})", NAMES[slot])),
            ),
        }
    }
    parts.join(" | ")
}

/// A generated value.
fn arb_value() -> BoxedStrategy<i64> { (0i64..4).boxed() }

/// A generated pattern.
fn arb_pat() -> BoxedStrategy<Pat> {
    prop_oneof![Just(Pat::Any), arb_value().prop_map(Pat::Lit)].boxed()
}

/// Any operation on the slot that terminates in its transaction: a send, a receive, a peek, or
/// a persistent receive that matches one number.
fn arb_slot_op(slot: usize, label: &'static str) -> BoxedStrategy<SlotOp> {
    prop_oneof![
        3 => arb_value().prop_map(move |value| SlotOp::Send { slot, value }),
        2 => arb_pat().prop_map(move |pat| SlotOp::Receive {
            slot,
            pat,
            arrow: Arrow::Consume,
            label: label.to_string(),
        }),
        1 => arb_pat().prop_map(move |pat| SlotOp::Receive {
            slot,
            pat,
            arrow: Arrow::Peek,
            label: label.to_string(),
        }),
        1 => arb_value().prop_map(move |value| SlotOp::Receive {
            slot,
            pat: Pat::Lit(value),
            arrow: Arrow::Persistent,
            label: label.to_string(),
        }),
    ]
    .boxed()
}

/// The records of an opened slot.
fn arb_opened() -> BoxedStrategy<Option<Vec<i64>>> { vec(arb_value(), 0..3).prop_map(Some).boxed() }

/// The label of a case: `prefix` and the hex form of the case seed.
fn case_label(prefix: &str, seed: &[u8]) -> String { format!("{prefix}-{}", hex::encode(seed)) }

// ─── Properties: CounterConflict.v ───────────────────────────────────────────────────────────

/// The cases of `prop_cell_ops_race_on_counter` that reached the race check on a fresh cell.
static CELL_RACES_FRESH: AtomicUsize = AtomicUsize::new(0);

/// The cases of `prop_cell_ops_race_on_counter` that reached the race check on an opened cell.
static CELL_RACES_OPENED: AtomicUsize = AtomicUsize::new(0);

/// The cases of `prop_merged_stack_state_is_serial` that compared a merge with the serial
/// states.
static STACK_MERGES_COMPARED: AtomicUsize = AtomicUsize::new(0);

/// The cases of `prop_merged_stack_state_is_serial` in which the merger found a conflict.
static STACK_MERGES_REJECTED: AtomicUsize = AtomicUsize::new(0);

/// `shared_collection_race` (F23 `cell_ops_race_on_counter` of v2): two branches from one
/// pre-state that each operate on one cell race in check #1, whatever else they do. The race is
/// on the counter `S(sid,k)` of an opened cell, and on the space counter `S(sid)` of a fresh
/// cell. A branch that fails with `CellOccupied` is not merged (`block_index.rs` drops a failed
/// deploy).
#[test]
fn prop_cell_ops_race_on_counter() {
    check(
        runtime_config(),
        (
            prop_oneof![Just(None), vec(arb_value(), 0..2).prop_map(Some)],
            (
                arb_slot_op(0, "left"),
                proptest::option::of(arb_slot_op(1, "left-b")),
            ),
            (
                arb_slot_op(0, "right"),
                proptest::option::of(arb_slot_op(1, "right-b")),
            ),
            vec(any::<u8>(), 4),
        ),
        |(opened, left, right, seed)| {
            tokio_runtime().block_on(cell_ops_race_on_counter_case(opened, left, right, seed))
        },
    );
    // A case whose branch fails with CellOccupied checks no race, so the counts show that the
    // property is not vacuous.
    let fresh = CELL_RACES_FRESH.load(Ordering::Relaxed);
    let opened = CELL_RACES_OPENED.load(Ordering::Relaxed);
    assert!(
        fresh >= 2 && opened >= 2 && 2 * (fresh + opened) >= RUNTIME_CASES as usize,
        "the race check ran on {fresh} fresh and {opened} opened cells of {RUNTIME_CASES} cases"
    );
    eprintln!("prop_cell_ops_race_on_counter: {fresh} fresh and {opened} opened cells checked");
}

/// One case of `prop_cell_ops_race_on_counter`.
async fn cell_ops_race_on_counter_case(
    opened: Option<Vec<i64>>,
    left: (SlotOp, Option<SlotOp>),
    right: (SlotOp, Option<SlotOp>),
    seed_bytes: Vec<u8>,
) -> Result<(), TestCaseError> {
    let label = case_label("rrs-p5-prop-cell-race", &seed_bytes);
    let fresh = opened.is_none();
    let mut rig = Rig::new(HASHMAP_CELL, DEFAULT, &seed(&[opened]), &label).await;
    let ops = |(first, other): (SlotOp, Option<SlotOp>)| -> Vec<SlotOp> {
        std::iter::once(first).chain(other).collect()
    };
    let left = rig
        .branch(&program(&ops(left)), &format!("{label}-left"))
        .await;
    let right = rig
        .branch(&program(&ops(right)), &format!("{label}-right"))
        .await;
    for branch in [&left, &right] {
        if !branch.deployed.ok() {
            prop_assert_eq!(branch.deployed.codes(), vec!["CellOccupied".to_string()]);
            return Ok(());
        }
    }
    let found = rig.conflicts(left.log(), right.log());
    let expected = match fresh {
        true => rig.space_counter(),
        false => rig.counter(0),
    };
    prop_assert!(
        found.contains(&expected),
        "no race on the counter of the cell: {:?}",
        rig.reason(left.log(), right.log())
    );
    match fresh {
        true => CELL_RACES_FRESH.fetch_add(1, Ordering::Relaxed),
        false => CELL_RACES_OPENED.fetch_add(1, Ordering::Relaxed),
    };
    Ok(())
}

/// `crossed_collections_race` (v2 `crossed_stacks_race`): branch A uses the stacks `a` and `b`,
/// and branch B uses `b` and `a`, in any order. The merge sees a race on both counters.
#[test]
fn prop_crossed_stacks_race() {
    check(
        runtime_config(),
        (
            vec(vec(arb_value(), 0..3), 2),
            (arb_slot_op(0, "a1"), arb_slot_op(1, "a2"), any::<bool>()),
            (arb_slot_op(1, "b2"), arb_slot_op(0, "b1"), any::<bool>()),
            vec(any::<u8>(), 4),
        ),
        |(records, first, second, seed)| {
            tokio_runtime().block_on(crossed_stacks_race_case(records, first, second, seed))
        },
    );
}

/// One case of `prop_crossed_stacks_race`.
async fn crossed_stacks_race_case(
    records: Vec<Vec<i64>>,
    first: (SlotOp, SlotOp, bool),
    second: (SlotOp, SlotOp, bool),
    seed_bytes: Vec<u8>,
) -> Result<(), TestCaseError> {
    let label = case_label("rrs-p5-prop-crossed", &seed_bytes);
    let opened: Vec<Option<Vec<i64>>> = records.into_iter().map(Some).collect();
    let mut rig = Rig::new(HASHMAP_STACK, DEFAULT, &seed(&opened), &label).await;
    let order = |(left, right, swap): (SlotOp, SlotOp, bool)| match swap {
        true => vec![right, left],
        false => vec![left, right],
    };
    let first = rig
        .branch(&program(&order(first)), &format!("{label}-left"))
        .await;
    let second = rig
        .branch(&program(&order(second)), &format!("{label}-right"))
        .await;
    for branch in [&first, &second] {
        prop_assert!(branch.deployed.ok(), "{:?}", branch.deployed.result.errors);
    }
    let found = rig.conflicts(first.log(), second.log());
    prop_assert!(
        found.contains(&rig.counter(0)),
        "no race on a: {:?}",
        rig.reason(first.log(), second.log())
    );
    prop_assert!(
        found.contains(&rig.counter(1)),
        "no race on b: {:?}",
        rig.reason(first.log(), second.log())
    );
    Ok(())
}

/// `crossed_stacks_without_removal_counters_fabricate`: the stacks `a` and `b` hold the tops `t1`
/// and `t2`. Branch A pushes `v1` onto `a` and pops `b`, and branch B pushes `v2` onto `b` and
/// pops `a`. With the counters of every operation the branches race on both counters. Without
/// the counter events of the pops they merge, but no serial order gives the merged outcome.
#[test]
fn prop_crossed_stacks_without_removal_counters_fabricate() {
    check(
        runtime_config(),
        (
            (arb_value(), arb_value()),
            (10i64..14, 10i64..14),
            vec(any::<u8>(), 4),
        ),
        |(tops, pushes, seed)| {
            tokio_runtime().block_on(crossed_stacks_fabricate_case(tops, pushes, seed))
        },
    );
}

/// One case of `prop_crossed_stacks_without_removal_counters_fabricate`.
async fn crossed_stacks_fabricate_case(
    (t1, t2): (i64, i64),
    (v1, v2): (i64, i64),
    seed_bytes: Vec<u8>,
) -> Result<(), TestCaseError> {
    let label = case_label("rrs-p5-prop-crossed-free", &seed_bytes);
    let mut rig = Rig::new(
        HASHMAP_STACK,
        DEFAULT,
        &format!("a!({t1}) | b!({t2})"),
        &label,
    )
    .await;
    let left_body = format!("a!({v1}) | for (@x <- b) {{ {} }}", out("left", "x"));
    let right_body = format!("b!({v2}) | for (@y <- a) {{ {} }}", out("right", "y"));
    let left_label = format!("{label}-left");
    let right_label = format!("{label}-right");
    let left = rig.branch(&left_body, &left_label).await;
    let right = rig.branch(&right_body, &right_label).await;
    for branch in [&left, &right] {
        prop_assert!(branch.deployed.ok(), "{:?}", branch.deployed.result.errors);
    }
    prop_assert_eq!(
        rig.conflicts(left.log(), right.log()),
        BTreeSet::from([rig.counter(0), rig.counter(1)])
    );
    let left_free = without_channels(left.log(), &[rig.counter(1)]);
    let right_free = without_channels(right.log(), &[rig.counter(0)]);
    prop_assert!(
        rig.conflicts(&left_free, &right_free).is_empty(),
        "the removal-counter-free logs conflict: {:?}",
        rig.reason(&left_free, &right_free)
    );
    let merged = rig.merged(&[(&left, &left_free), (&right, &right_free)]);
    let mut merged_out = left.out.clone();
    merged_out.extend(right.out.clone());
    merged_out.sort();
    prop_assert_eq!(&merged_out, &vec![
        ("left".to_string(), t2),
        ("right".to_string(), t1)
    ]);
    let (serial, serial_root, serial_out) = rig
        .serial(&[(&left_body, &left_label), (&right_body, &right_label)])
        .await;
    let (reversed, reversed_root, reversed_out) = rig
        .serial(&[(&right_body, &right_label), (&left_body, &left_label)])
        .await;
    prop_assert!(serial.iter().chain(reversed.iter()).all(Deployed::ok));
    prop_assert_ne!(&merged_out, &serial_out);
    prop_assert_ne!(&merged_out, &reversed_out);
    prop_assert_ne!(&merged, &serial_root);
    prop_assert_ne!(&merged, &reversed_root);
    Ok(())
}

/// `unordered_writes_commute` for the bag family: two branches of stores on the bags `a` and `b`
/// touch no counter and do not conflict. The merged state is the state of both serial orders.
#[test]
fn prop_bag_writes_commute() {
    check(
        runtime_config(),
        (
            vec(arb_value(), 0..3),
            vec((0usize..2, arb_value()), 1..4),
            vec((0usize..2, arb_value()), 1..4),
            vec(any::<u8>(), 4),
        ),
        |(records, first, second, seed)| {
            tokio_runtime().block_on(bag_writes_commute_case(records, first, second, seed))
        },
    );
}

/// One case of `prop_bag_writes_commute`.
async fn bag_writes_commute_case(
    records: Vec<i64>,
    first: Vec<(usize, i64)>,
    second: Vec<(usize, i64)>,
    seed_bytes: Vec<u8>,
) -> Result<(), TestCaseError> {
    let label = case_label("rrs-p5-prop-bag", &seed_bytes);
    let mut rig = Rig::new(HASHMAP_BAG, DEFAULT, &seed(&[Some(records)]), &label).await;
    let writes = |writes: &[(usize, i64)]| -> String {
        program(
            &writes
                .iter()
                .map(|(slot, value)| SlotOp::Send {
                    slot: *slot,
                    value: *value,
                })
                .collect::<Vec<_>>(),
        )
    };
    let (first_body, second_body) = (writes(&first), writes(&second));
    let (first_label, second_label) = (format!("{label}-left"), format!("{label}-right"));
    let left = rig.branch(&first_body, &first_label).await;
    let right = rig.branch(&second_body, &second_label).await;
    for branch in [&left, &right] {
        prop_assert!(branch.deployed.ok(), "{:?}", branch.deployed.result.errors);
        for counter in rig.counters() {
            prop_assert_eq!(touching(branch.log(), &counter), 0);
        }
    }
    prop_assert!(
        rig.conflicts(left.log(), right.log()).is_empty(),
        "bag writes conflict: {:?}",
        rig.reason(left.log(), right.log())
    );
    let merged = rig.merged(&[(&left, left.log()), (&right, right.log())]);
    let (_, serial, _) = rig
        .serial(&[(&first_body, &first_label), (&second_body, &second_label)])
        .await;
    let (_, reversed, _) = rig
        .serial(&[(&second_body, &second_label), (&first_body, &first_label)])
        .await;
    prop_assert_eq!(&merged, &serial);
    prop_assert_eq!(&merged, &reversed);
    Ok(())
}

// ─── Properties: CounterConflict.tla (MC_CounterConflict_Stacks.cfg) ─────────────────────────

/// `NoMergedOrderFabrication`, `SharedQueueRaces` and `FirstOpsRace` of `CounterConflict.tla`
/// with stacks (`MC_CounterConflict_Stacks.cfg`): two branches use three stacks of one space,
/// opened or fresh. They conflict whenever they use one stack or both open a stack. Every merge
/// that the merger accepts is the state of a serial order.
#[test]
fn prop_merged_stack_state_is_serial() {
    check(
        runtime_config(),
        (
            vec(prop_oneof![1 => Just(None), 2 => arb_opened()], 3),
            0u8..3,
            vec((0usize..3, arb_slot_op(0, "a")), 1..3),
            vec((0usize..3, arb_slot_op(0, "b")), 1..3),
            vec(any::<u8>(), 4),
        ),
        |(opened, layout, first, second, seed)| {
            tokio_runtime().block_on(merged_stack_state_is_serial_case(
                opened, layout, first, second, seed,
            ))
        },
    );
    let compared = STACK_MERGES_COMPARED.load(Ordering::Relaxed);
    let rejected = STACK_MERGES_REJECTED.load(Ordering::Relaxed);
    assert!(
        compared >= 1 && rejected >= 1,
        "{compared} cases compared a merge and {rejected} cases found a conflict"
    );
    eprintln!("prop_merged_stack_state_is_serial: {compared} merges compared, {rejected} rejected");
}

/// One case of `prop_merged_stack_state_is_serial`.
async fn merged_stack_state_is_serial_case(
    opened: Vec<Option<Vec<i64>>>,
    layout: u8,
    first: Vec<(usize, SlotOp)>,
    second: Vec<(usize, SlotOp)>,
    seed_bytes: Vec<u8>,
) -> Result<(), TestCaseError> {
    // Layout 0 keeps the branches on two stacks, layout 1 lets them share no stack or one,
    // and layout 2 lets them use any stack.
    let (first, second): (Vec<SlotOp>, Vec<SlotOp>) = match layout {
        0 => (
            first.into_iter().map(|(_, op)| op.on(0)).collect(),
            second.into_iter().map(|(_, op)| op.on(1)).collect(),
        ),
        1 => (
            first
                .into_iter()
                .map(|(slot, op)| op.on(slot % 2))
                .collect(),
            second.into_iter().map(|(_, op)| op.on(2)).collect(),
        ),
        _ => (
            first.into_iter().map(|(slot, op)| op.on(slot)).collect(),
            second.into_iter().map(|(slot, op)| op.on(slot)).collect(),
        ),
    };
    let slots = |ops: &[SlotOp]| -> BTreeSet<usize> { ops.iter().map(SlotOp::slot).collect() };
    let shared = !slots(&first).is_disjoint(&slots(&second));
    let opens = |ops: &[SlotOp]| slots(ops).iter().any(|slot| opened[*slot].is_none());
    let both_open = opens(&first) && opens(&second);
    let label = case_label("rrs-p5-prop-stack-serial", &seed_bytes);
    let mut rig = Rig::new(HASHMAP_STACK, DEFAULT, &seed(&opened), &label).await;
    let (first_body, second_body) = (program(&first), program(&second));
    let (first_label, second_label) = (format!("{label}-left"), format!("{label}-right"));
    let left = rig.branch(&first_body, &first_label).await;
    let right = rig.branch(&second_body, &second_label).await;
    for branch in [&left, &right] {
        prop_assert!(branch.deployed.ok(), "{:?}", branch.deployed.result.errors);
    }
    let found = rig.conflicts(left.log(), right.log());
    if shared || both_open {
        prop_assert!(
            !found.is_empty(),
            "a shared stack or two first operations merged: {:?}",
            rig.reason(left.log(), right.log())
        );
    }
    if found.is_empty() {
        let merged = rig.merged(&[(&left, left.log()), (&right, right.log())]);
        let (_, serial, _) = rig
            .serial(&[(&first_body, &first_label), (&second_body, &second_label)])
            .await;
        let (_, reversed, _) = rig
            .serial(&[(&second_body, &second_label), (&first_body, &first_label)])
            .await;
        prop_assert!(
            merged == serial || merged == reversed,
            "the merge is not a serial state"
        );
    }
    match found.is_empty() {
        true => STACK_MERGES_COMPARED.fetch_add(1, Ordering::Relaxed),
        false => STACK_MERGES_REJECTED.fetch_add(1, Ordering::Relaxed),
    };
    Ok(())
}

// ─── Set (wave W3): no counter, so writes merge, and retirements race (F24, F25) ─────────────

/// Two branches that store records in one set merge (F25 `unordered_set_writes_commute`): a set
/// uses no counter, so equal and new values commute. The merged set holds the records of both
/// branches, and it is the state of each serial order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_set_writes_merge() {
    let label = "rrs-p5-merge-set";
    let mut rig = Rig::new(HASHMAP_SET, DEFAULT, "a!(10)", label).await;
    let (left_label, right_label) = (format!("{label}-left"), format!("{label}-right"));
    let (left_body, right_body) = ("a!(1) | a!(10)", "a!(2) | a!(10)");
    let left = rig.branch_ok(left_body, &left_label).await;
    let right = rig.branch_ok(right_body, &right_label).await;
    for branch in [&left, &right] {
        for counter in rig.counters() {
            assert_eq!(
                touching(branch.log(), &counter),
                0,
                "a set operation touches no counter"
            );
        }
    }
    assert!(
        rig.conflicts(left.log(), right.log()).is_empty(),
        "set writes conflict: {:?}",
        rig.reason(left.log(), right.log())
    );
    let merged = rig.merged(&[(&left, left.log()), (&right, right.log())]);
    let (_, serial, _) = rig
        .serial(&[(left_body, &left_label), (right_body, &right_label)])
        .await;
    let (_, reversed, _) = rig
        .serial(&[(right_body, &right_label), (left_body, &left_label)])
        .await;
    assert_eq!(merged, serial, "the merge is the serial state");
    assert_eq!(merged, reversed, "the merge is the reversed serial state");
    assert_eq!(
        rig.node.data_at(&merged, &rig.data(0)).len(),
        5,
        "the merged set holds the seed record and the four writes"
    );
}

/// Two branches that retire the members of one class race on their produces (F24
/// `set_retirements_race`). The class of 1 has two members. The left branch takes the class with
/// one class fire `[D; 2]`. The right branch binds the class in a join, and the join retires the
/// other member with `K_retire`. Both branches consume both members, so merge check #1 sees
/// two produce races on `D(a)`. Two class fires race the same way.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_set_retirements_race() {
    let label = "rrs-p5-merge-set-retire";
    let mut rig = Rig::new(HASHMAP_SET, DEFAULT, "a!(1) | a!(1) | b!(5)", label).await;
    let class_fire = format!("for (@1 <- a) {{ {} }}", out("class", "1"));
    let join = format!("for (@1 <- a & @y <- b) {{ {} }}", out("join", "y"));
    let left = rig.branch_ok(&class_fire, &format!("{label}-class")).await;
    let right = rig.branch_ok(&join, &format!("{label}-join")).await;
    assert_eq!(
        consumes_on(right.log(), &rig.data(0)),
        1,
        "the join retires the other member with one K_retire"
    );
    assert_eq!(left.out, vec![("class".to_string(), 1)]);
    assert_eq!(right.out, vec![("join".to_string(), 5)]);
    assert_eq!(
        rig.conflicts(left.log(), right.log()),
        BTreeSet::from([rig.data(0)]),
        "the retirements race on the members"
    );
    assert_eq!(
        rig.reason(left.log(), right.log()).as_deref(),
        Some("racesForSameIOEvent: produceRaces=2"),
        "both members are consumed in both branches"
    );
    let twin = rig
        .branch_ok(&class_fire, &format!("{label}-class-twin"))
        .await;
    assert_eq!(
        rig.conflicts(left.log(), twin.log()),
        BTreeSet::from([rig.data(0)]),
        "two class fires race on the members"
    );
}

// ─── Priority queue (wave W3): one counter for every operation (decision 31) ─────────────────

/// Two dequeues of one priority queue race on its counter and on the head record that both
/// take. A send and a dequeue race on the counter only, and without the counter events they
/// merge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_pq_dequeue_race() {
    let label = "rrs-p5-merge-pq-dequeue";
    let mut rig = Rig::new(
        HASHMAP_PRIORITY_QUEUE,
        DEFAULT,
        "a!(10; priority=0) | a!(20; priority=1)",
        label,
    )
    .await;
    let expected = BTreeSet::from([rig.counter(0)]);
    assert_counter_race(
        &mut rig,
        "a!(1; priority=0)",
        "for (@x <- a) { Nil }",
        expected,
        &format!("{label}-send"),
    )
    .await;
    let left = rig
        .branch_ok(
            &format!("for (@x <- a) {{ {} }}", out("left", "x")),
            &format!("{label}-left"),
        )
        .await;
    let right = rig
        .branch_ok(
            &format!("for (@y <- a) {{ {} }}", out("right", "y")),
            &format!("{label}-right"),
        )
        .await;
    assert_eq!(left.out, vec![("left".to_string(), 10)]);
    assert_eq!(right.out, vec![("right".to_string(), 10)]);
    assert_eq!(
        rig.conflicts(left.log(), right.log()),
        BTreeSet::from([rig.counter(0), rig.data(0)]),
        "two dequeues race on the counter and on the head record"
    );
}

/// A peek of a priority queue bumps its counter (decision 31). A peek keeps the head record,
/// because Rule E sends it again with its identity (decision 45). The merge index counts no
/// consume of a peek COMM. So a peek races with a send and with a dequeue on the counter alone.
/// Without the counter events each pair merges, so the counter of a peek is necessary.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_pq_peek_race() {
    let label = "rrs-p5-merge-pq-peek";
    let mut rig = Rig::new(HASHMAP_PRIORITY_QUEUE, DEFAULT, "a!(10; priority=0)", label).await;
    let peek = format!("for (@x <<- a) {{ {} }}", out("peek", "x"));
    let dequeue = format!("for (@y <- a) {{ {} }}", out("take", "y"));
    let expected = BTreeSet::from([rig.counter(0)]);
    assert_counter_race(
        &mut rig,
        &peek,
        "a!(1; priority=1)",
        expected.clone(),
        &format!("{label}-send"),
    )
    .await;
    let (left, right) = assert_counter_race(
        &mut rig,
        &peek,
        &dequeue,
        expected,
        &format!("{label}-dequeue"),
    )
    .await;
    assert_eq!(left.out, vec![("peek".to_string(), 10)]);
    assert_eq!(right.out, vec![("take".to_string(), 10)]);
}

// ─── Joins (P5c, joins plan section 5) ───────────────────────────────────────────────────────

/// The seed that opens the collections of `a`, `b` and `c` and leaves them empty, so no branch
/// opens a collection and races on `S(sid)`.
const OPEN_ALL: &str =
    "a!(0) | b!(0) | c!(0) | for (@x <- a) { Nil } | for (@y <- b) { Nil } | for (@z <- c) { Nil }";

/// The join of `a` and `b` whose body delivers the value of `a`.
fn join_ab() -> String { format!("for (@x <- a & @y <- b) {{ {} }}", out("join", "x")) }

/// A join bumps the counter of each of its collections (decision 31, joins plan 5). So it races
/// with a branch that uses any of them (`join_ops_race_on_every_counter`, `CounterConflict.v`).
/// The join over `a` and `b` races with a receive on `a` on `S(sid,a)`. It races with a receive on
/// `b` and with a join over `b` and `c` on `S(sid,b)`. Without the counter events each pair
/// merges.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_join_ops_race_all_counters() {
    let label = "rrs-p5-merge-join-counters";
    let mut rig = Rig::new(HASHMAP_QUEUE, DEFAULT, OPEN_ALL, label).await;
    let join = join_ab();
    let on_b_and_c = format!("for (@y <- b & @z <- c) {{ {} }}", out("bc", "y"));
    for (slot, other, tag) in [
        (vec![0], "for (@z <- a) { Nil }".to_string(), "a"),
        (vec![1], "for (@z <- b) { Nil }".to_string(), "b"),
        (vec![1], on_b_and_c, "bc"),
    ] {
        let expected: BTreeSet<Blake2b256Hash> =
            slot.iter().map(|slot| rig.counter(*slot)).collect();
        assert_counter_race(&mut rig, &join, &other, expected, &format!("{label}-{tag}")).await;
    }
}

/// A join over `a` and `b` and operations on `c` merge: the join bumps only the counters of its
/// own collections. The merged state is the state of each serial order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_unrelated_collections_merge() {
    let label = "rrs-p5-merge-join-unrelated";
    let mut rig = Rig::new(HASHMAP_QUEUE, DEFAULT, OPEN_ALL, label).await;
    let (left_label, right_label) = (format!("{label}-left"), format!("{label}-right"));
    let join = join_ab();
    let other = format!("c!(7) | for (@z <- c) {{ {} }}", out("c", "z"));
    let left = rig.branch_ok(&join, &left_label).await;
    let right = rig.branch_ok(&other, &right_label).await;
    assert!(
        rig.conflicts(left.log(), right.log()).is_empty(),
        "unrelated collections conflict: {:?}",
        rig.reason(left.log(), right.log())
    );
    let (left_index, right_index) = (rig.index(left.log()), rig.index(right.log()));
    assert!(!depends(&left_index, &right_index));
    assert!(!depends(&right_index, &left_index));
    let merged = rig.merged(&[(&left, left.log()), (&right, right.log())]);
    let (_, serial, _) = rig
        .serial(&[(&join, &left_label), (&other, &right_label)])
        .await;
    let (_, reversed, _) = rig
        .serial(&[(&other, &right_label), (&join, &left_label)])
        .await;
    assert_eq!(merged, serial, "the merge is the serial state");
    assert_eq!(merged, reversed, "the merge is the reversed serial state");
}

/// Merge check #2 for a bag join (joins plan 5). Bag and set joins have no counter. A join over
/// `a` and `b` parks in one branch, and a send on `a` stores in the other. They meet as a
/// potential COMM on `D(a)`. A send on `c` merges with the join.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_bag_join_potential_comm() {
    let label = "rrs-p5-merge-bag-join";
    let mut rig = Rig::new(HASHMAP_BAG, DEFAULT, "b!(1)", label).await;
    let join = join_ab();
    let left = rig.branch_ok(&join, &format!("{label}-left")).await;
    let right = rig.branch_ok("a!(2)", &format!("{label}-right")).await;
    assert!(left.out.is_empty() && right.out.is_empty());
    assert_eq!(
        rig.conflicts(left.log(), right.log()),
        BTreeSet::from([rig.data(0)]),
        "the parked join and the stored send meet on D(a)"
    );
    let reason = rig.reason(left.log(), right.log());
    assert!(
        reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("potentialCOMMs")),
        "merge check #2 finds the potential COMM: {reason:?}"
    );
    let other = rig.branch_ok("c!(2)", &format!("{label}-other")).await;
    assert!(
        rig.conflicts(left.log(), other.log()).is_empty(),
        "a send on another collection merges with the join"
    );
}

/// A program of `reified_p5c_only_user_joins_persist`: the family label, the factory, the step
/// bodies, and the slots of the joins that the user writes.
type UserJoinProgram = (&'static str, &'static str, Vec<String>, Vec<[usize; 2]>);

/// The programs of `reified_p5c_only_user_joins_persist`: for each family, the step bodies and
/// the slots of the joins that the user writes.
fn user_join_programs() -> Vec<UserJoinProgram> {
    let lifts = vec![
        join_ab(),
        format!("for (@z <- b) {{ {} }}", out("v", "z")),
        "b!(10) | b!(20)".to_string(),
        "a!(1)".to_string(),
        format!(
            "for (@x <<- a & @y <- c) {{ {} }} | a!(5) | c!(6)",
            out("peek", "x")
        ),
    ];
    vec![
        ("queue", HASHMAP_QUEUE, lifts.clone(), vec![[0, 1], [0, 2]]),
        ("stack", HASHMAP_STACK, lifts, vec![[0, 1], [0, 2]]),
        (
            "bag",
            HASHMAP_BAG,
            vec![
                "a!(1) | b!!(2)".to_string(),
                format!("for (@x <<- a & @y <- b) {{ {} }}", out("peek", "x")),
                join_ab(),
                format!("for (@x <- a & @y <- c) {{ {} }}", out("ac", "x")),
                "c!(3)".to_string(),
            ],
            vec![[0, 1], [0, 2]],
        ),
        (
            "set",
            HASHMAP_SET,
            vec![
                "a!(1) | a!(1) | a!(2) | b!(5)".to_string(),
                format!("for (@1 <- a & @y <- b) {{ {} }}", out("retire", "y")),
                format!("for (@x <- a & @y <- c) {{ {} }}", out("ac", "x")),
                "c!(3)".to_string(),
            ],
            vec![[0, 1], [0, 2]],
        ),
    ]
}

/// Item 18 at P5c (joins plan 5, I-16): only the join of a user program persists as a join entry.
/// After each step of a join program, take every join entry of two or more channels on a channel
/// of the space. It is the group of a parked user join, whose continuation is a record of
/// version 2. In the logs, every consume that names two or more collections is the consume of a
/// user join. So `K_lift`, `K_retire`, the counter bumps and the second sends of Rule E add no
/// join. The queue program makes a `K_lift` and the set program a `K_retire`, both single-channel
/// consumes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_p5c_only_user_joins_persist() {
    for (family, urn, steps, joins) in user_join_programs() {
        let label = format!("rrs-p5-merge-user-joins-{family}");
        let mut rig = Rig::new(urn, DEFAULT, "", &label).await;
        rig.node.reset(&rig.root).await;
        let user: Vec<BTreeSet<Blake2b256Hash>> = joins
            .iter()
            .map(|slots| slots.iter().map(|slot| rig.data(*slot)).collect())
            .collect();
        let data: Vec<Blake2b256Hash> = (0..NAMES.len()).map(|slot| rig.data(slot)).collect();
        let mut persisted = 0;
        let mut synthesized = Vec::with_capacity(steps.len());
        for (step, body) in steps.iter().enumerate() {
            let log = rig
                .node
                .run(&with_names(body), &format!("{label}-step-{step}"))
                .await;
            synthesized.push(
                data.iter()
                    .map(|channel| consumes_on(&log, channel))
                    .sum::<usize>(),
            );
            for event in &log {
                let channels = match event {
                    Event::IoEvent(IOEvent::Consume(consume)) => &consume.channel_hashes,
                    Event::Comm(comm) => &comm.consume.channel_hashes,
                    Event::IoEvent(IOEvent::Produce(_)) => continue,
                };
                if one_collection(channels) {
                    continue;
                }
                let named: BTreeSet<Blake2b256Hash> = channels.iter().cloned().collect();
                assert!(
                    user.contains(&named),
                    "{label} step {step}: a consume on {} collections is not a user join",
                    named.len()
                );
            }
            let root = rig.node.checkpoint().await;
            for channel in &data {
                for group in rig.node.joins_at(&root, channel) {
                    if group.len() < 2 {
                        continue;
                    }
                    let waiters = rig.node.play.get_continuations(group.clone()).await;
                    assert!(
                        !waiters.is_empty(),
                        "{label} step {step}: a join entry has no waiter"
                    );
                    for waiter in waiters {
                        assert!(
                            rholang::rust::interpreter::spaces::record::unwrap_join_continuation(
                                &waiter.continuation
                            )
                            .is_ok(),
                            "{label} step {step}: a join entry is not the group of a user join"
                        );
                        persisted += 1;
                    }
                }
            }
        }
        assert!(persisted > 0, "{label}: a user join persists at some step");
        match family {
            "queue" => assert_eq!(
                synthesized[3], 1,
                "{label}: the cascade lifts the next head of b with K_lift"
            ),
            "set" => assert_eq!(
                synthesized[1], 1,
                "{label}: the join retires the other member of the class with K_retire"
            ),
            _ => {}
        }
    }
}

// ─── Properties: F24 and F25 (wave W3) ───────────────────────────────────────────────────────

/// `set_retirements_race` (F24, `CounterConflict.v`). Two branches each retire the members of
/// one class of a set, through the class fire `[D; n]` or through the `K_retire` of a join. They
/// race in merge check #1 on the produces of the members. The class has two to four members that
/// are not persistent.
#[test]
fn prop_set_retirements_race() {
    check(
        runtime_config(),
        (2usize..5, any::<bool>(), any::<bool>(), vec(any::<u8>(), 4)),
        |(members, left_join, right_join, seed)| {
            tokio_runtime().block_on(set_retirements_race_case(
                members, left_join, right_join, seed,
            ))
        },
    );
}

/// One case of `prop_set_retirements_race`.
async fn set_retirements_race_case(
    members: usize,
    left_join: bool,
    right_join: bool,
    seed_bytes: Vec<u8>,
) -> Result<(), TestCaseError> {
    let label = case_label("rrs-p5-prop-set-retire", &seed_bytes);
    let mut seed_parts: Vec<&str> = std::iter::repeat_n("a!(1)", members).collect();
    seed_parts.push("b!(5) | b!(6)");
    let mut rig = Rig::new(HASHMAP_SET, DEFAULT, &seed_parts.join(" | "), &label).await;
    let body = |join: bool, tag: &str| match join {
        true => format!("for (@1 <- a & @y <- b) {{ {} }}", out(tag, "y")),
        false => format!("for (@1 <- a) {{ {} }}", out(tag, "1")),
    };
    let left = rig
        .branch_ok(&body(left_join, "left"), &format!("{label}-left"))
        .await;
    let right = rig
        .branch_ok(&body(right_join, "right"), &format!("{label}-right"))
        .await;
    let found = rig.conflicts(left.log(), right.log());
    prop_assert!(
        found.contains(&rig.data(0)),
        "the retirements race on the members: {:?}",
        rig.reason(left.log(), right.log())
    );
    let reason = rig.reason(left.log(), right.log());
    prop_assert!(
        reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("racesForSameIOEvent")),
        "merge check #1 finds the race: {:?}",
        reason
    );
    Ok(())
}

/// `unordered_set_writes_commute` (F25, `CounterConflict.v`): two branches of stores on the sets
/// `a` and `b`, with equal and new values, touch no counter and do not conflict. The merged state
/// is the state of both serial orders.
#[test]
fn prop_unordered_set_writes_commute() {
    check(
        runtime_config(),
        (
            vec(arb_value(), 0..3),
            vec((0usize..2, arb_value()), 1..4),
            vec((0usize..2, arb_value()), 1..4),
            vec(any::<u8>(), 4),
        ),
        |(records, first, second, seed)| {
            tokio_runtime().block_on(set_writes_commute_case(records, first, second, seed))
        },
    );
}

/// One case of `prop_unordered_set_writes_commute`.
async fn set_writes_commute_case(
    records: Vec<i64>,
    first: Vec<(usize, i64)>,
    second: Vec<(usize, i64)>,
    seed_bytes: Vec<u8>,
) -> Result<(), TestCaseError> {
    let label = case_label("rrs-p5-prop-set", &seed_bytes);
    let mut rig = Rig::new(HASHMAP_SET, DEFAULT, &seed(&[Some(records)]), &label).await;
    let writes = |writes: &[(usize, i64)]| -> String {
        program(
            &writes
                .iter()
                .map(|(slot, value)| SlotOp::Send {
                    slot: *slot,
                    value: *value,
                })
                .collect::<Vec<_>>(),
        )
    };
    let (first_body, second_body) = (writes(&first), writes(&second));
    let (first_label, second_label) = (format!("{label}-left"), format!("{label}-right"));
    let left = rig.branch(&first_body, &first_label).await;
    let right = rig.branch(&second_body, &second_label).await;
    for branch in [&left, &right] {
        prop_assert!(branch.deployed.ok(), "{:?}", branch.deployed.result.errors);
        for counter in rig.counters() {
            prop_assert_eq!(touching(branch.log(), &counter), 0);
        }
    }
    prop_assert!(
        rig.conflicts(left.log(), right.log()).is_empty(),
        "set writes conflict: {:?}",
        rig.reason(left.log(), right.log())
    );
    let merged = rig.merged(&[(&left, left.log()), (&right, right.log())]);
    let (_, serial, _) = rig
        .serial(&[(&first_body, &first_label), (&second_body, &second_label)])
        .await;
    let (_, reversed, _) = rig
        .serial(&[(&second_body, &second_label), (&first_body, &first_label)])
        .await;
    prop_assert_eq!(&merged, &serial);
    prop_assert_eq!(&merged, &reversed);
    Ok(())
}

// ─── PathMap (P5e): one token for every prefix (decision 51) ─────────────────────────────────

/// The factory of the PathMap family with the bag discipline (P5e).
const PATHMAP_BAG: &str = "rho:space:pathmap:bag";

/// The factory of the PathMap family with the queue discipline (P5e).
const PATHMAP_QUEUE: &str = "rho:space:pathmap:queue";

/// `pathmap_token_race` (headline 15 of `CounterConflict.v`, decision 51). A PathMap space orders
/// every prefix with one token, the space counter `S(sid)`. Two branches that each change one
/// PathMap space race on the token in check #1, also when they write disjoint paths, and the same
/// logs without the token merge. A bag branch that only peeks restores the marker of its COMM and
/// bumps no token, so it merges with a write. An ordered discipline bumps the token in every
/// operation, so a peek of a queue races with a write.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_pathmap_token_race() {
    let label = "rrs-p5-merge-pathmap-token";
    let mut rig = Rig::new(PATHMAP_BAG, DEFAULT, "a!(10)", label).await;
    let token = rig.space_counter();
    assert_counter_race(
        &mut rig,
        "a!(1)",
        "b!(2)",
        BTreeSet::from([token.clone()]),
        label,
    )
    .await;
    let read = format!("for (@x <<- a) {{ {} }}", out("read", "x"));
    let left = rig.branch_ok(&read, &format!("{label}-read")).await;
    let right = rig.branch_ok("b!(2)", &format!("{label}-write")).await;
    assert_eq!(
        left.out,
        vec![("read".to_string(), 10)],
        "the peek reads the seed"
    );
    assert_eq!(
        touching(left.log(), &token),
        0,
        "a pure read bumps no token"
    );
    assert_eq!(
        touching(right.log(), &token),
        3,
        "a write bumps the token once"
    );
    assert!(
        rig.conflicts(left.log(), right.log()).is_empty(),
        "a pure read merges with a write: {:?}",
        rig.reason(left.log(), right.log())
    );
    let label = "rrs-p5-merge-pathmap-queue-token";
    let mut rig = Rig::new(PATHMAP_QUEUE, DEFAULT, "a!(10)", label).await;
    let token = rig.space_counter();
    let (left, _) =
        assert_counter_race(&mut rig, &read, "b!(2)", BTreeSet::from([token]), label).await;
    assert_eq!(left.out, vec![("read".to_string(), 10)]);
}

/// A generated branch of `prop_pathmap_token_race`: a send on the name `a` or `b` of a PathMap
/// space, or a peek of the seeded record of `a`.
#[derive(Clone, Copy, Debug)]
enum PathMapBranch {
    /// A send of the value on `a` (false) or on `b` (true).
    Send(bool, u8),
    /// A peek of the record of `a`.
    Peek,
}

impl PathMapBranch {
    /// The body of the branch.
    fn body(self) -> String {
        match self {
            PathMapBranch::Send(false, value) => format!("a!({value})"),
            PathMapBranch::Send(true, value) => format!("b!({value})"),
            PathMapBranch::Peek => format!("for (@x <<- a) {{ {} }}", out("read", "x")),
        }
    }

    /// True when the branch changes the state of a bag space: a send stores its record, and a
    /// peek restores the marker of its COMM.
    fn changes(self) -> bool { matches!(self, PathMapBranch::Send(..)) }
}

/// A generated branch of `prop_pathmap_token_race`.
fn arb_pathmap_branch() -> BoxedStrategy<PathMapBranch> {
    prop_oneof![
        2 => (any::<bool>(), 0u8..4).prop_map(|(name, value)| PathMapBranch::Send(name, value)),
        1 => Just(PathMapBranch::Peek),
    ]
    .boxed()
}

/// `pathmap_token_race` (headline 15 of `CounterConflict.v`, decision 51). For generated pairs of
/// branches on a PathMap space with the seed `a!(10)`, the branches race in check #1 on the token
/// `S(sid)` exactly when both bump it: always for the queue, which bumps in every operation, and
/// for the bag when both branches change the state. The same logs without the token merge.
#[test]
fn prop_pathmap_token_race() {
    check(
        runtime_config(),
        (
            any::<bool>(),
            arb_pathmap_branch(),
            arb_pathmap_branch(),
            vec(any::<u8>(), 4),
        ),
        |(ordered, left, right, seed)| {
            tokio_runtime().block_on(pathmap_token_race_case(ordered, left, right, seed))
        },
    );
}

/// One case of `prop_pathmap_token_race`.
async fn pathmap_token_race_case(
    ordered: bool,
    left: PathMapBranch,
    right: PathMapBranch,
    seed_bytes: Vec<u8>,
) -> Result<(), TestCaseError> {
    let label = case_label("rrs-p5-prop-pathmap-token", &seed_bytes);
    let urn = match ordered {
        true => PATHMAP_QUEUE,
        false => PATHMAP_BAG,
    };
    let mut rig = Rig::new(urn, DEFAULT, "a!(10)", &label).await;
    let token = rig.space_counter();
    let left_branch = rig.branch_ok(&left.body(), &format!("{label}-left")).await;
    let right_branch = rig
        .branch_ok(&right.body(), &format!("{label}-right"))
        .await;
    let bumps = ordered || (left.changes() && right.changes());
    let expected = match bumps {
        true => BTreeSet::from([token.clone()]),
        false => BTreeSet::new(),
    };
    prop_assert_eq!(
        rig.conflicts(left_branch.log(), right_branch.log()),
        expected,
        "{:?} {:?} ordered {}: {:?}",
        left,
        right,
        ordered,
        rig.reason(left_branch.log(), right_branch.log())
    );
    for (branch, op) in [(&left_branch, left), (&right_branch, right)] {
        let touched = touching(branch.log(), &token) > 0;
        prop_assert_eq!(touched, ordered || op.changes(), "{:?}", op);
    }
    prop_assert!(
        rig.conflicts(
            &without_channels(left_branch.log(), std::slice::from_ref(&token)),
            &without_channels(right_branch.log(), std::slice::from_ref(&token))
        )
        .is_empty(),
        "without the token the branches merge"
    );
    Ok(())
}

// ─── VectorDB: no counter, so writes merge as on a host channel (P6, decision 69) ────────────

/// The factory of the VectorDB family (P6).
const HASHMAP_VECTORDB: &str = "rho:space:hashmap:vectordb";

/// The constructor arguments of a VectorDB space of two dimensions with room for eight records
/// in each collection.
const VECTORDB: &str = "\"default\"; capacity=8, dimensions=2";

/// Two branches that store records in one VectorDB collection merge, in native execution
/// (decision 69). A VectorDB collection uses no counter (design v3 4.3), so its writes merge as
/// on a host channel (M-1). The merged collection holds the records of both branches, and it is
/// the state of each serial order. Two queries that take the one pre-state record still race on
/// the record, as two takes on a host channel do.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_vectordb_writes_merge() {
    let label = "rrs-p6-merge-vectordb";
    let mut rig = Rig::native(HASHMAP_VECTORDB, VECTORDB, "a!([1.0, 0.0])", label).await;
    let (left_body, right_body) = ("a!([0.6, 0.8])", "a!([0.0, 1.0])");
    let (left_label, right_label) = (format!("{label}-left"), format!("{label}-right"));
    let left = rig.branch_ok(left_body, &left_label).await;
    let right = rig.branch_ok(right_body, &right_label).await;
    for branch in [&left, &right] {
        for counter in rig.counters() {
            assert_eq!(
                touching(branch.log(), &counter),
                0,
                "a VectorDB operation touches no counter"
            );
        }
    }
    assert!(
        rig.conflicts(left.log(), right.log()).is_empty(),
        "VectorDB writes conflict: {:?}",
        rig.reason(left.log(), right.log())
    );
    let (left_index, right_index) = (rig.index(left.log()), rig.index(right.log()));
    assert!(!depends(&left_index, &right_index));
    assert!(!depends(&right_index, &left_index));
    let merged = rig.merged(&[(&left, left.log()), (&right, right.log())]);
    let (_, serial, _) = rig
        .serial(&[(left_body, &left_label), (right_body, &right_label)])
        .await;
    let (_, reversed, _) = rig
        .serial(&[(right_body, &right_label), (left_body, &left_label)])
        .await;
    assert_eq!(merged, serial, "the merge is the serial state");
    assert_eq!(merged, reversed, "the merge is the reversed serial state");
    assert_eq!(
        rig.node.data_at(&merged, &rig.data(0)).len(),
        3,
        "the merged collection holds the seed record and both writes"
    );
    // The control: both queries take the one pre-state record, so check #1 sees the race on the
    // data channel.
    let take_left = rig
        .branch_ok(
            "for (@x <- a ~ ([1.0, 0.0])) { Nil }",
            &format!("{label}-take-left"),
        )
        .await;
    let take_right = rig
        .branch_ok(
            "for (@y <- a ~ ([1.0, 0.0]; threshold=0.5)) { Nil }",
            &format!("{label}-take-right"),
        )
        .await;
    assert_eq!(
        rig.conflicts(take_left.log(), take_right.log()),
        BTreeSet::from([rig.data(0)]),
        "two takes of one record race on the record"
    );
}

/// The soft bound of `capacity` under merges (decision 71). A store checks `capacity` against
/// the view of its own transaction, and a VectorDB collection has no counter. So two branches
/// that each add a record to a collection one record below its capacity merge without a
/// conflict, and the merged collection holds one record more than its capacity. A serial order
/// holds `capacity` records: its second deploy fails with `CapacityExceeded`, and the merge
/// drops a failed deploy (`block_index.rs`). On the merged state every store fails until the
/// removals bring the count below `capacity`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_collection_conflicts_vectordb_capacity_soft_bound() {
    let label = "rrs-p6-merge-vectordb-capacity";
    let mut rig = Rig::native(
        HASHMAP_VECTORDB,
        "\"default\"; capacity=2, dimensions=2",
        "a!([1.0, 0.0])",
        label,
    )
    .await;
    let (left_body, right_body) = ("a!([0.6, 0.8])", "a!([0.0, 1.0])");
    let (left_label, right_label) = (format!("{label}-left"), format!("{label}-right"));
    let left = rig.branch_ok(left_body, &left_label).await;
    let right = rig.branch_ok(right_body, &right_label).await;
    assert!(
        rig.conflicts(left.log(), right.log()).is_empty(),
        "the stores of two branches do not conflict: {:?}",
        rig.reason(left.log(), right.log())
    );
    let merged = rig.merged(&[(&left, left.log()), (&right, right.log())]);
    assert_eq!(
        rig.node.data_at(&merged, &rig.data(0)).len(),
        3,
        "the merged collection holds one record more than its capacity"
    );
    let (serial, serial_root, _) = rig
        .serial(&[(left_body, &left_label), (right_body, &right_label)])
        .await;
    assert_eq!(
        serial.iter().map(Deployed::codes).collect::<Vec<_>>(),
        vec![Vec::new(), vec!["CapacityExceeded".to_string()]],
        "the second deploy of a serial order exceeds the capacity"
    );
    assert_eq!(
        rig.node.data_at(&serial_root, &rig.data(0)).len(),
        2,
        "a serial order holds the capacity"
    );
    rig.node.reset(&merged).await;
    let store = "a!([0.8, 0.6])";
    let take = "for (@x <- a ~ ([1.0, 0.0])) { Nil }";
    let steps = [
        (store, vec!["CapacityExceeded".to_string()], 3),
        (take, Vec::new(), 2),
        (store, vec!["CapacityExceeded".to_string()], 2),
        (take, Vec::new(), 1),
        (store, Vec::new(), 2),
    ];
    for (index, (body, codes, records)) in steps.into_iter().enumerate() {
        let step_label = format!("{label}-after-{index}");
        let deployed = rig.node.deploy(&with_names(body), &step_label).await;
        assert_eq!(deployed.codes(), codes, "{step_label}: {body}");
        let root = rig.node.checkpoint().await;
        assert_eq!(
            rig.node.data_at(&root, &rig.data(0)).len(),
            records,
            "{step_label}: the records after {body}"
        );
    }
}

/// The records of a seed deploy: `[x.0, y.0]` vectors on the slots `a` and `b`.
fn vectordb_seed(records: &[(usize, (i64, i64))]) -> String {
    records
        .iter()
        .map(|(slot, (x, y))| format!("{}!([{x}.0, {y}.0])", NAMES[*slot]))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The cases of `prop_vectordb_writes_commute` that compared a merge with the serial states.
static VECTORDB_MERGES_COMPARED: AtomicUsize = AtomicUsize::new(0);

/// `unordered_writes_commute` (R66) for the VectorDB family, in native execution (decision 69):
/// two branches of stores on the VectorDB collections `a` and `b` touch no counter and do not
/// conflict, and the merged state is the state of both serial orders. Every case stays below the
/// capacity, so the soft bound of decision 71 does not apply.
#[test]
fn prop_vectordb_writes_commute() {
    let point = || (0i64..4, 0i64..4);
    check(
        runtime_config(),
        (
            vec((0usize..2, point()), 0..3),
            vec((0usize..2, point()), 1..4),
            vec((0usize..2, point()), 1..4),
            vec(any::<u8>(), 4),
        ),
        |(seeded, first, second, seed)| {
            tokio_runtime().block_on(vectordb_writes_commute_case(seeded, first, second, seed))
        },
    );
    assert_eq!(
        VECTORDB_MERGES_COMPARED.load(Ordering::SeqCst),
        RUNTIME_CASES as usize,
        "every case compares the merge with the serial states"
    );
}

/// One case of `prop_vectordb_writes_commute`.
async fn vectordb_writes_commute_case(
    seeded: Vec<(usize, (i64, i64))>,
    first: Vec<(usize, (i64, i64))>,
    second: Vec<(usize, (i64, i64))>,
    seed_bytes: Vec<u8>,
) -> Result<(), TestCaseError> {
    let label = case_label("rrs-p6-prop-vectordb", &seed_bytes);
    let mut rig = Rig::native(HASHMAP_VECTORDB, VECTORDB, &vectordb_seed(&seeded), &label).await;
    let (first_body, second_body) = (vectordb_seed(&first), vectordb_seed(&second));
    let (first_label, second_label) = (format!("{label}-left"), format!("{label}-right"));
    let left = rig.branch(&first_body, &first_label).await;
    let right = rig.branch(&second_body, &second_label).await;
    for branch in [&left, &right] {
        prop_assert!(branch.deployed.ok(), "{:?}", branch.deployed.result.errors);
        for counter in rig.counters() {
            prop_assert_eq!(touching(branch.log(), &counter), 0);
        }
    }
    prop_assert!(
        rig.conflicts(left.log(), right.log()).is_empty(),
        "VectorDB writes conflict: {:?}",
        rig.reason(left.log(), right.log())
    );
    let merged = rig.merged(&[(&left, left.log()), (&right, right.log())]);
    let (_, serial, _) = rig
        .serial(&[(&first_body, &first_label), (&second_body, &second_label)])
        .await;
    let (_, reversed, _) = rig
        .serial(&[(&second_body, &second_label), (&first_body, &first_label)])
        .await;
    prop_assert_eq!(&merged, &serial);
    prop_assert_eq!(&merged, &reversed);
    for slot in 0..2 {
        let expected = [&seeded, &first, &second]
            .iter()
            .flat_map(|records| records.iter())
            .filter(|(record_slot, _)| *record_slot == slot)
            .count();
        prop_assert_eq!(rig.node.data_at(&merged, &rig.data(slot)).len(), expected);
    }
    VECTORDB_MERGES_COMPARED.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

/// The cases of `prop_vectordb_capacity_soft_bound` whose stores fit the capacity.
static VECTORDB_FITS: AtomicUsize = AtomicUsize::new(0);

/// The cases of `prop_vectordb_capacity_soft_bound` whose merge exceeds the capacity.
static VECTORDB_OVERFLOWS: AtomicUsize = AtomicUsize::new(0);

/// `capped_writes_soft_bound` (decision 71), in native execution: a collection of a space of
/// capacity `c` holds `n` records, and two branches each store `a` and `b` records, each within
/// the capacity. The branches merge without a conflict, and the merged collection holds
/// `n + a + b` records, at most `c + min(a, b)`. The serial order passes exactly when
/// `n + a + b <= c`, and then the merge is its state. Otherwise the second deploy of the serial
/// order fails with `CapacityExceeded`, and the merge holds more records than the capacity.
#[test]
fn prop_vectordb_capacity_soft_bound() {
    let cases = (1usize..=3).prop_flat_map(|capacity| {
        (0..capacity).prop_flat_map(move |seeded| {
            (
                Just(capacity),
                Just(seeded),
                1..=capacity - seeded,
                1..=capacity - seeded,
                vec(any::<u8>(), 4),
            )
        })
    });
    check(
        runtime_config(),
        cases,
        |(capacity, seeded, first, second, seed)| {
            tokio_runtime().block_on(vectordb_capacity_case(
                capacity, seeded, first, second, seed,
            ))
        },
    );
    let (fits, overflows) = (
        VECTORDB_FITS.load(Ordering::SeqCst),
        VECTORDB_OVERFLOWS.load(Ordering::SeqCst),
    );
    assert!(
        fits >= 1 && overflows >= 1 && fits + overflows == RUNTIME_CASES as usize,
        "both sides of the bound ran: {fits} fits, {overflows} overflows"
    );
}

/// One case of `prop_vectordb_capacity_soft_bound`.
async fn vectordb_capacity_case(
    capacity: usize,
    seeded: usize,
    first: usize,
    second: usize,
    seed_bytes: Vec<u8>,
) -> Result<(), TestCaseError> {
    let label = case_label("rrs-p6-prop-capacity", &seed_bytes);
    let stores = |count: usize, y: usize| -> String {
        (0..count)
            .map(|index| format!("a!([{index}.0, {y}.0])"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let ctor = format!("\"default\"; capacity={capacity}, dimensions=2");
    let mut rig = Rig::native(HASHMAP_VECTORDB, &ctor, &stores(seeded, 1), &label).await;
    let (first_body, second_body) = (stores(first, 2), stores(second, 3));
    let (first_label, second_label) = (format!("{label}-left"), format!("{label}-right"));
    let left = rig.branch(&first_body, &first_label).await;
    let right = rig.branch(&second_body, &second_label).await;
    for branch in [&left, &right] {
        prop_assert!(branch.deployed.ok(), "{:?}", branch.deployed.result.errors);
    }
    prop_assert!(
        rig.conflicts(left.log(), right.log()).is_empty(),
        "capped stores conflict: {:?}",
        rig.reason(left.log(), right.log())
    );
    let merged = rig.merged(&[(&left, left.log()), (&right, right.log())]);
    let records = rig.node.data_at(&merged, &rig.data(0)).len();
    prop_assert_eq!(records, seeded + first + second);
    prop_assert!(records <= capacity + first.min(second));
    let (serial, serial_root, _) = rig
        .serial(&[(&first_body, &first_label), (&second_body, &second_label)])
        .await;
    prop_assert!(serial[0].ok(), "{:?}", serial[0].result.errors);
    match seeded + first + second <= capacity {
        true => {
            prop_assert!(serial[1].ok(), "{:?}", serial[1].result.errors);
            prop_assert_eq!(&merged, &serial_root);
            VECTORDB_FITS.fetch_add(1, Ordering::SeqCst);
        }
        false => {
            let codes = serial[1].codes();
            prop_assert!(
                !codes.is_empty() && codes.iter().all(|code| code == "CapacityExceeded"),
                "{:?}",
                codes
            );
            prop_assert!(records > capacity);
            VECTORDB_OVERFLOWS.fetch_add(1, Ordering::SeqCst);
        }
    }
    Ok(())
}
