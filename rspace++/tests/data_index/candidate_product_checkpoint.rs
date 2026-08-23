//! Test-only differential and anti-vacuity gates for candidate suffix
//! checkpoints.
//!
//! Kept outside `src/` so the frozen legacy zipper-clone oracle cannot be
//! mistaken for a production implementation.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::*;
use crate::rspace::rho_path::{
    RHO_MATCH_FIELD_END, RHO_MATCH_FIELD_START, RHO_MATCH_I64, RHO_MATCH_NODE_END,
    RHO_MATCH_NODE_START, RHO_MATCH_PATH_VERSION, RhoMatchQuery, RhoMatchQueryPart,
    RhoPathCaptureId, RhoPathQueryAtom, encode_structural_sequence,
};

#[derive(Default)]
struct LegacyCloneMetrics {
    zipper_pairs_cloned: usize,
    total_prefix_bytes_cloned: usize,
    maximum_prefix_bytes_cloned: usize,
    visited_frames: usize,
}

impl LegacyCloneMetrics {
    fn record_clone(&mut self, cursor: &CandidateProductCursor<'_>) {
        let prefix_bytes = cursor.availability.path().len() + cursor.positions.path().len();
        self.zipper_pairs_cloned += 1;
        self.total_prefix_bytes_cloned += prefix_bytes;
        self.maximum_prefix_bytes_cloned = self.maximum_prefix_bytes_cloned.max(prefix_bytes);
    }
}

struct LegacyState<'a> {
    cursor: CandidateProductCursor<'a>,
    atom_index: usize,
    captures: PathMap<Arc<[u8]>>,
}

fn legacy_visit_sequence_frames<'a>(
    cursor: CandidateProductCursor<'a>,
    metrics: &mut LegacyCloneMetrics,
    mut visitor: impl FnMut(CandidateProductCursor<'a>, &[u8]),
) {
    legacy_visit_sequence_frames_with_prefix(cursor, &[], metrics, &mut visitor);
}

fn legacy_visit_sequence_frames_with_prefix<'a>(
    mut cursor: CandidateProductCursor<'a>,
    prefix: &[u8],
    metrics: &mut LegacyCloneMetrics,
    mut visitor: impl FnMut(CandidateProductCursor<'a>, &[u8]),
) {
    if !cursor.descend_to_existing(prefix) {
        return;
    }
    let mut scanner = StoredSequenceFrameScanner::default();
    for byte in prefix {
        if scanner.feed(*byte) != StoredSequenceFrameStep::NeedMore {
            return;
        }
    }
    let mut levels: Vec<(ByteMaskIter, StoredSequenceFrameScanner)> =
        vec![(cursor.child_mask().iter(), scanner)];
    let mut frame = prefix.to_vec();
    let base_len = frame.len();
    while let Some((children, scanner_base)) = levels.last_mut() {
        match children.next() {
            Some(byte) => {
                let mut scanner = *scanner_base;
                match scanner.feed(byte) {
                    StoredSequenceFrameStep::NeedMore => {
                        cursor.descend_to_byte(byte);
                        frame.push(byte);
                        levels.push((cursor.child_mask().iter(), scanner));
                    }
                    StoredSequenceFrameStep::Complete => {
                        cursor.descend_to_byte(byte);
                        frame.push(byte);
                        metrics.visited_frames += 1;
                        metrics.record_clone(&cursor);
                        visitor(cursor.clone(), &frame);
                        frame.pop();
                        cursor.ascend_byte();
                    }
                    StoredSequenceFrameStep::Invalid => {}
                }
            }
            None => {
                levels.pop();
                if frame.len() > base_len {
                    frame.pop();
                    cursor.ascend_byte();
                }
            }
        }
    }
}

fn legacy_positions_matching_query(
    availability: &PathMap<Multiplicity>,
    positions: &PathMap<CandidatePosition>,
    query: &RhoPathQuery,
) -> (Vec<usize>, LegacyCloneMetrics) {
    let mut metrics = LegacyCloneMetrics::default();
    let mut result = Vec::new();
    let mut root = CandidateProductCursor::new(availability, positions);
    if !root.descend_to_existing(&[1]) {
        return (result, metrics);
    }
    let mut work = vec![LegacyState {
        cursor: root,
        atom_index: 0,
        captures: PathMap::new(),
    }];
    while let Some(state) = work.pop() {
        if let Some(atom) = query.atoms().get(state.atom_index) {
            match atom {
                CompiledRhoPathQueryAtom::Exact { stored_frame, .. } => {
                    let mut cursor = state.cursor;
                    if cursor.descend_to_existing(stored_frame) {
                        work.push(LegacyState {
                            cursor,
                            atom_index: state.atom_index + 1,
                            captures: state.captures,
                        });
                    }
                }
                CompiledRhoPathQueryAtom::Capture(id) => {
                    let capture_path = id.path();
                    if let Some(frame) = state.captures.get(capture_path) {
                        let mut cursor = state.cursor;
                        if cursor.descend_to_existing(frame.as_ref()) {
                            work.push(LegacyState {
                                cursor,
                                atom_index: state.atom_index + 1,
                                captures: state.captures,
                            });
                        }
                    } else {
                        legacy_visit_sequence_frames(
                            state.cursor,
                            &mut metrics,
                            |cursor, frame| {
                                let mut captures = state.captures.clone();
                                captures.insert(capture_path, Arc::from(frame));
                                work.push(LegacyState {
                                    cursor,
                                    atom_index: state.atom_index + 1,
                                    captures,
                                });
                            },
                        );
                    }
                }
                CompiledRhoPathQueryAtom::Wildcard => {
                    legacy_visit_sequence_frames(state.cursor, &mut metrics, |cursor, _| {
                        work.push(LegacyState {
                            cursor,
                            atom_index: state.atom_index + 1,
                            captures: state.captures.clone(),
                        });
                    });
                }
                CompiledRhoPathQueryAtom::Match(query) => {
                    legacy_visit_sequence_frames_with_prefix(
                        state.cursor,
                        query.product_prefix(),
                        &mut metrics,
                        |cursor, frame| {
                            if query.matches_product_frame(frame) {
                                work.push(LegacyState {
                                    cursor,
                                    atom_index: state.atom_index + 1,
                                    captures: state.captures.clone(),
                                });
                            }
                        },
                    );
                }
            }
            continue;
        }

        metrics.record_clone(&state.cursor);
        let mut end = state.cursor.clone();
        if end.descend_to_existing(&[0, 0]) {
            visit_product_values(&mut end, |position| result.push(position.index()));
        }
        if query.accepts_remainder() {
            legacy_visit_sequence_frames(state.cursor, &mut metrics, |cursor, _| {
                work.push(LegacyState {
                    cursor,
                    atom_index: state.atom_index,
                    captures: state.captures.clone(),
                });
            });
        }
    }
    result.sort_unstable();
    result.dedup();
    (result, metrics)
}

fn never_decode(_: &[u8]) -> Datum<()> { unreachable!("query pruning does not decode data") }

struct ProductFixture {
    pool: CandidatePool<()>,
    structures: Vec<Vec<u8>>,
    available: Vec<bool>,
}

impl ProductFixture {
    fn from_parts(entries: Vec<(Vec<Vec<u8>>, bool)>) -> Self {
        let entries = entries
            .into_iter()
            .map(|(parts, available)| {
                let structure = encode_structural_sequence(parts.iter().map(Vec::as_slice));
                (structure, available)
            })
            .collect();
        Self::from_structures(entries)
    }

    fn from_structures(entries: Vec<(Vec<u8>, bool)>) -> Self {
        let mut availability = PathMap::new();
        let mut positions = PathMap::new();
        let mut structures = Vec::with_capacity(entries.len());
        let mut available_flags = Vec::with_capacity(entries.len());
        for (index, (structure, available)) in entries.into_iter().enumerate() {
            let identity = u64::try_from(index).unwrap().to_be_bytes();
            let path = RhoPathV1::relative_datum_key(&structure, &identity);
            positions.insert(&path, CandidatePosition::from_index(index));
            if available {
                availability.insert(&path, Multiplicity::ONE);
            }
            structures.push(structure);
            available_flags.push(available);
        }
        Self {
            pool: CandidatePool {
                availability: Bag::from_pathmap(availability),
                ordered: Vec::<CandidatePoolEntry<()>>::new().into(),
                positions: Arc::new(positions),
                fresh: None,
                fresh_available: false,
                decode: never_decode,
            },
            structures,
            available: available_flags,
        }
    }

    fn assert_matches(&self, query: &RhoPathQuery) -> ((usize, usize), LegacyCloneMetrics) {
        candidate_product_test_metrics::reset();
        let actual = self.pool.positions_matching_query(query, false);
        let checkpoint_metrics = candidate_product_test_metrics::take();
        let expected = self
            .structures
            .iter()
            .zip(&self.available)
            .enumerate()
            .filter_map(|(position, (structure, available))| {
                (*available && query.matches_structure(structure)).then_some(position)
            })
            .collect::<Vec<_>>();
        let (legacy, legacy_metrics) = legacy_positions_matching_query(
            self.pool.availability.as_pathmap(),
            &self.pool.positions,
            query,
        );
        assert_eq!(actual, expected, "checkpoint traversal must equal the byte oracle");
        assert_eq!(actual, legacy, "checkpoint traversal must equal the former cursor PDA");
        (checkpoint_metrics, legacy_metrics)
    }

    fn assert_production_matches_byte_oracle(&self, query: &RhoPathQuery) -> (usize, usize) {
        candidate_product_test_metrics::reset();
        let actual = self.pool.positions_matching_query(query, false);
        let checkpoint_metrics = candidate_product_test_metrics::take();
        let expected = self
            .structures
            .iter()
            .zip(&self.available)
            .enumerate()
            .filter_map(|(position, (structure, available))| {
                (*available && query.matches_structure(structure)).then_some(position)
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "checkpoint traversal must equal the byte oracle");
        checkpoint_metrics
    }
}

fn nested_value(discriminator: i64) -> Vec<u8> {
    let mut value = RHO_MATCH_PATH_VERSION.to_vec();
    value.extend_from_slice(&[
        RHO_MATCH_NODE_START,
        0,
        1,
        RHO_MATCH_FIELD_START,
        0,
        0,
        0,
        1,
        RHO_MATCH_I64,
    ]);
    value.extend_from_slice(&discriminator.to_be_bytes());
    value.extend_from_slice(&[
        RHO_MATCH_FIELD_END,
        RHO_MATCH_FIELD_START,
        0,
        0,
        0,
        2,
        RHO_MATCH_I64,
    ]);
    value.extend_from_slice(&discriminator.wrapping_neg().to_be_bytes());
    value.extend_from_slice(&[RHO_MATCH_FIELD_END, RHO_MATCH_NODE_END]);
    value
}

fn nested_query(discriminator: i64) -> RhoPathQuery {
    nested_query_with_suffix(discriminator, vec![RHO_MATCH_FIELD_END, RHO_MATCH_NODE_END])
}

fn nested_query_with_suffix(discriminator: i64, suffix: Vec<u8>) -> RhoPathQuery {
    let mut prefix = RHO_MATCH_PATH_VERSION.to_vec();
    prefix.extend_from_slice(&[
        RHO_MATCH_NODE_START,
        0,
        1,
        RHO_MATCH_FIELD_START,
        0,
        0,
        0,
        1,
        RHO_MATCH_I64,
    ]);
    prefix.extend_from_slice(&discriminator.to_be_bytes());
    prefix.extend_from_slice(&[RHO_MATCH_FIELD_END, RHO_MATCH_FIELD_START, 0, 0, 0, 2]);
    RhoPathQuery::structural_sequence(
        [RhoPathQueryAtom::Match(RhoMatchQuery::new([
            RhoMatchQueryPart::Exact(prefix),
            RhoMatchQueryPart::Wildcard,
            RhoMatchQueryPart::Exact(suffix),
        ]))],
        false,
    )
}

#[test]
fn checkpoint_product_matches_the_legacy_pda_and_byte_oracle() {
    let fixture = ProductFixture::from_parts(vec![
        (vec![], true),
        (vec![vec![]], true),
        (vec![b"a".to_vec()], true),
        (vec![b"a".to_vec(), b"a".to_vec()], true),
        (vec![b"a".to_vec(), b"b".to_vec()], false),
        (vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()], true),
        (vec![nested_value(17)], true),
        (vec![nested_value(18)], true),
    ]);
    let capture = RhoPathCaptureId::new(7);
    let queries = [
        RhoPathQuery::structural_sequence([], false),
        RhoPathQuery::structural_sequence([RhoPathQueryAtom::Exact(b"a".to_vec())], false),
        RhoPathQuery::structural_sequence([RhoPathQueryAtom::Wildcard], false),
        RhoPathQuery::structural_sequence([RhoPathQueryAtom::Capture(capture)], false),
        RhoPathQuery::structural_sequence(
            [RhoPathQueryAtom::Capture(capture), RhoPathQueryAtom::Capture(capture)],
            false,
        ),
        RhoPathQuery::structural_sequence([RhoPathQueryAtom::Exact(b"a".to_vec())], true),
        nested_query(17),
    ];
    for query in &queries {
        fixture.assert_matches(query);
    }
}

#[test]
fn terminal_endpoint_is_scanned_without_a_root_value_losing_checkpoint() {
    let mut availability = PathMap::new();
    let mut positions = PathMap::new();
    let terminator_endpoint = vec![1, 0, 0];
    availability.insert(&terminator_endpoint, Multiplicity::ONE);
    positions.insert(&terminator_endpoint, CandidatePosition::from_index(0));
    let pool = CandidatePool {
        availability: Bag::from_pathmap(availability),
        ordered: Vec::<CandidatePoolEntry<()>>::new().into(),
        positions: Arc::new(positions),
        fresh: None,
        fresh_available: false,
        decode: never_decode,
    };
    let query = RhoPathQuery::structural_sequence([], false);
    assert_eq!(pool.positions_matching_query(&query, false), vec![0]);
    let (legacy, _) =
        legacy_positions_matching_query(pool.availability.as_pathmap(), &pool.positions, &query);
    assert_eq!(legacy, vec![0]);
}

#[test]
fn checkpoint_product_handles_compressed_and_dense_focus_shapes() {
    let mut compressed_left = vec![0xA5; 4096];
    let mut compressed_right = compressed_left.clone();
    compressed_left.push(0x11);
    compressed_right.push(0x22);
    let compressed = ProductFixture::from_parts(vec![
        (vec![compressed_left], true),
        (vec![compressed_right], true),
    ]);
    let (checkpoint_metrics, legacy_metrics) = compressed
        .assert_matches(&RhoPathQuery::structural_sequence([RhoPathQueryAtom::Wildcard], false));
    assert_eq!(checkpoint_metrics, (2, 2));
    assert!(legacy_metrics.maximum_prefix_bytes_cloned >= 8_000);

    let dense = ProductFixture::from_parts(
        (1u8..=64)
            .map(|byte| (vec![vec![byte, byte.wrapping_mul(3)]], byte % 3 != 0))
            .collect(),
    );
    let (checkpoint_metrics, legacy_metrics) = dense
        .assert_matches(&RhoPathQuery::structural_sequence([RhoPathQueryAtom::Wildcard], false));
    assert_eq!(checkpoint_metrics, (43, 43));
    assert_eq!(legacy_metrics.visited_frames, 43);
}

#[test]
fn legacy_control_exposes_depth_growing_clone_work() {
    let parts = vec![vec![0xA5; 32]; 128];
    let fixture = ProductFixture::from_parts(vec![(parts, true)]);
    let query = RhoPathQuery::structural_sequence(
        std::iter::repeat_n(RhoPathQueryAtom::Wildcard, 128),
        false,
    );
    let ((checkpoints, visited_frames), legacy) = fixture.assert_matches(&query);
    assert_eq!(visited_frames, 128);
    assert_eq!(checkpoints, 128, "one checkpoint per accepted branch frame");
    assert_eq!(legacy.visited_frames, 128);
    assert_eq!(legacy.zipper_pairs_cloned, 129);
    assert!(legacy.maximum_prefix_bytes_cloned > 8_000);
    assert!(legacy.total_prefix_bytes_cloned > 500_000);
}

#[test]
fn deterministic_runs_create_no_suffix_checkpoints() {
    let depth = 128;
    let part = b"same".to_vec();
    let fixture = ProductFixture::from_parts(vec![(vec![part.clone(); depth], true)]);
    let query = RhoPathQuery::structural_sequence(
        std::iter::repeat_n(RhoPathQueryAtom::Exact(part), depth),
        false,
    );
    let ((checkpoints, visited_frames), legacy) = fixture.assert_matches(&query);
    assert_eq!((checkpoints, visited_frames), (0, 0));
    assert_eq!(legacy.zipper_pairs_cloned, 1);
}

#[test]
fn a_repeated_capture_checkpoints_only_its_first_branch() {
    let depth = 128;
    let fixture = ProductFixture::from_parts(vec![(vec![b"same".to_vec(); depth], true)]);
    let capture = RhoPathCaptureId::new(99);
    let query = RhoPathQuery::structural_sequence(
        std::iter::repeat_n(RhoPathQueryAtom::Capture(capture), depth),
        false,
    );
    let ((checkpoints, visited_frames), legacy) = fixture.assert_matches(&query);
    assert_eq!((checkpoints, visited_frames), (1, 1));
    assert_eq!(legacy.zipper_pairs_cloned, 2);
}

#[test]
fn rejected_nested_frames_do_not_create_suffix_checkpoints() {
    let fixture = ProductFixture::from_parts(vec![(vec![nested_value(17)], true)]);
    let query = nested_query_with_suffix(17, vec![RHO_MATCH_FIELD_END, RHO_MATCH_FIELD_END]);
    candidate_product_test_metrics::reset();
    assert!(
        fixture
            .pool
            .positions_matching_query(&query, false)
            .is_empty()
    );
    let (checkpoints, visited_frames) = candidate_product_test_metrics::take();
    assert_eq!(checkpoints, 0);
    assert_eq!(visited_frames, 1, "the rejected frame must have reached its predicate");
}

#[test]
fn asymmetric_failed_descent_restores_both_product_zippers() {
    let mut availability = PathMap::new();
    let mut positions = PathMap::new();
    availability.insert(&[1, 2], Multiplicity::ONE);
    positions.insert(&[1, 2, 3], CandidatePosition::from_index(0));
    let mut cursor = CandidateProductCursor::new(&availability, &positions);

    assert!(!cursor.descend_to_existing(&[1, 2, 3]));
    assert!(cursor.availability.path().is_empty());
    assert!(cursor.positions.path().is_empty());
    assert!(cursor.descend_to_existing(&[1, 2]));
    assert_eq!(cursor.availability.path(), &[1, 2]);
    assert_eq!(cursor.positions.path(), &[1, 2]);
}

#[test]
fn checkpoint_preserves_focus_endpoints_and_descendant_values() {
    let mut availability = PathMap::new();
    let mut positions = PathMap::new();
    availability.insert(&[7], Multiplicity::ONE);
    availability.insert(&[7, 9], Multiplicity::new(2).unwrap());
    positions.insert(&[7], CandidatePosition::from_index(3));
    positions.insert(&[7, 9], CandidatePosition::from_index(5));
    let mut cursor = CandidateProductCursor::new(&availability, &positions);
    assert!(cursor.descend_to_existing(&[7]));

    candidate_product_test_metrics::reset();
    let checkpoint = cursor
        .checkpoint()
        .expect("root-plus-child suffix checkpoint");
    assert_eq!(candidate_product_test_metrics::root_endpoint_restorations(), 1);
    let mut restored = checkpoint.cursor();
    assert_eq!(restored.position(), Some(CandidatePosition::from_index(3)));
    assert!(restored.descend_to_existing(&[9]));
    assert_eq!(restored.position(), Some(CandidatePosition::from_index(5)));
}

#[test]
fn canonical_rho_path_branches_never_need_defensive_endpoint_restoration() {
    let fixture = ProductFixture::from_parts(vec![
        (vec![b"left".to_vec(), b"tail".to_vec()], true),
        (vec![b"right".to_vec(), b"tail".to_vec()], true),
    ]);
    let query = RhoPathQuery::structural_sequence(
        [RhoPathQueryAtom::Wildcard, RhoPathQueryAtom::Exact(b"tail".to_vec())],
        false,
    );
    fixture.assert_matches(&query);
    assert_eq!(
        candidate_product_test_metrics::root_endpoint_restorations(),
        0,
        "canonical component boundaries must remain on the no-restoration fast path"
    );
}

#[test]
fn fixed_seed_small_tries_match_the_legacy_pda_and_byte_oracle() {
    let mut rng = StdRng::seed_from_u64(0x4131_5990_2026_0823);
    for case in 0..128 {
        let candidate_count = rng.gen_range(1..=16);
        let mut entries = Vec::with_capacity(candidate_count);
        for candidate in 0..candidate_count {
            let depth = rng.gen_range(0..=6);
            let parts = (0..depth)
                .map(|_| {
                    let width = rng.gen_range(0..=8);
                    (0..width).map(|_| rng.r#gen::<u8>()).collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            entries.push((parts, candidate == 0 || rng.gen_bool(0.7)));
        }
        let witness = entries[0].0.clone();
        let fixture = ProductFixture::from_parts(entries);
        let exact = RhoPathQuery::structural_sequence(
            witness.iter().cloned().map(RhoPathQueryAtom::Exact),
            false,
        );
        let wildcard = RhoPathQuery::structural_sequence(
            std::iter::repeat_n(RhoPathQueryAtom::Wildcard, witness.len()),
            false,
        );
        let prefix_len = witness.len() / 2;
        let prefix = RhoPathQuery::structural_sequence(
            witness[..prefix_len]
                .iter()
                .cloned()
                .map(RhoPathQueryAtom::Exact),
            true,
        );
        let mixed = RhoPathQuery::structural_sequence(
            witness.iter().enumerate().map(|(index, frame)| {
                if index % 2 == 0 {
                    RhoPathQueryAtom::Exact(frame.clone())
                } else {
                    RhoPathQueryAtom::Wildcard
                }
            }),
            false,
        );
        for query in [&exact, &wildcard, &prefix, &mixed] {
            fixture.assert_matches(query);
        }
        assert!(exact.matches_structure(&fixture.structures[0]), "case {case}");
    }
}

#[test]
fn depth_16384_candidate_queries_fit_a_256_kib_stack() {
    const DEPTH: usize = 16_384;
    std::thread::Builder::new()
        .name("candidate-query-small-stack".to_owned())
        .stack_size(256 * 1024)
        .spawn(|| {
            let part = b"same".to_vec();
            let fixture = ProductFixture::from_parts(vec![(vec![part.clone(); DEPTH], true)]);

            let exact = RhoPathQuery::structural_sequence(
                std::iter::repeat_n(RhoPathQueryAtom::Exact(part), DEPTH),
                false,
            );
            assert_eq!(fixture.assert_production_matches_byte_oracle(&exact), (0, 0));

            let capture = RhoPathCaptureId::new(4131);
            let repeated_capture = RhoPathQuery::structural_sequence(
                std::iter::repeat_n(RhoPathQueryAtom::Capture(capture), DEPTH),
                false,
            );
            assert_eq!(fixture.assert_production_matches_byte_oracle(&repeated_capture), (1, 1));

            let wildcard = RhoPathQuery::structural_sequence(
                std::iter::repeat_n(RhoPathQueryAtom::Wildcard, DEPTH),
                false,
            );
            assert_eq!(fixture.assert_production_matches_byte_oracle(&wildcard), (DEPTH, DEPTH));
        })
        .expect("the small-stack candidate-query thread must spawn")
        .join()
        .expect("candidate queries must remain stack-safe at depth 16,384");
}
