use std::collections::BTreeSet;
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
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{ISpace, RSpaceResult};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

#[path = "../../rspace++/tests/support/receive_occurrence_reference.rs"]
#[allow(dead_code)]
#[rustfmt::skip]
mod reference;

use reference::{
    AcceleratorClass, AcceleratorUnderTest, Answer, BuiltAccelerator, ChannelImage, GroupKey,
    Lineage, MaintenanceOp, Observation, Occurrence, ReceiveKind, ReceiveTarget,
    ReferenceAccelerator, ReferenceWorld,
};

type RhoSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
type RhoStore = Arc<Box<dyn HotStore<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>;
type RhoMatcher = Arc<Box<dyn TupleSpaceMatch<BindPattern, ListParWithRandom, TaggedContinuation>>>;
type RhoWaitingContinuation = WaitingContinuation<BindPattern, TaggedContinuation>;
type RhoBinding = RSpaceResult<Par, ListParWithRandom>;

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

fn alias_probes(channel: &Par, absent: &Par) -> Vec<ChannelImage> {
    let identity = reference::channel_image(channel);
    let Some((_, truncated)) = identity.split_last() else {
        panic!("a channel identity is never empty");
    };
    let extended_len = identity
        .len()
        .checked_add(1)
        .expect("an extended identity length fits usize");
    let mut extended = Vec::with_capacity(extended_len);
    extended.extend_from_slice(&identity);
    extended.push(0x00);
    vec![
        reference::channel_image(absent),
        truncated.to_vec(),
        extended,
    ]
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
        reference::capacity(&receive.key(), &reference::channel_image(channel)),
        "{label}: the COMM consumed one datum per position of the channel"
    );
}

fn assert_capacity_occurrence(
    observation: &Observation,
    receive: &RestingReceive,
    image: &[u8],
    positions: &[u64],
) {
    let key = receive.key();
    let expected = Occurrence {
        channel: image.to_vec(),
        group: key.clone(),
        positions: positions.to_vec(),
        target: receive.target(ReceiveKind::Resting),
    };
    assert_eq!(
        reference::derive(observation, image),
        BTreeSet::from([expected]),
        "{}: the channel has exactly one occurrence in the resting join",
        receive.label
    );
    assert_eq!(
        positions.len(),
        reference::capacity(&key, image),
        "{}: the occurrence carries one position per unit of join capacity",
        receive.label
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

fn listed_targets(store: &RhoStore, receive: &RestingReceive) -> Vec<ReceiveTarget> {
    let key = receive.key();
    store
        .get_continuations(&receive.channels)
        .iter()
        .map(|continuation| {
            reference::target_of_continuation(ReceiveKind::Resting, &key, continuation)
        })
        .collect()
}

fn probe_images(world: &ReferenceWorld, extra: &[ChannelImage]) -> BTreeSet<ChannelImage> {
    let mut images = world.observation.channel_images();
    images.extend(extra.iter().cloned());
    images
}

fn assert_matches_rebuild(
    accelerator: &ReferenceAccelerator,
    world: &ReferenceWorld,
    extra: &[ChannelImage],
) {
    assert_eq!(
        accelerator.classify(world),
        AcceleratorClass::Coherent,
        "the accelerator is coherent with the world it was built or maintained to"
    );
    let built = accelerator
        .built()
        .expect("a coherent accelerator is built");
    assert_eq!(
        built,
        &BuiltAccelerator::build(world),
        "the held accelerator equals a rebuild from the derived relation"
    );
    for image in probe_images(world, extra) {
        let derived = reference::derive(&world.observation, &image);
        assert_eq!(
            accelerator.answer(world, &image),
            Answer::Exact(derived.clone()),
            "a coherent accelerator answers the derivation"
        );
        assert_eq!(
            reference::read(accelerator, world, &world.observation, &image),
            derived,
            "a coherent read equals the derivation"
        );
    }
}

fn assert_every_version_replaced(
    before: &Observation,
    after: &Observation,
    universe: &[Vec<Par>],
    transition: &str,
) {
    for channels in universe {
        let key = reference::group_key(channels);
        let held = before
            .version(&key)
            .expect("the held observation loaded every universe group");
        let current = after
            .version(&key)
            .expect("the current observation loaded every universe group");
        assert_ne!(
            held, current,
            "{transition}: every held group version is replaced"
        );
    }
}

fn assert_foreign_then_rebuild(
    accelerator: &mut ReferenceAccelerator,
    world: &ReferenceWorld,
    extra: &[ChannelImage],
    transition: &str,
) {
    assert_eq!(
        accelerator.classify(world),
        AcceleratorClass::Foreign,
        "{transition}: the held accelerator belongs to a replaced generation"
    );
    for image in probe_images(world, extra) {
        assert_eq!(
            accelerator.answer(world, &image),
            Answer::Fallback(AcceleratorClass::Foreign),
            "{transition}: a foreign accelerator falls back"
        );
        assert_eq!(
            reference::read(&*accelerator, world, &world.observation, &image),
            reference::derive(&world.observation, &image),
            "{transition}: a foreign read equals the derivation"
        );
    }
    accelerator.rebuild(world);
    assert_matches_rebuild(accelerator, world, extra);
}

fn relation_targets(observation: &Observation) -> BTreeSet<ReceiveTarget> {
    reference::derive_all(observation)
        .values()
        .flatten()
        .map(|occurrence| occurrence.target.clone())
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn compiled_repeated_joins_derive_exact_occurrences() {
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
        peek_all.peeks,
        BTreeSet::from([0, 1, 2]),
        "a head peek bind makes the compiled receive peek at every position"
    );
    assert!(!peek_all.persistent, "a peek receive is not persistent");
    assert_eq!(
        peek_first.peeks,
        BTreeSet::from([0]),
        "the metadata-distinct record peeks only at position 0"
    );
    assert!(
        persistent.persistent,
        "a head repeated bind makes the compiled receive persistent"
    );
    assert!(
        persistent.peeks.is_empty(),
        "a persistent receive never peeks"
    );
    assert_eq!(
        reseeded.patterns, peek_all.patterns,
        "reseeding keeps every pattern"
    );
    assert_ne!(
        reseeded.continuation, peek_all.continuation,
        "reseeding changes the continuation random state"
    );

    let space = in_memory_space().await;
    for variant in variants {
        variant.consume_into(&space).await;
    }
    let universe = vec![group.clone()];
    let lineage = Lineage::new(Vec::new());
    let world = lineage.world(hydrated_space(&space, &universe));
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
    assert_eq!(
        world.observation.targets(&key),
        expected,
        "the group holds exactly the four resting variants"
    );
    assert_eq!(
        listed_targets(&space.get_store(), &peek_all),
        world.observation.ordered_targets(&key),
        "the reference order equals the production continuation order"
    );
    let a_image = reference::channel_image(&join_a);
    let b_image = reference::channel_image(&join_b);
    assert_eq!(
        reference::capacity(&key, &a_image),
        2,
        "a occupies two join positions"
    );
    assert_eq!(
        reference::capacity(&key, &b_image),
        1,
        "b occupies one join position"
    );
    for (image, positions) in [(&a_image, vec![0_u64, 2]), (&b_image, vec![1_u64])] {
        let occurrences = reference::derive(&world.observation, image);
        let targets: BTreeSet<ReceiveTarget> = occurrences
            .iter()
            .map(|occurrence| occurrence.target.clone())
            .collect();
        assert_eq!(
            occurrences.len(),
            variants.len(),
            "one occurrence per receive"
        );
        assert_eq!(targets, expected, "every variant occurs on the channel");
        for occurrence in &occurrences {
            assert_eq!(occurrence.group, key, "the occurrence names its group");
            assert_eq!(
                occurrence.positions, positions,
                "the occurrence lists every position of the channel"
            );
            assert_eq!(
                occurrence.positions.len(),
                reference::capacity(&key, image),
                "positions and capacity agree"
            );
        }
    }
    let extra = alias_probes(&join_a, &absent);
    for probe in &extra {
        assert!(
            reference::derive(&world.observation, probe).is_empty(),
            "absent and prefix-aliased images have no occurrences"
        );
    }
    assert_eq!(
        reference::derive_all(&world.observation).len(),
        2,
        "only the two channels of the group carry occurrences"
    );
    let mut accelerator = ReferenceAccelerator::new();
    accelerator.rebuild(&world);
    assert_matches_rebuild(&accelerator, &world, &extra);
}

#[tokio::test(flavor = "current_thread")]
async fn compiled_join_capacity_agrees_with_occurrence_multiplicity() {
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
    let a_image = reference::channel_image(&join_a);
    let b_image = reference::channel_image(&join_b);
    let universe = vec![join.channels.clone()];

    let receive_first = in_memory_space().await;
    join.consume_into(&receive_first).await;
    let rested = hydrated_space(&receive_first, &universe);
    assert_capacity_occurrence(&rested, &join, &a_image, &[0, 2]);
    assert_capacity_occurrence(&rested, &join, &b_image, &[1]);
    produce_resting_datum(&receive_first, &join_a, 10, join.label).await;
    produce_resting_datum(&receive_first, &join_b, 30, join.label).await;
    assert_eq!(
        hydrated_space(&receive_first, &universe),
        rested,
        "one datum on a cannot fill both a positions, so the join keeps resting"
    );
    let produced = produce_comm(&receive_first, &join_a, 20, &join).await;
    assert_position_bindings(&join, &produced, &join_a, &[10, 20]);
    assert_position_bindings(&join, &produced, &join_b, &[30]);
    assert!(
        reference::derive_all(&hydrated_space(&receive_first, &universe)).is_empty(),
        "the fired join leaves no occurrence"
    );
    let receive_first_store = receive_first.get_store();
    assert!(
        receive_first_store.get_data(&join_a).is_empty(),
        "the COMM consumed both data on a"
    );
    assert!(
        receive_first_store.get_data(&join_b).is_empty(),
        "the COMM consumed the datum on b"
    );

    let data_first_short = in_memory_space().await;
    produce_resting_datum(&data_first_short, &join_a, 10, join.label).await;
    produce_resting_datum(&data_first_short, &join_b, 30, join.label).await;
    join.consume_into(&data_first_short).await;
    let waiting = hydrated_space(&data_first_short, &universe);
    assert_capacity_occurrence(&waiting, &join, &a_image, &[0, 2]);
    assert_capacity_occurrence(&waiting, &join, &b_image, &[1]);
    assert_eq!(
        data_first_short.get_store().get_data(&join_a).len(),
        1,
        "the lone datum on a still waits for a second one"
    );

    let data_first = in_memory_space().await;
    for (target, value) in [(&join_a, 10), (&join_a, 20), (&join_b, 30)] {
        produce_resting_datum(&data_first, target, value, join.label).await;
    }
    let consumed = consume_comm(&data_first, &join).await;
    assert_position_bindings(&join, &consumed, &join_a, &[10, 20]);
    assert_position_bindings(&join, &consumed, &join_b, &[30]);
    assert!(
        reference::derive_all(&hydrated_space(&data_first, &universe)).is_empty(),
        "a consume-triggered COMM never rests the join"
    );
    let data_first_store = data_first.get_store();
    assert!(
        data_first_store.get_data(&join_a).is_empty(),
        "the COMM consumed both data on a"
    );
    assert!(
        data_first_store.get_data(&join_b).is_empty(),
        "the COMM consumed the datum on b"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn installed_and_resting_receives_derive_distinct_occurrences() {
    let twin = resolve_compiled("installed-twin", INSTALLED_TWIN_SOURCE);
    assert!(
        twin.persistent,
        "every install is persistent, so its resting twin is a repeated receive"
    );
    assert!(twin.peeks.is_empty(), "an install never peeks");
    let absent = compiled_process("absent name", ABSENT_PROCESS);
    let twin_channel = bind_channel(&twin, 0).clone();
    let twin_image = reference::channel_image(&twin_channel);
    let extra = alias_probes(&twin_channel, &absent);
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
    let lineage = Lineage::new(Vec::new());
    let installed_world = lineage.world(hydrated_space(&space, &universe));
    let installed_target = twin.target(ReceiveKind::Installed);
    let resting_target = twin.target(ReceiveKind::Resting);
    assert_eq!(
        installed_world.observation.targets(&key),
        BTreeSet::from([installed_target.clone()]),
        "the install alone is the installed receive"
    );
    let mut accelerator = ReferenceAccelerator::new();
    accelerator.rebuild(&installed_world);
    assert_matches_rebuild(&accelerator, &installed_world, &extra);

    twin.consume_into(&space).await;
    let twinned = lineage.world(hydrated_space(&space, &universe));
    assert_ne!(
        installed_world.observation.version(&key),
        twinned.observation.version(&key),
        "resting the twin replaces the group version"
    );
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
        twinned.observation.targets(&key),
        BTreeSet::from([installed_target.clone(), resting_target.clone()]),
        "the group holds the installed and the resting twin"
    );
    assert_eq!(
        space.get_store().get_continuations(&twin.channels),
        vec![twin.waiting(), twin.waiting()],
        "production lists the installed twin and then the identical resting twin"
    );
    let expected = BTreeSet::from([
        Occurrence {
            channel: twin_image.clone(),
            group: key.clone(),
            positions: vec![0],
            target: installed_target.clone(),
        },
        Occurrence {
            channel: twin_image.clone(),
            group: key.clone(),
            positions: vec![0],
            target: resting_target.clone(),
        },
    ]);
    assert_eq!(
        reference::derive(&twinned.observation, &twin_image),
        expected,
        "the twins derive two occurrences that differ only in kind"
    );
    assert_eq!(
        accelerator.classify(&twinned),
        AcceleratorClass::Stale,
        "the resting twin makes the held accelerator stale"
    );
    accelerator
        .apply(
            &MaintenanceOp::PutResting(resting_target.clone()),
            &installed_world,
            &twinned,
        )
        .expect("a coherent accelerator maintains the resting twin");
    assert_matches_rebuild(&accelerator, &twinned, &extra);

    let fired = produce_comm(&space, &twin_channel, 1, &twin).await;
    assert_position_bindings(&twin, &fired, &twin_channel, &[1]);
    let after_firing = lineage.world(hydrated_space(&space, &universe));
    assert_eq!(
        after_firing.observation, twinned.observation,
        "a persistent firing keeps both twins and the group version"
    );
    assert_matches_rebuild(&accelerator, &after_firing, &extra);
}

#[tokio::test(flavor = "current_thread")]
async fn checkpoint_reset_and_soft_revert_rebuild_identical_occurrences() {
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
    let extra = alias_probes(&join_a, &absent);
    let universe = vec![
        join.channels.clone(),
        single.channels.clone(),
        later.channels.clone(),
    ];
    let space = in_memory_space().await;
    join.consume_into(&space).await;
    single.consume_into(&space).await;
    let mut lineage = Lineage::new(space.get_root().await.bytes());
    let original = lineage.world(hydrated_space(&space, &universe));
    let original_relation = reference::derive_all(&original.observation);
    assert_eq!(
        relation_targets(&original.observation),
        BTreeSet::from([
            join.target(ReceiveKind::Resting),
            single.target(ReceiveKind::Resting),
        ]),
        "the original relation names the join and the single receive"
    );
    let mut accelerator = ReferenceAccelerator::new();
    accelerator.rebuild(&original);
    assert_matches_rebuild(&accelerator, &original, &extra);

    let checkpoint = space
        .create_checkpoint()
        .await
        .expect("an in-memory checkpoint succeeds");
    lineage.retarget(checkpoint.root.bytes());
    let checkpointed = lineage.world(hydrated_space(&space, &universe));
    assert_every_version_replaced(
        &original.observation,
        &checkpointed.observation,
        &universe,
        "checkpoint",
    );
    assert_eq!(
        reference::derive_all(&checkpointed.observation),
        original_relation,
        "a checkpoint reloads the identical relation from history"
    );
    assert_foreign_then_rebuild(&mut accelerator, &checkpointed, &extra, "checkpoint");

    later.consume_into(&space).await;
    let extended = lineage.world(hydrated_space(&space, &universe));
    assert_ne!(
        reference::derive_all(&extended.observation),
        original_relation,
        "the later receive extends the relation"
    );
    assert_eq!(
        accelerator.classify(&extended),
        AcceleratorClass::Stale,
        "the later receive makes the rebuilt accelerator stale"
    );

    space
        .reset(&checkpoint.root)
        .await
        .expect("an in-memory reset succeeds");
    lineage.retarget(checkpoint.root.bytes());
    let reset = lineage.world(hydrated_space(&space, &universe));
    assert_every_version_replaced(
        &extended.observation,
        &reset.observation,
        &universe,
        "reset",
    );
    assert_eq!(
        reference::derive_all(&reset.observation),
        original_relation,
        "a reset discards the uncheckpointed receive"
    );
    assert_foreign_then_rebuild(&mut accelerator, &reset, &extra, "reset");

    let soft = space.create_soft_checkpoint().await;
    later.consume_into(&space).await;
    let fired = produce_comm(&space, &single_channel, 1, &single).await;
    assert_position_bindings(&single, &fired, &single_channel, &[1]);
    let mutated = lineage.world(hydrated_space(&space, &universe));
    assert_ne!(
        reference::derive_all(&mutated.observation),
        original_relation,
        "the state after the soft checkpoint was mutated"
    );
    space
        .revert_to_soft_checkpoint(soft)
        .await
        .expect("an in-memory soft revert succeeds");
    lineage.regenerate();
    let reverted = lineage.world(hydrated_space(&space, &universe));
    assert_every_version_replaced(
        &mutated.observation,
        &reverted.observation,
        &universe,
        "soft revert",
    );
    assert_eq!(
        reference::derive_all(&reverted.observation),
        original_relation,
        "a soft revert restores the identical relation"
    );
    assert_foreign_then_rebuild(&mut accelerator, &reverted, &extra, "soft revert");
}

#[tokio::test(flavor = "current_thread")]
async fn both_matching_directions_maintain_the_derived_relation() {
    let linear = resolve_compiled("direction-linear", DIRECTION_LINEAR_SOURCE);
    let data_first = resolve_compiled("direction-data-first", DIRECTION_DATA_FIRST_SOURCE);
    let persistent = resolve_compiled("direction-persistent", DIRECTION_PERSISTENT_SOURCE);
    let absent = compiled_process("absent name", ABSENT_PROCESS);
    let linear_channel = bind_channel(&linear, 0).clone();
    let data_channel = bind_channel(&data_first, 0).clone();
    let persistent_channel = bind_channel(&persistent, 0).clone();
    let extra = alias_probes(&linear_channel, &absent);
    let universe = vec![
        linear.channels.clone(),
        data_first.channels.clone(),
        persistent.channels.clone(),
    ];
    let linear_key = linear.key();
    let linear_target = linear.target(ReceiveKind::Resting);
    let data_first_target = data_first.target(ReceiveKind::Resting);
    let persistent_target = persistent.target(ReceiveKind::Resting);
    let space = in_memory_space().await;
    linear.consume_into(&space).await;
    persistent.consume_into(&space).await;
    let lineage = Lineage::new(Vec::new());
    let rested = lineage.world(hydrated_space(&space, &universe));
    assert!(
        rested.observation.contains_target(&linear_target),
        "the linear receive rests"
    );
    assert!(
        rested.observation.contains_target(&persistent_target),
        "the persistent receive rests"
    );
    let mut accelerator = ReferenceAccelerator::new();
    accelerator.rebuild(&rested);
    assert_matches_rebuild(&accelerator, &rested, &extra);

    let produced = produce_comm(&space, &linear_channel, 1, &linear).await;
    assert_position_bindings(&linear, &produced, &linear_channel, &[1]);
    let produce_fired = lineage.world(hydrated_space(&space, &universe));
    assert_ne!(
        rested.observation.version(&linear_key),
        produce_fired.observation.version(&linear_key),
        "a produce-triggered COMM replaces the fired group's version"
    );
    assert!(
        !produce_fired.observation.contains_target(&linear_target),
        "the produce-triggered COMM removes the linear receive"
    );
    assert_eq!(
        accelerator.classify(&produce_fired),
        AcceleratorClass::Stale,
        "the removal makes the held accelerator stale"
    );
    accelerator
        .apply(
            &MaintenanceOp::RemoveResting(linear_target.clone()),
            &rested,
            &produce_fired,
        )
        .expect("a coherent accelerator maintains a produce-triggered COMM");
    assert_matches_rebuild(&accelerator, &produce_fired, &extra);

    produce_resting_datum(&space, &data_channel, 2, data_first.label).await;
    let data_waiting = lineage.world(hydrated_space(&space, &universe));
    assert_eq!(
        data_waiting.observation, produce_fired.observation,
        "a resting datum changes no receive"
    );
    let consumed = consume_comm(&space, &data_first).await;
    assert_position_bindings(&data_first, &consumed, &data_channel, &[2]);
    let consume_fired = lineage.world(hydrated_space(&space, &universe));
    assert_eq!(
        consume_fired.observation, data_waiting.observation,
        "a consume-triggered COMM never rests its receive or replaces a version"
    );
    assert!(
        !consume_fired
            .observation
            .contains_target(&data_first_target),
        "the consume-triggered receive never enters the relation"
    );
    assert_matches_rebuild(&accelerator, &consume_fired, &extra);

    let persisted = produce_comm(&space, &persistent_channel, 3, &persistent).await;
    assert_position_bindings(&persistent, &persisted, &persistent_channel, &[3]);
    let persistent_fired = lineage.world(hydrated_space(&space, &universe));
    assert_eq!(
        persistent_fired.observation, consume_fired.observation,
        "a persistent firing keeps its receive and its group version"
    );
    assert!(
        persistent_fired
            .observation
            .contains_target(&persistent_target),
        "the persistent receive stays in the relation"
    );
    assert_matches_rebuild(&accelerator, &persistent_fired, &extra);
}
