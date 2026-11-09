// P4 integration tests of the merge of space operations across branches (P4 design D.2 to D.4,
// M-1, decision 31, A2 and A25, consensus checklist item 18).
//
// The harness builds the merge indices as Casper builds them for a block
// (`casper/src/rust/merging/block_index.rs`, `create_event_log_index`): the pre-state closures
// read the history at the pre-state root, and `EventLogIndex::new` indexes the event log of each
// deploy. The tests call the merger functions `conflicts` and `depends` and the chain builder
// `compute_related_sets` of rspace++ on these indices.
//
// A branch is one deploy that a play runtime evaluates from the pre-state root. Two branches
// conflict when `conflicts` is not empty. Each counter race test also runs a negative control:
// the same logs without the events of the counter channels. Without the counters, the races of
// D.2 and its two write-skew counterexamples merge, so the counters are necessary.

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

// Added by decision 90 (C6, decision 69): the node runs native deploys through the native
// session of the shared harness.
#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeSet, HashMap, HashSet};

// Disabled by decision 90 (C6): the native session draws the random state of each deploy.
// use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use reified_strategies::gstring;
// Disabled by decision 90 (C6): the native session runs no legacy deploy.
// use rholang::rust::interpreter::accounting::costs::Cost;
// use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
// Changed by decision 90 (C6): the session holds the runtime.
// use rholang::rust::interpreter::rho_runtime::{RhoHistoryRepository, RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_runtime::{RhoHistoryRepository, RhoRuntime};
use rholang::rust::interpreter::spaces::namespace::{self, CollectionKey, NativeSpace};
// Disabled by decision 90 (C6): the native session builds the runtime.
// use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{
    compute_related_sets, conflict_reason, conflicts, depends, NumberChannelsDiff,
};
// Disabled by decision 90 (C6): the native session builds the runtime.
// use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
// use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};
use shared::rust::hashable_set::HashableSet;
use support::session::Session;

// Disabled by decision 90 (C6): the native session funds its own deploy signature and limit.
// const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-merge-deploy";
// const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-merge-payer";
// const PHLO: i64 = 100_000_000;

/// A deploy that creates a space and four names of it. The handle goes to `@"rrs-handle"` and
/// the names go to `@"rrs-names"`.
const SETUP: &str = r#"
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default")) {
    @"rrs-handle"!(*queue) |
    for (a <- queue!?("gensym")) { for (b <- queue!?("gensym")) {
      for (c <- queue!?("gensym")) { for (d <- queue!?("gensym")) {
        @"rrs-names"!(*a, *b, *c, *d)
      } }
    } }
  }
}
"#;

/// `body` with the handle bound to `queue` and the names bound to `a`, `b`, `c` and `d`.
fn with_space(body: &str) -> String {
    format!(
        "for (queue <<- @\"rrs-handle\") {{ for (a, b, c, d <<- @\"rrs-names\") {{\n{body}\n}} }}"
    )
}

fn injected(name: &str) -> String { format!("rho:rrs:test:{name}") }

// Changed by decision 90 (C6, decision 69): the node runs native deploys through the native
// session (`support::session`), and native replay certifies each deploy. The legacy node
// follows.
//
// /// A play runtime and the history of one in-memory store.
// struct Node {
//     play: RhoRuntimeImpl,
//     history: RhoHistoryRepository,
// }

/// A native session and the history of its store (decision 90, C6).
struct Node {
    session: Session,
    history: RhoHistoryRepository,
}

/// The pre-state of the branches: the space, its names `a` to `d`, and the root.
struct PreState {
    space: NativeSpace,
    names: Vec<Par>,
    root: Blake2b256Hash,
}

impl Node {
    // Changed by decision 90 (C6): the session builds the runtime and plays each deploy. The
    // legacy constructor and deploy follow.
    //
    // async fn new() -> Self {
    //     let mut manager = InMemoryStoreManager::new();
    //     let stores = manager
    //         .r_space_stores()
    //         .await
    //         .expect("create the in-memory RSpace stores");
    //     let (play, _replay, history) = create_runtimes(stores, false, &mut Vec::new()).await;
    //     play.cost
    //         .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
    //     Self { play, history }
    // }
    //
    // /// Evaluates one deploy that must succeed, and returns its event log.
    // async fn deploy_with(
    //     &mut self,
    //     source: &str,
    //     label: &str,
    //     injections: &HashMap<String, Par>,
    // ) -> Vec<Event> {
    //     let result = self
    //         .play
    //         .evaluate(
    //             source,
    //             Cost::create(PHLO, label.to_owned()),
    //             injections.clone(),
    //             Blake2b512Random::create_from_bytes(label.as_bytes()),
    //         )
    //         .await
    //         .unwrap_or_else(|error| panic!("{label}: the evaluation returns no result: {error:?}"));
    //     assert!(
    //         result.errors.is_empty(),
    //         "{label}: the deploy fails: {:?}",
    //         result.errors
    //     );
    //     self.play.take_event_log().await
    // }

    async fn new() -> Self {
        let session = Session::new().await;
        let history = session.history();
        Self { session, history }
    }

    /// Plays one deploy that must succeed, certifies it by native replay, and returns its event
    /// log.
    async fn deploy_with(
        &mut self,
        source: &str,
        label: &str,
        injections: &HashMap<String, Par>,
    ) -> Vec<Event> {
        let deploy = self.session.deploy_with(source, label, injections).await;
        assert!(
            deploy.result.errors.is_empty(),
            "{label}: the deploy fails: {:?}",
            deploy.result.errors
        );
        deploy.log
    }

    async fn deploy(&mut self, source: &str, label: &str) -> Vec<Event> {
        self.deploy_with(source, label, &HashMap::new()).await
    }

    // Changed by decision 90 (C6): the session holds the runtime. The legacy pre-state and
    // branch follow.
    //
    // /// Runs `SETUP` and `seed`, and checkpoints the pre-state of the branches.
    // async fn pre_state(&mut self, seed: &str) -> PreState {
    //     self.deploy(SETUP, "rrs-p4-merge-setup").await;
    //     let handle = self
    //         .play
    //         .get_data(&gstring("rrs-handle"))
    //         .await
    //         .first()
    //         .and_then(|datum| datum.a.pars.first().cloned())
    //         .expect("the setup stores the handle");
    //     let space = match classify_space_ref(&handle) {
    //         Ok(SpaceRef::Native(space)) => space,
    //         other => panic!("the handle is not a native space: {other:?}"),
    //     };
    //     let names = self
    //         .play
    //         .get_data(&gstring("rrs-names"))
    //         .await
    //         .first()
    //         .map(|datum| datum.a.pars.clone())
    //         .expect("the setup stores the names");
    //     self.deploy(&with_space(seed), "rrs-p4-merge-seed").await;
    //     let root = self.play.create_checkpoint().await.root;
    //     PreState { space, names, root }
    // }
    //
    // /// The event log of one branch: `source` evaluated from the pre-state root.
    // async fn branch(&mut self, pre: &PreState, source: &str, label: &str) -> Vec<Event> {
    //     self.play
    //         .reset(&pre.root)
    //         .await
    //         .expect("reset the play runtime to the pre-state root");
    //     self.deploy(&with_space(source), label).await
    // }

    /// Runs `SETUP` and `seed`, and checkpoints the pre-state of the branches.
    async fn pre_state(&mut self, seed: &str) -> PreState {
        self.deploy(SETUP, "rrs-p4-merge-setup").await;
        let handle = self
            .session
            .play
            .get_data(&gstring("rrs-handle"))
            .await
            .first()
            .and_then(|datum| datum.a.pars.first().cloned())
            .expect("the setup stores the handle");
        let space = match classify_space_ref(&handle) {
            Ok(SpaceRef::Native(space)) => space,
            other => panic!("the handle is not a native space: {other:?}"),
        };
        let names = self
            .session
            .play
            .get_data(&gstring("rrs-names"))
            .await
            .first()
            .map(|datum| datum.a.pars.clone())
            .expect("the setup stores the names");
        self.deploy(&with_space(seed), "rrs-p4-merge-seed").await;
        let root = self.session.checkpoint().await;
        PreState { space, names, root }
    }

    /// The event log of one branch: `source` evaluated from the pre-state root.
    async fn branch(&mut self, pre: &PreState, source: &str, label: &str) -> Vec<Event> {
        self.session.reset(&pre.root).await;
        self.deploy(&with_space(source), label).await
    }

    /// The merge index of a deploy log over the pre-state `root`, with the pre-state closures
    /// of `block_index.rs::create_event_log_index`.
    fn index(&self, root: &Blake2b256Hash, log: Vec<Event>) -> EventLogIndex {
        let reader = self
            .history
            .get_history_reader(root)
            .expect("read the history at the pre-state root");
        let produce_exists_in_pre_state = |p: &Produce| {
            reader
                .get_data(&p.channel_hash)
                .is_ok_and(|data| data.iter().any(|d| d.source == *p))
        };
        let produce_touches_pre_state_join = |p: &Produce| {
            reader
                .get_joins(&p.channel_hash)
                .is_ok_and(|joins| joins.iter().any(|j| j.len() > 1))
        };
        EventLogIndex::new(
            log,
            produce_exists_in_pre_state,
            produce_touches_pre_state_join,
            NumberChannelsDiff::new(),
        )
    }

    /// The conflicting channels of two branch logs.
    fn conflicts(
        &self,
        root: &Blake2b256Hash,
        left: &[Event],
        right: &[Event],
    ) -> BTreeSet<Blake2b256Hash> {
        let left = self.index(root, left.to_vec());
        let right = self.index(root, right.to_vec());
        conflicts(&left, &right).0.into_iter().collect()
    }
}

impl PreState {
    fn name(&self, index: usize) -> &Par {
        self.names.get(index).expect("the setup makes four names")
    }

    fn counter(&self, index: usize) -> Blake2b256Hash {
        let key = CollectionKey::of_channel(self.name(index)).expect("a name has a key");
        hash(&namespace::gprivate(namespace::collection_counter(
            self.space, &key,
        )))
    }

    fn data(&self, index: usize) -> Blake2b256Hash {
        let key = CollectionKey::of_channel(self.name(index)).expect("a name has a key");
        hash(&namespace::gprivate(namespace::data_channel(
            self.space, &key,
        )))
    }

    fn space_counter(&self) -> Blake2b256Hash {
        hash(&namespace::gprivate(namespace::space_counter(self.space)))
    }

    /// The hashes of every counter channel of the space.
    fn counters(&self) -> Vec<Blake2b256Hash> {
        let mut counters = Vec::with_capacity(self.names.len() + 1);
        counters.push(self.space_counter());
        counters.extend((0..self.names.len()).map(|index| self.counter(index)));
        counters
    }
}

/// `log` without the events on the counter channels `counters`: the log that the space would
/// make without M-1.
fn without_counters(log: &[Event], counters: &[Blake2b256Hash]) -> Vec<Event> {
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => !counters.contains(&produce.channel_hash),
            Event::IoEvent(IOEvent::Consume(consume)) => !consume
                .channel_hashes
                .iter()
                .any(|channel| counters.contains(channel)),
            Event::Comm(comm) => !comm
                .consume
                .channel_hashes
                .iter()
                .any(|channel| counters.contains(channel)),
        })
        .cloned()
        .collect()
}

/// Two branches race on exactly the counters `expected` (P4 design D.2), and the same logs
/// without the counter events merge.
async fn assert_counter_race(
    seed: &str,
    left: &str,
    right: &str,
    expected: fn(&PreState) -> Vec<Blake2b256Hash>,
    label: &str,
) {
    let mut node = Node::new().await;
    let pre = node.pre_state(seed).await;
    let left_log = node.branch(&pre, left, &format!("{label}-left")).await;
    let right_log = node.branch(&pre, right, &format!("{label}-right")).await;
    let found = node.conflicts(&pre.root, &left_log, &right_log);
    let expected: BTreeSet<Blake2b256Hash> = expected(&pre).into_iter().collect();
    assert_eq!(
        found, expected,
        "{label}: the branches race on the counters"
    );
    let reason = conflict_reason(
        &node.index(&pre.root, left_log.clone()),
        &node.index(&pre.root, right_log.clone()),
    );
    assert!(
        reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("racesForSameIOEvent")),
        "{label}: merge check #1 finds the race: {reason:?}"
    );
    let counters = pre.counters();
    let found = node.conflicts(
        &pre.root,
        &without_counters(&left_log, &counters),
        &without_counters(&right_log, &counters),
    );
    assert!(
        found.is_empty(),
        "{label}: without the counters the branches would merge"
    );
}

mod space_collection_conflicts {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_counter_race_conflict_enq_enq() {
        assert_counter_race(
            "a!(10) | b!(20)",
            "a!(1)",
            "a!(2)",
            |pre| vec![pre.counter(0)],
            "rrs-p4-merge-enq-enq",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_counter_race_conflict_enq_deq() {
        assert_counter_race(
            "a!(10) | b!(20)",
            "a!(1)",
            r#"for (@x <- a) { Nil }"#,
            |pre| vec![pre.counter(0)],
            "rrs-p4-merge-enq-deq",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_counter_race_conflict_deq_deq() {
        // Both branches take the head record, so they race on the counter and also on the record
        // itself.
        let mut node = Node::new().await;
        let pre = node.pre_state("a!(10) | b!(20)").await;
        let left = node
            .branch(
                &pre,
                r#"for (@x <- a) { Nil }"#,
                "rrs-p4-merge-deq-deq-left",
            )
            .await;
        let right = node
            .branch(
                &pre,
                r#"for (@y <- a) { @"rrs-y"!(y) }"#,
                "rrs-p4-merge-deq-deq-right",
            )
            .await;
        assert_eq!(
            node.conflicts(&pre.root, &left, &right),
            BTreeSet::from([pre.counter(0), pre.data(0)]),
            "the branches race on the counter and on the head record"
        );
        // A dequeue on another collection does not race.
        let other = node
            .branch(
                &pre,
                r#"for (@z <- b) { Nil }"#,
                "rrs-p4-merge-deq-deq-other",
            )
            .await;
        assert!(node.conflicts(&pre.root, &left, &other).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_counter_race_conflict_first_op_first_op() {
        // Collections c and d are new in both branches. Their first operations consume the one
        // space counter record (A25).
        assert_counter_race(
            "a!(10) | b!(20)",
            "c!(1)",
            "d!(2)",
            |pre| vec![pre.space_counter()],
            "rrs-p4-merge-first-op",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_counter_race_conflict_crossed_queues() {
        // Each branch moves the head of one queue to the other queue, in opposite directions.
        assert_counter_race(
            "a!(10) | b!(20)",
            r#"for (@x <- a) { b!(x) }"#,
            r#"for (@y <- b) { a!(y) }"#,
            |pre| vec![pre.counter(0), pre.counter(1)],
            "rrs-p4-merge-crossed",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_counter_race_conflict_peek_dequeue_write_skew() {
        // The heads are h1 on a and h2 on b. Branch A peeks a and takes the head of b. Branch B
        // peeks b and takes the head of a. Without the counters the peeks race on nothing and
        // the takes consume different records. The merge would then be neither A then B nor B
        // then A (D.2, A2).
        assert_counter_race(
            "a!(10) | b!(20)",
            r#"for (@x <<- a) { for (@y <- b) { Nil } }"#,
            r#"for (@x <<- b) { for (@y <- a) { Nil } }"#,
            |pre| vec![pre.counter(0), pre.counter(1)],
            "rrs-p4-merge-write-skew",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_counter_race_conflict_persistent_fire_store_write_skew() {
        // Persistent head waiters on c and d take only "x". Branch A fires the waiter of c and
        // stores "e" on d. Branch B fires the waiter of d and stores "f" on c. No serial order
        // runs both fires and both stores like this, so the branches must race (D.2, A2).
        assert_counter_race(
            r#"for (@"x" <= c) { Nil } | for (@"x" <= d) { Nil }"#,
            r#"c!("x") | d!("e")"#,
            r#"d!("x") | c!("f")"#,
            |pre| vec![pre.counter(2), pre.counter(3)],
            "rrs-p4-merge-persistent-skew",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_counter_race_conflict_same_block_one_chain() {
        // The deploys of one block run in order on one state. A later operation on a
        // collection consumes the counter record that an earlier deploy produced, so the
        // deploys of one collection form one chain (D.3). A failed deploy is reverted, and
        // the block index leaves it out.
        let mut node = Node::new().await;
        let pre = node.pre_state("a!(10) | b!(20)").await;
        // Changed by decision 90 (C6): the session resets the runtime, and it plays, reverts and
        // certifies the failed deploy. The legacy lines follow.
        // node.play
        //     .reset(&pre.root)
        //     .await
        //     .expect("reset the play runtime to the pre-state root");
        // let mut logs = Vec::with_capacity(4);
        // logs.push(node.deploy(&with_space("a!(1)"), "rrs-p4-chain-1").await);
        // logs.push(node.deploy(&with_space("a!(2)"), "rrs-p4-chain-2").await);
        // let fallback = node.play.create_soft_checkpoint().await;
        // let failed = node
        //     .play
        //     .evaluate(
        //         &with_space("a!(3; priority=0)"),
        //         Cost::create(PHLO, "rrs-p4-chain-failed".to_owned()),
        //         HashMap::new(),
        //         Blake2b512Random::create_from_bytes(b"rrs-p4-chain-failed"),
        //     )
        //     .await
        //     .expect("the failed deploy returns a result");
        // assert!(!failed.errors.is_empty(), "the deploy with params fails");
        // node.play.take_event_log().await;
        // node.play.revert_to_soft_checkpoint(fallback).await;
        node.session.reset(&pre.root).await;
        let mut logs = Vec::with_capacity(4);
        logs.push(node.deploy(&with_space("a!(1)"), "rrs-p4-chain-1").await);
        logs.push(node.deploy(&with_space("a!(2)"), "rrs-p4-chain-2").await);
        let failed = node
            .session
            .deploy_with(
                &with_space("a!(3; priority=0)"),
                "rrs-p4-chain-failed",
                &HashMap::new(),
            )
            .await;
        assert!(
            !failed.result.errors.is_empty(),
            "the deploy with params fails"
        );
        logs.push(node.deploy(&with_space("b!(4)"), "rrs-p4-chain-3").await);
        logs.push(
            node.deploy(&with_space("for (@x <- a) { Nil }"), "rrs-p4-chain-4")
                .await,
        );
        let indices: Vec<EventLogIndex> = logs
            .into_iter()
            .map(|log| node.index(&pre.root, log))
            .collect();
        assert!(
            depends(&indices[1], &indices[0]),
            "the second enqueue follows the first"
        );
        assert!(!depends(&indices[0], &indices[1]));
        assert!(
            depends(&indices[3], &indices[1]),
            "the dequeue follows the second enqueue"
        );
        for other in [0, 1, 3] {
            assert!(
                !depends(&indices[2], &indices[other]),
                "the deploy on b is independent"
            );
            assert!(!depends(&indices[other], &indices[2]));
        }
        let items = HashableSet((0..indices.len()).collect::<HashSet<usize>>());
        let chains: BTreeSet<BTreeSet<usize>> = compute_related_sets(&items, |left, right| {
            depends(&indices[*left], &indices[*right])
        })
        .0
        .into_iter()
        .map(|chain| chain.0.into_iter().collect())
        .collect();
        assert_eq!(
            chains,
            BTreeSet::from([BTreeSet::from([0, 1, 3]), BTreeSet::from([2])]),
            "the deploys of one queue form one chain"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_counter_race_conflict_unrelated_queues_merge() {
        let mut node = Node::new().await;
        let pre = node.pre_state("a!(10) | b!(20)").await;
        for (label, left, right) in [
            ("enqueues", "a!(1)", "b!(2)"),
            (
                "mixed",
                r#"a!(1) | for (@x <- a) { Nil }"#,
                r#"for (@y <<- b) { b!(y) }"#,
            ),
            (
                "parks",
                r#"for (@"z" <- a) { Nil }"#,
                r#"for (@"z" <- b) { Nil }"#,
            ),
        ] {
            let left_log = node
                .branch(&pre, left, &format!("rrs-p4-unrelated-{label}-left"))
                .await;
            let right_log = node
                .branch(&pre, right, &format!("rrs-p4-unrelated-{label}-right"))
                .await;
            assert!(
                node.conflicts(&pre.root, &left_log, &right_log).is_empty(),
                "{label}: operations on unrelated queues merge"
            );
            let left_index = node.index(&pre.root, left_log);
            let right_index = node.index(&pre.root, right_log);
            assert!(!depends(&left_index, &right_index), "{label}");
            assert!(!depends(&right_index, &left_index), "{label}");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_handle_cross_block_merge_no_conflict() {
    // Handle calls merge like contract calls (P4 design D.4): the user produce on H(sid) and
    // the forward to HANDLE_OP are consumed in the branch, the handle continuation and the
    // system process install are persistent, and the space operations race only on the
    // counters of the collections that they touch.
    let mut node = Node::new().await;
    let pre = node.pre_state("a!(10) | b!(20)").await;
    let handle = hash(&namespace::gprivate(namespace::handle_channel(pre.space)));
    let left = node
        .branch(
            &pre,
            r#"for (_ <- queue!?("produce", *a, [1])) { for (n <- queue!?("gensym")) { for (@c <- queue!?("config")) { n!(c) } } }"#,
            "rrs-p4-handle-merge-left",
        )
        .await;
    let right = node
        .branch(
            &pre,
            r#"for (_ <- queue!?("produce", *b, [2])) { for (@k <- queue!?("qualifier")) { @"rrs-k"!(k) } }"#,
            "rrs-p4-handle-merge-right",
        )
        .await;
    let left_index = node.index(&pre.root, left.clone());
    let right_index = node.index(&pre.root, right.clone());
    // Both branches fire the one persistent handle continuation.
    for index in [&left_index, &right_index] {
        assert!(
            index
                .consumes_produced
                .0
                .iter()
                .any(|consume| consume.channel_hashes == [handle.clone()] && consume.persistent),
            "each call meets the persistent handle continuation"
        );
        assert!(index.produces_touching_base_joins.0.is_empty());
    }
    assert!(
        node.conflicts(&pre.root, &left, &right).is_empty(),
        "handle calls on disjoint collections merge: {:?}",
        conflict_reason(&left_index, &right_index)
    );
    // The control: two produce calls on one collection race on its counter.
    let same = node
        .branch(
            &pre,
            r#"for (_ <- queue!?("produce", *a, [3])) { Nil }"#,
            "rrs-p4-handle-merge-same",
        )
        .await;
    assert_eq!(
        node.conflicts(&pre.root, &left, &same),
        BTreeSet::from([pre.counter(0)])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_handle_cross_block_merge_join_handle_conflicts() {
    // The negative control of D.4: a join on the bare handle and a persistent descriptor puts
    // the join [H, D] into the pre-state. Merge check #3 then makes every produce on H in
    // either branch conflict. No program can hold the bare handle, so the deploy that makes the
    // join receives it through injection.
    let mut node = Node::new().await;
    let pre = node.pre_state("a!(10) | b!(20)").await;
    let handle = namespace::gprivate(namespace::handle_channel(pre.space));
    let injections = HashMap::from([(injected("handle"), handle.clone())]);
    // Changed by decision 90 (C6): the session resets the runtime. The legacy reset follows.
    // node.play
    //     .reset(&pre.root)
    //     .await
    //     .expect("reset the play runtime to the pre-state root");
    node.session.reset(&pre.root).await;
    node.deploy_with(
        &format!(
            "new h(`{}`) in {{ for (@x <= h & @y <= @\"rrs-desc\") {{ Nil }} | @\"rrs-desc\"!!(0) }}",
            injected("handle")
        ),
        "rrs-p4-join-handle",
        &injections,
    )
    .await;
    // Changed by decision 90 (C6): the session checkpoints the runtime.
    // let root = node.play.create_checkpoint().await.root;
    let root = node.session.checkpoint().await;
    let joined = PreState {
        space: pre.space,
        names: pre.names.clone(),
        root,
    };
    let left = node
        .branch(
            &joined,
            r#"for (_ <- queue!?("produce", *a, [1])) { Nil }"#,
            "rrs-p4-join-handle-left",
        )
        .await;
    let right = node
        .branch(
            &joined,
            r#"for (_ <- queue!?("produce", *b, [2])) { Nil }"#,
            "rrs-p4-join-handle-right",
        )
        .await;
    let found = node.conflicts(&joined.root, &left, &right);
    assert!(
        found.contains(&hash(&handle)),
        "the produce on the joined handle conflicts: {found:?}"
    );
    let reason = conflict_reason(
        &node.index(&joined.root, left.clone()),
        &node.index(&joined.root, right.clone()),
    );
    assert!(
        reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("produceTouchBaseJoin")),
        "merge check #3 finds the conflict: {reason:?}"
    );
    // One handle call is enough: the other branch makes only a host send.
    let host = node
        .branch(
            &joined,
            r#"@"rrs-elsewhere"!(1)"#,
            "rrs-p4-join-handle-host",
        )
        .await;
    assert!(node
        .conflicts(&joined.root, &left, &host)
        .contains(&hash(&handle)));
    // Without the join, the same branches merge.
    let plain_left = node
        .branch(
            &pre,
            r#"for (_ <- queue!?("produce", *a, [1])) { Nil }"#,
            "rrs-p4-join-handle-left",
        )
        .await;
    let plain_right = node
        .branch(
            &pre,
            r#"for (_ <- queue!?("produce", *b, [2])) { Nil }"#,
            "rrs-p4-join-handle-right",
        )
        .await;
    assert!(node
        .conflicts(&pre.root, &plain_left, &plain_right)
        .is_empty());
}
