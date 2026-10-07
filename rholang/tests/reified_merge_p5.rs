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
//! `vector:queue`. The set and priority-queue rows follow in wave W3, after their families are
//! wired. The formal counterparts are `CounterConflict.v` and `CounterConflict.tla`. Each
//! property names the headline or the invariant that it tests.

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

/// A play runtime and the history of one in-memory store, as a validator evaluates a block.
struct Node {
    play: RhoRuntimeImpl,
    history: RhoHistoryRepository,
}

impl Node {
    /// A node over a fresh in-memory store, with a funded deploy signature.
    async fn new() -> Node {
        let mut manager = InMemoryStoreManager::new();
        let stores = manager
            .r_space_stores()
            .await
            .expect("create the in-memory RSpace stores");
        let (play, _replay, history) = create_runtimes(stores, false, &mut Vec::new()).await;
        play.cost
            .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
        Node { play, history }
    }

    /// Evaluates one deploy as Casper processes a user deploy. It takes a soft checkpoint,
    /// evaluates `source`, takes the event log, and reverts a failed deploy. The label seeds the
    /// random state, so one label gives the same deploy in every run.
    async fn deploy(&mut self, source: &str, label: &str) -> Deployed {
        let fallback = self.play.create_soft_checkpoint().await;
        let result = self
            .play
            .evaluate(
                source,
                Cost::create(PHLO, label.to_owned()),
                HashMap::new(),
                Blake2b512Random::create_from_bytes(label.as_bytes()),
            )
            .await
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
    /// The label `label` seeds the random states of the setup and of the seed.
    async fn new(urn: &str, ctor: &str, seed: &str, label: &str) -> Rig {
        let mut node = Node::new().await;
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

/// Item 18 at P5: every consume and every COMM of `logs` names one channel. The state at `root`
/// holds no join of two or more channels on a channel of the logs. So the space machinery adds
/// no pre-state join for merge check #3. It returns the number of channels that it checked.
fn assert_no_join(node: &Node, root: &Blake2b256Hash, logs: &[Vec<Event>], label: &str) -> usize {
    let mut channels: BTreeSet<Blake2b256Hash> = BTreeSet::new();
    for event in logs.iter().flatten() {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => {
                channels.insert(produce.channel_hash.clone());
            }
            Event::IoEvent(IOEvent::Consume(consume)) => {
                assert_eq!(
                    consume.channel_hashes.len(),
                    1,
                    "{label}: a consume names {} channels",
                    consume.channel_hashes.len()
                );
                channels.extend(consume.channel_hashes.iter().cloned());
            }
            Event::Comm(comm) => {
                assert_eq!(
                    comm.consume.channel_hashes.len(),
                    1,
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

/// Consensus checklist item 18 at P5: the bag, cell, stack and allocator operations of wave W2
/// use no multi-channel join (I-16). This includes the `K_lift` of the stack settle and the
/// `K_alloc` of an allocation. An ordinary host join is the control: the check sees it.
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
