//! P9 step 2: the merge of native space state across blocks, and the event coverage of space
//! writes (P9 design v2, section 5, amendments A4, A6 and A7).
//!
//! Block 1 creates a space and draws four names `a` to `d` from it. A second deploy of block 1
//! stores a record on `a`, parks a persistent waiter on `b` and a linear waiter on `c`, and
//! writes and takes one record on `d`, so the collection of `d` exists and is empty. The chain
//! checkpoints the root R1. Two branches then play one deploy each on R1, as two sibling
//! blocks do. Every deploy runs in native execution (decision 69).
//!
//! The rig builds the merge index of each branch as Casper builds it for a block
//! (`casper/src/rust/merging/block_index.rs`, `create_event_log_index`). It calls the merger
//! functions `conflicts`, `conflict_reason` and `depends` of rspace++, and it computes the
//! conflict-free merged state as `dag_merger.rs` computes it (`support::merge`). The rig does
//! not reproduce Casper's rejection choice, its dedup or its edges between the deploys of one
//! block (amendment A7).
//!
//! | Test | Row | Expected |
//! |---|---|---|
//! | `space_cross_block_merge_unordered_writes` | Both branches write one slot of a bag, a set or a VectorDB space. | No conflict. The merged root is the root of both serial orders. Block 3 takes both records, and the set holds one class. |
//! | `space_cross_block_merge_ordered_operations_race_on_the_counter` | Both branches write one slot of a queue, a stack, a cell or a priority queue. In a second pair, the left branch also takes its own record. | The branches race on the counter of the slot (M-1). Without the counter events, they merge. |
//! | `space_cross_block_merge_persistent_waiter_fired_on_both_branches` | Both branches fire the persistent waiter of block 1. | The ordered families race on the counter (decision 31). The unordered families merge. |
//! | `space_cross_block_merge_linear_waiter_fired_on_both_branches` | Both branches fire the linear waiter of block 1. | Every family races on the waiter in merge check #1. The ordered families also race on the counter. |
//! | `space_cross_block_merge_new_in_use_races_on_the_allocator` | Both branches run `new` inside `use` of one Array or Vector space. | The branches race on the allocator. Without the allocator events, they merge. |
//! | `space_cross_block_merge_pathmap_writes_race_on_the_token` | The branches write two names of one PathMap space. | The branches race on the space token. Without the token events, they merge. |
//! | `space_cross_block_merge_distinct_spaces` | Each branch writes a slot of another queue space of block 1. | No conflict. The merged root is the root of both serial orders. |
//! | `space_cross_block_merge_own_spaces` | Each branch creates a queue space and writes it. | No conflict. The merged root is the root of both serial orders. |
//! | `space_cross_block_merge_agent_calls_race_on_its_state` | Both branches call the reference agent of block 1. | The branches race on ordinary channels only, as two calls of a contract with a state cell do. |
//! | `space_event_coverage_debug_assert` | The 24 family programs with `default` and with `temp`, the six HashSet families with `seq`, the stored families of P7 across two deploys, and the conformance kit of the reference agent. | The merge of the log of each deploy over its pre-state gives its post-state (A4). |
//! | `space_event_coverage_names_dropped_channels` | A queue branch without its counter events, and a PathMap branch without its token events. | The coverage check fails, and it names the dropped channels. |
//!
//! Each branch succeeds and delivers its values. Its log touches the counter, the token or the
//! allocator of its row, or no counter for an unordered family, and its merge index holds its
//! space events. No mergeable channel of a deploy is a Reified channel (item 19 of the consensus
//! checklist). For a conflict-free row, block 3 runs on the merged root in a restarted runtime,
//! and native replay reproduces it. Block 3 on the root of a serial order certifies and delivers
//! the same labels. It gives the same root and phlo when it logs the events of block 3 on the
//! merged root in any order (decision 93). For a conflict row, block 3 runs on the root of the left
// Changed by decision 93 (decision 111): block 3 races on both roots. The old text follows.
// //! and native replay reproduces it. Block 3 on the root of a serial order gives the same root and
// //! the same phlo. For a conflict row, block 3 runs on the root of the left
//! branch, and native replay reproduces it. Every deploy of every row also passes the coverage
//! check of A4. The check runs in every build profile, and the test name keeps the word of the
//! plan.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeSet, HashMap};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::spaces::allocator;
use rholang::rust::interpreter::spaces::namespace::{self, NativeSpace};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{conflict_reason, conflicts, depends};
use rspace_plus_plus::rspace::trace::event::Event;
use support::coverage::{assert_merge_reproduces, coverage};
use support::family_programs::{
    hashset_program, kit_deploys, program_with, reference_agent_creation, with_kit_names, Inner,
    Outer, HASHSET_INNERS, HASHSET_UNIT_TAKE, KIT_NAMES, STORED_FAMILIES,
};
use support::merge::{index, merged_root, touching, without_channels};
use support::native::{
    assert_reproduces, certify, footprint_channels, native_play, unordered_log, NativeChain,
    Played, AMPLE_LIMIT, OUT,
};
use support::session::{counter_channel, data_channel, native, space_counter_channel, Injections};

// ─── Programs ──────────────────────────────────────────────────────────────────────────────────

/// The phlo limit of the deploys of the rows.
const LIMIT: u64 = AMPLE_LIMIT;

/// The phlo limit of the deploys of the HashSet families (`reified_hashset.rs`).
const HASHSET_LIMIT: u64 = 20 * AMPLE_LIMIT;

/// The prefix of the public channels that hold the handles of the spaces of block 1.
const HANDLE: &str = "rrs-p9-handle";

/// The prefix of the public channels that hold the names of the spaces of block 1.
const NAMES: &str = "rrs-p9-names";

/// The slots of a space of block 1, as the deploys bind them.
const SLOT_A: usize = 0;
const SLOT_B: usize = 1;
const SLOT_C: usize = 2;
const SLOT_D: usize = 3;

/// The public channel of the names that the branches of the own-spaces row make.
const OWN: &str = "rrs-p9-own";

/// The public channel of the names that a `new` inside `use` makes.
const MINTED: &str = "rrs-p9-minted";

/// A send of `[label, value]` to the public channel [`OUT`].
fn out(label: &str, value: &str) -> String { format!("@\"{OUT}\"!([\"{label}\", {value}])") }

/// The deploy of block 1 that creates a space with the factory `urn` and the constructor
/// arguments `ctor`, and draws the names `a` to `d` from it. The handle goes to
/// `@"rrs-p9-handle-{stash}"` and the names go to `@"rrs-p9-names-{stash}"`.
fn setup_program(urn: &str, ctor: &str, stash: usize) -> String {
    format!(
        r#"new factory(`{urn}`) in {{
  for (space <- factory!?({ctor})) {{
    for (a <- space!?("gensym")) {{
      for (b <- space!?("gensym")) {{
        for (c <- space!?("gensym")) {{
          for (d <- space!?("gensym")) {{
            @"{HANDLE}-{stash}"!(*space) | @"{NAMES}-{stash}"!(*a, *b, *c, *d)
          }}
        }}
      }}
    }}
  }}
}}"#
    )
}

/// `body` with the handle and the names of the first space of block 1 bound to `space` and `a`
/// to `d`. The space `i > 0` binds `space{i+1}` and `a{i+1}` to `d{i+1}`.
fn with_names(body: &str, spaces: usize) -> String {
    let mut term = body.to_string();
    for stash in (0..spaces).rev() {
        let suffix = match stash {
            0 => String::new(),
            other => (other + 1).to_string(),
        };
        term = format!(
            "for (space{suffix} <<- @\"{HANDLE}-{stash}\") {{ for (a{suffix}, b{suffix}, c{suffix}, d{suffix} <<- @\"{NAMES}-{stash}\") {{\n{term}\n}} }}"
        );
    }
    term
}

/// A family of the rows of the disciplines.
#[derive(Clone, Copy)]
struct Family {
    name: &'static str,
    urn: &'static str,
    ctor: &'static str,
    /// True for the queue, the stack, the cell and the priority queue. Every operation on one of
    /// their collections bumps its counter (M-1, decision 31).
    ordered: bool,
    /// The values that the left and the right branch send. The two values of the set are equal,
    /// so the merged set holds two members of one class.
    values: [&'static str; 2],
    /// The query of the linear waiter of block 1 and of the takes: empty, or a similarity query
    /// that both values pass.
    query: &'static str,
}

/// The families of the rows of the disciplines, with the HashMap outer store.
const FAMILIES: [Family; 7] = [
    Family {
        name: "bag",
        urn: "rho:space:hashmap:bag",
        ctor: r#""default""#,
        ordered: false,
        values: ["1", "2"],
        query: "",
    },
    Family {
        name: "set",
        urn: "rho:space:hashmap:set",
        ctor: r#""default""#,
        ordered: false,
        values: ["1", "1"],
        query: "",
    },
    Family {
        name: "vectordb",
        urn: "rho:space:hashmap:vectordb",
        ctor: r#""default"; capacity=16, dimensions=2"#,
        ordered: false,
        values: ["[1.0, 0.0]", "[0.6, 0.8]"],
        query: " ~ ([1.0, 0.0]; threshold=0.5)",
    },
    Family {
        name: "queue",
        urn: "rho:space:hashmap:queue",
        ctor: r#""default""#,
        ordered: true,
        values: ["1", "2"],
        query: "",
    },
    Family {
        name: "stack",
        urn: "rho:space:hashmap:stack",
        ctor: r#""default""#,
        ordered: true,
        values: ["1", "2"],
        query: "",
    },
    Family {
        name: "cell",
        urn: "rho:space:hashmap:cell",
        ctor: r#""default""#,
        ordered: true,
        values: ["1", "2"],
        query: "",
    },
    Family {
        name: "priorityqueue",
        urn: "rho:space:hashmap:priorityqueue",
        ctor: r#""default""#,
        ordered: true,
        values: ["1", "2"],
        query: "",
    },
];

impl Family {
    /// The second deploy of block 1: a record on `a`, a persistent waiter on `b`, a linear
    /// waiter on `c`, and a write and a take on `d`. So the collection of `d` exists at R1, with
    /// its counter for an ordered family, and holds no record. A persistent waiter takes no
    /// query, because only a linear bind takes a query in source text (specification, rule
    /// VD-3).
    fn seed(&self) -> String {
        format!(
            "a!({value}) | for (@v <= b) {{ {each} }} | for (@v <- c{query}) {{ {once} }} | d!({value}) | for (@_ <- d{query}) {{ Nil }}",
            value = self.values[0],
            query = self.query,
            each = out("each", "v"),
            once = out("once", "v"),
        )
    }

    /// A take of one record of the slot `slot`, which delivers it with the label `label`.
    fn take(&self, slot: &str, label: &str) -> String {
        format!(
            "for (@x <- {slot}{query}) {{ {deliver} }}",
            query = self.query,
            deliver = out(label, "x")
        )
    }
}

// ─── The rig ───────────────────────────────────────────────────────────────────────────────────

/// The checks of every deploy of the suite: it succeeds, the sweep leaves no volatile channel,
/// no mergeable channel is a Reified channel (item 19), and the merge of its log over its
/// pre-state gives its post-state (A4).
fn check_deploy(played: &Played, label: &str) {
    assert!(
        played.result.errors.is_empty(),
        "{label}: the deploy fails: {:?}",
        played.result.errors
    );
    assert!(
        played.residue.is_empty(),
        "{label}: the sweep leaves no volatile channel: {:?}",
        played.residue
    );
    for channel in played.result.mergeable.keys() {
        assert!(
            namespace::reified_id_of(channel).is_none(),
            "{label}: a Reified channel is mergeable: {channel:?}"
        );
    }
    assert_merge_reproduces(label, played);
}

/// Plays `term` on `chain` with the label `label` and the phlo limit `limit`, and checks it with
/// [`check_deploy`].
async fn play_checked(chain: &mut NativeChain, term: &str, label: &str, limit: u64) -> Played {
    let played = chain.play(term, label, limit).await;
    check_deploy(&played, label);
    played
}

/// The one value of the one datum on a public channel.
fn sole(mut data: Vec<Vec<Par>>, what: &str) -> Par {
    assert_eq!(data.len(), 1, "one datum holds the {what}");
    let mut pars = data.remove(0);
    assert_eq!(pars.len(), 1, "the datum of the {what} holds one value");
    pars.remove(0)
}

/// The one expression of `par`, when it has exactly one.
fn sole_expr(par: &Par) -> Option<&ExprInstance> {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(instance),
        }] => Some(instance),
        _ => None,
    }
}

/// The label of a delivery `[label, …]` on [`OUT`].
fn label_of(value: &Par) -> Option<String> {
    let first = match sole_expr(value)? {
        ExprInstance::EListBody(list) => list.ps.first()?,
        _ => return None,
    };
    match sole_expr(first)? {
        ExprInstance::GString(label) => Some(label.clone()),
        _ => None,
    }
}

/// The labels of the deliveries on [`OUT`] after `played`, in bytewise order.
fn labels(played: &Played) -> Vec<String> {
    let mut labels: Vec<String> = played.out.iter().flatten().filter_map(label_of).collect();
    labels.sort();
    labels
}

/// The labels `expected`, in bytewise order.
fn expect(expected: &[&str]) -> Vec<String> {
    let mut labels: Vec<String> = expected.iter().map(|label| label.to_string()).collect();
    labels.sort();
    labels
}

/// True when the merge index `index` holds an event on the channel with the hash `channel`.
fn index_touches(index: &EventLogIndex, channel: &Blake2b256Hash) -> bool {
    index
        .produces_linear
        .0
        .iter()
        .chain(index.produces_persistent.0.iter())
        .chain(index.produces_consumed.0.iter())
        .chain(index.produces_peeked.0.iter())
        .any(|produce| produce.channel_hash == *channel)
        || index
            .consumes_linear_and_peeks
            .0
            .iter()
            .chain(index.consumes_persistent.0.iter())
            .chain(index.consumes_produced.0.iter())
            .any(|consume| consume.channel_hashes.contains(channel))
}

/// A chain with the spaces of block 1 at their checkpoint root R1.
struct Rig {
    chain: NativeChain,
    spaces: Vec<(NativeSpace, Vec<Par>)>,
    root: Blake2b256Hash,
}

impl Rig {
    /// Block 1: one space for each `(urn, ctor)` pair, and the seed `seed` over the names of the
    /// spaces when it is not empty. The label `label` seeds the random states of block 1.
    async fn new(label: &str, spaces: &[(&str, &str)], seed: &str) -> Rig {
        let mut chain = NativeChain::new().await;
        let mut created = Vec::with_capacity(spaces.len());
        let mut root = None;
        for (stash, (urn, ctor)) in spaces.iter().enumerate() {
            let played = play_checked(
                &mut chain,
                &setup_program(urn, ctor, stash),
                &format!("{label}-setup-{stash}"),
                LIMIT,
            )
            .await;
            root = Some(played.root);
            let handle = sole(chain.values(&format!("{HANDLE}-{stash}")).await, "handle");
            let names = chain.values(&format!("{NAMES}-{stash}")).await;
            assert_eq!(names.len(), 1, "one datum holds the names");
            created.push((native(&handle), names[0].clone()));
        }
        if !seed.is_empty() {
            let played = play_checked(
                &mut chain,
                &with_names(seed, spaces.len()),
                &format!("{label}-seed"),
                LIMIT,
            )
            .await;
            root = Some(played.root);
        }
        Rig {
            chain,
            spaces: created,
            root: root.expect("block 1 plays a deploy"),
        }
    }

    /// One branch: `body` over the names, played from R1 with the label `label`.
    async fn branch(&mut self, body: &str, label: &str) -> Played {
        self.chain.reset(&self.root).await;
        let term = with_names(body, self.spaces.len());
        play_checked(&mut self.chain, &term, label, LIMIT).await
    }

    /// The serial run of `deploys` from R1, one deploy for each `(body, label)` pair, in order.
    /// Returns the post-state root.
    async fn serial(&mut self, deploys: &[(&str, &str)]) -> Blake2b256Hash {
        self.chain.reset(&self.root).await;
        let mut root = self.root.clone();
        for (body, label) in deploys {
            let term = with_names(body, self.spaces.len());
            root = play_checked(&mut self.chain, &term, label, LIMIT)
                .await
                .root;
        }
        root
    }

    /// The merge index of a log over R1.
    fn index(&self, log: &[Event]) -> EventLogIndex {
        index(&self.chain.history(), &self.root, log)
    }

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

    /// The merged state of two branches over R1.
    fn merged(&self, left: &Played, right: &Played) -> Blake2b256Hash {
        let (left_index, right_index) = (self.index(&left.log), self.index(&right.log));
        merged_root(&self.chain.history(), &self.root, &[
            (&left.root, &left_index),
            (&right.root, &right_index),
        ])
    }

    /// The hash of the data channel `D(sid,k)` of the slot `slot` of the space `space`.
    fn data(&self, space: usize, slot: usize) -> Blake2b256Hash {
        let (native, names) = &self.spaces[space];
        hash(&data_channel(*native, &names[slot]))
    }

    /// The hash of the counter channel `S(sid,k)` of the slot `slot` of the space `space`.
    fn counter(&self, space: usize, slot: usize) -> Blake2b256Hash {
        let (native, names) = &self.spaces[space];
        hash(&counter_channel(*native, &names[slot]))
    }

    /// The hash of the space counter `S(sid)` of the space `space`: the token of a PathMap
    /// space.
    fn token(&self, space: usize) -> Blake2b256Hash {
        hash(&space_counter_channel(self.spaces[space].0))
    }

    /// The hash of the allocator `A(sid)` of an Array or a Vector space.
    fn allocator(&self, space: usize) -> Blake2b256Hash {
        hash(&allocator::allocator_channel(self.spaces[space].0))
    }

    /// The hashes of the space counter and of the slot counters of every space.
    fn counters(&self) -> Vec<Blake2b256Hash> {
        let mut counters = Vec::with_capacity(self.spaces.len() * 5);
        for space in 0..self.spaces.len() {
            counters.push(self.token(space));
            counters.extend((SLOT_A..=SLOT_D).map(|slot| self.counter(space, slot)));
        }
        counters
    }
}

/// The checks of a branch that writes, takes or fires on the slot `slot` of the first space:
/// its log touches the slot, and its counter exactly when the family is ordered. Its merge index
/// holds the events of the slot.
fn check_branch(rig: &Rig, branch: &Played, slot: usize, ordered: bool, label: &str) {
    let data = rig.data(0, slot);
    assert!(
        touching(&branch.log, &data) > 0,
        "{label}: the log touches the data channel of the slot"
    );
    if ordered {
        assert!(
            touching(&branch.log, &rig.counter(0, slot)) > 0,
            "{label}: the log touches the counter of the slot"
        );
    } else {
        for counter in rig.counters() {
            assert_eq!(
                touching(&branch.log, &counter),
                0,
                "{label}: an operation of an unordered family touches no counter"
            );
        }
    }
    assert!(
        index_touches(&rig.index(&branch.log), &data),
        "{label}: the merge index holds the events of the slot"
    );
}

/// A conflict-free row: the branches `left` and `right` merge with no conflict and no
/// dependency, and the merged root is the root of both serial orders. Block 3, the program
/// `block3` over the names, then runs on the merged root in a restarted runtime and delivers the
/// labels `delivered` in all. Native replay reproduces it. Block 3 on the serial root certifies
/// and delivers the same labels. It gives the same root and phlo when both plays log the same
/// events in any order (decision 93).
// Changed by decision 93 (decision 111): block 3 races on both roots. The old text follows.
// /// labels `delivered` in all. Native replay reproduces it, and block 3 on the serial root gives
// /// the same root and phlo.
async fn assert_merge_row(
    rig: &mut Rig,
    (left_body, right_body): (&str, &str),
    block3: &str,
    delivered: &[&str],
    label: &str,
) -> (Played, Played) {
    let left_label = format!("{label}-left");
    let right_label = format!("{label}-right");
    let left = rig.branch(left_body, &left_label).await;
    let right = rig.branch(right_body, &right_label).await;
    assert!(
        rig.conflicts(&left.log, &right.log).is_empty(),
        "{label}: the branches conflict: {:?}",
        rig.reason(&left.log, &right.log)
    );
    let (left_index, right_index) = (rig.index(&left.log), rig.index(&right.log));
    assert!(
        !depends(&left_index, &right_index) && !depends(&right_index, &left_index),
        "{label}: a branch depends on the other"
    );
    let merged = rig.merged(&left, &right);
    let serial = rig
        .serial(&[(left_body, &left_label), (right_body, &right_label)])
        .await;
    let reversed = rig
        .serial(&[(right_body, &right_label), (left_body, &left_label)])
        .await;
    assert_eq!(merged, serial, "{label}: the merge is the serial state");
    assert_eq!(
        merged, reversed,
        "{label}: the merge is the reversed serial state"
    );
    let block3_label = format!("{label}-block3");
    let term = with_names(block3, rig.spaces.len());
    let mut restarted = rig.chain.restarted(&merged).await;
    let on_merge = restarted
        .play_with(&term, &block3_label, LIMIT, Injections::new())
        .await;
    check_deploy(&on_merge, &block3_label);
    assert_eq!(
        labels(&on_merge),
        expect(delivered),
        "{label}: the deliveries after block 3 on the merged root"
    );
    let certification = certify(&on_merge, &term, LIMIT).await;
    assert_reproduces(&block3_label, &on_merge, &certification);
    rig.chain.reset(&serial).await;
    let on_serial = rig
        .chain
        .play_with(&term, &block3_label, LIMIT, Injections::new())
        .await;
    check_deploy(&on_serial, &block3_label);
    // Changed by decision 93 (decision 111): block 3 races on both roots, so its root and its
    // phlo are equal only when both plays log the same events in any order. The old checks
    // follow.
    // assert_eq!(
    //     on_merge.root, on_serial.root,
    //     "{label}: block 3 on the merged root and on the serial root"
    // );
    // assert_eq!(
    //     on_merge.result.native_phlo_usage, on_serial.result.native_phlo_usage,
    //     "{label}: the phlo of block 3 on the merged root and on the serial root"
    // );
    let certification = certify(&on_serial, &term, LIMIT).await;
    assert_reproduces(
        &format!("{block3_label} on the serial root"),
        &on_serial,
        &certification,
    );
    assert_eq!(
        labels(&on_serial),
        expect(delivered),
        "{label}: the deliveries after block 3 on the serial root"
    );
    if unordered_log(&on_merge.log) == unordered_log(&on_serial.log) {
        assert_eq!(
            on_merge.root, on_serial.root,
            "{label}: block 3 on the merged root and on the serial root"
        );
        assert_eq!(
            on_merge.result.native_phlo_usage, on_serial.result.native_phlo_usage,
            "{label}: the phlo of block 3 on the merged root and on the serial root"
        );
    }
    (left, right)
}

/// A conflict row: the branches `left` and `right` race in merge check #1 on exactly the
/// channels `expected`. Without the events of the channels `control`, when it is not empty, the
/// branches merge. Block 3, the program `block3` over the names, then runs on the root of the
/// left branch, and native replay reproduces it.
async fn assert_race_row(
    rig: &mut Rig,
    (left_body, right_body): (&str, &str),
    expected: BTreeSet<Blake2b256Hash>,
    control: &[Blake2b256Hash],
    block3: &str,
    label: &str,
) -> (Played, Played) {
    let left = rig.branch(left_body, &format!("{label}-left")).await;
    let right = rig.branch(right_body, &format!("{label}-right")).await;
    assert_eq!(
        rig.conflicts(&left.log, &right.log),
        expected,
        "{label}: the channels of the race"
    );
    let reason = rig.reason(&left.log, &right.log);
    assert!(
        reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("racesForSameIOEvent")),
        "{label}: merge check #1 finds the race: {reason:?}"
    );
    if !control.is_empty() {
        assert!(
            rig.conflicts(
                &without_channels(&left.log, control),
                &without_channels(&right.log, control)
            )
            .is_empty(),
            "{label}: without the events of the control channels the branches merge"
        );
    }
    rig.chain.reset(&left.root).await;
    let block3_label = format!("{label}-block3");
    let term = with_names(block3, rig.spaces.len());
    let on_left = play_checked(&mut rig.chain, &term, &block3_label, LIMIT).await;
    let certification = certify(&on_left, &term, LIMIT).await;
    assert_reproduces(&block3_label, &on_left, &certification);
    (left, right)
}

// ─── The rows of the disciplines ───────────────────────────────────────────────────────────────

/// Both branches write the empty slot `d` of a bag, a set or a VectorDB space. The families use
/// no counter, so the branches merge, and the merged root is the root of both serial orders.
/// Block 3 takes the records of both branches. The two members of the set are equal, so the
/// set holds one class: its first take retires both members, and its second take waits.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_cross_block_merge_unordered_writes() {
    for family in FAMILIES.iter().filter(|family| !family.ordered) {
        let label = format!("rrs-p9-writes-{}", family.name);
        let mut rig = Rig::new(&label, &[(family.urn, family.ctor)], &family.seed()).await;
        let left_body = format!("d!({})", family.values[0]);
        let right_body = format!("d!({})", family.values[1]);
        let (block3, delivered): (String, &[&str]) = match family.name {
            "set" => (
                format!(
                    "for (@x <- d) {{ {} | for (@y <- d) {{ {} }} }}",
                    out("class", "x"),
                    out("again", "y")
                ),
                &["class"],
            ),
            _ => (
                format!(
                    "{} | {}",
                    family.take("d", "take"),
                    family.take("d", "take")
                ),
                &["take", "take"],
            ),
        };
        let (left, right) = assert_merge_row(
            &mut rig,
            (&left_body, &right_body),
            &block3,
            delivered,
            &label,
        )
        .await;
        for (branch, side) in [(&left, "left"), (&right, "right")] {
            check_branch(&rig, branch, SLOT_D, false, &format!("{label}-{side}"));
            assert_eq!(labels(branch), expect(&[]), "{label}-{side}: no delivery");
        }
    }
}

/// Both branches write the empty slot `d` of a queue, a stack, a cell or a priority queue. Each
/// write bumps the counter of `d`, so the branches race on it (M-1). In a second pair, the left
/// branch takes its own record after the write, which changes no record of the pre-state, and the
/// branches still race on the counter. Without the counter events, both pairs merge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_cross_block_merge_ordered_operations_race_on_the_counter() {
    for family in FAMILIES.iter().filter(|family| family.ordered) {
        let label = format!("rrs-p9-ordered-{}", family.name);
        let mut rig = Rig::new(&label, &[(family.urn, family.ctor)], &family.seed()).await;
        let counter = rig.counter(0, SLOT_D);
        let counters = rig.counters();
        let left_body = format!("d!({})", family.values[0]);
        let right_body = format!("d!({})", family.values[1]);
        let (left, right) = assert_race_row(
            &mut rig,
            (&left_body, &right_body),
            BTreeSet::from([counter.clone()]),
            &counters,
            &family.take("d", "take"),
            &format!("{label}-writes"),
        )
        .await;
        for (branch, side) in [(&left, "left"), (&right, "right")] {
            check_branch(
                &rig,
                branch,
                SLOT_D,
                true,
                &format!("{label}-writes-{side}"),
            );
        }
        let own = format!("d!({}) | {}", family.values[0], family.take("d", "own"));
        let (left, _) = assert_race_row(
            &mut rig,
            (&own, &right_body),
            BTreeSet::from([counter]),
            &counters,
            &format!("d!({})", family.values[1]),
            &format!("{label}-own"),
        )
        .await;
        assert_eq!(
            labels(&left),
            expect(&["own"]),
            "{label}-own: the left branch takes its own record"
        );
    }
}

/// Both branches fire the persistent waiter of block 1 on `b`. The fire of an ordered family
/// bumps the counter of `b` (decision 31), so the branches race on it. An unordered family has no
/// counter, and a persistent waiter stays, so the branches merge, and block 3 fires the waiter
/// again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_cross_block_merge_persistent_waiter_fired_on_both_branches() {
    for family in FAMILIES.iter() {
        let label = format!("rrs-p9-persistent-{}", family.name);
        let mut rig = Rig::new(&label, &[(family.urn, family.ctor)], &family.seed()).await;
        let left_body = format!("b!({})", family.values[0]);
        let right_body = format!("b!({})", family.values[1]);
        let (left, right) = if family.ordered {
            let counters = rig.counters();
            let counter = rig.counter(0, SLOT_B);
            assert_race_row(
                &mut rig,
                (&left_body, &right_body),
                BTreeSet::from([counter]),
                &counters,
                &left_body,
                &label,
            )
            .await
        } else {
            assert_merge_row(
                &mut rig,
                (&left_body, &right_body),
                &left_body,
                &["each", "each", "each"],
                &label,
            )
            .await
        };
        for (branch, side) in [(&left, "left"), (&right, "right")] {
            check_branch(
                &rig,
                branch,
                SLOT_B,
                family.ordered,
                &format!("{label}-{side}"),
            );
            assert_eq!(
                labels(branch),
                expect(&["each"]),
                "{label}-{side}: the waiter fires"
            );
        }
    }
}

/// Both branches fire the linear waiter of block 1 on `c`, so both consume it, and the branches
/// race on it in merge check #1, as two fires of one waiting continuation of a host channel do.
/// An ordered family also races on the counter of `c`. Block 3 sends again on `c` on the root of
/// the left branch, where the waiter is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_cross_block_merge_linear_waiter_fired_on_both_branches() {
    for family in FAMILIES.iter() {
        let label = format!("rrs-p9-linear-{}", family.name);
        let mut rig = Rig::new(&label, &[(family.urn, family.ctor)], &family.seed()).await;
        let left_body = format!("c!({})", family.values[0]);
        let right_body = format!("c!({})", family.values[1]);
        let mut expected = BTreeSet::from([rig.data(0, SLOT_C)]);
        if family.ordered {
            expected.insert(rig.counter(0, SLOT_C));
        }
        let (left, right) = assert_race_row(
            &mut rig,
            (&left_body, &right_body),
            expected,
            &[],
            &right_body,
            &label,
        )
        .await;
        for (branch, side) in [(&left, "left"), (&right, "right")] {
            check_branch(
                &rig,
                branch,
                SLOT_C,
                family.ordered,
                &format!("{label}-{side}"),
            );
            assert_eq!(
                labels(branch),
                expect(&["once"]),
                "{label}-{side}: the waiter fires"
            );
        }
    }
}

// ─── The rows of the outer stores ──────────────────────────────────────────────────────────────

/// Both branches run `new` inside `use` of one Array or Vector space. Each allocation takes the
/// allocator `A(sid)`, so the branches race on it. Block 1 drew the indices 0 to 3, so each
/// branch makes the index name 4. Without the allocator events, the branches merge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_cross_block_merge_new_in_use_races_on_the_allocator() {
    let body = format!(
        "new out in {{ use space {{ new n in {{ out!(*n) }} }} | for (@n <- out) {{ @\"{MINTED}\"!(n) }} }}"
    );
    for (store, urn, ctor) in [
        ("array", "rho:space:array:queue", r#""default"; size=8"#),
        ("vector", "rho:space:vector:queue", r#""default""#),
    ] {
        let label = format!("rrs-p9-allocator-{store}");
        let mut rig = Rig::new(&label, &[(urn, ctor)], "").await;
        let allocator = rig.allocator(0);
        let (left, right) = assert_race_row(
            &mut rig,
            (&body, &body),
            BTreeSet::from([allocator.clone()]),
            std::slice::from_ref(&allocator),
            &body,
            &label,
        )
        .await;
        let fourth = allocator::index_name(rig.spaces[0].0, 4);
        for (branch, side) in [(&left, "left"), (&right, "right")] {
            assert!(
                touching(&branch.log, &allocator) > 0,
                "{label}-{side}: the log touches the allocator"
            );
            assert!(
                index_touches(&rig.index(&branch.log), &allocator),
                "{label}-{side}: the merge index holds the allocation"
            );
            rig.chain.reset(&branch.root).await;
            assert_eq!(
                rig.chain.values(MINTED).await,
                vec![vec![fourth.clone()]],
                "{label}-{side}: the branch makes the index name 4"
            );
        }
    }
}

/// The branches write the names `a` and `b` of one PathMap space. Each write bumps the space
/// token `S(sid)`, so the branches race on it, as the P5 rig shows in the legacy model. Without
/// the token events, the branches merge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_cross_block_merge_pathmap_writes_race_on_the_token() {
    let label = "rrs-p9-pathmap-token";
    let mut rig = Rig::new(
        label,
        &[("rho:space:pathmap:bag", r#""default""#)],
        "a!(10)",
    )
    .await;
    let token = rig.token(0);
    let (left, right) = assert_race_row(
        &mut rig,
        ("a!(1)", "b!(2)"),
        BTreeSet::from([token.clone()]),
        std::slice::from_ref(&token),
        &format!("for (@x <- a) {{ {} }}", out("take", "x")),
        label,
    )
    .await;
    for (branch, side) in [(&left, "left"), (&right, "right")] {
        assert!(
            touching(&branch.log, &token) > 0,
            "{label}-{side}: the log touches the token"
        );
        assert!(
            index_touches(&rig.index(&branch.log), &token),
            "{label}-{side}: the merge index holds the token events"
        );
    }
}

/// Each branch writes the slot `d` of another queue space of block 1. The branches touch two
/// counters, so they merge, and the merged root is the root of both serial orders. Block 3
/// takes both records.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_cross_block_merge_distinct_spaces() {
    let label = "rrs-p9-distinct-spaces";
    let queue = ("rho:space:hashmap:queue", r#""default""#);
    let mut rig = Rig::new(label, &[queue, queue], "").await;
    let block3 = format!(
        "for (@x <- d) {{ {} }} | for (@y <- d2) {{ {} }}",
        out("first", "x"),
        out("second", "y")
    );
    let (left, right) = assert_merge_row(
        &mut rig,
        ("d!(1)", "d2!(2)"),
        &block3,
        &["first", "second"],
        label,
    )
    .await;
    assert!(
        touching(&left.log, &rig.counter(0, SLOT_D)) > 0
            && touching(&left.log, &rig.counter(1, SLOT_D)) == 0,
        "{label}: the left branch touches the counter of its own space only"
    );
    assert!(
        touching(&right.log, &rig.counter(1, SLOT_D)) > 0
            && touching(&right.log, &rig.counter(0, SLOT_D)) == 0,
        "{label}: the right branch touches the counter of its own space only"
    );
}

/// Each branch creates a queue space, writes one of its names, and publishes the name. The two
/// spaces have distinct ids, because the random states of the branches differ, so the branches
/// merge, and the merged root is the root of both serial orders. Block 3 takes the record of
/// each published name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_cross_block_merge_own_spaces() {
    let label = "rrs-p9-own-spaces";
    let mut rig = Rig::new(label, &[("rho:space:hashmap:queue", r#""default""#)], "").await;
    let body = format!(
        r#"new factory(`rho:space:hashmap:queue`) in {{
  for (s <- factory!?("default")) {{
    for (n <- s!?("gensym")) {{ n!(1) | @"{OWN}"!(*n) }}
  }}
}}"#
    );
    let block3 = format!(
        "for (n <= @\"{OWN}\") {{ for (@x <- n) {{ {} }} }}",
        out("take", "x")
    );
    let (left, right) =
        assert_merge_row(&mut rig, (&body, &body), &block3, &["take", "take"], label).await;
    for counter in rig.counters() {
        assert_eq!(
            touching(&left.log, &counter) + touching(&right.log, &counter),
            0,
            "{label}: the branches touch no counter of block 1"
        );
    }
}

// ─── The row of the reference agent ────────────────────────────────────────────────────────────

// The reference agent, its kit names and `with_kit_names` moved to `support::family_programs`
// (P9 step 3), so the P9 suites share them.

/// Both branches call the reference agent of block 1 with a send on `x`. The agent keeps its
/// messages in a state cell, an ordinary channel that each call consumes and produces again, so
/// the branches race on it in merge check #1, as two calls of a contract with a state cell do.
/// No conflict channel is a Reified channel. Block 3 takes the message of the left branch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_cross_block_merge_agent_calls_race_on_its_state() {
    let label = "rrs-p9-agent";
    let mut chain = NativeChain::new().await;
    play_checked(
        &mut chain,
        &reference_agent_creation(),
        &format!("{label}-creation"),
        LIMIT,
    )
    .await;
    let root = play_checked(&mut chain, KIT_NAMES, &format!("{label}-names"), LIMIT)
        .await
        .root;
    let mut branches = Vec::with_capacity(2);
    for (side, value) in [("left", 1), ("right", 2)] {
        chain.reset(&root).await;
        let term = with_kit_names(&format!("@x!({value})"));
        branches.push(play_checked(&mut chain, &term, &format!("{label}-{side}"), LIMIT).await);
    }
    let history = chain.history();
    let indices: Vec<EventLogIndex> = branches
        .iter()
        .map(|branch| index(&history, &root, &branch.log))
        .collect();
    let raced: BTreeSet<Blake2b256Hash> =
        conflicts(&indices[0], &indices[1]).0.into_iter().collect();
    assert!(!raced.is_empty(), "{label}: two calls of the agent race");
    let reason = conflict_reason(&indices[0], &indices[1]);
    assert!(
        reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("racesForSameIOEvent")),
        "{label}: merge check #1 finds the race: {reason:?}"
    );
    let mut channels: HashMap<Blake2b256Hash, Par> = HashMap::new();
    for branch in &branches {
        let journal = branch
            .result
            .native_operation_recording
            .as_ref()
            .expect("native play records its operations");
        for row in journal.iter() {
            for channel in footprint_channels(row) {
                channels.insert(hash(&channel), channel);
            }
        }
    }
    for conflict in &raced {
        let channel = channels
            .get(conflict)
            .unwrap_or_else(|| panic!("{label}: a conflict channel is in no footprint"));
        assert!(
            namespace::reified_id_of(channel).is_none(),
            "{label}: the race is on a Reified channel: {channel:?}"
        );
    }
    chain.reset(&branches[0].root).await;
    let block3 = with_kit_names(&format!("for (@v <- @x) {{ {} }}", out("took", "v")));
    let played = play_checked(&mut chain, &block3, &format!("{label}-block3"), LIMIT).await;
    assert_eq!(
        labels(&played),
        expect(&["took"]),
        "{label}: block 3 takes the message"
    );
    let certification = certify(&played, &block3, LIMIT).await;
    assert_reproduces(&format!("{label}-block3"), &played, &certification);
}

// ─── Event coverage (A4) ───────────────────────────────────────────────────────────────────────

// `HASHSET_INNERS` and `hashset_program` moved to `support::family_programs` (P9 step 3), so the
// P9 suites share them.

/// The merge of the log of each deploy over its pre-state gives its post-state (A4): the 24
/// family programs with the qualifiers `default` and `temp`, the six HashSet families with
/// `seq`, the stored families of P7 across two deploys, and the conformance kit of the
/// reference agent. Peeks are in the family programs and in each deploy that reads a stash.
/// The roots of the merge trace of the specification (section 11, Determinism): R1, the left
/// branch, the right branch and the merge.
// Changed by decision 90 (step E): the merge with `feature/cost-accounted-rho` at `702496775`
// changes the four roots of the trace. The old values follow.
// const MERGE_TRACE_ROOTS: [&str; 4] = [
//     "98c68b5e2deb454a81cded3fec6dfa349081beb6f7f234b2501ec0a97795165f",
//     "d7cabfa35f3dd880fdcf8705d1c5d3656940c3f96f3e85db472845bda9ec7b12",
//     "dc94fd62a404c3e956dc4da1afafecedc95289808b1beee2b09d1cc94744a2fa",
//     "8825f3dd7faa2838e09f01cc305babe2d3580d7c9798944c22673d2189e69bf3",
// ];
const MERGE_TRACE_ROOTS: [&str; 4] = [
    "23f3bef617558e0271677109023fc2eb3d9b8f16cc4270459ab9e88d095d2300",
    "3b2074213e0ee26f24c049990ebd9c5401e7291029eeb4f464b6356a043bcc01",
    "4b0df8853f40f33b839884bcaed13ce209f78452d32b2380cf2ddd04ddf24c1d",
    "c74fc252fbb9692596c7cd0bd9667971b6eca981dccbd31eb02d82f5bd1c5413",
];

/// The merge trace of the specification (section 11, Determinism). Block 1 creates a bag space
/// and draws the names `a` to `d` from it, and the chain checkpoints R1. Two branches each store
/// one record on `d` from R1. They do not conflict, and the merged root is the root of both
/// serial orders. The test pins the four roots that the specification records, so a change of
/// one of them changes the trace of the specification too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_determinism_merge_trace_roots_are_pinned() {
    let mut rig = Rig::new(
        "rrs-spec-merge",
        &[("rho:space:hashmap:bag", r#""default""#)],
        "",
    )
    .await;
    let (left_body, left_label) = ("d!(1)", "rrs-spec-merge-left");
    let (right_body, right_label) = ("d!(2)", "rrs-spec-merge-right");
    let left = rig.branch(left_body, left_label).await;
    let right = rig.branch(right_body, right_label).await;
    assert!(
        rig.conflicts(&left.log, &right.log).is_empty(),
        "the branches of the merge trace conflict: {:?}",
        rig.reason(&left.log, &right.log)
    );
    let merged = rig.merged(&left, &right);
    let serial = rig
        .serial(&[(left_body, left_label), (right_body, right_label)])
        .await;
    let reversed = rig
        .serial(&[(right_body, right_label), (left_body, left_label)])
        .await;
    assert_eq!(merged, serial, "the merge is the serial state");
    assert_eq!(merged, reversed, "the merge is the reversed serial state");
    let roots = [&rig.root, &left.root, &right.root, &merged].map(|root| hex::encode(root.bytes()));
    assert_eq!(
        roots, MERGE_TRACE_ROOTS,
        "the roots of the merge trace of specification section 11"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_event_coverage_debug_assert() {
    for qualifier in ["default", "temp"] {
        for outer in Outer::ALL {
            for inner in Inner::ALL {
                let label = format!("{}:{}:{qualifier}", outer.name(), inner.name());
                let played = native_play(&program_with(outer, inner, qualifier), LIMIT).await;
                check_deploy(&played, &label);
            }
        }
    }
    for inner in HASHSET_INNERS {
        let label = format!("hashset:{inner}:seq");
        let played = native_play(&hashset_program(inner, HASHSET_UNIT_TAKE), HASHSET_LIMIT).await;
        check_deploy(&played, &label);
        assert_eq!(labels(&played), expect(&["took"]), "{label}: the take");
    }
    let mut chain = NativeChain::new().await;
    for family in STORED_FAMILIES.iter() {
        play_checked(
            &mut chain,
            &family.creation(),
            &format!("stored-{}-creation", family.label),
            LIMIT,
        )
        .await;
        play_checked(
            &mut chain,
            &family.touching(),
            &format!("stored-{}-touch", family.label),
            LIMIT,
        )
        .await;
    }
    let mut chain = NativeChain::new().await;
    let kit = kit_deploys();
    for (step, term) in kit.iter().enumerate() {
        play_checked(&mut chain, term, &format!("kit-step-{step}"), LIMIT).await;
    }
}

/// The negative control of the coverage check: a branch log without its counter events, or
/// without its token events. The merge of the log over the pre-state then misses the change of
/// the dropped channel, so the reproduction fails, and the diagnostic names the channel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_event_coverage_names_dropped_channels() {
    let queue = FAMILIES
        .iter()
        .find(|family| family.name == "queue")
        .expect("the queue family");
    let label = "rrs-p9-coverage-queue";
    let mut rig = Rig::new(label, &[(queue.urn, queue.ctor)], &queue.seed()).await;
    let branch = rig.branch("d!(1)", &format!("{label}-branch")).await;
    let counter = rig.counter(0, SLOT_D);
    let label = "rrs-p9-coverage-pathmap";
    let mut pathmap = Rig::new(
        label,
        &[("rho:space:pathmap:bag", r#""default""#)],
        "a!(10)",
    )
    .await;
    let write = pathmap.branch("a!(1)", &format!("{label}-branch")).await;
    let token = pathmap.token(0);
    for (rig, branch, dropped, what) in [
        (&rig, &branch, counter, "the counter"),
        (&pathmap, &write, token, "the token"),
    ] {
        let stripped = without_channels(&branch.log, std::slice::from_ref(&dropped));
        assert!(
            stripped.len() < branch.log.len(),
            "the control drops the events of {what}"
        );
        let outcome = coverage(
            &rig.chain.history(),
            &branch.pre_root,
            &branch.root,
            &stripped,
            &branch.snapshot_keys,
        );
        assert!(
            !outcome.reproduced,
            "the log without the events of {what} does not reproduce the post-state"
        );
        assert!(
            outcome.uncovered.data.contains(&dropped),
            "the diagnostic names {what}: {:?}",
            outcome.uncovered
        );
    }
}
