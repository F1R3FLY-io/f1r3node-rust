//! The pattern-enforcement matrix N3 of the native spaces (tracker leaf
//! `rrs-tests-verification-n3`, P5 design v2 sections 2.8.4 and 3.3): every pair of a space family
//! and a qualifier either runs with real matching, or fails closed.
//!
//! **Supported pairs.** A family that the family agent wires runs with the `default` qualifier.
//! For each of these families, a pattern for 42 never receives 43, in both orders: a send of 43
//! before a receive for 42, and a receive for 42 before a send of 43. Both entries stay in the
//! collection with their orders, and no COMM touches the collection. A receive for 42 on a fresh
//! name then takes a send of 42, so the matcher is not vacuous. The rows of this wave are the
//! families that the family agent wires at the base of wave W2: `hashmap:bag`, `hashmap:queue`,
//! `hashmap:stack`, `hashmap:cell`, `array:queue` and `vector:queue`. The set and priority-queue
//! rows follow in wave W3.
//!
//! **Unsupported pairs.** These families reject before any space exists:
//!
//! * The six HashSet families (P7, F1), which take only the `seq` qualifier.
//! * The VectorDB families `hashmap:vectordb` and `vector:vectordb` (P6).
//! * The pairs that the URN grammar refuses: `pathmap:vectordb`, `array:vectordb` and
//!   `hashset:vectordb`.
//!
//! No factory of them is registered, so a Rholang deploy that names one fails at `new` and leaves
//! no space data. A creation or a stored config that names one fails in the family agent with
//! `FamilyUnavailable`.
//!
//! **Qualifiers.** The `temp` and `seq` qualifiers fail closed with `QualifierUnavailable` until
//! P7 (F17 of decision 39), on every factory of this wave, before the gates of the params.
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
use rholang::rust::interpreter::spaces::agent::{SpaceAgent, SpaceOp};
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::factory::constructor::FactoryConstructor;
use rholang::rust::interpreter::spaces::factory::defaults::constructor_schema;
use rholang::rust::interpreter::spaces::factory::urn::registered_channel;
use rholang::rust::interpreter::spaces::families::{send_params, Family, FamilyAgent};
use rholang::rust::interpreter::spaces::namespace::{self, Durability, NativeSpace, SID_LEN};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier as ConfigQualifier;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use support::agent::{budget, creation, env_over, play, run, send_on};
use support::props::{check, runtime_config, tokio_runtime};
use support::session::{comms_on, data_channel, with_space, Injections, Session};

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
    /// collection have the orders of their operations. Bag records and waiters have the order 0.
    fn counted(self) -> bool { self.inner != SpaceInner::Bag }
}

/// The rows of this wave.
const ROWS: [Row; 6] = [
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
];

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

// ─── The unsupported pairs ───────────────────────────────────────────────────────────────────

/// The unsupported families of this phase: the HashSet families (P7) and the VectorDB families
/// (P6). Their URNs parse, but no factory is registered.
fn unsupported_families() -> Vec<SpaceFactoryUrn> {
    SpaceFactoryUrn::all()
        .filter(|family| {
            family.outer == SpaceOuter::HashSet || family.inner == SpaceInner::VectorDb
        })
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
    }
}

/// The error of an unavailable family.
fn unavailable(outer: SpaceOuter, inner: SpaceInner) -> SpaceError {
    SpaceError::FamilyUnavailable {
        outer: outer.as_str().to_string(),
        inner: inner.as_str().to_string(),
    }
}

/// The HashSet families, VectorDB and the pairs that the URN grammar refuses reject before any
/// space exists, at every layer:
///
/// * The URN parser refuses the pairs of [`REFUSED_PAIRS`] with `UnsupportedPair`.
/// * No unsupported family has a registered factory, a dispatch arm, a send schema or a
///   constructor schema. So its constructor fails with `FamilyUnavailable` for every qualifier.
/// * The family agent refuses the creation of each pair with `FamilyUnavailable`, and it stores
///   nothing. A send on a space whose stored config names the pair fails the same way.
/// * A Rholang deploy that names the factory of the pair fails at `new`, before any space work,
///   and leaves no space data.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_unsupported_pairs_reject() {
    let unsupported = unsupported_families();
    assert_eq!(
        unsupported.len(),
        8,
        "six HashSet families and two VectorDB families"
    );
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
    for family in &unsupported {
        assert_eq!(SpaceFactoryUrn::parse(&family.to_string()), Ok(*family));
        assert_eq!(registered_channel(*family), None, "{family}");
        assert_eq!(Family::of_urn(*family), None, "{family}");
        assert_eq!(send_params(*family), None, "{family}");
        assert_eq!(constructor_schema(*family), None, "{family}");
        for qualifier in SpaceQualifier::ALL {
            let ctor = new_elist_par(
                vec![gstring(qualifier.as_str())],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            );
            assert_eq!(
                FactoryConstructor::validate(*family, &ctor),
                Err(unavailable(family.outer, family.inner)),
                "{family} {qualifier}"
            );
        }
    }

    // The family agent refuses the creation of every unsupported pair, and a collection
    // operation on a space whose stored config names one.
    let pairs: Vec<(SpaceOuter, SpaceInner)> = unsupported
        .iter()
        .map(|family| (family.outer, family.inner))
        .chain(REFUSED_PAIRS)
        .collect();
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

    // A Rholang deploy that names an unsupported factory fails at `new`.
    let mut session = Session::new().await;
    let urns: Vec<String> = unsupported
        .iter()
        .map(SpaceFactoryUrn::to_string)
        .chain(
            REFUSED_PAIRS
                .iter()
                .map(|(outer, inner)| format!("rho:space:{}:{}", outer.as_str(), inner.as_str())),
        )
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
    assert!(session.values(GOT).await.is_empty());
}

// ─── Qualifiers ──────────────────────────────────────────────────────────────────────────────

/// The `temp` and `seq` qualifiers fail closed with `QualifierUnavailable` until P7 (F17). The
/// constructor of every family with a schema refuses them, before its params: an Array factory
/// without its required `size` reports the qualifier, not the missing param. Every factory of
/// the rows of this wave refuses them in a Rholang deploy, which leaves no space data and runs
/// no body.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pattern_enforcement_qualifiers_fail_closed() {
    for family in SpaceFactoryUrn::all().filter(|family| constructor_schema(*family).is_some()) {
        for qualifier in [SpaceQualifier::Temp, SpaceQualifier::Seq] {
            let ctor = new_elist_par(
                vec![gstring(qualifier.as_str())],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            );
            assert_eq!(
                FactoryConstructor::validate(family, &ctor),
                Err(SpaceError::QualifierUnavailable(
                    qualifier.as_str().to_string()
                )),
                "{family} {qualifier}"
            );
        }
    }
    let mut session = Session::new().await;
    for row in ROWS {
        let urn = row.family().to_string();
        for qualifier in ["temp", "seq"] {
            let mut ctors = vec![row.constructor.replace("default", qualifier)];
            let without_params = format!(r#""{qualifier}""#);
            if !ctors.contains(&without_params) {
                ctors.push(without_params);
            }
            for ctor in ctors {
                let source = format!(
                    r#"new factory(`{urn}`) in {{ for (space <- factory!?({ctor})) {{ @"{GOT}"!(["created", 0]) }} }}"#
                );
                session
                    .fail_with(
                        &source,
                        &format!("rrs-n3 qualifier {urn} {ctor}"),
                        &Injections::new(),
                        &["QualifierUnavailable"],
                    )
                    .await;
            }
        }
    }
    assert!(session.values(GOT).await.is_empty());
}

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
