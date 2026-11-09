//! The pattern-enforcement matrix N3 of the native spaces (tracker leaf
//! `rrs-tests-verification-n3`, P5 design v2 sections 2.8.4 and 3.3): every pair of a space family
//! and a qualifier either runs with real matching, or fails closed.
//!
//! **Supported pairs.** A family that the family agent wires runs with the `default` qualifier.
//! For each of these families, a pattern for 42 never receives 43, in both orders: a send of 43
//! before a receive for 42, and a receive for 42 before a send of 43. Both entries stay in the
//! collection with their orders, and no COMM touches the collection. A receive for 42 on a fresh
//! name then takes a send of 42, so the matcher is not vacuous. The rows are the families that
//! the family agent wires. They combine the disciplines `bag`, `queue`, `stack`, `set`, `cell`
//! and `priorityqueue` with the stores `hashmap`, `array` and `vector`. Wave W2 added the rows
//! `hashmap:{bag,queue,stack,cell}`, `array:queue` and `vector:queue`, and wave W3 adds the set and
//! priority-queue rows and the other Array and Vector rows. P6 adds the rows `hashmap:vectordb`
//! and `vector:vectordb`. A receive without a query reads the records of a VectorDB collection in
//! their canonical order, so a literal pattern selects the records as on a bag.
//!
//! **Joins (P5c, decision 29).** A join of two names of one space runs in the join engine. A join
//! whose first bind is a pattern for 42 never receives 43 either, in both orders, on a queue, a
//! bag, a set and a VectorDB collection. The records and the join waiter stay, and no COMM
//! touches either collection. A join for 42 on two fresh names receives 42.
//!
//! **Unsupported pairs.** These pairs reject before any space exists:
//!
//! * The pairs that the URN grammar refuses: `pathmap:vectordb`, `array:vectordb` and
//!   `hashset:vectordb`. No factory of them is registered, so a Rholang deploy that names one
//!   fails at `new` and leaves no space data. A creation or a stored config that names one fails
//!   in the family agent with `FamilyUnavailable`.
//! * The six HashSet families (P7 step 3, HS-6) with a qualifier other than `seq`: their factory
//!   refuses `default` and `temp` with `BadConstructorArgument`. The HashSet rows of the matrix
//!   run in `seq` spaces (below).
//!
//! **Qualifiers.** The rows above run in native execution, the one execution model of spaces
//! (decision 90, C6). No factory refuses a qualifier that its store takes, so the native rows
//! below run the wired families in `temp` and `seq` spaces.
// Changed by decision 90 (C6, step D): the session runs native deploys, and gate 3 reads no
// execution model. The legacy text follows.
// //! * The six HashSet families (P7 step 3, HS-6) with a qualifier other than `seq`: their factory
// //!   refuses `default` and `temp` with `BadConstructorArgument`. Legacy execution also refuses
// //!   `seq` (QT-5), so the legacy rows create no HashSet space. The HashSet rows of the matrix run
// //!   in native execution (below).
// //!
// //! **Qualifiers.** The rows above run in legacy execution, which refuses the `seq` qualifier with
// //! `QualifierUnavailable` on every factory of this wave, before the gates of the params (P7,
// //! design v3 QT-5).
//!
//! **The native rows of P7 (design v3 section 10, A7; F17 of decision 39).** A `temp` or `seq`
//! space lives for one deploy, so each of these rows runs in one native deploy that certifies
//! through its committed evidence, and it reads its result from the event log and the replies:
//!
//! * `pattern_enforcement_temp_rows`: on the 26 wired families, a pattern for 42 never receives
//!   43 in a `temp` space, in both source orders, and the log shows the record and the waiter on
//!   the channels of their name with no COMM. A pattern for 42 receives 42.
//! * `pattern_enforcement_seq_rows`: the same in a `seq` space, with a handle `produce` before
//!   each receive (SQ-3). The names of a seq space are never sent, so the row reads its
//!   collections from the journal and compares its COMMs with the row without its negative part.
//! * `pattern_enforcement_hashset_rows`: on the six HashSet families, a unit is taken by an empty
//!   receive, a send of 43 fails with `PayloadNotEmpty`, and a receive for 42 with
//!   `PatternNotEmpty`.
//! * `pattern_enforcement_native_unsupported_pairs_reject`: a HashSet family with `default` or
//!   `temp` fails with `BadConstructorArgument`, and a VectorDB family with `temp` or `seq` and a
//!   mirror with `InvalidParam`, before any space exists.
//!
//! The property `prop_pattern_enforcement_any_wired_family` restates `PatternEnforcement` (V2 of
//! `GenericRSpace.tla`) and `wildcard_matcher_degenerate` (`Match.v`) end to end through Rholang:
//! for generated families, values, orders and arrows, a literal pattern never receives another
//! value, and it receives its own value.

#[path = "support/mod.rs"]
mod support;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::space_factory_urn::{
    SpaceFactoryUrn, SpaceFactoryUrnError, SpaceInner, SpaceOuter, SpaceQualifier,
};
use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::spaces::agent::{SpaceAgent, SpaceOp};
use rholang::rust::interpreter::spaces::errors::SpaceError;
// Changed by decision 90 (C6, step D): the factory reads no execution model.
// use rholang::rust::interpreter::spaces::factory::constructor::{Execution, FactoryConstructor};
use rholang::rust::interpreter::spaces::factory::constructor::FactoryConstructor;
use rholang::rust::interpreter::spaces::factory::defaults::constructor_schema;
use rholang::rust::interpreter::spaces::factory::urn::registered_channel;
use rholang::rust::interpreter::spaces::families::{send_params, Family, FamilyAgent};
use rholang::rust::interpreter::spaces::namespace::{
    self, Durability, NativeSpace, ReifiedId, SID_LEN,
};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier as ConfigQualifier;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use support::agent::{budget, creation, env_over, play, run, send_on};
use support::native::{
    assert_reproduces, certify, footprint_channels, is_space_row, native_play, Played, AMPLE_LIMIT,
    OUT,
};
use support::props::{check, runtime_config, tokio_runtime};
use support::session::{comms_on, data_channel, error_codes, with_space, Injections, Session};

// ─── Families and programs ───────────────────────────────────────────────────────────────────

/// A row of the matrix: a family that the family agent wires in this wave, with the constructor
/// of its factory as Rholang source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    /// The outer store of the family.
    outer: SpaceOuter,
    /// The inner collection of the family.
    inner: SpaceInner,
    /// The constructor arguments of the factory call, as Rholang source.
    constructor: &'static str,
}

impl Row {
    /// The family of the row.
    fn family(self) -> SpaceFactoryUrn {
        SpaceFactoryUrn {
            outer: self.outer,
            inner: self.inner,
        }
    }

    /// True when every operation of the discipline bumps a counter, so the entries of a
    /// collection have the orders of their operations. Bag, set and VectorDB records and waiters
    /// have the order 0.
    fn counted(self) -> bool {
        !matches!(
            self.inner,
            SpaceInner::Bag | SpaceInner::Set | SpaceInner::VectorDb
        )
    }
}

/// The constructor arguments of a family without params.
const DEFAULT: &str = r#""default""#;

/// The constructor arguments of a fixed Array space of four index names.
const SIZED: &str = r#""default"; size=4"#;

/// The constructor arguments of a VectorDB space of four records with two dimensions and the
/// default metric.
const SIMILAR: &str = r#""default"; capacity=4, dimensions=2"#;

/// The rows: the six of wave W2, then the twelve of wave W3, then the two of P6.
const ROWS: [Row; 20] = [
    Row {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::Bag,
        constructor: r#""default""#,
    },
    Row {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::Queue,
        constructor: r#""default""#,
    },
    Row {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::Stack,
        constructor: r#""default""#,
    },
    Row {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::Cell,
        constructor: r#""default""#,
    },
    Row {
        outer: SpaceOuter::Array,
        inner: SpaceInner::Queue,
        constructor: r#""default"; size=4"#,
    },
    Row {
        outer: SpaceOuter::Vector,
        inner: SpaceInner::Queue,
        constructor: r#""default""#,
    },
    Row {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::Set,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::PriorityQueue,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::Array,
        inner: SpaceInner::Bag,
        constructor: SIZED,
    },
    Row {
        outer: SpaceOuter::Array,
        inner: SpaceInner::Stack,
        constructor: SIZED,
    },
    Row {
        outer: SpaceOuter::Array,
        inner: SpaceInner::Set,
        constructor: SIZED,
    },
    Row {
        outer: SpaceOuter::Array,
        inner: SpaceInner::Cell,
        constructor: SIZED,
    },
    Row {
        outer: SpaceOuter::Array,
        inner: SpaceInner::PriorityQueue,
        constructor: SIZED,
    },
    Row {
        outer: SpaceOuter::Vector,
        inner: SpaceInner::Bag,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::Vector,
        inner: SpaceInner::Stack,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::Vector,
        inner: SpaceInner::Set,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::Vector,
        inner: SpaceInner::Cell,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::Vector,
        inner: SpaceInner::PriorityQueue,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::VectorDb,
        constructor: SIMILAR,
    },
    Row {
        outer: SpaceOuter::Vector,
        inner: SpaceInner::VectorDb,
        constructor: SIMILAR,
    },
];

/// The row of the family `outer:inner`.
fn row(outer: SpaceOuter, inner: SpaceInner) -> Row {
    ROWS.into_iter()
        .find(|row| row.outer == outer && row.inner == inner)
        .unwrap_or_else(|| panic!("no row for {}:{}", outer.as_str(), inner.as_str()))
}

/// The output channel of the programs of the matrix.
const GOT: &str = "rrs-n3-got";

/// The integer `value` as a `Par`.
fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

/// The string `value` as a `Par`.
fn gstring(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

/// The output `[label, value]` of a fire.
fn got(label: &str, value: i64) -> Vec<Par> {
    vec![new_elist_par(
        vec![gstring(label), gint(value)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    )]
}

/// The elements of the only list of `par`.
fn list_elements(par: &Par) -> Vec<Par> {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
        }] => ps.clone(),
        other => panic!("the value is not one list: {other:?}"),
    }
}

/// The arrow of a receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arrow {
    /// `<-`
    Plain,
    /// `<<-`
    Peek,
    /// `<=`
    Persistent,
}

impl Arrow {
    /// The arrow as Rholang source.
    fn source(self) -> &'static str {
        match self {
            Arrow::Plain => "<-",
            Arrow::Peek => "<<-",
            Arrow::Persistent => "<=",
        }
    }
}

/// The receive on the name `name` for the literal `pattern`, with `arrow`. Its body sends
/// `[label, pattern]` to the output channel.
fn receive_source(name: &str, pattern: i64, arrow: Arrow, label: &str) -> String {
    format!(
        r#"for (@{pattern} {} {name}) {{ @"{GOT}"!(["{label}", {pattern}]) }}"#,
        arrow.source()
    )
}

/// The send of `value` on the name `name`, persistent with `persistent`.
fn send_source(name: &str, value: i64, persistent: bool) -> String {
    match persistent {
        true => format!("{name}!!({value})"),
        false => format!("{name}!({value})"),
    }
}

/// The positive control: a receive for `value` and a send of `value` on a fresh name of the
/// space. The receive fires once, whatever the order of the two.
fn positive_source(value: i64) -> String {
    format!(
        r#"for (c <- space!?("gensym")) {{ for (@{value} <- c) {{ @"{GOT}"!(["positive", {value}]) }} | c!({value}) }}"#
    )
}

/// One order of the negative case on one name: the deploys in order.
fn negative_deploys(name: &str, send_first: bool, row_label: &str) -> [String; 2] {
    let send = with_space(&send_source(name, 43, false));
    let receive = with_space(&receive_source(name, 42, Arrow::Plain, row_label));
    match send_first {
        true => [send, receive],
        false => [receive, send],
    }
}

/// The matrix row of `row`: a pattern for 42 never receives 43, in both orders, and a pattern for
/// 42 receives 42.
async fn check_row(row: Row) {
    let urn = row.family().to_string();
    let mut session = Session::new().await;
    let space = session.setup(&urn, row.constructor).await;
    for (name, par, send_first) in [("a", &space.a, true), ("b", &space.b, false)] {
        let data = data_channel(space.space, par);
        let [first, second] = negative_deploys(name, send_first, &urn);
        for (index, source) in [first, second].iter().enumerate() {
            let deploy = session
                .run(source, &format!("rrs-n3 {urn} {name} {index}"))
                .await;
            assert_eq!(
                comms_on(&deploy.log, &data),
                0,
                "{urn}: no COMM touches the collection of {name}"
            );
        }
        assert!(
            session.values(GOT).await.is_empty(),
            "{urn}: a pattern for 42 never receives 43 ({name})"
        );
        let (send_ord, receive_ord) = match (row.counted(), send_first) {
            (false, _) => (0, 0),
            (true, true) => (0, 1),
            (true, false) => (1, 0),
        };
        assert_eq!(
            session.records(space.space, par).await,
            vec![(send_ord, vec![gint(43)], false)],
            "{urn}: the record 43 stays in the collection of {name}"
        );
        assert_eq!(
            session.parked(space.space, par).await,
            vec![(receive_ord, false, false)],
            "{urn}: the waiter for 42 stays in the collection of {name}"
        );
    }
    session
        .run(
            &with_space(&positive_source(42)),
            &format!("rrs-n3 {urn} positive"),
        )
        .await;
    assert_eq!(
        session.values(GOT).await,
        vec![got("positive", 42)],
        "{urn}: a pattern for 42 receives 42"
    );
}

// ─── The supported rows ──────────────────────────────────────────────────────────────────────

/// `hashmap:bag`: a pattern for 42 never receives 43, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_bag() { check_row(ROWS[0]).await; }

/// `hashmap:queue`: a pattern for 42 never receives 43, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_queue() { check_row(ROWS[1]).await; }

/// `hashmap:stack`: a pattern for 42 never receives 43, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_stack() { check_row(ROWS[2]).await; }

/// `hashmap:cell`: a pattern for 42 never receives 43, in both orders. The cell then holds the
/// record 43 in its record slot and the waiter for 42 in its waiter slot.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_cell() { check_row(ROWS[3]).await; }

/// `array:queue`, a fixed Array space of size 4: a pattern for 42 never receives 43 on its index
/// names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_array_queue() { check_row(ROWS[4]).await; }

/// `vector:queue`: a pattern for 42 never receives 43 on its index names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_vector_queue() { check_row(ROWS[5]).await; }

/// `hashmap:set`: a pattern for 42 never receives 43, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_set() {
    check_row(row(SpaceOuter::HashMap, SpaceInner::Set)).await;
}

/// `hashmap:priorityqueue`: a pattern for 42 never receives 43, in both orders, with sends and
/// receives without a priority.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_priorityqueue() {
    check_row(row(SpaceOuter::HashMap, SpaceInner::PriorityQueue)).await;
}

/// `array:bag`, a fixed Array space of size 4: a pattern for 42 never receives 43 on its index
/// names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_array_bag() {
    check_row(row(SpaceOuter::Array, SpaceInner::Bag)).await;
}

/// `array:stack`: a pattern for 42 never receives 43 on its index names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_array_stack() {
    check_row(row(SpaceOuter::Array, SpaceInner::Stack)).await;
}

/// `array:set`: a pattern for 42 never receives 43 on its index names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_array_set() {
    check_row(row(SpaceOuter::Array, SpaceInner::Set)).await;
}

/// `array:cell`: a pattern for 42 never receives 43 on its index names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_array_cell() {
    check_row(row(SpaceOuter::Array, SpaceInner::Cell)).await;
}

/// `array:priorityqueue`: a pattern for 42 never receives 43 on its index names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_array_priorityqueue() {
    check_row(row(SpaceOuter::Array, SpaceInner::PriorityQueue)).await;
}

/// `vector:bag`: a pattern for 42 never receives 43 on its index names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_vector_bag() {
    check_row(row(SpaceOuter::Vector, SpaceInner::Bag)).await;
}

/// `vector:stack`: a pattern for 42 never receives 43 on its index names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_vector_stack() {
    check_row(row(SpaceOuter::Vector, SpaceInner::Stack)).await;
}

/// `vector:set`: a pattern for 42 never receives 43 on its index names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_vector_set() {
    check_row(row(SpaceOuter::Vector, SpaceInner::Set)).await;
}

/// `vector:cell`: a pattern for 42 never receives 43 on its index names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_vector_cell() {
    check_row(row(SpaceOuter::Vector, SpaceInner::Cell)).await;
}

/// `vector:priorityqueue`: a pattern for 42 never receives 43 on its index names, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_vector_priorityqueue() {
    check_row(row(SpaceOuter::Vector, SpaceInner::PriorityQueue)).await;
}

/// `hashmap:vectordb`: a pattern for 42 never receives 43, in both orders, with receives without
/// a query. The record 43 is not a vector, so it has no embedding.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_vectordb() {
    check_row(row(SpaceOuter::HashMap, SpaceInner::VectorDb)).await;
}

/// `vector:vectordb`: a pattern for 42 never receives 43 on its index names, in both orders, with
/// receives without a query.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_vector_vectordb() {
    check_row(row(SpaceOuter::Vector, SpaceInner::VectorDb)).await;
}

// ─── The join rows ───────────────────────────────────────────────────────────────────────────

/// The number of COMMs of `log` whose consume names one of the channels `channels`.
fn join_comms(log: &[Event], channels: &[Par]) -> usize {
    let hashes: Vec<Blake2b256Hash> = channels.iter().map(hash).collect();
    log.iter()
        .filter(|event| match event {
            Event::Comm(comm) => comm
                .consume
                .channel_hashes
                .iter()
                .any(|channel| hashes.contains(channel)),
            Event::IoEvent(_) => false,
        })
        .count()
}

/// The join row of `row` (P5c, decision 29): the join `for (@42 <- a & @y <- b)` never receives
/// the record 43 of `a`, in both orders. The two records and the join waiter stay, and no COMM
/// touches either collection. A join for 42 on two fresh names receives 42.
async fn check_join_row(row: Row) {
    let urn = row.family().to_string();
    for send_first in [true, false] {
        let mut session = Session::new().await;
        let space = session.setup(&urn, row.constructor).await;
        let channels = [
            data_channel(space.space, &space.a),
            data_channel(space.space, &space.b),
        ];
        let sends = with_space("a!(43) | b!(1)");
        let join = with_space(&format!(
            r#"for (@42 <- a & @y <- b) {{ @"{GOT}"!(["join", y]) }}"#
        ));
        let deploys = match send_first {
            true => [sends, join],
            false => [join, sends],
        };
        for (index, source) in deploys.iter().enumerate() {
            let deploy = session
                .run(source, &format!("rrs-n3 join {urn} {send_first} {index}"))
                .await;
            assert_eq!(
                join_comms(&deploy.log, &channels),
                0,
                "{urn}: no COMM touches the joined collections"
            );
        }
        assert!(
            session.values(GOT).await.is_empty(),
            "{urn}: a join for 42 never receives 43"
        );
        for (name, value) in [(&space.a, 43), (&space.b, 1)] {
            let records = session.records(space.space, name).await;
            assert_eq!(
                records
                    .iter()
                    .map(|(_, payload, persistent)| (payload.clone(), *persistent))
                    .collect::<Vec<_>>(),
                vec![(vec![gint(value)], false)],
                "{urn}: the record {value} stays"
            );
        }
        let mut group = channels.to_vec();
        group.sort_by_key(hash);
        assert_eq!(
            session.play.get_continuations(group).await.len(),
            1,
            "{urn}: the join waiter stays"
        );
    }
    let mut session = Session::new().await;
    session.setup(&urn, row.constructor).await;
    session
        .run(
            &with_space(&format!(
                r#"for (c <- space!?("gensym")) {{ for (d <- space!?("gensym")) {{ for (@42 <- c & @y <- d) {{ @"{GOT}"!(["positive", y]) }} | c!(42) | d!(7) }} }}"#
            )),
            &format!("rrs-n3 join {urn} positive"),
        )
        .await;
    assert_eq!(
        session.values(GOT).await,
        vec![got("positive", 7)],
        "{urn}: a join for 42 receives 42"
    );
}

/// `hashmap:queue`: a join whose first bind is a pattern for 42 never receives 43.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_queue_join() {
    check_join_row(row(SpaceOuter::HashMap, SpaceInner::Queue)).await;
}

/// `hashmap:bag`: a join whose first bind is a pattern for 42 never receives 43.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_bag_join() {
    check_join_row(row(SpaceOuter::HashMap, SpaceInner::Bag)).await;
}

/// `hashmap:set`: a join whose first bind is a pattern for 42 never receives 43.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_set_join() {
    check_join_row(row(SpaceOuter::HashMap, SpaceInner::Set)).await;
}

/// `hashmap:vectordb`: a join whose first bind is a pattern for 42 never receives 43. The binds
/// have no query, so each position reads its records in their canonical order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashmap_vectordb_join() {
    check_join_row(row(SpaceOuter::HashMap, SpaceInner::VectorDb)).await;
}

// ─── The unsupported pairs ───────────────────────────────────────────────────────────────────

// Changed by P7 step 3 (design v3 HS-7): the six HashSet families are registered, so the URN
// grammar names no unsupported family. A HashSet family refuses only its other qualifiers.
// /// The unsupported families of this phase: the HashSet families (P7). Their URNs parse, but no
// /// factory is registered.
// fn unsupported_families() -> Vec<SpaceFactoryUrn> {
//     SpaceFactoryUrn::all()
//         .filter(|family| family.outer == SpaceOuter::HashSet)
//         .collect()
// }

/// The six HashSet families (P7), which take only the `seq` qualifier.
fn hashset_families() -> Vec<SpaceFactoryUrn> {
    SpaceFactoryUrn::all()
        .filter(|family| family.outer == SpaceOuter::HashSet)
        .collect()
}

/// The pairs that the URN grammar refuses: VectorDB on a store other than HashMap and Vector.
const REFUSED_PAIRS: [(SpaceOuter, SpaceInner); 3] = [
    (SpaceOuter::PathMap, SpaceInner::VectorDb),
    (SpaceOuter::Array, SpaceInner::VectorDb),
    (SpaceOuter::HashSet, SpaceInner::VectorDb),
];

/// The space of the agent checks of the unsupported pairs.
const SPACE: NativeSpace = NativeSpace {
    sid: [0x4E; SID_LEN],
    durability: Durability::Durable,
};

/// The qualifier of a config for `family`: `seq` for a HashSet family, which takes no other, and
/// `default` otherwise.
fn config_qualifier(family: SpaceFactoryUrn) -> ConfigQualifier {
    match family.outer {
        SpaceOuter::HashSet => ConfigQualifier::Seq,
        SpaceOuter::HashMap | SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::Vector => {
            ConfigQualifier::Default
        }
        // Added by decision 127 (kind registry): `SpaceOuter` is a name, so the match needs a
        // wildcard. No family of the suite reaches it.
        _ => ConfigQualifier::Default,
    }
}

/// The error of an unavailable family.
fn unavailable(outer: SpaceOuter, inner: SpaceInner) -> SpaceError {
    SpaceError::FamilyUnavailable {
        outer: outer.as_str().to_string(),
        inner: inner.as_str().to_string(),
    }
}

// Changed by decision 90 (C6, step D): gate 3 reads no execution model, and the session runs
// native deploys. The legacy text follows.
//
// /// The pairs that the URN grammar refuses, and the HashSet families with a qualifier other than
// /// `seq`, reject before any space exists, at every layer:
// ///
// /// * The URN parser refuses the pairs of [`REFUSED_PAIRS`] with `UnsupportedPair`.
// /// * Each HashSet family has a registered factory, a dispatch arm, a send schema and a
// ///   constructor schema (P7 step 3). Its constructor refuses `default` and `temp` with
// ///   `BadConstructorArgument` in both execution models, and legacy execution refuses `seq` with
// ///   `QualifierUnavailable` (QT-5).
// /// * The family agent refuses the creation of each refused pair with `FamilyUnavailable`, and it
// ///   stores nothing. A send on a space whose stored config names the pair fails the same way.
// /// * A Rholang deploy that names the factory of a refused pair fails at `new`, before any space
// ///   work. A legacy deploy that calls a HashSet factory fails with the code of its qualifier. No
// ///   deploy leaves space data.
/// The pairs that the URN grammar refuses, and the HashSet families with a qualifier other than
/// `seq`, reject before any space exists, at every layer:
///
/// * The URN parser refuses the pairs of [`REFUSED_PAIRS`] with `UnsupportedPair`.
/// * Each HashSet family has a registered factory, a dispatch arm, a send schema and a
///   constructor schema (P7 step 3). Its constructor takes `seq`, and it refuses `default` and
///   `temp` with `BadConstructorArgument`.
/// * The family agent refuses the creation of each refused pair with `FamilyUnavailable`, and it
///   stores nothing. A send on a space whose stored config names the pair fails the same way.
/// * A Rholang deploy that names the factory of a refused pair fails at `new`, before any space
///   work. A deploy that calls a HashSet factory with `default` or `temp` fails with
///   `BadConstructorArgument`. No deploy leaves space data.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_unsupported_pairs_reject() {
    // Changed by P7 step 3 (design v3 HS-7): the HashSet families are registered.
    // let unsupported = unsupported_families();
    // assert_eq!(unsupported.len(), 6, "six HashSet families");
    let hashsets = hashset_families();
    assert_eq!(hashsets.len(), 6, "six HashSet families");
    for (outer, inner) in REFUSED_PAIRS {
        let urn = format!("rho:space:{}:{}", outer.as_str(), inner.as_str());
        assert_eq!(
            SpaceFactoryUrn::parse(&urn),
            Err(SpaceFactoryUrnError::UnsupportedPair {
                outer: outer.as_str(),
                inner: inner.as_str(),
            }),
            "{urn}"
        );
    }
    // Changed by P7 step 3 (design v3 HS-7): each HashSet family is registered and refuses only
    // the qualifiers other than `seq`.
    // for family in &unsupported {
    //     assert_eq!(SpaceFactoryUrn::parse(&family.to_string()), Ok(*family));
    //     assert_eq!(registered_channel(*family), None, "{family}");
    //     assert_eq!(Family::of_urn(*family), None, "{family}");
    //     assert_eq!(send_params(*family), None, "{family}");
    //     assert_eq!(constructor_schema(*family), None, "{family}");
    //     for qualifier in SpaceQualifier::ALL {
    //         let ctor = new_elist_par(
    //             vec![gstring(qualifier.as_str())],
    //             Vec::new(),
    //             false,
    //             None,
    //             Vec::new(),
    //             false,
    //         );
    //         assert_eq!(
    //             FactoryConstructor::validate(*family, &ctor),
    //             Err(unavailable(family.outer, family.inner)),
    //             "{family} {qualifier}"
    //         );
    //     }
    // }
    for family in &hashsets {
        assert_eq!(SpaceFactoryUrn::parse(&family.to_string()), Ok(*family));
        assert!(registered_channel(*family).is_some(), "{family}");
        assert!(Family::of_urn(*family).is_some(), "{family}");
        assert!(send_params(*family).is_some(), "{family}");
        assert!(constructor_schema(*family).is_some(), "{family}");
        for qualifier in SpaceQualifier::ALL {
            let ctor = new_elist_par(
                vec![gstring(qualifier.as_str())],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            );
            // Changed by decision 90 (C6, step D): gate 3 reads no execution model.
            // for execution in [Execution::Native, Execution::Legacy] {
            //     let validated = FactoryConstructor::validate_in(*family, &ctor, execution);
            //     match (qualifier, execution) {
            //         (SpaceQualifier::Seq, Execution::Native) => {
            //             assert!(validated.is_ok(), "{family} {qualifier}: {validated:?}")
            //         }
            //         (SpaceQualifier::Seq, Execution::Legacy) => assert_eq!(
            //             validated,
            //             Err(SpaceError::QualifierUnavailable("seq".to_string())),
            //             "{family}"
            //         ),
            //         (SpaceQualifier::Default | SpaceQualifier::Temp, _) => assert_eq!(
            //             validated,
            //             Err(SpaceError::BadConstructorArgument(format!(
            //                 "the hashset outer store does not take the {qualifier} qualifier"
            //             ))),
            //             "{family} {qualifier} {execution:?}"
            //         ),
            //     }
            // }
            let validated = FactoryConstructor::validate(*family, &ctor);
            match qualifier {
                SpaceQualifier::Seq => {
                    assert!(validated.is_ok(), "{family} {qualifier}: {validated:?}")
                }
                SpaceQualifier::Default | SpaceQualifier::Temp => assert_eq!(
                    validated,
                    Err(SpaceError::BadConstructorArgument(format!(
                        "the hashset outer store does not take the {qualifier} qualifier"
                    ))),
                    "{family} {qualifier}"
                ),
            }
        }
    }

    // The family agent refuses the creation of every refused pair, and a collection operation on
    // a space whose stored config names one.
    // Changed by P7 step 3: the HashSet families have an agent, so only the refused pairs remain.
    // let pairs: Vec<(SpaceOuter, SpaceInner)> = unsupported
    //     .iter()
    //     .map(|family| (family.outer, family.inner))
    //     .chain(REFUSED_PAIRS)
    //     .collect();
    let pairs: Vec<(SpaceOuter, SpaceInner)> = REFUSED_PAIRS.to_vec();
    for (outer, inner) in pairs {
        let family = SpaceFactoryUrn { outer, inner };
        let config = SpaceConfig::new(
            SPACE.sid,
            outer.as_str(),
            inner.as_str(),
            config_qualifier(family),
        )
        .to_par();
        let host = play().await;
        let env = env_over(host.clone(), &budget());
        let created = FamilyAgent
            .prepare_create(&env, creation(SPACE, config.clone(), family))
            .await;
        assert!(
            matches!(&created, Err(error) if *error == unavailable(outer, inner)),
            "{family}: the creation fails closed"
        );
        let config_channel = namespace::gprivate(namespace::config_channel(SPACE));
        assert!(host.get_data(&config_channel).await.is_empty(), "{family}");
        let random = Blake2b512Random::create_from_bytes(b"rrs-n3 stored config");
        host.produce(
            config_channel.clone(),
            record::config_record(config, &random),
            false,
        )
        .await
        .expect("a raw config record is stored");
        host.take_event_log().await;
        let channel = gstring("rrs-n3");
        let op = SpaceOp::Produce(send_on(SPACE, &channel, 43, false));
        assert_eq!(
            run(&FamilyAgent, &env, op).await,
            Err(unavailable(outer, inner)),
            "{family}: a send on a space of the family fails closed"
        );
        assert!(host.take_event_log().await.is_empty(), "{family}");
        assert!(
            host.get_data(&data_channel(SPACE, &channel))
                .await
                .is_empty(),
            "{family}"
        );
    }

    // A Rholang deploy that names the factory of a refused pair fails at `new`.
    let mut session = Session::new().await;
    // Changed by P7 step 3: the HashSet factories are registered, see the loop after this one.
    // let urns: Vec<String> = unsupported
    //     .iter()
    //     .map(SpaceFactoryUrn::to_string)
    //     .chain(
    //         REFUSED_PAIRS
    //             .iter()
    //             .map(|(outer, inner)| format!("rho:space:{}:{}", outer.as_str(), inner.as_str())),
    //     )
    //     .collect();
    let urns: Vec<String> = REFUSED_PAIRS
        .iter()
        .map(|(outer, inner)| format!("rho:space:{}:{}", outer.as_str(), inner.as_str()))
        .collect();
    for urn in urns {
        for qualifier in ["default", "seq"] {
            let source = format!(
                r#"new factory(`{urn}`) in {{ for (space <- factory!?("{qualifier}")) {{ for (a <- space!?("gensym")) {{ for (@42 <- a) {{ @"{GOT}"!(["unsupported", 42]) }} | a!(43) | a!(42) }} }} }}"#
            );
            let deploy = session
                .deploy_with(
                    &source,
                    &format!("rrs-n3 unsupported {urn} {qualifier}"),
                    &Injections::new(),
                )
                .await;
            let errors = format!("{:?}", deploy.result.errors);
            assert!(
                errors.contains(&format!("No value set for {urn}")),
                "{urn} {qualifier}: {errors}"
            );
            assert!(deploy.residue.is_empty(), "{urn} {qualifier}");
        }
    }
    // Changed by decision 90 (C6, step D): the session runs native deploys, where a HashSet
    // factory creates a seq space. The legacy comment follows.
    // // A legacy deploy that calls a HashSet factory fails with the code of its qualifier and
    // // leaves no space data.
    // A deploy that calls a HashSet factory with `default` or `temp` fails with
    // `BadConstructorArgument` and leaves no space data.
    for family in &hashsets {
        for (qualifier, code) in [
            ("default", "BadConstructorArgument"),
            ("temp", "BadConstructorArgument"),
            // Retired by decision 90 (C6, step D): a native deploy creates a seq HashSet space.
            // ("seq", "QualifierUnavailable"),
        ] {
            let source = format!(
                r#"new factory(`{family}`) in {{ for (space <- factory!?("{qualifier}")) {{ for (a <- space!?("gensym")) {{ for ( <- a) {{ @"{GOT}"!(["unsupported", 42]) }} | a!() }} }} }}"#
            );
            session
                .fail_with(
                    &source,
                    &format!("rrs-n3 hashset {family} {qualifier}"),
                    &Injections::new(),
                    &[code],
                )
                .await;
        }
    }
    assert!(session.values(GOT).await.is_empty());
}

// ─── Qualifiers ──────────────────────────────────────────────────────────────────────────────

// Disabled by decision 90 (C6, step D): spaces run only in native execution, so no factory
// refuses `seq`, and the session runs native deploys. Its successors are
// `pattern_enforcement_seq_rows`, which creates a seq space of every wired family, and the
// factory tests of `reified/factory.rs`. Outside native execution, a factory call fails with
// `RuntimeUnavailable` before every gate.
//
// ```
// /// Legacy execution refuses the `seq` qualifier with `QualifierUnavailable`, because its rules
// /// need the job order of native execution (P7, design v3 QT-5). The constructor of every family
// /// with a schema refuses it in legacy execution, before its params: an Array factory without
// /// its required `size` reports the qualifier, not the missing param. Every factory of the rows
// /// of this wave refuses it in a legacy Rholang deploy, which leaves no space data and runs no
// /// body. P7 step 1 makes `temp` available (design v3 QT-1). Its rows of the matrix follow F17.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn pattern_enforcement_qualifiers_fail_closed() {
//     for family in SpaceFactoryUrn::all().filter(|family| constructor_schema(*family).is_some()) {
//         // Changed by P7 step 1 (design v3 QT-1): `temp` creates a space for one deploy.
//         // for qualifier in [SpaceQualifier::Temp, SpaceQualifier::Seq] {
//         // Changed by P7 step 6: a loop over `seq` alone has one element, which clippy rejects
//         // (single_element_loop), so the qualifier is a binding.
//         // for qualifier in [SpaceQualifier::Seq] {
//         {
//             let qualifier = SpaceQualifier::Seq;
//             let ctor = new_elist_par(
//                 vec![gstring(qualifier.as_str())],
//                 Vec::new(),
//                 false,
//                 None,
//                 Vec::new(),
//                 false,
//             );
//             // Changed by P7 step 2 (design v3 QT-5): native execution accepts `seq`, so the
//             // check names the legacy model.
//             // assert_eq!(
//             //     FactoryConstructor::validate(family, &ctor),
//             assert_eq!(
//                 FactoryConstructor::validate_in(family, &ctor, Execution::Legacy),
//                 Err(SpaceError::QualifierUnavailable(
//                     qualifier.as_str().to_string()
//                 )),
//                 "{family} {qualifier}"
//             );
//         }
//     }
//     let mut session = Session::new().await;
//     for row in ROWS {
//         let urn = row.family().to_string();
//         // Changed by P7 step 1 (design v3 QT-1): `temp` creates a space for one deploy.
//         // for qualifier in ["temp", "seq"] {
//         // Changed by P7 step 6: a loop over `seq` alone has one element, which clippy rejects
//         // (single_element_loop), so the qualifier is a binding.
//         // for qualifier in ["seq"] {
//         {
//             let qualifier = "seq";
//             let mut ctors = vec![row.constructor.replace("default", qualifier)];
//             let without_params = format!(r#""{qualifier}""#);
//             if !ctors.contains(&without_params) {
//                 ctors.push(without_params);
//             }
//             for ctor in ctors {
//                 let source = format!(
//                     r#"new factory(`{urn}`) in {{ for (space <- factory!?({ctor})) {{ @"{GOT}"!(["created", 0]) }} }}"#
//                 );
//                 session
//                     .fail_with(
//                         &source,
//                         &format!("rrs-n3 qualifier {urn} {ctor}"),
//                         &Injections::new(),
//                         &["QualifierUnavailable"],
//                     )
//                     .await;
//             }
//         }
//     }
//     assert!(session.values(GOT).await.is_empty());
// }
// ```

// ─── Property ────────────────────────────────────────────────────────────────────────────────

/// One generated case of the matrix: a row, the literal of the pattern, another value, the order,
/// the arrow of the receive and the persistence of the send.
#[derive(Clone, Copy, Debug)]
struct Case {
    /// The family.
    row: Row,
    /// The literal of the pattern.
    pattern: i64,
    /// The value of the send, never the literal.
    other: i64,
    /// True when the send comes before the receive.
    send_first: bool,
    /// The arrow of the receive.
    arrow: Arrow,
    /// True for a persistent send.
    persistent_send: bool,
}

/// A case of any row, with a literal in `0..100` and another value in `0..100`.
fn arb_case() -> impl Strategy<Value = Case> {
    (
        select(ROWS.to_vec()),
        0i64..100,
        1i64..100,
        any::<bool>(),
        prop_oneof![
            Just(Arrow::Plain),
            Just(Arrow::Peek),
            Just(Arrow::Persistent)
        ],
        any::<bool>(),
    )
        .prop_map(
            |(row, pattern, offset, send_first, arrow, persistent_send)| Case {
                row,
                pattern,
                other: (pattern + offset) % 100,
                send_first,
                arrow,
                persistent_send,
            },
        )
}

/// Runs one case in a fresh runtime: the negative deploys on the name `a`, and the positive
/// control on a fresh name.
async fn run_case(case: Case) -> Result<(), TestCaseError> {
    let urn = case.row.family().to_string();
    let mut session = Session::new().await;
    let space = session.setup(&urn, case.row.constructor).await;
    let data = data_channel(space.space, &space.a);
    let send = with_space(&send_source("a", case.other, case.persistent_send));
    let receive = with_space(&receive_source("a", case.pattern, case.arrow, "negative"));
    let deploys = match case.send_first {
        true => [send, receive],
        false => [receive, send],
    };
    for (index, source) in deploys.iter().enumerate() {
        let deploy = session
            .run(source, &format!("rrs-n3 prop {case:?} {index}"))
            .await;
        prop_assert_eq!(comms_on(&deploy.log, &data), 0, "{:?}", case);
    }
    prop_assert!(session.values(GOT).await.is_empty(), "{:?}", case);
    let records = session.records(space.space, &space.a).await;
    prop_assert_eq!(records.len(), 1, "{:?}", case);
    prop_assert_eq!(&records[0].1, &vec![gint(case.other)]);
    prop_assert_eq!(records[0].2, case.persistent_send);
    let parked = session.parked(space.space, &space.a).await;
    prop_assert_eq!(parked.len(), 1, "{:?}", case);
    prop_assert_eq!(parked[0].1, case.arrow == Arrow::Persistent);
    prop_assert_eq!(parked[0].2, case.arrow == Arrow::Peek);
    session
        .run(
            &with_space(&positive_source(case.pattern)),
            &format!("rrs-n3 prop {case:?} positive"),
        )
        .await;
    let outputs = session.values(GOT).await;
    prop_assert_eq!(outputs.len(), 1, "{:?}", case);
    let output = list_elements(&outputs[0][0]);
    prop_assert_eq!(output, vec![gstring("positive"), gint(case.pattern)]);
    Ok(())
}

/// `PatternEnforcement` (V2 of `GenericRSpace.tla`) and `wildcard_matcher_degenerate`
/// (`Match.v`), end to end through Rholang: for generated rows, literals, other values, orders,
/// receive arrows and send arrows, a literal pattern never receives another value, both entries
/// stay with their persistence and peek flags, and the literal receives its own value.
#[test]
fn prop_pattern_enforcement_any_wired_family() {
    check(runtime_config(), arb_case(), |case| {
        tokio_runtime().block_on(run_case(case))
    });
}

// ─── The PathMap rows (P5e) ──────────────────────────────────────────────────────────────────

// Phase 2 of the PathMap engine adds the six rows of `pathmap:<inner>`. A name of a PathMap space
// is the path of one element, so its records lie on the data channel of that path, and its
// waiters and markers on the index channel of that prefix. The token of the space orders every
// operation of the space, so the orders of a counted discipline run across the names.

/// The PathMap rows: the six disciplines of `pathmap:<inner>`, which Phase 2 of the PathMap
/// engine wires (wave W3).
const PATHMAP_ROWS: [Row; 6] = [
    Row {
        outer: SpaceOuter::PathMap,
        inner: SpaceInner::Bag,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::PathMap,
        inner: SpaceInner::Queue,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::PathMap,
        inner: SpaceInner::Stack,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::PathMap,
        inner: SpaceInner::Set,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::PathMap,
        inner: SpaceInner::Cell,
        constructor: DEFAULT,
    },
    Row {
        outer: SpaceOuter::PathMap,
        inner: SpaceInner::PriorityQueue,
        constructor: DEFAULT,
    },
];

/// The channels of the name `name` in the PathMap space `space`. A name is the path of one
/// element (`@c ≡ @[c]`), so its records lie on `D(sid, path_key([name]))`, and its waiters and
/// markers on `I(sid, prefix_key([name]))`.
fn path_channels(space: NativeSpace, name: &Par) -> (Par, Par) {
    let path = std::slice::from_ref(name);
    let key = rholang::rust::interpreter::spaces::pathmap::path_key(path)
        .expect("the path key of a name");
    let encoded =
        rholang::rust::interpreter::spaces::pathmap::encode_path(path).expect("the path of a name");
    let prefix = encoded.prefix_key(1).expect("the prefix key of a name");
    (
        rholang::rust::interpreter::spaces::outer::pathmap::PathMapOuter::path_data(space, &key),
        rholang::rust::interpreter::spaces::outer::pathmap::PathMapOuter::index(space, prefix),
    )
}

/// The records on the data channel `data` of a path: `(ord, payload, persistent)`, by `ord`.
async fn path_records(session: &Session, data: &Par) -> Vec<(i64, Vec<Par>, bool)> {
    let mut records: Vec<(i64, Vec<Par>, bool)> = session
        .data(data)
        .await
        .into_iter()
        .map(|datum| {
            let decoded = record::unwrap_pq_datum(&datum.a).expect("a datum record decodes");
            (decoded.ord, decoded.datum.pars, datum.persist)
        })
        .collect();
    records.sort_by_key(|(ord, _, _)| *ord);
    records
}

/// The single-bind waiters on the index channel `index` of a prefix: `(ord, persistent,
/// peek)`, by `ord`.
async fn path_parked(session: &Session, index: &Par) -> Vec<(i64, bool, bool)> {
    let mut parked: Vec<(i64, bool, bool)> = session
        .waiters(index)
        .await
        .into_iter()
        .map(|waiter| {
            let ord = record::continuation_ord(&waiter.continuation)
                .expect("a parked continuation decodes");
            (ord, waiter.persist, !waiter.peeks.is_empty())
        })
        .collect();
    parked.sort_by_key(|(ord, _, _)| *ord);
    parked
}

/// The PathMap row of `row`: a pattern for 42 never receives 43, in both orders, and a pattern
/// for 42 receives 42. The record 43 stays on the data channel of its name, the waiter for 42
/// stays on the index channel of the name, and no COMM touches these channels or the root index
/// channel `I(ε)`. The token `S(sid)` orders every operation of the space (decision 31), so the
/// orders of a counted discipline run across the two names: 0 and 1 on `a`, then 2 and 3 on `b`.
/// A bag and a set have the order 0.
async fn check_pathmap_row(row: Row) {
    let urn = row.family().to_string();
    let mut session = Session::new().await;
    let space = session.setup(&urn, row.constructor).await;
    let root =
        rholang::rust::interpreter::spaces::outer::pathmap::PathMapOuter::index(space.space, &[]);
    for (name, par, send_first, (send_ord, receive_ord)) in [
        ("a", &space.a, true, (0, 1)),
        ("b", &space.b, false, (3, 2)),
    ] {
        let (data, index) = path_channels(space.space, par);
        let [first, second] = negative_deploys(name, send_first, &urn);
        for (position, source) in [first, second].iter().enumerate() {
            let deploy = session
                .run(source, &format!("rrs-n3 {urn} {name} {position}"))
                .await;
            for channel in [&data, &index, &root] {
                assert_eq!(
                    comms_on(&deploy.log, channel),
                    0,
                    "{urn}: no COMM touches the channels of {name}"
                );
            }
        }
        assert!(
            session.values(GOT).await.is_empty(),
            "{urn}: a pattern for 42 never receives 43 ({name})"
        );
        let (send_ord, receive_ord) = match row.counted() {
            true => (send_ord, receive_ord),
            false => (0, 0),
        };
        assert_eq!(
            path_records(&session, &data).await,
            vec![(send_ord, vec![gint(43)], false)],
            "{urn}: the record 43 stays on the path of {name}"
        );
        assert_eq!(
            path_parked(&session, &index).await,
            vec![(receive_ord, false, false)],
            "{urn}: the waiter for 42 stays on the prefix of {name}"
        );
    }
    session
        .run(
            &with_space(&positive_source(42)),
            &format!("rrs-n3 {urn} positive"),
        )
        .await;
    assert_eq!(
        session.values(GOT).await,
        vec![got("positive", 42)],
        "{urn}: a pattern for 42 receives 42"
    );
}

/// `pathmap:bag`: a pattern for 42 never receives 43 on a name of the space, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_pathmap_bag() { check_pathmap_row(PATHMAP_ROWS[0]).await; }

/// `pathmap:queue`: a pattern for 42 never receives 43 on a name of the space, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_pathmap_queue() { check_pathmap_row(PATHMAP_ROWS[1]).await; }

/// `pathmap:stack`: a pattern for 42 never receives 43 on a name of the space, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_pathmap_stack() { check_pathmap_row(PATHMAP_ROWS[2]).await; }

/// `pathmap:set`: a pattern for 42 never receives 43 on a name of the space, in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_pathmap_set() { check_pathmap_row(PATHMAP_ROWS[3]).await; }

/// `pathmap:cell`: a pattern for 42 never receives 43 on a name of the space, in both orders. The
/// path of the name then holds the record 43, and its prefix the waiter for 42.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_pathmap_cell() { check_pathmap_row(PATHMAP_ROWS[4]).await; }

/// `pathmap:priorityqueue`: a pattern for 42 never receives 43 on a name of the space, in both
/// orders, with sends and receives without a priority.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_pathmap_priorityqueue() { check_pathmap_row(PATHMAP_ROWS[5]).await; }

// ─── P7: the native rows of `temp`, `seq` and HashSet (design v3 section 10, A7) ─────────────

// A `temp` or `seq` space lives for one deploy, so each row runs in one native deploy. A row
// reads its result from the event log and from the values on the public channel `OUT` of the
// native harness. Each deploy certifies through its committed evidence, and the sweep leaves no
// volatile channel. The rows of `seq` make their uses of each channel in one causal chain
// (SQ-3): a handle `produce`, then a receive.

/// The phlo limit of the native rows, as in the Seq suite.
const NATIVE_LIMIT: u64 = 20 * AMPLE_LIMIT;

/// Every wired row of the matrix: the rows of the HashMap, Array and Vector stores, then the
/// PathMap rows.
fn wired_rows() -> Vec<Row> { ROWS.into_iter().chain(PATHMAP_ROWS).collect() }

/// The constructor of `row` with the qualifier `qualifier`.
fn qualified(row: Row, qualifier: &str) -> String { row.constructor.replace("default", qualifier) }

/// The channels that the operations on the name `name` of `space` store on: the data channel of
/// its collection, and for a PathMap space also the index channel of its prefix, where its
/// waiters park.
fn stored_channels(row: Row, space: NativeSpace, name: &Par) -> Vec<Par> {
    match row.outer {
        SpaceOuter::PathMap => {
            let (data, index) = path_channels(space, name);
            vec![data, index]
        }
        _ => vec![data_channel(space, name)],
    }
}

/// The produce, consume and COMM events of `log` on the channels with the hashes `hashes`. A
/// COMM counts when its consume or one of its produces is on such a channel.
fn events_on(log: &[Event], hashes: &[Blake2b256Hash]) -> (usize, usize, usize) {
    let on = |key: &Blake2b256Hash| hashes.contains(key);
    log.iter().fold(
        (0, 0, 0),
        |(produces, consumes, comms), event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) if on(&produce.channel_hash) => {
                (produces + 1, consumes, comms)
            }
            Event::IoEvent(IOEvent::Consume(consume)) if consume.channel_hashes.iter().any(on) => {
                (produces, consumes + 1, comms)
            }
            Event::Comm(comm)
                if comm.consume.channel_hashes.iter().any(on)
                    || comm
                        .produces
                        .iter()
                        .any(|produce| on(&produce.channel_hash)) =>
            {
                (produces, consumes, comms + 1)
            }
            Event::IoEvent(_) | Event::Comm(_) => (produces, consumes, comms),
        },
    )
}

/// The values on `OUT` of `played` whose first element is the string `label`: the other
/// elements of each list.
fn labelled(played: &Played, label: &str) -> Vec<Vec<Par>> {
    played
        .out
        .iter()
        .filter_map(|pars| match pars.as_slice() {
            [value] => {
                let elements = list_elements(value);
                match elements.split_first() {
                    Some((head, rest)) if *head == gstring(label) => Some(rest.to_vec()),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// Plays `term` natively, checks that the sweep leaves no volatile channel, and certifies it.
async fn play_native(label: &str, term: &str) -> Played {
    let played = native_play(term, NATIVE_LIMIT).await;
    assert!(
        played.residue.is_empty(),
        "{label}: the sweep leaves no volatile channel: {:?}",
        played.residue
    );
    let certification = certify(&played, term, NATIVE_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

/// The deploy of a `temp` row of `urn` with the constructor `ctor`. On `a` the send comes first
/// in the source, and on `b` the receive. A pattern for 42 never receives 43 on either name, and
/// a pattern for 42 receives 42 on `c`. The handle and the names `a` and `b` go to `OUT`.
fn temp_row_program(urn: &str, ctor: &str) -> String {
    format!(
        r#"new factory(`{urn}`) in {{
  for (space <- factory!?({ctor})) {{
    for (a <- space!?("gensym"); b <- space!?("gensym"); c <- space!?("gensym")) {{
      @"{OUT}"!(["stash", *space, *a, *b]) |
      a!(43) | for (@42 <- a) {{ @"{OUT}"!(["negative", 42]) }} |
      for (@42 <- b) {{ @"{OUT}"!(["negative", 42]) }} | b!(43) |
      for (@42 <- c) {{ @"{OUT}"!(["positive", 42]) }} | c!(42)
    }}
  }}
}}"#
    )
}

/// The `temp` rows of N3 (F17 of decision 39): on every wired family, a pattern for 42 never
/// receives 43 in a `temp` space, in both source orders, and a pattern for 42 receives 42. The
/// log shows that the record 43 and the waiter for 42 were stored on the channels of their name,
/// and that no COMM touched these channels.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_temp_rows() {
    for row in wired_rows() {
        let urn = row.family().to_string();
        let label = format!("temp {urn}");
        let played = play_native(&label, &temp_row_program(&urn, &qualified(row, "temp"))).await;
        assert!(
            played.result.errors.is_empty(),
            "{label}: {:?}",
            played.result.errors
        );
        assert!(
            labelled(&played, "negative").is_empty(),
            "{label}: a pattern for 42 never receives 43"
        );
        assert_eq!(
            labelled(&played, "positive"),
            vec![vec![gint(42)]],
            "{label}: a pattern for 42 receives 42"
        );
        let stash = labelled(&played, "stash");
        let [entry] = stash.as_slice() else {
            panic!("{label}: one stash entry: {stash:?}");
        };
        let [handle, a, b] = entry.as_slice() else {
            panic!("{label}: the stash holds the handle and two names");
        };
        let space = support::session::native(handle);
        assert_eq!(space.durability, Durability::Ephemeral, "{label}");
        for (name, par) in [("a", a), ("b", b)] {
            let hashes: Vec<Blake2b256Hash> =
                stored_channels(row, space, par).iter().map(hash).collect();
            let (produces, consumes, comms) = events_on(&played.log, &hashes);
            assert_eq!(
                comms, 0,
                "{label}: no COMM touches the collection of {name}"
            );
            assert!(
                produces >= 1 && consumes >= 1,
                "{label}: the record 43 and the waiter for 42 are stored on {name} \
                 ({produces} produces, {consumes} consumes)"
            );
        }
    }
}

/// The positive part of a `seq` row: a handle `produce` of 42 on `c`, then a receive for 42.
fn seq_positive() -> String {
    format!(
        r#"{{ space!?("produce", *c, [42]); for (@42 <- c) {{ @"{OUT}"!(["positive", 42]) }} }}"#
    )
}

/// The negative part of a `seq` row: a handle `produce` of 43 on `a`, then a receive for 42.
fn seq_negative() -> String {
    format!(
        r#"{{ space!?("produce", *a, [43]); for (@42 <- a) {{ @"{OUT}"!(["negative", 42]) }} }}"#
    )
}

/// The deploy of a `seq` row of `urn` with the constructor `ctor`, which runs `body` over the
/// names `a` and `c`. The handle goes to `OUT`, because the send rule exempts the handle of a seq
/// space (SQ-2).
fn seq_row_program(urn: &str, ctor: &str, body: &str) -> String {
    format!(
        r#"new factory(`{urn}`) in {{
  for (space <- factory!?({ctor})) {{
    @"{OUT}"!(["handle", *space]) |
    for (a <- space!?("gensym"); c <- space!?("gensym")) {{ {body} }}
  }}
}}"#
    )
}

/// The produce, consume and COMM events of a `seq` row on the data and index channels of its
/// space, which the row reads from its handle.
fn seq_row_events(label: &str, played: &Played) -> (usize, usize, usize) {
    let handles = labelled(played, "handle");
    let [entry] = handles.as_slice() else {
        panic!("{label}: one handle: {handles:?}");
    };
    let space = support::session::native(entry.first().expect("the handle"));
    assert_eq!(space.durability, Durability::Ephemeral, "{label}");
    events_on(&played.log, &stored_hashes_of(played, space))
}

/// The hashes of the data and index channels of `space` that the space rows of `played` lock.
fn stored_hashes_of(played: &Played, space: NativeSpace) -> Vec<Blake2b256Hash> {
    // Changed by decision 92: a space row names the counter channel of its space, so
    // `is_space_row` reads only the row, not the recording.
    // let recording = played
    //     .result
    //     .native_budget_recording
    //     .as_ref()
    //     .expect("native play records its budget");
    let journal = played
        .result
        .native_operation_recording
        .as_deref()
        .expect("native play records its operations");
    let mut hashes: Vec<Blake2b256Hash> = journal
        .iter()
        .filter(|row| is_space_row(row))
        .flat_map(footprint_channels)
        .filter(|channel| {
            matches!(
                namespace::reified_id_of(channel),
                Some(ReifiedId::Data(of, _) | ReifiedId::PathIndex(of, _)) if of == space
            )
        })
        .map(|channel| hash(&channel))
        .collect();
    hashes.sort_by_key(|key| key.bytes());
    hashes.dedup();
    hashes
}

/// The `seq` rows of N3 (F17 of decision 39): on every wired family, a pattern for 42 never
/// receives 43 in a `seq` space, and a pattern for 42 receives 42. The names of a seq space are
/// never sent (SQ-1), so the row reads the collections of its space from the journal. The row
/// with its negative part makes as many COMMs on them as the row without it, so the negative
/// part fires nothing. A PathMap fire makes three COMMs (the delivery through the marker and the
/// retire COMMs of the record and of the root marker, F4), so the row compares counts with this
/// baseline and does not fix them. The negative part stores its record and its waiter.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_seq_rows() {
    for row in wired_rows() {
        let urn = row.family().to_string();
        let label = format!("seq {urn}");
        let ctor = qualified(row, "seq");
        let baseline = play_native(
            &format!("{label} positive part"),
            &seq_row_program(&urn, &ctor, &seq_positive()),
        )
        .await;
        let (base_produces, base_consumes, base_comms) =
            seq_row_events(&format!("{label} positive part"), &baseline);
        assert!(base_comms >= 1, "{label}: the fire of 42 makes a COMM");
        let played = play_native(
            &label,
            &seq_row_program(
                &urn,
                &ctor,
                &format!("{} | {}", seq_negative(), seq_positive()),
            ),
        )
        .await;
        assert!(
            played.result.errors.is_empty(),
            "{label}: {:?}",
            played.result.errors
        );
        assert!(
            labelled(&played, "negative").is_empty(),
            "{label}: a pattern for 42 never receives 43"
        );
        assert_eq!(
            labelled(&played, "positive"),
            vec![vec![gint(42)]],
            "{label}: a pattern for 42 receives 42"
        );
        let (produces, consumes, comms) = seq_row_events(&label, &played);
        assert_eq!(
            comms, base_comms,
            "{label}: the negative part makes no COMM on the collections of the space"
        );
        assert!(
            produces > base_produces && consumes > base_consumes,
            "{label}: the record 43 and the waiter for 42 are stored \
             ({produces} produces and {consumes} consumes, \
             {base_produces} and {base_consumes} without the negative part)"
        );
    }
}

/// The inner collections of the six HashSet families.
const HASHSET_INNERS: [&str; 6] = ["bag", "queue", "stack", "set", "cell", "priorityqueue"];

/// A deploy that creates a `seq` space of the family `hashset:inner`, draws its name `x`, and
/// runs `body`.
fn hashset_row_program(inner: &str, body: &str) -> String {
    format!(
        r#"new factory(`rho:space:hashset:{inner}`) in {{
  for (space <- factory!?("seq")) {{
    for (x <- space!?("gensym")) {{ {body} }}
  }}
}}"#
    )
}

/// The HashSet rows of N3 (design v3 HS-2, HS-3): a HashSet collection holds units only. On
/// each family, a unit that the handle adds is taken by an empty receive, a send of 43 aborts with
/// `PayloadNotEmpty`, and a receive for 42 aborts with `PatternNotEmpty`, so no pattern of a
/// value ever meets a stored value.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_hashset_rows() {
    for inner in HASHSET_INNERS {
        let label = format!("hashset:{inner} unit");
        let played = play_native(
            &label,
            &hashset_row_program(
                inner,
                &format!(
                    r#"space!?("produce", *x, []); for ( <- x) {{ @"{OUT}"!(["positive", 0]) }}"#
                ),
            ),
        )
        .await;
        assert!(
            played.result.errors.is_empty(),
            "{label}: {:?}",
            played.result.errors
        );
        assert_eq!(
            labelled(&played, "positive"),
            vec![vec![gint(0)]],
            "{label}"
        );
        for (case, body, code) in [
            ("a payload", "x!(43)".to_string(), "PayloadNotEmpty"),
            (
                "a pattern",
                format!(r#"for (@42 <- x) {{ @"{OUT}"!(["negative", 42]) }}"#),
                "PatternNotEmpty",
            ),
        ] {
            let label = format!("hashset:{inner} {case}");
            let played = play_native(&label, &hashset_row_program(inner, &body)).await;
            assert_eq!(
                error_codes(&played.result.errors),
                vec![code.to_string()],
                "{label}"
            );
            assert!(labelled(&played, "negative").is_empty(), "{label}");
        }
    }
}

/// The unsupported pairs of P7 in native execution (design v3 QT-1 and QT-4): a HashSet family
/// with `default` or `temp` fails with `BadConstructorArgument`, and a VectorDB family with
/// `temp` or `seq` and a mirror fails with `InvalidParam`. Each deploy fails before any space
/// exists, so it leaves no space channel and runs no body.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_native_unsupported_pairs_reject() {
    let mut pairs: Vec<(String, String, &str)> = Vec::new();
    for inner in HASHSET_INNERS {
        for qualifier in ["default", "temp"] {
            pairs.push((
                format!("rho:space:hashset:{inner}"),
                format!(r#""{qualifier}""#),
                "BadConstructorArgument",
            ));
        }
    }
    for urn in ["rho:space:hashmap:vectordb", "rho:space:vector:vectordb"] {
        for qualifier in ["temp", "seq"] {
            pairs.push((
                urn.to_string(),
                format!(r#""{qualifier}"; capacity=4, dimensions=2, mirror="chroma""#),
                "InvalidParam",
            ));
        }
    }
    for (urn, ctor, code) in pairs {
        let label = format!("{urn} {ctor}");
        let played = play_native(
            &label,
            &format!(
                r#"new factory(`{urn}`) in {{
  for (space <- factory!?({ctor})) {{ @"{OUT}"!(["created", 0]) }}
}}"#
            ),
        )
        .await;
        assert_eq!(
            error_codes(&played.result.errors),
            vec![code.to_string()],
            "{label}"
        );
        assert!(labelled(&played, "created").is_empty(), "{label}");
        assert!(
            played.durable.is_empty(),
            "{label}: no space channel stays: {:?}",
            played.durable
        );
    }
}
