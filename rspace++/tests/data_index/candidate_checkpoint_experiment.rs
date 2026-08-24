//! Measurement-only control for the retained candidate-product checkpoint
//! experiment.
//!
//! This file is compiled as a child of `rspace::data_index` only when the
//! `candidate-checkpoint-experiment` feature is selected. The checkpoint arm
//! calls the production `CandidatePool::positions_matching_query` method. The
//! legacy arm freezes the removed pair-of-read-zippers continuation so the two
//! representations can be compared at one source coordinate without exposing
//! either implementation in an ordinary build.

use std::sync::Arc;

use pathmap::PathMap;

use super::*;
use crate::rspace::rho_path::{
    RHO_MATCH_FIELD_END, RHO_MATCH_FIELD_START, RHO_MATCH_I64, RHO_MATCH_NODE_END,
    RHO_MATCH_NODE_START, RHO_MATCH_PATH_VERSION, RhoMatchQuery, RhoMatchQueryPart,
    RhoPathCaptureId, RhoPathQueryAtom, encode_structural_sequence,
};

const DENSE_CANDIDATES: u8 = 64;
const SHARED_DISCRIMINATOR: u8 = 17;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExperimentArm {
    LegacyClone,
    CowCheckpoint,
}

impl ExperimentArm {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LegacyClone => "legacy_clone",
            Self::CowCheckpoint => "cow_checkpoint",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExperimentShape {
    Compressed,
    Dense64,
}

impl ExperimentShape {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Compressed => "compressed",
            Self::Dense64 => "dense64",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExperimentQuery {
    Exact,
    Wildcard,
    RepeatedCapture,
    NestedMatch,
    AcceptsRemainder,
}

impl ExperimentQuery {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Wildcard => "wildcard",
            Self::RepeatedCapture => "repeated_capture",
            Self::NestedMatch => "nested_match",
            Self::AcceptsRemainder => "accepts_remainder",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LegacyCloneMetrics {
    pub zipper_pairs_cloned: u64,
    pub total_prefix_bytes_cloned: u64,
    pub maximum_prefix_bytes_cloned: u64,
    pub visited_frames: u64,
}

impl LegacyCloneMetrics {
    fn record_clone<const AUDIT: bool>(&mut self, cursor: &CandidateProductCursor<'_>) {
        if AUDIT {
            let prefix_bytes = cursor.availability.path().len() + cursor.positions.path().len();
            self.zipper_pairs_cloned += 1;
            self.total_prefix_bytes_cloned +=
                u64::try_from(prefix_bytes).expect("an allocated zipper prefix fits u64");
            self.maximum_prefix_bytes_cloned = self
                .maximum_prefix_bytes_cloned
                .max(u64::try_from(prefix_bytes).expect("an allocated zipper prefix fits u64"));
        }
    }

    fn record_frame<const AUDIT: bool>(&mut self) {
        if AUDIT {
            self.visited_frames += 1;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExperimentOutcome {
    positions: Vec<u64>,
    digest: [u8; 32],
}

impl ExperimentOutcome {
    fn from_positions(positions: Vec<usize>) -> Self {
        let positions = positions
            .into_iter()
            .map(|position| u64::try_from(position).expect("candidate position fits u64"))
            .collect::<Vec<_>>();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"RSpace/CandidateCheckpointExperiment/results/v1");
        hasher.update(
            &u64::try_from(positions.len())
                .expect("result length fits u64")
                .to_be_bytes(),
        );
        for position in &positions {
            hasher.update(&position.to_be_bytes());
        }
        Self {
            positions,
            digest: *hasher.finalize().as_bytes(),
        }
    }

    pub fn count(&self) -> usize { self.positions.len() }

    pub fn digest_hex(&self) -> String { hex::encode(self.digest) }
}

#[derive(Clone, Debug)]
pub struct ExperimentAudit {
    pub expected: ExperimentOutcome,
    pub legacy: ExperimentOutcome,
    pub checkpoint: ExperimentOutcome,
    pub legacy_metrics: LegacyCloneMetrics,
    pub logical_structure_bytes: u64,
}

pub struct CandidateCheckpointExperiment {
    shape: ExperimentShape,
    query_kind: ExperimentQuery,
    depth: usize,
    pool: CandidatePool<()>,
    structures: Vec<Vec<u8>>,
    available: Vec<bool>,
    query: RhoPathQuery,
}

impl CandidateCheckpointExperiment {
    pub fn new(shape: ExperimentShape, query_kind: ExperimentQuery, depth: usize) -> Self {
        assert!(depth >= 2, "the experiment requires at least two structural components");
        let entries = match shape {
            ExperimentShape::Compressed => vec![(parts_for(query_kind, depth, None), true)],
            ExperimentShape::Dense64 => (1..=DENSE_CANDIDATES)
                .map(|branch| (parts_for(query_kind, depth, Some(branch)), branch % 3 != 0))
                .collect(),
        };
        let query = query_for(query_kind, depth);
        let mut availability = PathMap::new();
        let mut positions = PathMap::new();
        let mut structures = Vec::with_capacity(entries.len());
        let mut available_flags = Vec::with_capacity(entries.len());
        for (index, (parts, available)) in entries.into_iter().enumerate() {
            let structure = encode_structural_sequence(parts.iter().map(Vec::as_slice));
            let identity = u64::try_from(index)
                .expect("candidate index fits u64")
                .to_be_bytes();
            let path = RhoPathV1::relative_datum_key(&structure, &identity);
            positions.insert(&path, CandidatePosition::from_index(index));
            if available {
                availability.insert(&path, Multiplicity::ONE);
            }
            structures.push(structure);
            available_flags.push(available);
        }
        Self {
            shape,
            query_kind,
            depth,
            pool: CandidatePool {
                authority: Bag::from_pathmap(availability),
                reserved: Bag::new(),
                ordered: Vec::<CandidatePoolEntry<()>>::new().into(),
                positions: Arc::new(positions),
                fresh: None,
                fresh_available: false,
                decode: never_decode,
            },
            structures,
            available: available_flags,
            query,
        }
    }

    pub const fn shape(&self) -> ExperimentShape { self.shape }

    pub const fn query_kind(&self) -> ExperimentQuery { self.query_kind }

    pub const fn depth(&self) -> usize { self.depth }

    #[inline(never)]
    pub fn run(&self, arm: ExperimentArm) -> ExperimentOutcome {
        let positions = match arm {
            ExperimentArm::LegacyClone => {
                legacy_positions_matching_query::<false>(
                    self.pool.authority.as_pathmap(),
                    &self.pool.positions,
                    &self.query,
                )
                .0
            }
            ExperimentArm::CowCheckpoint => self.pool.positions_matching_query(&self.query, false),
        };
        ExperimentOutcome::from_positions(positions)
    }

    pub fn audit(&self) -> ExperimentAudit {
        let expected = ExperimentOutcome::from_positions(
            self.structures
                .iter()
                .zip(&self.available)
                .enumerate()
                .filter_map(|(position, (structure, available))| {
                    (*available && self.query.matches_structure(structure)).then_some(position)
                })
                .collect(),
        );
        let (legacy_positions, legacy_metrics) = legacy_positions_matching_query::<true>(
            self.pool.authority.as_pathmap(),
            &self.pool.positions,
            &self.query,
        );
        let legacy = ExperimentOutcome::from_positions(legacy_positions);
        let checkpoint = self.run(ExperimentArm::CowCheckpoint);
        assert_eq!(legacy, expected, "legacy control diverged from the byte oracle");
        assert_eq!(checkpoint, expected, "checkpoint treatment diverged from the byte oracle");
        assert_ne!(expected.count(), 0, "the experiment cell needs a positive witness");

        if self.shape == ExperimentShape::Compressed && self.query_kind == ExperimentQuery::Wildcard
        {
            let depth = u64::try_from(self.depth).expect("experiment depth fits u64");
            assert_eq!(legacy_metrics.visited_frames, depth);
            assert_eq!(legacy_metrics.zipper_pairs_cloned, depth + 1);
            assert!(legacy_metrics.maximum_prefix_bytes_cloned > depth);
            assert!(
                legacy_metrics.total_prefix_bytes_cloned > depth.saturating_mul(depth),
                "the legacy control must expose cumulative depth-growing prefix copies"
            );
        }
        if self.shape == ExperimentShape::Dense64 {
            assert_eq!(self.structures.len(), usize::from(DENSE_CANDIDATES));
        }

        ExperimentAudit {
            expected,
            legacy,
            checkpoint,
            legacy_metrics,
            logical_structure_bytes: self
                .structures
                .iter()
                .map(|structure| u64::try_from(structure.len()).expect("structure length fits u64"))
                .sum(),
        }
    }
}

struct LegacyState<'a> {
    cursor: CandidateProductCursor<'a>,
    atom_index: usize,
    captures: PathMap<Arc<[u8]>>,
}

fn legacy_visit_sequence_frames<'a, const AUDIT: bool>(
    cursor: CandidateProductCursor<'a>,
    metrics: &mut LegacyCloneMetrics,
    mut visitor: impl FnMut(CandidateProductCursor<'a>, &[u8]),
) {
    legacy_visit_sequence_frames_with_prefix::<AUDIT>(cursor, &[], metrics, &mut visitor);
}

fn legacy_visit_sequence_frames_with_prefix<'a, const AUDIT: bool>(
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
                        metrics.record_frame::<AUDIT>();
                        metrics.record_clone::<AUDIT>(&cursor);
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

fn legacy_positions_matching_query<const AUDIT: bool>(
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
                        legacy_visit_sequence_frames::<AUDIT>(
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
                    legacy_visit_sequence_frames::<AUDIT>(
                        state.cursor,
                        &mut metrics,
                        |cursor, _| {
                            work.push(LegacyState {
                                cursor,
                                atom_index: state.atom_index + 1,
                                captures: state.captures.clone(),
                            });
                        },
                    );
                }
                CompiledRhoPathQueryAtom::Match(query) => {
                    legacy_visit_sequence_frames_with_prefix::<AUDIT>(
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

        metrics.record_clone::<AUDIT>(&state.cursor);
        let mut end = state.cursor.clone();
        if end.descend_to_existing(&[0, 0]) {
            visit_product_values(&mut end, |position| result.push(position.index()));
        }
        if query.accepts_remainder() {
            legacy_visit_sequence_frames::<AUDIT>(state.cursor, &mut metrics, |cursor, _| {
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

fn parts_for(query_kind: ExperimentQuery, depth: usize, final_branch: Option<u8>) -> Vec<Vec<u8>> {
    let final_branch = final_branch.unwrap_or(SHARED_DISCRIMINATOR);
    match query_kind {
        ExperimentQuery::NestedMatch => {
            let mut parts = vec![nested_value(i64::from(SHARED_DISCRIMINATOR)); depth];
            *parts.last_mut().expect("depth is nonzero") = nested_value(i64::from(final_branch));
            parts
        }
        _ => {
            let shared = vec![SHARED_DISCRIMINATOR, 0xA5];
            let mut parts = vec![shared; depth];
            *parts.last_mut().expect("depth is nonzero") = vec![final_branch, 0xA5];
            parts
        }
    }
}

fn query_for(query_kind: ExperimentQuery, depth: usize) -> RhoPathQuery {
    match query_kind {
        ExperimentQuery::Exact => RhoPathQuery::structural_sequence(
            std::iter::repeat_n(RhoPathQueryAtom::Exact(vec![SHARED_DISCRIMINATOR, 0xA5]), depth),
            false,
        ),
        ExperimentQuery::Wildcard => RhoPathQuery::structural_sequence(
            std::iter::repeat_n(RhoPathQueryAtom::Wildcard, depth),
            false,
        ),
        ExperimentQuery::RepeatedCapture => RhoPathQuery::structural_sequence(
            std::iter::repeat_n(RhoPathQueryAtom::Capture(RhoPathCaptureId::new(7)), depth),
            false,
        ),
        ExperimentQuery::NestedMatch => RhoPathQuery::structural_sequence(
            std::iter::repeat_n(
                RhoPathQueryAtom::Match(nested_match_query(i64::from(SHARED_DISCRIMINATOR))),
                depth,
            ),
            false,
        ),
        ExperimentQuery::AcceptsRemainder => RhoPathQuery::structural_sequence(
            [RhoPathQueryAtom::Exact(vec![SHARED_DISCRIMINATOR, 0xA5])],
            true,
        ),
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

fn nested_match_query(discriminator: i64) -> RhoMatchQuery {
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
    RhoMatchQuery::new([
        RhoMatchQueryPart::Exact(prefix),
        RhoMatchQueryPart::Wildcard,
        RhoMatchQueryPart::Exact(vec![RHO_MATCH_FIELD_END, RHO_MATCH_NODE_END]),
    ])
}

fn never_decode(_: &[u8]) -> Datum<()> {
    unreachable!("the candidate checkpoint experiment never decodes data")
}
