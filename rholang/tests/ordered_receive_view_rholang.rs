use std::collections::BTreeSet;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, EList, Expr, ListParWithRandom, Match, MatchCase, Par, ParWithRandom, Receive,
    ReceiveBind, TaggedContinuation,
};
use models::rust::par_construction::ParConstruction;
use models::rust::rholang::par_children::dismantle_all;
use models::rust::utils::{new_freevar_par, new_gint_par, new_gstring_par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::substitute::Substitute;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::hot_store::HotStore;
use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::ordered_receive::{
    OrderedReceiveView, PreparedContinuationRecord, RestingReceiveVisit,
};
use rspace_plus_plus::rspace::r#match::Match as TupleSpaceMatch;
use rspace_plus_plus::rspace::rho_path::{ReceiveChannelQuery, RhoPathEncode};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

#[path = "support/counting_alloc.rs"]
#[expect(
    dead_code,
    reason = "this target counts allocations exactly and does not use the subquadratic envelope"
)]
mod counting_alloc;

#[global_allocator]
static ALLOCATOR: counting_alloc::Counting = counting_alloc::Counting;

type RhoSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
type RhoStore = Arc<Box<dyn HotStore<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>;
type RhoMatcher = Arc<Box<dyn TupleSpaceMatch<BindPattern, ListParWithRandom, TaggedContinuation>>>;
type RhoWaitingContinuation = WaitingContinuation<BindPattern, TaggedContinuation>;

const LEGACY_CONTINUATION_HEADER: [u8; 6] = *b"RHP\x01\x02\x01";
const LEGACY_GROUP_END: [u8; 2] = [0x00, 0x01];
const LEGACY_ESCAPE: u8 = 0x00;
const LEGACY_ESCAPED_ZERO: u8 = 0xFF;
const LEGACY_FRAME_END: u8 = 0x00;
const LEGACY_FRAME_TERMINATOR_WIDTH: usize = 2;
const SYNTHETIC_QUERY_COUNT: usize = 3;
const RESOLUTION_BUDGET: i64 = i64::MAX / 4;

const ADMISSION_ARITIES: [usize; 3] = [1, 65, 4097];
const ADMISSION_QUERY_COUNT: usize = 4;
const ADMISSION_CHANNEL_NAMES: [&str; 3] = ["admission-alpha", "admission-beta", "admission-gamma"];
const ADMISSION_QUERY_NAMES: [&str; ADMISSION_QUERY_COUNT] = [
    "admission-alpha",
    "admission-beta",
    "admission-gamma",
    "admission-absent",
];

const SMALL_RESTING_GROUP: usize = 64;
const LARGE_RESTING_GROUP: usize = 4096;

const DEEP_PATTERN_DEPTH: usize = 20_000;
const DEEP_VIEW_STACK_BYTES: usize = 256 * 1024;

const JOIN_A_PROCESS: &str = r#""join-a""#;
const JOIN_B_PROCESS: &str = r#""join-b""#;
const ABSENT_PROCESS: &str = r#""absent""#;
const PREFIX_JOIN_SOURCE: &str = r#"for (@left <- @"join-a" & @right <- @"join-b") { Nil }"#;
const REPEATED_JOIN_SOURCE: &str = concat!(
    r#"match ["join-a", "join-b", "join-a"] { "#,
    r#"[p, q, r] => for (@x <- @p & @y <- @q & @z <- @r) { Nil } }"#
);
const CONJUNCTION_SOURCE: &str = r#"for (@{whole /\ [head, tail]} <- @"conjunction") { Nil }"#;
const NESTED_CONNECTIVES_SOURCE: &str = concat!(
    r#"for (@{for (@{~1 /\ {2 \/ 3}} <- @"connective-inner") { Nil }} "#,
    r#"<- @"connective-shapes") { Nil }"#
);
const LIST_REMAINDER_SOURCE: &str =
    r#"for (@{[head, second ...tail]} <- @"list-remainder") { Nil }"#;
const RECEIVE_REMAINDER_SOURCE: &str = r#"for (@head ...@rest <- @"receive-remainder") { Nil }"#;
const PEEK_JOIN_SOURCE: &str = r#"for (@left <<- @"peek-left" & @right <<- @"peek-right") { Nil }"#;
const PERSISTENT_SOURCE: &str = r#"for (@value <= @"persistent") { Nil }"#;
const NESTED_QUOTE_SOURCE: &str = r#"for (@{@{inner}!(Nil)} <- @{@{@"deep"!(Nil)}!(Nil)}) { Nil }"#;
const ORDERED_FIRST_SOURCE: &str = r#"for (@1 <- @"ordered") { Nil }"#;
const ORDERED_SECOND_SOURCE: &str = r#"for (@2 <- @"ordered") { Nil }"#;

struct RestingReceive {
    label: &'static str,
    channels: Vec<Par>,
    channel_identities: Vec<Vec<u8>>,
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
                "{}: no datum waits on its channels, so the receive must rest",
                self.label
            );
        };
    }
}

struct ExpectedRecord {
    prepared: PreparedContinuationRecord,
    record_path: Vec<u8>,
    prefix_len: usize,
}

impl ExpectedRecord {
    fn of(receive: &RestingReceive) -> Self {
        let prepared = PreparedContinuationRecord::encode_waiting(&receive.waiting());
        let prefix = legacy_continuation_prefix(&receive.channel_identities);
        let record_path = joined_path(&prefix, prepared.canonical());
        Self {
            prepared,
            record_path,
            prefix_len: prefix.len(),
        }
    }

    fn view(&self) -> OrderedReceiveView<'_> {
        OrderedReceiveView::admit_legacy_v1(
            &self.record_path,
            self.prefix_len,
            self.prepared.field_bounds(),
        )
        .expect("a hand-built legacy record path admits its ordered view")
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ViewIterationTally {
    binds: usize,
    pattern_bytes: usize,
    free_count_total: i64,
    matching_channel_frames: usize,
    bind_effective_peeks: usize,
    record_effective_peeks: usize,
    raw_peeks: usize,
    raw_peek_total: i64,
    position_counts: [usize; ADMISSION_QUERY_COUNT],
    position_totals: [u64; ADMISSION_QUERY_COUNT],
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct RestingVisitTally {
    records: usize,
    single_order_records: usize,
    order_minimum: Option<u64>,
    order_maximum: Option<u64>,
    order_total: u64,
    binds: usize,
    pattern_bytes: usize,
    free_count_total: i64,
    matching_channels: usize,
    effective_peeks: usize,
    raw_peeks: usize,
    persistent_records: usize,
    admission_failures: usize,
}

struct DeepViewProbe<'a> {
    record_path: &'a [u8],
    prefix_len: usize,
    prepared: &'a PreparedContinuationRecord,
    channel_identity: &'a [u8],
    absent_identity: &'a [u8],
    expected_pattern: &'a [u8],
    expected_body: &'a [u8],
}

#[derive(Debug, PartialEq, Eq)]
struct DeepViewObservation {
    arity: usize,
    bind_positions: Vec<u64>,
    pattern_images_match: Vec<bool>,
    free_counts: Vec<Option<i32>>,
    channel_matches: Vec<bool>,
    effective_peeks: Vec<bool>,
    record_effective_peeks: Vec<bool>,
    channel_positions: Vec<u64>,
    absent_positions: Vec<u64>,
    raw_peeks: Vec<i32>,
    body_image_matches: bool,
    persist: bool,
}

impl DeepViewProbe<'_> {
    fn observe(&self) -> DeepViewObservation {
        let view = OrderedReceiveView::admit_legacy_v1(
            self.record_path,
            self.prefix_len,
            self.prepared.field_bounds(),
        )
        .expect("the deep legacy record admits its ordered view");
        let record = view.record();
        let arity = record.arity();
        let mut observation = DeepViewObservation {
            arity,
            bind_positions: Vec::with_capacity(arity),
            pattern_images_match: Vec::with_capacity(arity),
            free_counts: Vec::with_capacity(arity),
            channel_matches: Vec::with_capacity(arity),
            effective_peeks: Vec::with_capacity(arity),
            record_effective_peeks: Vec::with_capacity(arity),
            channel_positions: view
                .positions(ReceiveChannelQuery::LegacyV1(self.channel_identity))
                .expect("a legacy channel query addresses a legacy view")
                .map(|position| position.get())
                .collect(),
            absent_positions: view
                .positions(ReceiveChannelQuery::LegacyV1(self.absent_identity))
                .expect("a legacy channel query addresses a legacy view")
                .map(|position| position.get())
                .collect(),
            raw_peeks: record.raw_peeks().collect(),
            body_image_matches: record.body() == self.expected_body,
            persist: record.persist(),
        };
        for bind in view.binds() {
            observation.bind_positions.push(bind.position().get());
            observation
                .pattern_images_match
                .push(bind.pattern() == self.expected_pattern);
            observation
                .free_counts
                .push(bind.free_count::<BindPattern>());
            observation
                .channel_matches
                .push(bind.channel_matches(self.channel_identity));
            observation.effective_peeks.push(bind.effective_peek());
            observation
                .record_effective_peeks
                .push(record.effective_peek(bind.position()));
        }
        observation
    }
}

fn integer(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn channel(name: &str) -> Par { new_gstring_par(name.to_owned(), Vec::new(), false) }

fn datum(pars: Vec<Par>) -> ListParWithRandom {
    ListParWithRandom {
        pars,
        ..ListParWithRandom::default()
    }
}

fn free_variable_bind() -> BindPattern {
    BindPattern {
        patterns: vec![new_freevar_par(0, Vec::new())],
        remainder: None,
        free_count: 1,
    }
}

fn par_body_continuation(body: Option<Par>, random_state: Vec<u8>) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom { body, random_state })),
        ..TaggedContinuation::default()
    }
}

fn increment(count: usize) -> usize { count.checked_add(1).expect("a tally count fits usize") }

fn checked_sum(total: usize, addend: usize) -> usize {
    total.checked_add(addend).expect("a tally sum fits usize")
}

fn signed_position(index: usize) -> i32 {
    i32::try_from(index).expect("a receive position fits the i32 peek domain")
}

fn legacy_frame_len(raw: &[u8]) -> usize {
    let escaped_zeros = raw.iter().filter(|byte| **byte == LEGACY_ESCAPE).count();
    raw.len()
        .checked_add(escaped_zeros)
        .and_then(|length| length.checked_add(LEGACY_FRAME_TERMINATOR_WIDTH))
        .expect("a framed channel identity length fits usize")
}

fn append_legacy_frame(output: &mut Vec<u8>, raw: &[u8]) {
    for &byte in raw {
        match byte {
            LEGACY_ESCAPE => output.extend_from_slice(&[LEGACY_ESCAPE, LEGACY_ESCAPED_ZERO]),
            other => output.push(other),
        }
    }
    output.extend_from_slice(&[LEGACY_ESCAPE, LEGACY_FRAME_END]);
}

fn legacy_frame(raw: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(legacy_frame_len(raw));
    append_legacy_frame(&mut frame, raw);
    frame
}

fn legacy_continuation_prefix(channel_identities: &[Vec<u8>]) -> Vec<u8> {
    let framed_len = channel_identities
        .iter()
        .map(|identity| legacy_frame_len(identity))
        .fold(0, checked_sum);
    let capacity = LEGACY_CONTINUATION_HEADER
        .len()
        .checked_add(framed_len)
        .and_then(|length| length.checked_add(LEGACY_GROUP_END.len()))
        .expect("a continuation prefix length fits usize");
    let mut prefix = Vec::with_capacity(capacity);
    prefix.extend_from_slice(&LEGACY_CONTINUATION_HEADER);
    for identity in channel_identities {
        append_legacy_frame(&mut prefix, identity);
    }
    prefix.extend_from_slice(&LEGACY_GROUP_END);
    prefix
}

fn joined_path(prefix: &[u8], canonical: &[u8]) -> Vec<u8> {
    let capacity = prefix
        .len()
        .checked_add(canonical.len())
        .expect("a record path length fits usize");
    let mut path = Vec::with_capacity(capacity);
    path.extend_from_slice(prefix);
    path.extend_from_slice(canonical);
    path
}

fn oracle_positions(channel_identities: &[Vec<u8>], query: &[u8]) -> Vec<u64> {
    channel_identities
        .iter()
        .enumerate()
        .filter(|(_, identity)| identity.as_slice() == query)
        .map(|(index, _)| u64::try_from(index).expect("a channel position fits u64"))
        .collect()
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
    let substitution = Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(Cost::create(
            RESOLUTION_BUDGET,
            "ordered receive view resolution",
        ))),
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
    let channel_identities = channels
        .iter()
        .map(|source_channel| source_channel.rho_path_identity())
        .collect();
    let continuation = par_body_continuation(body, label.as_bytes().to_vec());
    RestingReceive {
        label,
        channels,
        channel_identities,
        patterns,
        continuation,
        persistent,
        peeks,
    }
}

fn resolve_compiled(label: &'static str, source: &str, env: &Env<Par>) -> RestingReceive {
    resolve_receive(
        label,
        only_receive(label, &compiled_process(label, source)),
        env,
    )
}

fn bind_channel(receive: &RestingReceive, position: usize) -> &Par {
    match receive.channels.get(position) {
        Some(bound) => bound,
        None => panic!("{}: no channel at bind position {position}", receive.label),
    }
}

fn channel_query_images(receives: &[&RestingReceive], absent: &Par, probe: &Par) -> Vec<Vec<u8>> {
    let present: BTreeSet<Vec<u8>> = receives
        .iter()
        .flat_map(|receive| receive.channel_identities.iter().cloned())
        .collect();
    let probe_identity = probe.rho_path_identity();
    let Some((_, truncated_probe)) = probe_identity.split_last() else {
        panic!("a channel identity is never empty");
    };
    let extended_capacity = probe_identity
        .len()
        .checked_add(1)
        .expect("an extended identity length fits usize");
    let mut extended_probe = Vec::with_capacity(extended_capacity);
    extended_probe.extend_from_slice(&probe_identity);
    extended_probe.push(0x00);
    let image_count = present
        .len()
        .checked_add(SYNTHETIC_QUERY_COUNT)
        .expect("the query image count fits usize");
    let mut images = Vec::with_capacity(image_count);
    images.extend(present);
    images.push(absent.rho_path_identity());
    images.push(truncated_probe.to_vec());
    images.push(extended_probe);
    images
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

async fn produce_resting_datum(space: &RhoSpace, target: &Par, pars: Vec<Par>, label: &str) {
    let produced = space
        .produce(target.clone(), datum(pars), false)
        .await
        .expect("an in-memory produce succeeds");
    let None = produced else {
        panic!("{label}: the datum must wait for the rest of its join");
    };
}

async fn produce_firing_datum(
    space: &RhoSpace,
    target: &Par,
    pars: Vec<Par>,
    fired: &RestingReceive,
) {
    let produced = space
        .produce(target.clone(), datum(pars), false)
        .await
        .expect("an in-memory produce succeeds");
    let Some((result, _, _)) = produced else {
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
}

fn assert_view_semantics(
    member: &RestingReceive,
    view: OrderedReceiveView<'_>,
    query_images: &[Vec<u8>],
) {
    let label = member.label;
    let record = view.record();
    let continuation_image = member.continuation.stable_hash_bytes();
    let expected_peeks: Vec<i32> = member.peeks.iter().copied().collect();
    let observed_peeks: Vec<i32> = record.raw_peeks().collect();
    assert_eq!(
        record.arity(),
        member.patterns.len(),
        "{label}: one record pattern per bind"
    );
    assert_eq!(
        view.binds().len(),
        member.patterns.len(),
        "{label}: one view bind per bind"
    );
    assert!(
        record.body() == continuation_image.as_slice(),
        "{label}: body image"
    );
    assert_eq!(record.persist(), member.persistent, "{label}: persistence");
    assert_eq!(
        observed_peeks, expected_peeks,
        "{label}: raw peeks equal the peek set"
    );
    let expected_binds = member.patterns.iter().zip(&member.channel_identities);
    for (index, (bind, (pattern, identity))) in view.binds().zip(expected_binds).enumerate() {
        let peeked = member.peeks.contains(&signed_position(index));
        let pattern_image = pattern.stable_hash_bytes();
        let channel_frame = legacy_frame(identity);
        assert_eq!(
            bind.position().index(),
            index,
            "{label}: bind {index} position"
        );
        assert!(
            bind.pattern() == pattern_image.as_slice(),
            "{label}: bind {index} pattern"
        );
        assert_eq!(
            bind.free_count::<BindPattern>(),
            Some(pattern.free_count),
            "{label}: bind {index} free count"
        );
        assert!(
            bind.channel_matches(identity),
            "{label}: bind {index} channel identity"
        );
        assert!(
            bind.channel_frame() == channel_frame.as_slice(),
            "{label}: bind {index} frame"
        );
        assert_eq!(
            bind.effective_peek(),
            peeked,
            "{label}: bind {index} effective peek"
        );
        assert_eq!(
            record.effective_peek(bind.position()),
            peeked,
            "{label}: record effective peek at {index}"
        );
        assert_eq!(
            view.bind_at(bind.position()),
            Some(bind),
            "{label}: bind_at {index}"
        );
    }
    for query in query_images {
        let observed: Vec<u64> = view
            .positions(ReceiveChannelQuery::LegacyV1(query))
            .expect("a legacy channel query addresses a legacy view")
            .map(|position| position.get())
            .collect();
        assert_eq!(
            observed,
            oracle_positions(&member.channel_identities, query),
            "{label}: channel positions"
        );
    }
}

fn assert_resting_group(
    store: &RhoStore,
    members: &[&RestingReceive],
    query_images: &[Vec<u8>],
) -> Vec<u64> {
    let Some(group_head) = members.first() else {
        panic!("a resting group names at least one expected receive");
    };
    let expected_records: Vec<ExpectedRecord> = members
        .iter()
        .map(|member| ExpectedRecord::of(member))
        .collect();
    let mut member_orders: Vec<Option<u64>> = vec![None; members.len()];
    let visit =
        store.try_visit_resting_receive_views(&group_head.channels, &mut |orders, admission| {
            let view = match admission {
                Ok(view) => view,
                Err(error) => panic!("{}: a resting record must admit: {error}", group_head.label),
            };
            let Some(member_index) = expected_records
                .iter()
                .position(|expected| expected.view() == view)
            else {
                panic!(
                    "{}: the visited view equals no hand-built record",
                    group_head.label
                );
            };
            let member = members[member_index];
            let Ok([order]) = <[u64; 1]>::try_from(orders) else {
                panic!(
                    "{}: a resting record carries one order, not {orders:?}",
                    member.label
                );
            };
            let None = member_orders[member_index].replace(order) else {
                panic!("{}: the resting record was visited twice", member.label);
            };
            assert_view_semantics(member, view, query_images);
        });
    assert_eq!(
        visit,
        RestingReceiveVisit::Visited(members.len()),
        "{}: one visit per resting record",
        group_head.label
    );
    member_orders
        .into_iter()
        .zip(members)
        .map(|(order, member)| match order {
            Some(order) => order,
            None => panic!("{}: the resting record was never visited", member.label),
        })
        .collect()
}

fn tally_view_iteration(
    view: OrderedReceiveView<'_>,
    pool_identities: &[Vec<u8>],
    query_images: &[Vec<u8>; ADMISSION_QUERY_COUNT],
) -> ViewIterationTally {
    let mut tally = ViewIterationTally::default();
    let record = view.record();
    for bind in view.binds() {
        let pool_slot = bind
            .position()
            .index()
            .checked_rem(pool_identities.len())
            .expect("the admission channel pool is not empty");
        let Some(free_count) = bind.free_count::<BindPattern>() else {
            panic!("every admitted bind pattern ends in its free count");
        };
        tally.binds = increment(tally.binds);
        tally.pattern_bytes = checked_sum(tally.pattern_bytes, bind.pattern().len());
        tally.free_count_total = tally
            .free_count_total
            .checked_add(i64::from(free_count))
            .expect("the free-count total fits i64");
        tally.matching_channel_frames = checked_sum(
            tally.matching_channel_frames,
            usize::from(bind.channel_matches(&pool_identities[pool_slot])),
        );
        tally.bind_effective_peeks = checked_sum(
            tally.bind_effective_peeks,
            usize::from(bind.effective_peek()),
        );
        tally.record_effective_peeks = checked_sum(
            tally.record_effective_peeks,
            usize::from(record.effective_peek(bind.position())),
        );
    }
    for peek in record.raw_peeks() {
        tally.raw_peeks = increment(tally.raw_peeks);
        tally.raw_peek_total = tally
            .raw_peek_total
            .checked_add(i64::from(peek))
            .expect("the peek total fits i64");
    }
    let position_tallies = tally
        .position_counts
        .iter_mut()
        .zip(tally.position_totals.iter_mut());
    for ((count, total), query) in position_tallies.zip(query_images) {
        let positions = view
            .positions(ReceiveChannelQuery::LegacyV1(query))
            .expect("a legacy channel query addresses a legacy view");
        for position in positions {
            *count = increment(*count);
            *total = total
                .checked_add(position.get())
                .expect("a position total fits u64");
        }
    }
    tally
}

fn expected_view_iteration(
    patterns: &[BindPattern],
    channel_identities: &[Vec<u8>],
    peeks: &BTreeSet<i32>,
    query_images: &[Vec<u8>; ADMISSION_QUERY_COUNT],
) -> ViewIterationTally {
    let mut expected = ViewIterationTally {
        binds: patterns.len(),
        pattern_bytes: patterns
            .iter()
            .map(|pattern| pattern.stable_hash_bytes().len())
            .fold(0, checked_sum),
        free_count_total: patterns
            .iter()
            .map(|pattern| i64::from(pattern.free_count))
            .try_fold(0i64, i64::checked_add)
            .expect("the free-count total fits i64"),
        matching_channel_frames: channel_identities.len(),
        bind_effective_peeks: peeks.len(),
        record_effective_peeks: peeks.len(),
        raw_peeks: peeks.len(),
        raw_peek_total: peeks
            .iter()
            .map(|&peek| i64::from(peek))
            .try_fold(0i64, i64::checked_add)
            .expect("the peek total fits i64"),
        ..ViewIterationTally::default()
    };
    let position_tallies = expected
        .position_counts
        .iter_mut()
        .zip(expected.position_totals.iter_mut());
    for ((count, total), query) in position_tallies.zip(query_images) {
        let positions = oracle_positions(channel_identities, query);
        *count = positions.len();
        *total = positions
            .iter()
            .try_fold(0u64, |sum, &position| sum.checked_add(position))
            .expect("a position total fits u64");
    }
    expected
}

async fn resting_group_space(channels: &[Par], group_size: usize) -> RhoSpace {
    let space = in_memory_space().await;
    let store = space.get_store();
    let channel_group = channels.to_vec();
    let patterns = vec![free_variable_bind()];
    for index in 0..group_size {
        let random_state = u64::try_from(index)
            .expect("a resting continuation index fits u64")
            .to_le_bytes()
            .to_vec();
        let continuation = par_body_continuation(Some(Par::default()), random_state);
        let waiting = WaitingContinuation::create(
            &channel_group,
            &patterns,
            &continuation,
            false,
            BTreeSet::new(),
        );
        let Some(true) = store.put_continuation(channels, waiting) else {
            panic!("resting continuation {index} must be a new record of its group");
        };
    }
    space
}

fn tally_resting_visit(
    store: &RhoStore,
    channels: &[Par],
    channel_identity: &[u8],
) -> (RestingReceiveVisit, RestingVisitTally) {
    let mut tally = RestingVisitTally::default();
    let visit = store.try_visit_resting_receive_views(channels, &mut |orders, admission| {
        tally.records = increment(tally.records);
        if let [order] = orders {
            tally.single_order_records = increment(tally.single_order_records);
            tally.order_minimum = Some(match tally.order_minimum {
                Some(minimum) => minimum.min(*order),
                None => *order,
            });
            tally.order_maximum = Some(match tally.order_maximum {
                Some(maximum) => maximum.max(*order),
                None => *order,
            });
            tally.order_total = tally
                .order_total
                .checked_add(*order)
                .expect("an order total fits u64");
        }
        match admission {
            Ok(view) => {
                let record = view.record();
                tally.persistent_records =
                    checked_sum(tally.persistent_records, usize::from(record.persist()));
                for bind in view.binds() {
                    let Some(free_count) = bind.free_count::<BindPattern>() else {
                        panic!("every resting bind pattern ends in its free count");
                    };
                    tally.binds = increment(tally.binds);
                    tally.pattern_bytes = checked_sum(tally.pattern_bytes, bind.pattern().len());
                    tally.free_count_total = tally
                        .free_count_total
                        .checked_add(i64::from(free_count))
                        .expect("the free-count total fits i64");
                    tally.matching_channels = checked_sum(
                        tally.matching_channels,
                        usize::from(bind.channel_matches(channel_identity)),
                    );
                    tally.effective_peeks =
                        checked_sum(tally.effective_peeks, usize::from(bind.effective_peek()));
                }
                for _ in record.raw_peeks() {
                    tally.raw_peeks = increment(tally.raw_peeks);
                }
            }
            Err(_) => tally.admission_failures = increment(tally.admission_failures),
        }
    });
    (visit, tally)
}

fn expected_resting_visit(
    group_size: usize,
    pattern_image_len: usize,
) -> (RestingReceiveVisit, RestingVisitTally) {
    let largest_order = u64::try_from(group_size).expect("a group size fits u64");
    let order_total = largest_order
        .checked_add(1)
        .and_then(|successor| successor.checked_mul(largest_order))
        .and_then(|product| product.checked_div(2))
        .expect("the order total fits u64");
    (
        RestingReceiveVisit::Visited(group_size),
        RestingVisitTally {
            records: group_size,
            single_order_records: group_size,
            order_minimum: Some(1),
            order_maximum: Some(largest_order),
            order_total,
            binds: group_size,
            pattern_bytes: group_size
                .checked_mul(pattern_image_len)
                .expect("the pattern byte total fits usize"),
            free_count_total: i64::try_from(group_size).expect("a group size fits i64"),
            matching_channels: group_size,
            effective_peeks: 0,
            raw_peeks: 0,
            persistent_records: 0,
            admission_failures: 0,
        },
    )
}

fn deep_list_pattern(depth: usize) -> Par {
    let mut arena = ParConstruction::new();
    let mut planned = arena
        .import_par(&new_freevar_par(0, Vec::new()))
        .expect("plan the free-variable leaf");
    let shell = Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![Par::default()],
            locally_free: Vec::new(),
            connective_used: true,
            remainder: None,
        })),
    };
    for _ in 0..depth {
        let component = arena
            .expr_component(&shell, &[planned], false)
            .expect("plan one list-pattern level");
        planned = arena
            .singleton_component(component, Vec::new(), true)
            .expect("plan one list-pattern Par");
    }
    arena.finish(planned).expect("finish the deep list pattern")
}

#[tokio::test(flavor = "current_thread")]
async fn compiled_rholang_receives_admit_exact_ordered_views() {
    let join_a = compiled_process("join-a name", JOIN_A_PROCESS);
    let join_b = compiled_process("join-b name", JOIN_B_PROCESS);
    let absent = compiled_process("absent name", ABSENT_PROCESS);
    let empty_env: Env<Par> = Env::new();
    let repeated_env = Env::new()
        .put(join_a.clone())
        .put(join_b.clone())
        .put(join_a.clone());
    let repeated_case_body = only_match_case_body(
        "repeated-join",
        &compiled_process("repeated-join", REPEATED_JOIN_SOURCE),
    );

    let prefix_join = resolve_compiled("prefix-join", PREFIX_JOIN_SOURCE, &empty_env);
    let repeated_join = resolve_receive(
        "repeated-join",
        only_receive("repeated-join", &repeated_case_body),
        &repeated_env,
    );
    let conjunction = resolve_compiled("conjunction", CONJUNCTION_SOURCE, &empty_env);
    let nested_connectives =
        resolve_compiled("nested-connectives", NESTED_CONNECTIVES_SOURCE, &empty_env);
    let list_remainder = resolve_compiled("list-remainder", LIST_REMAINDER_SOURCE, &empty_env);
    let receive_remainder =
        resolve_compiled("receive-remainder", RECEIVE_REMAINDER_SOURCE, &empty_env);
    let peek_join = resolve_compiled("peek-join", PEEK_JOIN_SOURCE, &empty_env);
    let persistent = resolve_compiled("persistent", PERSISTENT_SOURCE, &empty_env);
    let nested_quote = resolve_compiled("nested-quote", NESTED_QUOTE_SOURCE, &empty_env);
    let ordered_first = resolve_compiled("ordered-first", ORDERED_FIRST_SOURCE, &empty_env);
    let ordered_second = resolve_compiled("ordered-second", ORDERED_SECOND_SOURCE, &empty_env);

    assert_eq!(
        prefix_join.channels,
        vec![join_a.clone(), join_b.clone()],
        "the prefix join orders its binds by source score"
    );
    assert_eq!(
        repeated_join.channels,
        vec![join_a.clone(), join_b.clone(), join_a.clone()],
        "the repeated join resolves to the group [a, b, a], extending the prefix join's group"
    );
    assert_eq!(
        peek_join.peeks,
        BTreeSet::from([0, 1]),
        "a peek join peeks at every position"
    );
    assert!(persistent.persistent, "a <= receive is persistent");
    let [BindPattern {
        remainder: Some(_), ..
    }] = receive_remainder.patterns.as_slice()
    else {
        panic!("the receive remainder compiles to one bind carrying a remainder");
    };

    let every_receive = [
        &prefix_join,
        &repeated_join,
        &conjunction,
        &nested_connectives,
        &list_remainder,
        &receive_remainder,
        &peek_join,
        &persistent,
        &nested_quote,
        &ordered_first,
        &ordered_second,
    ];
    let single_record_receives = [
        &prefix_join,
        &repeated_join,
        &conjunction,
        &nested_connectives,
        &list_remainder,
        &receive_remainder,
        &peek_join,
        &persistent,
        &nested_quote,
    ];
    let ordered_group = [&ordered_first, &ordered_second];
    let query_images = channel_query_images(&every_receive, &absent, &join_a);

    let space = in_memory_space().await;
    for receive in every_receive {
        receive.consume_into(&space).await;
    }

    let store = space.get_store();
    let mut absent_visits = 0usize;
    let absent_visit =
        store.try_visit_resting_receive_views(std::slice::from_ref(&absent), &mut |_, _| {
            absent_visits = increment(absent_visits);
        });
    assert_eq!(
        absent_visit,
        RestingReceiveVisit::Unloaded,
        "an untouched group is unloaded"
    );
    assert_eq!(absent_visits, 0, "an unloaded group visits nothing");
    for receive in single_record_receives {
        assert_resting_group(&store, &[receive], &query_images);
        assert_eq!(
            store.get_continuations(&receive.channels),
            vec![receive.waiting()],
            "{}: the hot store lists exactly the consumed continuation",
            receive.label
        );
    }
    let ordered_orders = assert_resting_group(&store, &ordered_group, &query_images);
    let Ok([first_order, second_order]) = <[u64; 2]>::try_from(ordered_orders.as_slice()) else {
        panic!("the ordered group holds two resting records");
    };
    assert!(
        first_order < second_order,
        "the later consume carries the larger order"
    );
    assert_eq!(
        store.get_continuations(&ordered_first.channels),
        vec![ordered_second.waiting(), ordered_first.waiting()],
        "get_continuations lists the newest resting continuation first"
    );

    produce_resting_datum(&space, &join_a, vec![integer(1)], prefix_join.label).await;
    produce_firing_datum(&space, &join_b, vec![integer(2)], &prefix_join).await;
    produce_resting_datum(&space, &join_a, vec![integer(3)], repeated_join.label).await;
    produce_resting_datum(&space, &join_a, vec![integer(4)], repeated_join.label).await;
    produce_firing_datum(&space, &join_b, vec![integer(5)], &repeated_join).await;
    let peek_left = bind_channel(&peek_join, 0);
    let peek_right = bind_channel(&peek_join, 1);
    produce_resting_datum(&space, peek_left, vec![integer(6)], peek_join.label).await;
    produce_firing_datum(&space, peek_right, vec![integer(7)], &peek_join).await;
    let remainder_pars = vec![integer(8), integer(9), integer(10)];
    let remainder_channel = bind_channel(&receive_remainder, 0);
    produce_firing_datum(
        &space,
        remainder_channel,
        remainder_pars,
        &receive_remainder,
    )
    .await;
    let persistent_channel = bind_channel(&persistent, 0);
    produce_firing_datum(&space, persistent_channel, vec![integer(11)], &persistent).await;

    let store = space.get_store();
    for fired in [&prefix_join, &repeated_join, &peek_join, &receive_remainder] {
        let mut fired_visits = 0usize;
        let visit = store.try_visit_resting_receive_views(&fired.channels, &mut |_, _| {
            fired_visits = increment(fired_visits);
        });
        assert_eq!(
            visit,
            RestingReceiveVisit::Visited(0),
            "{}: a fired non-persistent group stays loaded and empty",
            fired.label
        );
        assert_eq!(
            fired_visits, 0,
            "{}: a fired continuation is not visited",
            fired.label
        );
        assert!(
            store.get_continuations(&fired.channels).is_empty(),
            "{}: a fired non-persistent continuation leaves the hot store",
            fired.label
        );
    }
    for resting in [
        &persistent,
        &conjunction,
        &nested_connectives,
        &list_remainder,
        &nested_quote,
    ] {
        assert_resting_group(&store, &[resting], &query_images);
        assert_eq!(
            store.get_continuations(&resting.channels),
            vec![resting.waiting()],
            "{}: a persistent or unfired continuation remains",
            resting.label
        );
    }
    assert_eq!(
        assert_resting_group(&store, &ordered_group, &query_images),
        ordered_orders,
        "the unfired ordered group keeps its orders"
    );
}

#[test]
fn view_admission_and_iteration_do_not_allocate() {
    let channel_pool = ADMISSION_CHANNEL_NAMES.map(channel);
    let pool_identities = ADMISSION_CHANNEL_NAMES.map(|name| channel(name).rho_path_identity());
    let query_images = ADMISSION_QUERY_NAMES.map(|name| channel(name).rho_path_identity());
    for arity in ADMISSION_ARITIES {
        let pool_slots: Vec<usize> = (0..arity)
            .map(|index| {
                index
                    .checked_rem(channel_pool.len())
                    .expect("the admission channel pool is not empty")
            })
            .collect();
        let channels: Vec<Par> = pool_slots
            .iter()
            .map(|&slot| channel_pool[slot].clone())
            .collect();
        let channel_identities: Vec<Vec<u8>> = pool_slots
            .iter()
            .map(|&slot| pool_identities[slot].clone())
            .collect();
        let patterns: Vec<BindPattern> = (0..arity).map(|_| free_variable_bind()).collect();
        let peeks: BTreeSet<i32> = (0..arity).step_by(2).map(signed_position).collect();
        let continuation = par_body_continuation(Some(Par::default()), b"admission-view".to_vec());
        let waiting =
            WaitingContinuation::create(&channels, &patterns, &continuation, false, peeks.clone());
        let prepared = PreparedContinuationRecord::encode_waiting(&waiting);
        let prefix = legacy_continuation_prefix(&channel_identities);
        let record_path = joined_path(&prefix, prepared.canonical());
        let prefix_len = prefix.len();
        let bounds = prepared.field_bounds();
        let expected =
            expected_view_iteration(&patterns, &channel_identities, &peeks, &query_images);
        let (tally, allocations, bytes) = counting_alloc::measure(|| {
            let view = OrderedReceiveView::admit_legacy_v1(&record_path, prefix_len, bounds)
                .expect("a prepared legacy record admits its ordered view");
            tally_view_iteration(view, &pool_identities, &query_images)
        });
        assert_eq!(
            allocations, 0,
            "arity {arity}: admission and iteration allocated {allocations} times ({bytes} bytes)"
        );
        assert_eq!(
            tally, expected,
            "arity {arity}: the iterated view disagrees with the oracle"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn resting_visit_allocations_do_not_grow_with_group_size() {
    let channels = vec![channel("resting-visit-group")];
    let channel_identity = channels
        .first()
        .expect("the resting group has one channel")
        .rho_path_identity();
    let pattern_image_len = free_variable_bind().stable_hash_bytes().len();
    let small_space = resting_group_space(&channels, SMALL_RESTING_GROUP).await;
    let large_space = resting_group_space(&channels, LARGE_RESTING_GROUP).await;
    let small_store = small_space.get_store();
    let large_store = large_space.get_store();
    let small_warm_up = tally_resting_visit(&small_store, &channels, &channel_identity);
    let large_warm_up = tally_resting_visit(&large_store, &channels, &channel_identity);
    let (small_visit, small_allocations, small_bytes) =
        counting_alloc::measure(|| tally_resting_visit(&small_store, &channels, &channel_identity));
    let (large_visit, large_allocations, large_bytes) =
        counting_alloc::measure(|| tally_resting_visit(&large_store, &channels, &channel_identity));
    assert_eq!(
        small_visit, small_warm_up,
        "the measured small visit repeats its warm-up"
    );
    assert_eq!(
        large_visit, large_warm_up,
        "the measured large visit repeats its warm-up"
    );
    assert_eq!(
        small_visit,
        expected_resting_visit(SMALL_RESTING_GROUP, pattern_image_len),
        "the small visit admits and iterates every resting view"
    );
    assert_eq!(
        large_visit,
        expected_resting_visit(LARGE_RESTING_GROUP, pattern_image_len),
        "the large visit admits and iterates every resting view"
    );
    assert_eq!(
        large_allocations, small_allocations,
        "a visit of {LARGE_RESTING_GROUP} views allocated {large_allocations} times \
         ({large_bytes} bytes); a visit of {SMALL_RESTING_GROUP} views allocated \
         {small_allocations} times ({small_bytes} bytes)"
    );
}

#[test]
fn deep_pattern_views_fit_a_256_kib_stack() {
    let channels = vec![channel("deep-pattern-view")];
    let channel_identity = channels
        .first()
        .expect("the deep group has one channel")
        .rho_path_identity();
    let absent_identity = channel("deep-pattern-absent").rho_path_identity();
    let deep_bind = BindPattern {
        patterns: vec![deep_list_pattern(DEEP_PATTERN_DEPTH)],
        remainder: None,
        free_count: 1,
    };
    let expected_pattern = deep_bind.stable_hash_bytes();
    let patterns = vec![deep_bind];
    let continuation = par_body_continuation(Some(Par::default()), b"deep-pattern-view".to_vec());
    let expected_body = continuation.stable_hash_bytes();
    let waiting = WaitingContinuation::create(
        &channels,
        &patterns,
        &continuation,
        false,
        BTreeSet::from([0]),
    );
    let prepared = PreparedContinuationRecord::encode_waiting(&waiting);
    let prefix = legacy_continuation_prefix(std::slice::from_ref(&channel_identity));
    let record_path = joined_path(&prefix, prepared.canonical());
    let probe = DeepViewProbe {
        record_path: &record_path,
        prefix_len: prefix.len(),
        prepared: &prepared,
        channel_identity: &channel_identity,
        absent_identity: &absent_identity,
        expected_pattern: &expected_pattern,
        expected_body: &expected_body,
    };
    let observation = std::thread::scope(|scope| {
        let viewer = std::thread::Builder::new()
            .stack_size(DEEP_VIEW_STACK_BYTES)
            .spawn_scoped(scope, || probe.observe())
            .expect("spawn the 256 KiB view thread");
        match viewer.join() {
            Ok(observation) => observation,
            Err(_) => panic!("admitting and iterating the deep view must fit a 256 KiB stack"),
        }
    });
    assert_eq!(observation, DeepViewObservation {
        arity: 1,
        bind_positions: vec![0],
        pattern_images_match: vec![true],
        free_counts: vec![Some(1)],
        channel_matches: vec![true],
        effective_peeks: vec![true],
        record_effective_peeks: vec![true],
        channel_positions: vec![0],
        absent_positions: Vec::new(),
        raw_peeks: vec![0],
        body_image_matches: true,
        persist: false,
    });
    let WaitingContinuation {
        patterns: waiting_patterns,
        ..
    } = waiting;
    let waiting_patterns = match Arc::try_unwrap(waiting_patterns) {
        Ok(owned) => owned,
        Err(_) => panic!("the waiting continuation solely owns its deep patterns"),
    };
    dismantle_all(
        patterns
            .into_iter()
            .chain(waiting_patterns)
            .flat_map(|bind_pattern| bind_pattern.patterns),
    );
}
