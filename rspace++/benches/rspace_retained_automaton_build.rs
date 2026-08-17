//! Causal rebuild benchmark for retained residual-query sharing.
//!
//! The control freezes the retained representation that production kept after
//! experiment 241: every terminal owns its complete `RhoPathQuery`. The local
//! treatment reproduces the rejected fixed-width `QueryId` plus hash-consed
//! query arena exactly, without leaving that slower representation on the
//! production path. Both arms construct the same `PathMap<TerminalId>` chain
//! in candidate order and preflight to the same selection vector. Distinct
//! continuation bodies make the repeated receive shape a valid live RSpace
//! workload rather than duplicate continuations that the hot store would
//! reject.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use pathmap::PathMap;
use pathmap::zipper::{ZipperMoving, ZipperReadOnlyValues};
use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::retained_set_automaton::RetainedSetAutomaton;
use rspace_plus_plus::rspace::rho_path::{
    RhoPathQuery, RhoPathQueryAtom, RhoPathV1, encode_structural_sequence,
};

#[derive(Clone, Default)]
struct OrderedPattern {
    expected: Vec<u8>,
    first: Vec<u8>,
}

struct OrderedMatcher;

impl Match<OrderedPattern, Vec<u8>, u64> for OrderedMatcher {
    fn get(&self, pattern: &OrderedPattern, datum: &Vec<u8>) -> Option<Vec<u8>> {
        (pattern.expected == *datum).then(|| datum.clone())
    }

    fn retained_datum_path(&self, datum: &Vec<u8>) -> Option<Vec<u8>> { Some(datum.clone()) }

    fn rho_path_query(&self, pattern: &OrderedPattern) -> Option<RhoPathQuery> {
        Some(RhoPathQuery::structural_sequence(
            [RhoPathQueryAtom::Exact(pattern.first.clone()), RhoPathQueryAtom::Wildcard],
            false,
        ))
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool { true }
}

fn waiting(index: usize, pattern: OrderedPattern) -> WaitingContinuation<OrderedPattern, u64> {
    WaitingContinuation {
        patterns: Arc::new(vec![pattern]),
        continuation: Arc::new(u64::try_from(index).expect("benchmark index fits u64")),
        ..WaitingContinuation::default()
    }
}

struct ControlTerminal {
    candidate: usize,
    previous: Option<u64>,
    query: RhoPathQuery,
}

struct UninternedAutomaton {
    segment: PathMap<u64>,
    terminals: Vec<ControlTerminal>,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct QueryId(NonZeroU64);

impl QueryId {
    fn from_index(index: usize) -> Self {
        let ordinal = u64::try_from(index)
            .expect("a Rust Vec query index always fits u64")
            .checked_add(1)
            .expect("a benchmark query arena cannot exceed u64::MAX entries");
        Self(NonZeroU64::new(ordinal).expect("a one-based query identifier is nonzero"))
    }

    fn index(self) -> usize {
        usize::try_from(self.0.get() - 1).expect("a benchmark query identifier fits usize")
    }
}

struct ArenaTerminal {
    candidate: usize,
    previous: Option<u64>,
    query: QueryId,
}

struct QueryArenaAutomaton {
    segment: PathMap<u64>,
    terminals: Vec<ArenaTerminal>,
    queries: Vec<RhoPathQuery>,
}

impl QueryArenaAutomaton {
    fn compile(candidates: &[WaitingContinuation<OrderedPattern, u64>]) -> Self {
        let mut segment = PathMap::new();
        let mut terminals = Vec::with_capacity(candidates.len());
        let mut queries = Vec::new();
        let mut buckets = HashMap::<u64, Vec<QueryId>>::new();
        for (candidate, waiting) in candidates.iter().enumerate() {
            let query = OrderedMatcher
                .rho_path_query(&waiting.patterns[0])
                .filter(RhoPathQuery::is_selective)
                .expect("benchmark patterns are selective");
            let path = RhoPathV1::relative_datum_structure_prefix(&query.retained_prefix());
            let query = intern_query(query, &mut queries, &mut buckets);
            let terminal_id = u64::try_from(terminals.len())
                .expect("terminal index fits u64")
                .checked_add(1)
                .expect("terminal identifier is one based");
            terminals.push(ArenaTerminal {
                candidate,
                previous: segment.get(&path).copied(),
                query,
            });
            segment.insert(&path, terminal_id);
        }
        Self {
            segment,
            terminals,
            queries,
        }
    }

    fn matching_candidates(&self, datum: &[u8]) -> Vec<bool> {
        let mut selected = vec![false; self.terminals.len()];
        let mut zipper = self.segment.read_zipper();
        self.mark(zipper.get_val().copied(), datum, &mut selected);
        for byte in RhoPathV1::relative_datum_structure_prefix(datum) {
            if !zipper.descend_to_existing_byte(byte) {
                break;
            }
            self.mark(zipper.get_val().copied(), datum, &mut selected);
        }
        selected
    }

    fn mark(&self, mut terminal: Option<u64>, datum: &[u8], selected: &mut [bool]) {
        while let Some(id) = terminal {
            let index = usize::try_from(id - 1).expect("compiled terminal fits usize");
            let node = &self.terminals[index];
            if self.queries[node.query.index()].matches_structure(datum) {
                selected[node.candidate] = true;
            }
            terminal = node.previous;
        }
    }
}

fn intern_query(
    query: RhoPathQuery,
    queries: &mut Vec<RhoPathQuery>,
    buckets: &mut HashMap<u64, Vec<QueryId>>,
) -> QueryId {
    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    let digest = hasher.finish();
    if let Some(existing) = buckets.get(&digest).and_then(|ids| {
        ids.iter()
            .copied()
            .find(|id| queries.get(id.index()) == Some(&query))
    }) {
        return existing;
    }

    let id = QueryId::from_index(queries.len());
    queries.push(query);
    buckets.entry(digest).or_default().push(id);
    id
}

impl UninternedAutomaton {
    fn compile(candidates: &[WaitingContinuation<OrderedPattern, u64>]) -> Self {
        let mut segment = PathMap::new();
        let mut terminals = Vec::with_capacity(candidates.len());
        for (candidate, waiting) in candidates.iter().enumerate() {
            let query = OrderedMatcher
                .rho_path_query(&waiting.patterns[0])
                .filter(RhoPathQuery::is_selective)
                .expect("benchmark patterns are selective");
            let path = RhoPathV1::relative_datum_structure_prefix(&query.retained_prefix());
            let terminal_id = u64::try_from(terminals.len())
                .expect("terminal index fits u64")
                .checked_add(1)
                .expect("terminal identifier is one based");
            terminals.push(ControlTerminal {
                candidate,
                previous: segment.get(&path).copied(),
                query,
            });
            segment.insert(&path, terminal_id);
        }
        Self { segment, terminals }
    }

    fn matching_candidates(&self, datum: &[u8]) -> Vec<bool> {
        let mut selected = vec![false; self.terminals.len()];
        let mut zipper = self.segment.read_zipper();
        self.mark(zipper.get_val().copied(), datum, &mut selected);
        for byte in RhoPathV1::relative_datum_structure_prefix(datum) {
            if !zipper.descend_to_existing_byte(byte) {
                break;
            }
            self.mark(zipper.get_val().copied(), datum, &mut selected);
        }
        selected
    }

    fn mark(&self, mut terminal: Option<u64>, datum: &[u8], selected: &mut [bool]) {
        while let Some(id) = terminal {
            let index = usize::try_from(id - 1).expect("compiled terminal fits usize");
            let node = &self.terminals[index];
            if node.query.matches_structure(datum) {
                selected[node.candidate] = true;
            }
            terminal = node.previous;
        }
    }
}

struct Fixture {
    candidates: Vec<WaitingContinuation<OrderedPattern, u64>>,
    datum: Vec<u8>,
}

impl Fixture {
    fn new(cardinality: usize) -> Self {
        let first = b"shared-retained-query".to_vec();
        let datum = encode_structural_sequence([first.as_slice(), b"tail".as_slice()]);
        let pattern = OrderedPattern {
            expected: datum.clone(),
            first,
        };
        let candidates = (0..cardinality)
            .map(|index| waiting(index, pattern.clone()))
            .collect::<Vec<_>>();
        let fixture = Self { candidates, datum };

        let control = UninternedAutomaton::compile(&fixture.candidates);
        let treatment = QueryArenaAutomaton::compile(&fixture.candidates);
        let production = RetainedSetAutomaton::compile(&OrderedMatcher, &fixture.candidates);
        assert_eq!(
            control.matching_candidates(&fixture.datum),
            treatment.matching_candidates(&fixture.datum)
        );
        assert_eq!(
            control.matching_candidates(&fixture.datum),
            production.matching_candidates(&fixture.datum, &[0])
        );
        assert_eq!(treatment.queries.len(), 1);
        fixture
    }
}

fn bench_retained_automaton_build(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("retained_automaton_build_shared_query");
    for cardinality in [16usize, 256, 4_096] {
        let fixture = Fixture::new(cardinality);
        group.throughput(Throughput::Elements(cardinality as u64));
        group.bench_function(BenchmarkId::new("control_owned_query", cardinality), |bencher| {
            bencher.iter(|| black_box(UninternedAutomaton::compile(black_box(&fixture.candidates))))
        });
        group.bench_function(BenchmarkId::new("treatment_query_arena", cardinality), |bencher| {
            bencher.iter(|| black_box(QueryArenaAutomaton::compile(black_box(&fixture.candidates))))
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(82)
        .confidence_level(0.99)
        .significance_level(0.01)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(5));
    targets = bench_retained_automaton_build
}
criterion_main!(benches);
