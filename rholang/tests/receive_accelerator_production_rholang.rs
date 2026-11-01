use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, Expr, ListParWithRandom, Match, MatchCase, Par, ParWithRandom, Receive,
    ReceiveBind, TaggedContinuation,
};
use models::rust::utils::new_gint_par;
use rholang::rust::interpreter::accounting;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::substitute::Substitute;
use rspace_plus_plus::rspace::hot_store::HotStore;
use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::r#match::Match as TupleSpaceMatch;
use rspace_plus_plus::rspace::receive_accelerator::{
    ReceiveAcceleratorClass, ReceiveAcceleratorHandle, ReceiveAnswer,
    ReceiveKind as ProductionKind, ReceiveOccurrences, ReceiveTarget as ProductionTarget,
};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{ISpace, RSpaceResult};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

#[path = "../../rspace++/tests/support/receive_occurrence_reference.rs"]
#[allow(dead_code)]
#[rustfmt::skip]
mod reference;

use reference::{ChannelImage, GroupKey, Observation, Occurrence, ReceiveKind, ReceiveTarget};

type RhoSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
type RhoStore = Arc<Box<dyn HotStore<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>;
type RhoMatcher = Arc<Box<dyn TupleSpaceMatch<BindPattern, ListParWithRandom, TaggedContinuation>>>;
type RhoWaitingContinuation = WaitingContinuation<BindPattern, TaggedContinuation>;
type RhoBinding = RSpaceResult<Par, ListParWithRandom>;
type RhoHandle = ReceiveAcceleratorHandle<Par, BindPattern, TaggedContinuation>;
type RhoAnswer = ReceiveAnswer<Par, BindPattern, TaggedContinuation>;
type RhoOccurrences = ReceiveOccurrences<Par, BindPattern, TaggedContinuation>;
type RhoTarget = ProductionTarget<Par, BindPattern, TaggedContinuation>;

const RESOLUTION_BUDGET: i64 = i64::MAX / 4;

const JOIN_A_PROCESS: &str = r#""join-a""#;
const JOIN_B_PROCESS: &str = r#""join-b""#;
const ABSENT_PROCESS: &str = r#""absent""#;
const REPEATED_JOIN_SOURCE: &str = concat!(
    r#"match ["join-a", "join-b", "join-a"] { "#,
    r#"[p, q, r] => for (@x <- @p & @y <- @q & @z <- @r) { Nil } }"#
);
const ASYMMETRIC_PEEK_SOURCE: &str = concat!(
    r#"match ["join-a", "join-b", "join-a"] { "#,
    r#"[p, q, r] => for (@1 <<- @p & @x <- @q & @2 <- @r) { Nil } }"#
);
const ASYMMETRIC_PERSISTENT_SOURCE: &str = concat!(
    r#"match ["join-a", "join-b", "join-a"] { "#,
    r#"[p, q, r] => for (@1 <= @p & @x <- @q & @2 <- @r) { Nil } }"#
);
const INSTALLED_TWIN_SOURCE: &str = r#"for (@value <= @"installed-twin") { Nil }"#;
const LIFECYCLE_SINGLE_SOURCE: &str = r#"for (@value <- @"lifecycle-single") { Nil }"#;
const LIFECYCLE_LATER_SOURCE: &str = r#"for (@value <- @"lifecycle-later") { Nil }"#;
const DIRECTION_LINEAR_SOURCE: &str = r#"for (@value <- @"direction-linear") { Nil }"#;
const DIRECTION_DATA_FIRST_SOURCE: &str = r#"for (@value <- @"direction-data-first") { Nil }"#;
const DIRECTION_PERSISTENT_SOURCE: &str = r#"for (@value <= @"direction-persistent") { Nil }"#;

const ASYMMETRIC_FREE_COUNTS: [i32; 3] = [0, 1, 0];

struct RestingReceive {
    label: &'static str,
    channels: Vec<Par>,
    patterns: Vec<BindPattern>,
    continuation: TaggedContinuation,
    persistent: bool,
    peeks: BTreeSet<i32>,
}

impl RestingReceive {
    fn waiting(&self) -> RhoWaitingContinuation {
        WaitingContinuation::create(
            &self.channels,
            &self.patterns,
            &self.continuation,
            self.persistent,
            self.peeks.clone(),
        )
    }

    fn key(&self) -> GroupKey { reference::group_key(&self.channels) }

    fn target(&self, kind: ReceiveKind) -> ReceiveTarget {
        reference::target_of_continuation(kind, &self.key(), &self.waiting())
    }

    fn free_counts(&self) -> Vec<i32> {
        self.patterns
            .iter()
            .map(|pattern| pattern.free_count)
            .collect()
    }

    async fn consume_into(&self, space: &RhoSpace) {
        let consumed = space
            .consume(
                self.channels.clone(),
                self.patterns.clone(),
                self.continuation.clone(),
                self.persistent,
                self.peeks.clone(),
            )
            .await
            .expect("an in-memory consume succeeds");
        let None = consumed else {
            panic!(
                "{}: no matching datum waits on its channels, so the receive must rest",
                self.label
            );
        };
    }
}

fn integer(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn datum(pars: Vec<Par>) -> ListParWithRandom {
    ListParWithRandom {
        pars,
        ..ListParWithRandom::default()
    }
}

fn par_body_continuation(body: Option<Par>, random_state: Vec<u8>) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom { body, random_state })),
        ..TaggedContinuation::default()
    }
}

fn signed_position(index: usize) -> i32 {
    i32::try_from(index).expect("a receive position fits the i32 peek domain")
}

fn compiled_process(label: &str, source: &str) -> Par {
    match Compiler::source_to_adt(source) {
        Ok(process) => process,
        Err(error) => panic!("{label}: the Rholang source must compile: {error:?}"),
    }
}

fn only_receive(label: &str, process: &Par) -> Receive {
    match <[Receive; 1]>::try_from(process.decode_occurrences::<Receive>()) {
        Ok([receive]) => receive,
        Err(receives) => panic!("{label}: expected one receive, found {}", receives.len()),
    }
}

fn only_match_case_body(label: &str, process: &Par) -> Par {
    let matched = match <[Match; 1]>::try_from(process.decode_occurrences::<Match>()) {
        Ok([matched]) => matched,
        Err(matches) => panic!("{label}: expected one match, found {}", matches.len()),
    };
    let case = match <[MatchCase; 1]>::try_from(matched.cases) {
        Ok([case]) => case,
        Err(cases) => panic!("{label}: expected one match case, found {}", cases.len()),
    };
    case.source.expect("a compiled match case carries its body")
}

fn resolve_receive(label: &'static str, receive: Receive, env: &Env<Par>) -> RestingReceive {
    let cost = accounting::costs::Cost::create(RESOLUTION_BUDGET, "receive accelerator resolution");
    let substitution = Substitute {
        metering: MeteredMachine::new(accounting::RuntimeBudget::new(cost)),
    };
    let Receive {
        binds,
        body,
        persistent,
        peek,
        ..
    } = receive;
    let arity = binds.len();
    let mut channels = Vec::with_capacity(arity);
    let mut patterns = Vec::with_capacity(arity);
    for bind in binds {
        let ReceiveBind {
            patterns: bind_patterns,
            source,
            remainder,
            free_count,
            ..
        } = bind;
        let source = source.expect("a compiled receive bind carries its source");
        channels.push(
            substitution
                .substitute_and_charge(source, 0, env)
                .expect("a receive source substitutes at depth zero"),
        );
        let resolved_patterns = bind_patterns
            .into_iter()
            .map(|pattern| {
                substitution
                    .substitute_and_charge(pattern, 1, env)
                    .expect("a receive pattern substitutes at depth one")
            })
            .collect();
        patterns.push(BindPattern {
            patterns: resolved_patterns,
            remainder,
            free_count,
        });
    }
    let peeks: BTreeSet<i32> = match peek {
        true => (0..arity).map(signed_position).collect(),
        false => BTreeSet::new(),
    };
    let continuation = par_body_continuation(body, label.as_bytes().to_vec());
    RestingReceive {
        label,
        channels,
        patterns,
        continuation,
        persistent,
        peeks,
    }
}

fn resolve_compiled(label: &'static str, source: &str) -> RestingReceive {
    resolve_receive(
        label,
        only_receive(label, &compiled_process(label, source)),
        &Env::new(),
    )
}

fn resolve_case(label: &'static str, source: &str, env: &Env<Par>) -> RestingReceive {
    let case_body = only_match_case_body(label, &compiled_process(label, source));
    resolve_receive(label, only_receive(label, &case_body), env)
}

fn repeated_env(join_a: &Par, join_b: &Par) -> Env<Par> {
    Env::new()
        .put(join_a.clone())
        .put(join_b.clone())
        .put(join_a.clone())
}

fn peeking_only(receive: RestingReceive, label: &'static str, position: usize) -> RestingReceive {
    RestingReceive {
        label,
        peeks: BTreeSet::from([signed_position(position)]),
        ..receive
    }
}

fn bind_channel(receive: &RestingReceive, position: usize) -> &Par {
    match receive.channels.get(position) {
        Some(bound) => bound,
        None => panic!("{}: no channel at bind position {position}", receive.label),
    }
}

fn datum_integer(label: &str, bound: &ListParWithRandom) -> i64 {
    let [par] = bound.pars.as_slice() else {
        panic!(
            "{label}: a bound datum carries one process, not {}",
            bound.pars.len()
        );
    };
    let expr = match <[Expr; 1]>::try_from(par.decode_occurrences::<Expr>()) {
        Ok([expr]) => expr,
        Err(exprs) => panic!("{label}: expected one expression, found {}", exprs.len()),
    };
    match expr.expr_instance {
        Some(ExprInstance::GInt(value)) => value,
        other => panic!("{label}: the bound datum is not an integer: {other:?}"),
    }
}

fn assert_position_bindings(
    receive: &RestingReceive,
    bindings: &[RhoBinding],
    channel: &Par,
    expected: &[i64],
    positions: usize,
) {
    let label = receive.label;
    assert_eq!(
        bindings.len(),
        receive.channels.len(),
        "{label}: one binding per receive position"
    );
    let mut bound = Vec::with_capacity(bindings.len());
    for (position, (binding, position_channel)) in
        bindings.iter().zip(&receive.channels).enumerate()
    {
        assert_eq!(
            &binding.channel, position_channel,
            "{label}: binding {position} names the channel of its position"
        );
        if &binding.channel == channel {
            bound.push(datum_integer(label, &binding.removed_datum));
        }
    }
    bound.sort_unstable();
    assert_eq!(
        bound, expected,
        "{label}: the channel's positions consumed exactly its waiting data"
    );
    assert_eq!(
        bound.len(),
        positions,
        "{label}: the COMM consumed one datum per position of the channel in its join"
    );
}

async fn in_memory_space() -> RhoSpace {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager
        .r_space_stores()
        .await
        .expect("in-memory RSpace stores");
    let matcher: RhoMatcher = Arc::new(Box::new(Matcher));
    let (space, _replay) =
        RSpace::create_with_replay(stores, matcher).expect("an in-memory play/replay pair");
    space
}

async fn produce_resting_datum(space: &RhoSpace, target: &Par, value: i64, label: &str) {
    let produced = space
        .produce(target.clone(), datum(vec![integer(value)]), false)
        .await
        .expect("an in-memory produce succeeds");
    let None = produced else {
        panic!("{label}: the datum {value} must wait, because no receive can fire yet");
    };
}

async fn produce_comm(
    space: &RhoSpace,
    target: &Par,
    value: i64,
    fired: &RestingReceive,
) -> Vec<RhoBinding> {
    let produced = space
        .produce(target.clone(), datum(vec![integer(value)]), false)
        .await
        .expect("an in-memory produce succeeds");
    let Some((result, bindings, _)) = produced else {
        panic!("{}: the produce must fire the resting receive", fired.label);
    };
    assert_eq!(
        result.channels, fired.channels,
        "{}: fired channels",
        fired.label
    );
    assert_eq!(
        result.persistent, fired.persistent,
        "{}: fired persistence",
        fired.label
    );
    bindings
}

async fn consume_comm(space: &RhoSpace, receive: &RestingReceive) -> Vec<RhoBinding> {
    let consumed = space
        .consume(
            receive.channels.clone(),
            receive.patterns.clone(),
            receive.continuation.clone(),
            receive.persistent,
            receive.peeks.clone(),
        )
        .await
        .expect("an in-memory consume succeeds");
    let Some((result, bindings)) = consumed else {
        panic!(
            "{}: every datum of the receive waits, so the consume must fire",
            receive.label
        );
    };
    assert_eq!(
        result.channels, receive.channels,
        "{}: fired channels",
        receive.label
    );
    assert_eq!(
        result.persistent, receive.persistent,
        "{}: fired persistence",
        receive.label
    );
    bindings
}

fn hydrated_space(space: &RhoSpace, universe: &[Vec<Par>]) -> Observation {
    reference::observe(&**space.get_store(), universe, true)
}

fn rebuilt_handle(store: &RhoStore) -> RhoHandle {
    assert_eq!(
        store.rebuild_receive_accelerator(),
        ReceiveAcceleratorClass::Coherent
    );
    store
        .receive_accelerator()
        .expect("a rebuilt store publishes its accelerator")
}

fn published_handle(store: &RhoStore) -> RhoHandle {
    store
        .receive_accelerator()
        .expect("the maintained store keeps its accelerator published")
}

fn exact_answer(
    store: &RhoStore,
    handle: &RhoHandle,
    channel: &Par,
    label: &str,
) -> RhoOccurrences {
    store.get_joins(channel);
    match store.receive_occurrences(Some(handle), channel) {
        ReceiveAnswer::Exact(occurrences) => occurrences,
        ReceiveAnswer::Fallback(class) => {
            panic!("{label}: a current handle over a loaded scope answers exactly, not {class:?}")
        }
    }
}

fn fallback_class(answer: RhoAnswer) -> Option<ReceiveAcceleratorClass> {
    match answer {
        ReceiveAnswer::Exact(_) => None,
        ReceiveAnswer::Fallback(class) => Some(class),
    }
}

fn production_kind(kind: ProductionKind) -> ReceiveKind {
    match kind {
        ProductionKind::Installed => ReceiveKind::Installed,
        ProductionKind::Resting => ReceiveKind::Resting,
    }
}

fn production_target(group: &GroupKey, target: &RhoTarget) -> ReceiveTarget {
    ReceiveTarget {
        kind: production_kind(target.kind()),
        group: group.clone(),
        identity: target.identity_bytes().into_owned(),
        source: target.source_bytes().into_owned(),
    }
}

fn production_occurrences(occurrences: &RhoOccurrences, image: &[u8]) -> BTreeSet<Occurrence> {
    let mut mapped = BTreeSet::new();
    for group in occurrences.groups() {
        let key = reference::group_key(group.channels());
        let mut positions = Vec::with_capacity(group.positions().len());
        positions.extend(group.positions().iter().map(|position| position.get()));
        for target in group.targets() {
            let occurrence = Occurrence {
                channel: image.to_vec(),
                group: key.clone(),
                positions: positions.clone(),
                target: production_target(&key, target),
            };
            assert!(
                mapped.insert(occurrence),
                "a production answer names each occurrence once"
            );
        }
    }
    mapped
}

fn answered_occurrences(
    store: &RhoStore,
    handle: &RhoHandle,
    channel: &Par,
    label: &str,
) -> BTreeSet<Occurrence> {
    let image = reference::channel_image(channel);
    production_occurrences(&exact_answer(store, handle, channel, label), &image)
}

fn answered_positions(
    store: &RhoStore,
    handle: &RhoHandle,
    channel: &Par,
    label: &str,
) -> Vec<u64> {
    let answer = exact_answer(store, handle, channel, label);
    let [group] = answer.groups() else {
        panic!(
            "{label}: exactly one group holds the channel, not {}",
            answer.groups().len()
        );
    };
    let mut positions = Vec::with_capacity(group.positions().len());
    positions.extend(group.positions().iter().map(|position| position.get()));
    positions
}

fn assert_capacity_answer(
    store: &RhoStore,
    handle: &RhoHandle,
    observation: &Observation,
    receive: &RestingReceive,
    channel: &Par,
    positions: &[u64],
) {
    let key = receive.key();
    let image = reference::channel_image(channel);
    let expected = Occurrence {
        channel: image.clone(),
        group: key.clone(),
        positions: positions.to_vec(),
        target: receive.target(ReceiveKind::Resting),
    };
    let answered = answered_occurrences(store, handle, channel, receive.label);
    assert_eq!(
        answered,
        reference::derive(observation, &image),
        "{}: the published accelerator answers the derivation",
        receive.label
    );
    assert_eq!(
        answered,
        BTreeSet::from([expected]),
        "{}: the channel has exactly one occurrence in the resting join",
        receive.label
    );
    assert_eq!(
        positions.len(),
        reference::capacity(&key, &image),
        "{}: the occurrence carries one position per unit of join capacity",
        receive.label
    );
}

#[tokio::test(flavor = "current_thread")]
async fn production_compiled_repeated_joins_answer_exact_occurrences() {
    let join_a = compiled_process("join-a name", JOIN_A_PROCESS);
    let join_b = compiled_process("join-b name", JOIN_B_PROCESS);
    let absent = compiled_process("absent name", ABSENT_PROCESS);
    let env = repeated_env(&join_a, &join_b);
    let peek_all = resolve_case("asymmetric-peek", ASYMMETRIC_PEEK_SOURCE, &env);
    let peek_first = peeking_only(
        resolve_case("asymmetric-peek", ASYMMETRIC_PEEK_SOURCE, &env),
        "asymmetric-peek-first",
        0,
    );
    let reseeded = resolve_case("asymmetric-peek-reseeded", ASYMMETRIC_PEEK_SOURCE, &env);
    let persistent = resolve_case("asymmetric-persistent", ASYMMETRIC_PERSISTENT_SOURCE, &env);
    let variants = [&peek_all, &peek_first, &reseeded, &persistent];
    let group = vec![join_a.clone(), join_b.clone(), join_a.clone()];
    for variant in variants {
        assert_eq!(
            variant.channels, group,
            "{}: the compiled join resolves to the palindromic group [a, b, a]",
            variant.label
        );
        assert_eq!(
            variant.free_counts(),
            ASYMMETRIC_FREE_COUNTS,
            "{}: only the middle bind binds a variable",
            variant.label
        );
    }
    assert_eq!(
        peek_first.peeks,
        BTreeSet::from([0]),
        "the metadata-distinct record peeks only at position 0"
    );
    assert!(
        persistent.persistent,
        "a head repeated bind makes the compiled receive persistent"
    );
    assert_ne!(
        reseeded.continuation, peek_all.continuation,
        "reseeding changes the continuation random state"
    );

    let space = in_memory_space().await;
    for variant in variants {
        variant.consume_into(&space).await;
    }
    let store = space.get_store();
    let observation = hydrated_space(&space, std::slice::from_ref(&group));
    let handle = rebuilt_handle(&store);
    let key = reference::group_key(&group);
    let expected: BTreeSet<ReceiveTarget> = variants
        .iter()
        .map(|variant| variant.target(ReceiveKind::Resting))
        .collect();
    assert_eq!(
        expected.len(),
        variants.len(),
        "every compiled or metadata-distinct variant is a distinct receive"
    );
    for (channel, positions) in [(&join_a, vec![0_u64, 2]), (&join_b, vec![1_u64])] {
        let image = reference::channel_image(channel);
        let answered = answered_occurrences(&store, &handle, channel, "repeated join");
        assert_eq!(
            answered,
            reference::derive(&observation, &image),
            "the published accelerator answers the derivation"
        );
        assert_eq!(answered.len(), variants.len(), "one occurrence per receive");
        let targets: BTreeSet<ReceiveTarget> = answered
            .iter()
            .map(|occurrence| occurrence.target.clone())
            .collect();
        assert_eq!(
            targets, expected,
            "the persist and metadata variants answer distinct targets"
        );
        for occurrence in &answered {
            assert_eq!(occurrence.group, key, "the occurrence names its group");
            assert_eq!(
                occurrence.positions, positions,
                "the occurrence lists every position of the channel"
            );
            assert_eq!(
                occurrence.positions.len(),
                reference::capacity(&key, &image),
                "positions and capacity agree"
            );
        }
    }
    assert!(
        answered_occurrences(&store, &handle, &absent, "absent").is_empty(),
        "an absent channel has no occurrences"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn production_compiled_join_capacity_agrees_with_occurrence_multiplicity() {
    let join_a = compiled_process("join-a name", JOIN_A_PROCESS);
    let join_b = compiled_process("join-b name", JOIN_B_PROCESS);
    let join = resolve_case(
        "capacity-join",
        REPEATED_JOIN_SOURCE,
        &repeated_env(&join_a, &join_b),
    );
    assert_eq!(
        join.channels,
        vec![join_a.clone(), join_b.clone(), join_a.clone()],
        "the capacity join resolves to the group [a, b, a]"
    );
    let universe = vec![join.channels.clone()];

    let receive_first = in_memory_space().await;
    join.consume_into(&receive_first).await;
    let receive_first_store = receive_first.get_store();
    let rested = hydrated_space(&receive_first, &universe);
    let held = rebuilt_handle(&receive_first_store);
    assert_capacity_answer(&receive_first_store, &held, &rested, &join, &join_a, &[
        0, 2,
    ]);
    assert_capacity_answer(&receive_first_store, &held, &rested, &join, &join_b, &[1]);
    let a_positions = answered_positions(&receive_first_store, &held, &join_a, join.label);
    let b_positions = answered_positions(&receive_first_store, &held, &join_b, join.label);
    produce_resting_datum(&receive_first, &join_a, 10, join.label).await;
    produce_resting_datum(&receive_first, &join_b, 30, join.label).await;
    assert_eq!(
        hydrated_space(&receive_first, &universe),
        rested,
        "one datum on a cannot fill both a positions, so the join keeps resting"
    );
    assert_capacity_answer(&receive_first_store, &held, &rested, &join, &join_a, &[
        0, 2,
    ]);
    let produced = produce_comm(&receive_first, &join_a, 20, &join).await;
    assert_position_bindings(&join, &produced, &join_a, &[10, 20], a_positions.len());
    assert_position_bindings(&join, &produced, &join_b, &[30], b_positions.len());
    let fired = hydrated_space(&receive_first, &universe);
    let maintained = published_handle(&receive_first_store);
    for channel in [&join_a, &join_b] {
        assert!(
            answered_occurrences(&receive_first_store, &maintained, channel, join.label).is_empty(),
            "the fired join leaves no occurrence"
        );
        assert!(
            reference::derive(&fired, &reference::channel_image(channel)).is_empty(),
            "the fired join leaves no derived occurrence"
        );
        assert!(
            receive_first_store.get_data(channel).is_empty(),
            "the COMM consumed every datum of the join"
        );
    }

    let data_first_short = in_memory_space().await;
    produce_resting_datum(&data_first_short, &join_a, 10, join.label).await;
    produce_resting_datum(&data_first_short, &join_b, 30, join.label).await;
    join.consume_into(&data_first_short).await;
    let data_first_short_store = data_first_short.get_store();
    let waiting = hydrated_space(&data_first_short, &universe);
    let short_handle = rebuilt_handle(&data_first_short_store);
    assert_capacity_answer(
        &data_first_short_store,
        &short_handle,
        &waiting,
        &join,
        &join_a,
        &[0, 2],
    );
    assert_capacity_answer(
        &data_first_short_store,
        &short_handle,
        &waiting,
        &join,
        &join_b,
        &[1],
    );
    assert_eq!(
        data_first_short_store.get_data(&join_a).len(),
        1,
        "the lone datum on a still waits for a second one"
    );

    let data_first = in_memory_space().await;
    for (target, value) in [(&join_a, 10), (&join_a, 20), (&join_b, 30)] {
        produce_resting_datum(&data_first, target, value, join.label).await;
    }
    let data_first_store = data_first.get_store();
    let data_handle = rebuilt_handle(&data_first_store);
    let consumed = consume_comm(&data_first, &join).await;
    assert_position_bindings(&join, &consumed, &join_a, &[10, 20], a_positions.len());
    assert_position_bindings(&join, &consumed, &join_b, &[30], b_positions.len());
    let consumed_world = hydrated_space(&data_first, &universe);
    for channel in [&join_a, &join_b] {
        assert!(
            answered_occurrences(&data_first_store, &data_handle, channel, join.label).is_empty(),
            "a consume-triggered COMM never rests the join"
        );
        assert!(
            reference::derive(&consumed_world, &reference::channel_image(channel)).is_empty(),
            "a consume-triggered COMM never enters the relation"
        );
        assert!(
            data_first_store.get_data(channel).is_empty(),
            "the COMM consumed every datum of the join"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn production_installed_and_resting_receives_answer_distinct_occurrences() {
    let twin = resolve_compiled("installed-twin", INSTALLED_TWIN_SOURCE);
    assert!(
        twin.persistent,
        "every install is persistent, so its resting twin is a repeated receive"
    );
    assert!(twin.peeks.is_empty(), "an install never peeks");
    let absent = compiled_process("absent name", ABSENT_PROCESS);
    let twin_channel = bind_channel(&twin, 0).clone();
    let twin_image = reference::channel_image(&twin_channel);
    let key = twin.key();
    let universe = vec![twin.channels.clone()];
    let space = in_memory_space().await;
    let installed = space
        .install(
            twin.channels.clone(),
            twin.patterns.clone(),
            twin.continuation.clone(),
        )
        .await
        .expect("an in-memory install succeeds");
    let None = installed else {
        panic!("no datum waits on the installed channel, so the install records its continuation");
    };
    let store = space.get_store();
    let installed_world = hydrated_space(&space, &universe);
    let held = rebuilt_handle(&store);
    let installed_target = twin.target(ReceiveKind::Installed);
    let resting_target = twin.target(ReceiveKind::Resting);
    let installed_occurrence = Occurrence {
        channel: twin_image.clone(),
        group: key.clone(),
        positions: vec![0],
        target: installed_target.clone(),
    };
    let installed_only = BTreeSet::from([installed_occurrence.clone()]);
    assert_eq!(
        reference::derive(&installed_world, &twin_image),
        installed_only,
        "the install alone is the installed receive"
    );
    assert_eq!(
        answered_occurrences(&store, &held, &twin_channel, twin.label),
        installed_only,
        "the published accelerator answers the installed receive"
    );

    twin.consume_into(&space).await;
    let twinned = hydrated_space(&space, &universe);
    assert_ne!(
        installed_target, resting_target,
        "an installed record and a byte-identical resting record are distinct receives"
    );
    assert_eq!(
        installed_target.identity, resting_target.identity,
        "the twins share their record identity"
    );
    assert_eq!(
        installed_target.source, resting_target.source,
        "the twins share their source"
    );
    assert_eq!(
        store.get_continuations(&twin.channels),
        vec![twin.waiting(), twin.waiting()],
        "production lists the installed twin and then the identical resting twin"
    );
    let expected = BTreeSet::from([installed_occurrence, Occurrence {
        channel: twin_image.clone(),
        group: key.clone(),
        positions: vec![0],
        target: resting_target.clone(),
    }]);
    assert_eq!(
        reference::derive(&twinned, &twin_image),
        expected,
        "the twins derive two occurrences that differ only in kind"
    );
    assert_eq!(
        fallback_class(store.receive_occurrences(Some(&held), &twin_channel)),
        Some(ReceiveAcceleratorClass::Stale),
        "the resting twin makes the held handle stale"
    );
    let maintained = published_handle(&store);
    let maintained_answer = exact_answer(&store, &maintained, &twin_channel, twin.label);
    assert_eq!(
        production_occurrences(&maintained_answer, &twin_image),
        expected,
        "the maintained accelerator answers both twins"
    );
    let detached = store
        .detached_receive_accelerator()
        .expect("a detached build over the loaded scope succeeds");
    assert_eq!(
        answered_occurrences(&store, &detached, &twin_channel, twin.label),
        expected,
        "a detached rebuild answers both twins"
    );
    let mut resolved_kinds = BTreeSet::new();
    for group in maintained_answer.groups() {
        for target in group.targets() {
            let id = maintained_answer.receive_id(target);
            assert!(
                store
                    .resolve_receive(&id)
                    .is_some_and(|found| found == *target),
                "each twin's id resolves to its own target"
            );
            resolved_kinds.insert(production_kind(target.kind()));
        }
    }
    assert_eq!(
        resolved_kinds,
        BTreeSet::from([ReceiveKind::Installed, ReceiveKind::Resting]),
        "the twins carry distinct ids, one per kind"
    );

    let fired = produce_comm(&space, &twin_channel, 1, &twin).await;
    assert_position_bindings(&twin, &fired, &twin_channel, &[1], 1);
    assert_eq!(
        hydrated_space(&space, &universe),
        twinned,
        "a persistent firing keeps both twins and the group version"
    );
    let current = published_handle(&store);
    assert_eq!(
        answered_occurrences(&store, &current, &twin_channel, twin.label),
        expected,
        "the maintained accelerator still answers both twins"
    );
    assert!(
        answered_occurrences(&store, &current, &absent, "absent").is_empty(),
        "an absent channel has no occurrences"
    );
}

fn relation_targets(observation: &Observation) -> BTreeSet<ReceiveTarget> {
    reference::derive_all(observation)
        .values()
        .flatten()
        .map(|occurrence| occurrence.target.clone())
        .collect()
}

fn space_answers(
    store: &RhoStore,
    handle: &RhoHandle,
    probed: &[&Par],
    label: &str,
) -> BTreeMap<ChannelImage, BTreeSet<Occurrence>> {
    probed
        .iter()
        .map(|channel| {
            (
                reference::channel_image(*channel),
                answered_occurrences(store, handle, channel, label),
            )
        })
        .collect()
}

fn derived_answers(
    observation: &Observation,
    probed: &[&Par],
) -> BTreeMap<ChannelImage, BTreeSet<Occurrence>> {
    probed
        .iter()
        .map(|channel| {
            let image = reference::channel_image(*channel);
            let derived = reference::derive(observation, &image);
            (image, derived)
        })
        .collect()
}

fn group_answer(answer: &RhoOccurrences) -> Vec<(GroupKey, Vec<u64>, Vec<ReceiveTarget>)> {
    let mut groups = Vec::with_capacity(answer.groups().len());
    for group in answer.groups() {
        let key = reference::group_key(group.channels());
        let mut positions = Vec::with_capacity(group.positions().len());
        positions.extend(group.positions().iter().map(|position| position.get()));
        let mut targets = Vec::with_capacity(group.targets().len());
        targets.extend(
            group
                .targets()
                .iter()
                .map(|target| production_target(&key, target)),
        );
        groups.push((key, positions, targets));
    }
    groups
}

fn assert_absent_everywhere(store: &RhoStore, probed: &[&Par], transition: &str) {
    assert!(
        store.receive_accelerator().is_none(),
        "{transition}: the replaced store publishes no accelerator"
    );
    for channel in probed {
        assert_eq!(
            fallback_class(store.receive_occurrences(None, channel)),
            Some(ReceiveAcceleratorClass::Absent),
            "{transition}: a read without a handle falls back as Absent"
        );
        assert_eq!(
            fallback_class(store.current_receive_occurrences(channel)),
            Some(ReceiveAcceleratorClass::Absent),
            "{transition}: a read of the current accelerator falls back as Absent"
        );
    }
}

fn assert_foreign_everywhere(
    store: &RhoStore,
    handles: &[&RhoHandle],
    probed: &[&Par],
    transition: &str,
) {
    for handle in handles {
        for channel in probed {
            assert_eq!(
                fallback_class(store.receive_occurrences(Some(*handle), channel)),
                Some(ReceiveAcceleratorClass::Foreign),
                "{transition}: a handle taken from a replaced store falls back as Foreign"
            );
        }
    }
}

fn assert_stale_everywhere(
    store: &RhoStore,
    handle: &RhoHandle,
    probed: &[&Par],
    transition: &str,
) {
    for channel in probed {
        assert_eq!(
            fallback_class(store.receive_occurrences(Some(handle), channel)),
            Some(ReceiveAcceleratorClass::Stale),
            "{transition}: a handle taken before the receive changed falls back as Stale"
        );
    }
}

fn assert_published_equals_rebuild(
    store: &RhoStore,
    handle: &RhoHandle,
    observation: &Observation,
    probed: &[&Par],
    transition: &str,
) {
    let detached = store
        .detached_receive_accelerator()
        .expect("a detached build over the loaded scope succeeds");
    for channel in probed {
        let image = reference::channel_image(*channel);
        let published = exact_answer(store, handle, channel, transition);
        let rebuilt = exact_answer(store, &detached, channel, transition);
        let groups = group_answer(&published);
        assert_eq!(
            groups,
            group_answer(&rebuilt),
            "{transition}: the published accelerator equals a detached rebuild, group by group"
        );
        let answered_groups: BTreeSet<(GroupKey, Vec<u64>)> = groups
            .into_iter()
            .map(|(key, positions, _)| (key, positions))
            .collect();
        assert_eq!(
            answered_groups,
            reference::channel_groups(observation, &image),
            "{transition}: the published groups are the derived groups"
        );
        assert_eq!(
            production_occurrences(&published, &image),
            reference::derive(observation, &image),
            "{transition}: the published accelerator answers the derivation"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn production_checkpoint_reset_and_soft_revert_rebuild_identical_occurrences() {
    let join_a = compiled_process("join-a name", JOIN_A_PROCESS);
    let join_b = compiled_process("join-b name", JOIN_B_PROCESS);
    let absent = compiled_process("absent name", ABSENT_PROCESS);
    let join = resolve_case(
        "lifecycle-join",
        REPEATED_JOIN_SOURCE,
        &repeated_env(&join_a, &join_b),
    );
    let single = resolve_compiled("lifecycle-single", LIFECYCLE_SINGLE_SOURCE);
    let later = resolve_compiled("lifecycle-later", LIFECYCLE_LATER_SOURCE);
    let single_channel = bind_channel(&single, 0).clone();
    let later_channel = bind_channel(&later, 0).clone();
    let universe = vec![
        join.channels.clone(),
        single.channels.clone(),
        later.channels.clone(),
    ];
    let probed = [&join_a, &join_b, &single_channel, &later_channel, &absent];
    let space = in_memory_space().await;
    join.consume_into(&space).await;
    single.consume_into(&space).await;
    let original_store = space.get_store();
    let original = hydrated_space(&space, &universe);
    let original_relation = reference::derive_all(&original);
    assert_eq!(
        relation_targets(&original),
        BTreeSet::from([
            join.target(ReceiveKind::Resting),
            single.target(ReceiveKind::Resting),
        ]),
        "the original relation names the join and the single receive"
    );
    let original_handle = rebuilt_handle(&original_store);
    let original_answers = space_answers(&original_store, &original_handle, &probed, "original");
    assert_eq!(
        original_answers,
        derived_answers(&original, &probed),
        "the published accelerator answers the original derivation"
    );

    let checkpoint = space
        .create_checkpoint()
        .await
        .expect("an in-memory checkpoint succeeds");
    let checkpointed_store = space.get_store();
    assert_absent_everywhere(&checkpointed_store, &probed, "checkpoint");
    assert_foreign_everywhere(
        &checkpointed_store,
        &[&original_handle],
        &probed,
        "checkpoint",
    );
    let checkpointed = hydrated_space(&space, &universe);
    assert_eq!(
        reference::derive_all(&checkpointed),
        original_relation,
        "a checkpoint reloads the identical relation from history"
    );
    let checkpoint_handle = rebuilt_handle(&checkpointed_store);
    assert_eq!(
        space_answers(
            &checkpointed_store,
            &checkpoint_handle,
            &probed,
            "checkpoint"
        ),
        original_answers,
        "after a checkpoint, a rebuild answers the original occurrences"
    );

    later.consume_into(&space).await;
    let extended = hydrated_space(&space, &universe);
    assert_ne!(
        reference::derive_all(&extended),
        original_relation,
        "the later receive extends the relation"
    );
    assert_stale_everywhere(
        &checkpointed_store,
        &checkpoint_handle,
        &probed,
        "later receive",
    );
    let extended_handle = published_handle(&checkpointed_store);
    assert_eq!(
        space_answers(
            &checkpointed_store,
            &extended_handle,
            &probed,
            "later receive"
        ),
        derived_answers(&extended, &probed),
        "the maintained accelerator answers the extended relation"
    );

    space
        .reset(&checkpoint.root)
        .await
        .expect("an in-memory reset succeeds");
    let reset_store = space.get_store();
    assert_absent_everywhere(&reset_store, &probed, "reset");
    assert_foreign_everywhere(
        &reset_store,
        &[&original_handle, &checkpoint_handle, &extended_handle],
        &probed,
        "reset",
    );
    let reset = hydrated_space(&space, &universe);
    assert_eq!(
        reference::derive_all(&reset),
        original_relation,
        "a reset discards the uncheckpointed receive"
    );
    let reset_handle = rebuilt_handle(&reset_store);
    assert_eq!(
        space_answers(&reset_store, &reset_handle, &probed, "reset"),
        original_answers,
        "after a reset, a rebuild answers the original occurrences"
    );

    let single_positions =
        answered_positions(&reset_store, &reset_handle, &single_channel, single.label);
    let soft = space.create_soft_checkpoint().await;
    later.consume_into(&space).await;
    let fired = produce_comm(&space, &single_channel, 1, &single).await;
    assert_position_bindings(
        &single,
        &fired,
        &single_channel,
        &[1],
        single_positions.len(),
    );
    let mutated = hydrated_space(&space, &universe);
    assert_ne!(
        reference::derive_all(&mutated),
        original_relation,
        "the state after the soft checkpoint was mutated"
    );
    let mutated_handle = published_handle(&reset_store);
    space
        .revert_to_soft_checkpoint(soft)
        .await
        .expect("an in-memory soft revert succeeds");
    let reverted_store = space.get_store();
    assert_absent_everywhere(&reverted_store, &probed, "soft revert");
    assert_foreign_everywhere(
        &reverted_store,
        &[&reset_handle, &mutated_handle],
        &probed,
        "soft revert",
    );
    let reverted = hydrated_space(&space, &universe);
    assert_eq!(
        reference::derive_all(&reverted),
        original_relation,
        "a soft revert restores the identical relation"
    );
    let reverted_handle = rebuilt_handle(&reverted_store);
    assert_eq!(
        space_answers(&reverted_store, &reverted_handle, &probed, "soft revert"),
        original_answers,
        "after a soft revert, a rebuild answers the original occurrences"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn production_both_matching_directions_keep_the_published_accelerator_equal_to_a_rebuild() {
    let linear = resolve_compiled("direction-linear", DIRECTION_LINEAR_SOURCE);
    let data_first = resolve_compiled("direction-data-first", DIRECTION_DATA_FIRST_SOURCE);
    let persistent = resolve_compiled("direction-persistent", DIRECTION_PERSISTENT_SOURCE);
    let absent = compiled_process("absent name", ABSENT_PROCESS);
    let linear_channel = bind_channel(&linear, 0).clone();
    let data_channel = bind_channel(&data_first, 0).clone();
    let persistent_channel = bind_channel(&persistent, 0).clone();
    let universe = vec![
        linear.channels.clone(),
        data_first.channels.clone(),
        persistent.channels.clone(),
    ];
    let probed = [&linear_channel, &data_channel, &persistent_channel, &absent];
    let linear_target = linear.target(ReceiveKind::Resting);
    let data_first_target = data_first.target(ReceiveKind::Resting);
    let persistent_target = persistent.target(ReceiveKind::Resting);
    let space = in_memory_space().await;
    linear.consume_into(&space).await;
    persistent.consume_into(&space).await;
    let store = space.get_store();
    let rested = hydrated_space(&space, &universe);
    assert!(
        rested.contains_target(&linear_target),
        "the linear receive rests"
    );
    assert!(
        rested.contains_target(&persistent_target),
        "the persistent receive rests"
    );
    let held = rebuilt_handle(&store);
    assert_published_equals_rebuild(&store, &held, &rested, &probed, "rested");

    let linear_positions = answered_positions(&store, &held, &linear_channel, linear.label);
    let produced = produce_comm(&space, &linear_channel, 1, &linear).await;
    assert_position_bindings(
        &linear,
        &produced,
        &linear_channel,
        &[1],
        linear_positions.len(),
    );
    let produce_fired = hydrated_space(&space, &universe);
    assert!(
        !produce_fired.contains_target(&linear_target),
        "the produce-triggered COMM removes the linear receive"
    );
    assert_eq!(
        fallback_class(store.receive_occurrences(Some(&held), &linear_channel)),
        Some(ReceiveAcceleratorClass::Stale),
        "the removal makes the held handle stale"
    );
    let maintained = published_handle(&store);
    assert_published_equals_rebuild(
        &store,
        &maintained,
        &produce_fired,
        &probed,
        "produce-triggered COMM",
    );

    produce_resting_datum(&space, &data_channel, 2, data_first.label).await;
    let data_waiting = hydrated_space(&space, &universe);
    assert_eq!(
        data_waiting, produce_fired,
        "a resting datum changes no receive"
    );
    assert_published_equals_rebuild(&store, &maintained, &data_waiting, &probed, "resting datum");
    let consumed = consume_comm(&space, &data_first).await;
    let data_capacity =
        reference::capacity(&data_first.key(), &reference::channel_image(&data_channel));
    assert_position_bindings(&data_first, &consumed, &data_channel, &[2], data_capacity);
    let consume_fired = hydrated_space(&space, &universe);
    assert_eq!(
        consume_fired, data_waiting,
        "a consume-triggered COMM never rests its receive or replaces a version"
    );
    assert!(
        !consume_fired.contains_target(&data_first_target),
        "the consume-triggered receive never enters the relation"
    );
    assert_published_equals_rebuild(
        &store,
        &maintained,
        &consume_fired,
        &probed,
        "consume-triggered COMM",
    );

    let persistent_positions =
        answered_positions(&store, &maintained, &persistent_channel, persistent.label);
    let persisted = produce_comm(&space, &persistent_channel, 3, &persistent).await;
    assert_position_bindings(
        &persistent,
        &persisted,
        &persistent_channel,
        &[3],
        persistent_positions.len(),
    );
    let persistent_fired = hydrated_space(&space, &universe);
    assert_eq!(
        persistent_fired, consume_fired,
        "a persistent firing keeps its receive and its group version"
    );
    assert!(
        persistent_fired.contains_target(&persistent_target),
        "the persistent receive stays in the relation"
    );
    assert_published_equals_rebuild(
        &store,
        &maintained,
        &persistent_fired,
        &probed,
        "persistent firing",
    );
}
