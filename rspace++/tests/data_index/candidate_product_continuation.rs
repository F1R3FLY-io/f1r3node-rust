//! Test-only differential and anti-vacuity gates for candidate continuation
//! rebasing.
//!
//! Kept outside `src/` so the frozen legacy zipper-clone oracle cannot be
//! mistaken for a production implementation.

use pathmap::PathMap;
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
    fn record_clone(&mut self, cursor: &CandidateProductCursor<'_, '_>) {
        let prefix_bytes = cursor.availability.path().len() + cursor.positions.path().len();
        self.zipper_pairs_cloned += 1;
        self.total_prefix_bytes_cloned += prefix_bytes;
        self.maximum_prefix_bytes_cloned = self.maximum_prefix_bytes_cloned.max(prefix_bytes);
    }
}

struct LegacyState<'availability, 'positions> {
    cursor: CandidateProductCursor<'availability, 'positions>,
    atom_index: usize,
    captures: PathMap<Arc<[u8]>>,
}

fn legacy_visit_sequence_frames<'availability, 'positions>(
    cursor: CandidateProductCursor<'availability, 'positions>,
    metrics: &mut LegacyCloneMetrics,
    mut visitor: impl FnMut(CandidateProductCursor<'availability, 'positions>, &[u8]),
) {
    legacy_visit_sequence_frames_with_prefix(cursor, &[], metrics, &mut visitor);
}

fn legacy_visit_sequence_frames_with_prefix<'availability, 'positions>(
    mut cursor: CandidateProductCursor<'availability, 'positions>,
    prefix: &[u8],
    metrics: &mut LegacyCloneMetrics,
    mut visitor: impl FnMut(CandidateProductCursor<'availability, 'positions>, &[u8]),
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
    availability: &Bag,
    positions: &CandidatePositionMap,
    query: &RhoPathQuery,
) -> (Vec<usize>, LegacyCloneMetrics) {
    availability.with_read_cursor(|availability| {
        positions.with_read_cursor(|positions| {
            legacy_positions_matching_query_from_cursor(availability, positions, query)
        })
    })
}

fn legacy_positions_matching_query_from_cursor(
    availability: BagReadCursor<'_, 'static>,
    positions: CandidatePositionReadCursor<'_, 'static>,
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
                CompiledRhoPathQueryAtom::Capture { slot } => {
                    let capture_path = slot.to_be_bytes();
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
        let mut availability = Bag::new();
        let mut positions = CandidatePositionMap::new();
        let mut structures = Vec::with_capacity(entries.len());
        let mut available_flags = Vec::with_capacity(entries.len());
        for (index, (structure, available)) in entries.into_iter().enumerate() {
            let identity = u64::try_from(index).unwrap().to_be_bytes();
            let path = RhoPathV1::relative_datum_key(&structure, &identity);
            assert!(!positions.insert(&path, CandidatePosition::from_index(index)));
            if available {
                availability.insert_many(&path, Multiplicity::ONE).unwrap();
            }
            structures.push(structure);
            available_flags.push(available);
        }
        Self {
            pool: CandidatePool {
                authority: availability,
                excluded: Bag::new(),
                reserved: Bag::new(),
                ordered: Vec::<CandidatePoolEntry<()>>::new().into(),
                positions,
                fresh: None,
                fresh_available: false,
                decode: never_decode,
            },
            structures,
            available: available_flags,
        }
    }

    fn assert_matches(
        &self,
        query: &RhoPathQuery,
    ) -> (candidate_product_test_metrics::Metrics, LegacyCloneMetrics) {
        candidate_product_test_metrics::reset();
        let actual = self.pool.positions_matching_query(query, false);
        let arena_metrics = candidate_product_test_metrics::take();
        let expected = self
            .structures
            .iter()
            .zip(&self.available)
            .enumerate()
            .filter_map(|(position, (structure, available))| {
                (*available && query.matches_structure(structure)).then_some(position)
            })
            .collect::<Vec<_>>();
        let (legacy, legacy_metrics) =
            legacy_positions_matching_query(&self.pool.authority, &self.pool.positions, query);
        assert_eq!(actual, expected, "arena traversal must equal the byte oracle");
        assert_eq!(actual, legacy, "arena traversal must equal the former cursor PDA");
        (arena_metrics, legacy_metrics)
    }

    fn assert_production_matches_byte_oracle(
        &self,
        query: &RhoPathQuery,
    ) -> candidate_product_test_metrics::Metrics {
        candidate_product_test_metrics::reset();
        let actual = self.pool.positions_matching_query(query, false);
        let arena_metrics = candidate_product_test_metrics::take();
        let expected = self
            .structures
            .iter()
            .zip(&self.available)
            .enumerate()
            .filter_map(|(position, (structure, available))| {
                (*available && query.matches_structure(structure)).then_some(position)
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "arena traversal must equal the byte oracle");
        arena_metrics
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
fn continuation_arena_matches_the_legacy_pda_and_byte_oracle() {
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
fn normalized_query_positions_preserve_catalog_gaps_and_fresh_offset() {
    let mut fixture = ProductFixture::from_parts(vec![
        (vec![b"b".to_vec()], false),
        (vec![b"z".to_vec()], true),
        (vec![b"c".to_vec()], false),
        (vec![b"a".to_vec()], true),
    ]);
    let query = RhoPathQuery::structural_sequence([RhoPathQueryAtom::Wildcard], false);
    assert_eq!(fixture.pool.positions_matching_query(&query, false), [1, 3]);
    assert_eq!(fixture.pool.positions_with_structure_prefix(&[], false), [0, 1, 2, 3]);
    assert!(fixture.pool.prepend_fresh(Datum::default()));
    assert_eq!(fixture.pool.positions_matching_query(&query, false), [2, 4]);
    assert_eq!(fixture.pool.positions_matching_query(&query, true), [0, 2, 4]);
    assert_eq!(fixture.pool.positions_with_structure_prefix(&[], false), [1, 2, 3, 4]);
    assert_eq!(fixture.pool.positions_with_structure_prefix(&[], true), [0, 1, 2, 3, 4]);
}

#[test]
fn synthetic_duplicate_position_reports_are_deduplicated_without_decoding() {
    let mut fixture =
        ProductFixture::from_parts(vec![(vec![b"b".to_vec()], true), (vec![b"a".to_vec()], true)]);
    let second = RhoPathV1::relative_datum_key(&fixture.structures[1], &1u64.to_be_bytes());
    fixture
        .pool
        .positions
        .insert(&second, CandidatePosition::from_index(0));
    let query = RhoPathQuery::structural_sequence([RhoPathQueryAtom::Wildcard], false);
    assert_eq!(fixture.pool.positions_with_structure_prefix(&[], false), [0, 0]);
    assert_eq!(fixture.pool.positions_matching_query(&query, false), [0]);
}

#[test]
fn lazy_catalog_isolates_good_and_empty_channels_from_unrelated_malformed_keys() {
    let good = "good".to_owned();
    let empty = "empty".to_owned();
    let poisoned = "poisoned".to_owned();
    let mut index = RSpaceDataIndex::new();
    let value = Datum {
        a: Arc::new("payload".to_owned()),
        ..Datum::default()
    };
    index.insert(&good, &value).unwrap();
    index
        .replace_channel_at_marker(&RhoPathV1::datum_prefix(&empty), Bag::new())
        .unwrap();
    let mut malformed = Bag::new();
    malformed.insert(&[0, 2]).unwrap();
    index
        .replace_channel_at_marker(&RhoPathV1::datum_prefix(&poisoned), malformed)
        .unwrap();

    let pool = index.candidate_pool::<_, String>(&good).unwrap();
    assert_eq!(pool.len(), 1);
    assert!(pool.ordered.iter().all(|entry| entry.datum.get().is_none()));
    assert!(
        index
            .candidate_pool::<_, String>(&empty)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(index.candidate_pool::<_, String>(&poisoned), Err(DataIndexError::Path(_))));
    assert!(pool.ordered.iter().all(|entry| entry.datum.get().is_none()));
}

#[test]
fn terminal_endpoint_is_scanned_at_the_live_product_cursor() {
    let mut availability = Bag::new();
    let mut positions = CandidatePositionMap::new();
    let terminator_endpoint = vec![1, 0, 0];
    availability
        .insert_many(&terminator_endpoint, Multiplicity::ONE)
        .unwrap();
    assert!(!positions.insert(&terminator_endpoint, CandidatePosition::from_index(0)));
    let pool = CandidatePool {
        authority: availability,
        excluded: Bag::new(),
        reserved: Bag::new(),
        ordered: Vec::<CandidatePoolEntry<()>>::new().into(),
        positions,
        fresh: None,
        fresh_available: false,
        decode: never_decode,
    };
    let query = RhoPathQuery::structural_sequence([], false);
    assert_eq!(pool.positions_matching_query(&query, false), vec![0]);
    let (legacy, _) = legacy_positions_matching_query(&pool.authority, &pool.positions, &query);
    assert_eq!(legacy, vec![0]);
}

#[test]
fn continuation_arena_handles_compressed_and_dense_focus_shapes() {
    let mut compressed_left = vec![0xA5; 4096];
    let mut compressed_right = compressed_left.clone();
    compressed_left.push(0x11);
    compressed_right.push(0x22);
    let compressed = ProductFixture::from_parts(vec![
        (vec![compressed_left], true),
        (vec![compressed_right], true),
    ]);
    let (arena_metrics, legacy_metrics) = compressed
        .assert_matches(&RhoPathQuery::structural_sequence([RhoPathQueryAtom::Wildcard], false));
    assert_eq!(arena_metrics.continuations, 2);
    assert_eq!(arena_metrics.visited_frames, 2);
    assert!(legacy_metrics.maximum_prefix_bytes_cloned >= 8_000);

    let dense = ProductFixture::from_parts(
        (1u8..=64)
            .map(|byte| (vec![vec![byte, byte.wrapping_mul(3)]], byte % 3 != 0))
            .collect(),
    );
    let (arena_metrics, legacy_metrics) = dense
        .assert_matches(&RhoPathQuery::structural_sequence([RhoPathQueryAtom::Wildcard], false));
    assert_eq!(arena_metrics.continuations, 43);
    assert_eq!(arena_metrics.visited_frames, 43);
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
    let (arena, legacy) = fixture.assert_matches(&query);
    assert_eq!(arena.visited_frames, 128);
    assert_eq!(arena.continuations, 128, "one continuation per accepted branch frame");
    assert_eq!(legacy.visited_frames, 128);
    assert_eq!(legacy.zipper_pairs_cloned, 129);
    assert!(legacy.maximum_prefix_bytes_cloned > 8_000);
    assert!(legacy.total_prefix_bytes_cloned > 500_000);
}

#[test]
fn deterministic_runs_borrow_exact_frames_without_scanner_visits() {
    let depth = 128;
    let part = b"same".to_vec();
    let fixture = ProductFixture::from_parts(vec![(vec![part.clone(); depth], true)]);
    let query = RhoPathQuery::structural_sequence(
        std::iter::repeat_n(RhoPathQueryAtom::Exact(part), depth),
        false,
    );
    let (arena, legacy) = fixture.assert_matches(&query);
    assert_eq!(arena.continuations, depth);
    assert_eq!(arena.visited_frames, 0);
    assert_eq!(legacy.zipper_pairs_cloned, 1);
}

#[test]
fn a_repeated_capture_owns_only_its_first_scanned_frame() {
    let depth = 128;
    let fixture = ProductFixture::from_parts(vec![(vec![b"same".to_vec(); depth], true)]);
    let capture = RhoPathCaptureId::new(99);
    let query = RhoPathQuery::structural_sequence(
        std::iter::repeat_n(RhoPathQueryAtom::Capture(capture), depth),
        false,
    );
    let (arena, legacy) = fixture.assert_matches(&query);
    assert_eq!(arena.continuations, depth);
    assert_eq!(arena.visited_frames, 1);
    assert_eq!(legacy.zipper_pairs_cloned, 2);
}

#[test]
fn rejected_nested_frames_do_not_create_continuations() {
    let fixture = ProductFixture::from_parts(vec![(vec![nested_value(17)], true)]);
    let query = nested_query_with_suffix(17, vec![RHO_MATCH_FIELD_END, RHO_MATCH_FIELD_END]);
    candidate_product_test_metrics::reset();
    assert!(
        fixture
            .pool
            .positions_matching_query(&query, false)
            .is_empty()
    );
    let metrics = candidate_product_test_metrics::take();
    assert_eq!(metrics.continuations, 0);
    assert_eq!(metrics.visited_frames, 1, "the rejected frame must reach its predicate");
}

#[test]
fn asymmetric_failed_descent_restores_both_product_zippers() {
    let mut availability = Bag::new();
    let mut positions = CandidatePositionMap::new();
    availability
        .insert_many(&[1, 2], Multiplicity::ONE)
        .unwrap();
    assert!(!positions.insert(&[1, 2, 3], CandidatePosition::from_index(0)));
    availability.with_read_cursor(|availability| {
        positions.with_read_cursor(|positions| {
            let mut cursor = CandidateProductCursor::new(availability, positions);

            assert!(!cursor.descend_to_existing(&[1, 2, 3]));
            assert!(cursor.availability.path().is_empty());
            assert!(cursor.positions.path().is_empty());
            assert!(cursor.descend_to_existing(&[1, 2]));
            assert_eq!(cursor.availability.path(), &[1, 2]);
            assert_eq!(cursor.positions.path(), &[1, 2]);
        });
    });
}

#[test]
fn moving_cursor_preserves_focus_endpoints_and_descendant_values() {
    let mut availability = Bag::new();
    let mut positions = CandidatePositionMap::new();
    availability.insert_many(&[7], Multiplicity::ONE).unwrap();
    availability
        .insert_many(&[7, 9], Multiplicity::new(2).unwrap())
        .unwrap();
    assert!(!positions.insert(&[7], CandidatePosition::from_index(3)));
    assert!(!positions.insert(&[7, 9], CandidatePosition::from_index(5)));

    availability.with_read_cursor(|availability| {
        positions.with_read_cursor(|positions| {
            let mut cursor = CandidateProductCursor::new(availability, positions);
            assert!(cursor.descend_to_existing(&[7]));
            assert_eq!(cursor.position(), Some(CandidatePosition::from_index(3)));
            assert!(cursor.descend_to_existing(&[9]));
            assert_eq!(cursor.position(), Some(CandidatePosition::from_index(5)));
            cursor.ascend_bytes(1);
            assert_eq!(cursor.position(), Some(CandidatePosition::from_index(3)));
        });
    });
}

#[test]
fn canonical_rho_path_branches_rebase_without_owned_focus_snapshots() {
    let fixture = ProductFixture::from_parts(vec![
        (vec![b"left".to_vec(), b"tail".to_vec()], true),
        (vec![b"right".to_vec(), b"tail".to_vec()], true),
    ]);
    let query = RhoPathQuery::structural_sequence(
        [RhoPathQueryAtom::Wildcard, RhoPathQueryAtom::Exact(b"tail".to_vec())],
        false,
    );
    let (metrics, _) = fixture.assert_matches(&query);
    assert!(metrics.navigated_bytes > 0);
}

#[test]
fn lca_rebase_undoes_capture_edits_before_descending_a_sibling() {
    let mut availability = Bag::new();
    let mut positions = CandidatePositionMap::new();
    for (position, path) in [[1, 10, 11].as_slice(), [1, 20].as_slice()]
        .into_iter()
        .enumerate()
    {
        availability.insert_many(path, Multiplicity::ONE).unwrap();
        assert!(!positions.insert(path, CandidatePosition::from_index(position)));
    }
    let query = RhoPathQuery::structural_sequence([], false);
    let mut arena = CandidateContinuationArena::at_structure_root(2);
    let captured_left = arena.push_owned(0, &[10], Some(0));
    let left_leaf = arena.push_owned(captured_left, &[11], None);
    let right_leaf = arena.push_owned(0, &[20], None);
    let mut capture_slots = [None];
    let mut suffix = Vec::new();

    candidate_product_test_metrics::reset();
    availability.with_read_cursor(|availability| {
        positions.with_read_cursor(|positions| {
            let mut cursor = CandidateProductCursor::new(availability, positions);
            assert!(cursor.descend_to_existing(&[1]));
            let mut current = 0;
            assert!(rebase_candidate_cursor(
                &mut cursor,
                &mut current,
                left_leaf,
                &arena,
                &query,
                &mut capture_slots,
                &mut suffix,
            ));
            assert_eq!(cursor.availability.path(), &[1, 10, 11]);
            assert_eq!(cursor.positions.path(), &[1, 10, 11]);
            assert_eq!(capture_slots, [Some(captured_left)]);

            assert!(rebase_candidate_cursor(
                &mut cursor,
                &mut current,
                right_leaf,
                &arena,
                &query,
                &mut capture_slots,
                &mut suffix,
            ));
            assert_eq!(cursor.availability.path(), &[1, 20]);
            assert_eq!(cursor.positions.path(), &[1, 20]);
            assert_eq!(capture_slots, [None]);
        });
    });
    let metrics = candidate_product_test_metrics::take();
    assert_eq!(metrics.navigated_bytes, 5);
    assert_eq!(metrics.capture_edits, 2);
}

#[test]
fn classified_scanner_prefixes_restore_both_cursors_to_their_base() {
    let prefixes = [[0, 0].as_slice(), [0, 7].as_slice(), [0, 255, 0, 255].as_slice()];
    let mut availability = Bag::new();
    let mut positions = CandidatePositionMap::new();
    for (position, prefix) in prefixes.into_iter().enumerate() {
        availability.insert_many(prefix, Multiplicity::ONE).unwrap();
        assert!(!positions.insert(prefix, CandidatePosition::from_index(position)));
    }
    availability.with_read_cursor(|availability| {
        positions.with_read_cursor(|positions| {
            let mut cursor = CandidateProductCursor::new(availability, positions);
            let mut arena = CandidateContinuationArena::at_structure_root(0);
            let mut scratch = CandidateFrameScanScratch::default();
            for prefix in prefixes {
                visit_sequence_frames_with_prefix(
                    &mut cursor,
                    0,
                    &mut arena,
                    None,
                    &mut scratch,
                    prefix,
                    |_| true,
                    |_, _| panic!("a pre-classified prefix must not schedule a continuation"),
                );
                assert!(cursor.availability.path().is_empty());
                assert!(cursor.positions.path().is_empty());
            }
        });
    });
}

#[test]
fn production_candidate_lane_cannot_regain_owned_subtree_checkpoints() {
    let source = include_str!("../../src/rspace/data_index.rs");
    let start = source
        .find("struct CandidateProductCursor")
        .expect("candidate product lane starts at its cursor");
    let end = source[start..]
        .find("impl<A: Clone + PartialEq>")
        .map(|offset| start + offset)
        .expect("candidate product lane ends before CandidatePool equality");
    let lane = &source[start..end];
    for forbidden in [
        "CandidateProductCheckpoint",
        "guarded_focus_snapshot",
        "ZipperSubtries",
        "ZipperInfallibleSubtries",
        "make_map(",
        "try_make_map",
        "trie_ref",
        "graft_src_at",
        "PathMap<Arc<[u8]>>",
        "cursor.clone()",
    ] {
        assert!(!lane.contains(forbidden), "forbidden candidate-lane capability: {forbidden}");
    }
    for required in [
        "CandidateContinuationArena",
        "CandidatePositionReadCursor",
        "rebase_candidate_cursor",
        "capture_slots: &mut [Option<usize>]",
    ] {
        assert!(lane.contains(required), "missing candidate-lane invariant: {required}");
    }
}

#[test]
fn production_candidate_authority_cannot_own_an_interior_pathmap_focus() {
    let index_source = include_str!("../../src/rspace/data_index.rs");
    assert!(
        !index_source.contains("guarded_focus_snapshot"),
        "candidate construction must use the independently rebuilt catalog Bag"
    );
    let guard_source = include_str!("../../src/rspace/guarded_pathmap.rs");
    assert!(
        !guard_source.contains("fn guarded_focus_snapshot"),
        "the guarded facade must not expose an owned interior PathMap header"
    );
    for required in ["authority: Bag", "fn from_catalog(authority: Bag", "excluded: Bag"] {
        assert!(
            index_source.contains(required),
            "missing candidate-authority isolation invariant: {required}"
        );
    }
}

#[test]
fn repeated_capture_ids_compile_to_stable_dense_direct_slots() {
    let left = RhoPathCaptureId::new(u32::MAX);
    let right = RhoPathCaptureId::new(7);
    let singleton = RhoPathCaptureId::new(9);
    let query = RhoPathQuery::structural_sequence(
        [
            RhoPathQueryAtom::Capture(left),
            RhoPathQueryAtom::Capture(right),
            RhoPathQueryAtom::Capture(left),
            RhoPathQueryAtom::Capture(singleton),
            RhoPathQueryAtom::Capture(right),
        ],
        false,
    );
    assert_eq!(query.capture_slot_count(), 2);
    assert!(matches!(query.atoms(), [
        CompiledRhoPathQueryAtom::Capture { slot: 0 },
        CompiledRhoPathQueryAtom::Capture { slot: 1 },
        CompiledRhoPathQueryAtom::Capture { slot: 0 },
        CompiledRhoPathQueryAtom::Wildcard,
        CompiledRhoPathQueryAtom::Capture { slot: 1 },
    ]));
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
            let exact_metrics = fixture.assert_production_matches_byte_oracle(&exact);
            assert_eq!(exact_metrics.continuations, DEPTH);
            assert_eq!(exact_metrics.visited_frames, 0);

            let capture = RhoPathCaptureId::new(4131);
            let repeated_capture = RhoPathQuery::structural_sequence(
                std::iter::repeat_n(RhoPathQueryAtom::Capture(capture), DEPTH),
                false,
            );
            let capture_metrics = fixture.assert_production_matches_byte_oracle(&repeated_capture);
            assert_eq!(capture_metrics.continuations, DEPTH);
            assert_eq!(capture_metrics.visited_frames, 1);

            let wildcard = RhoPathQuery::structural_sequence(
                std::iter::repeat_n(RhoPathQueryAtom::Wildcard, DEPTH),
                false,
            );
            let wildcard_metrics = fixture.assert_production_matches_byte_oracle(&wildcard);
            assert_eq!(wildcard_metrics.continuations, DEPTH);
            assert_eq!(wildcard_metrics.visited_frames, DEPTH);
        })
        .expect("the small-stack candidate-query thread must spawn")
        .join()
        .expect("candidate queries must remain stack-safe at depth 16,384");
}
