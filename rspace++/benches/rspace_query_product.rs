//! Causal microbenchmark for the receive-first paired-PathMap query product.
//!
//! The control freezes the pre-`TrieRef` implementation: two mutable read
//! zippers descend and ascend together, and a completed wildcard frame clones
//! both zipper states. The treatment carries two borrowed `TrieRef` focuses;
//! each accepted label descends from the current focus and no ancestor stack is
//! copied. Both arms traverse the same `RhoPathV1` keys and return the same
//! sorted candidate positions before Criterion measures them.

use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use pathmap::PathMap;
use pathmap::utils::{BitMask, ByteMask, ByteMaskIter};
use pathmap::zipper::{
    ReadZipperUntracked, TrieRefBorrowed, Zipper, ZipperMoving, ZipperReadOnlySubtries,
    ZipperReadOnlyValues,
};
use rspace_plus_plus::rspace::rho_path::{RhoPathV1, encode_structural_sequence};

const ESCAPE: u8 = 0;
const ESCAPED_ZERO: u8 = 0xff;
const FRAME_END: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameStep {
    NeedMore,
    Complete,
    Invalid,
}

#[derive(Clone, Copy, Default)]
struct FrameScanner {
    outer_escape: bool,
    inner_escape: bool,
}

impl FrameScanner {
    fn feed(&mut self, byte: u8) -> FrameStep {
        if self.outer_escape {
            self.outer_escape = false;
            return match byte {
                ESCAPED_ZERO => self.feed_inner(ESCAPE),
                FRAME_END => FrameStep::Invalid,
                _ => FrameStep::Invalid,
            };
        }
        if byte == ESCAPE {
            self.outer_escape = true;
            FrameStep::NeedMore
        } else {
            self.feed_inner(byte)
        }
    }

    fn feed_inner(&mut self, byte: u8) -> FrameStep {
        if self.inner_escape {
            self.inner_escape = false;
            return match byte {
                ESCAPED_ZERO => FrameStep::NeedMore,
                FRAME_END => FrameStep::Complete,
                _ => FrameStep::Invalid,
            };
        }
        if byte == ESCAPE {
            self.inner_escape = true;
        }
        FrameStep::NeedMore
    }
}

#[derive(Clone)]
struct CursorProduct<'a> {
    availability: ReadZipperUntracked<'a, 'static, u64>,
    positions: ReadZipperUntracked<'a, 'static, u64>,
}

impl<'a> CursorProduct<'a> {
    fn new(availability: &'a PathMap<u64>, positions: &'a PathMap<u64>) -> Self {
        Self {
            availability: availability.read_zipper(),
            positions: positions.read_zipper(),
        }
    }

    fn descend(&mut self, path: &[u8]) -> bool {
        self.availability.descend_to_existing(path) == path.len() &&
            self.positions.descend_to_existing(path) == path.len()
    }

    fn descend_byte(&mut self, byte: u8) {
        self.availability.descend_to_byte(byte);
        self.positions.descend_to_byte(byte);
    }

    fn ascend_byte(&mut self) {
        let availability = self.availability.ascend_byte();
        let positions = self.positions.ascend_byte();
        debug_assert_eq!(availability, positions);
    }

    fn child_mask(&self) -> ByteMask {
        self.availability.child_mask() & self.positions.child_mask()
    }

    fn position(&self) -> Option<u64> {
        self.availability.get_val()?;
        self.positions.get_val().copied()
    }
}

#[derive(Clone)]
struct FocusProduct<'a> {
    availability: TrieRefBorrowed<'a, u64>,
    positions: TrieRefBorrowed<'a, u64>,
}

impl<'a> FocusProduct<'a> {
    fn new(availability: &'a PathMap<u64>, positions: &'a PathMap<u64>) -> Self {
        Self {
            availability: availability.trie_ref_at_path(&[]),
            positions: positions.trie_ref_at_path(&[]),
        }
    }

    fn descendant(&self, path: &[u8]) -> Option<Self> {
        let availability = self.availability.trie_ref_at_path(path);
        let positions = self.positions.trie_ref_at_path(path);
        (availability.path_exists() && positions.path_exists()).then_some(Self {
            availability,
            positions,
        })
    }

    fn child(&self, byte: u8) -> Self {
        debug_assert!(self.child_mask().test_bit(byte));
        self.descendant(&[byte])
            .expect("the intersected mask proves both children exist")
    }

    fn child_mask(&self) -> ByteMask {
        self.availability.child_mask() & self.positions.child_mask()
    }

    fn position(&self) -> Option<u64> {
        self.availability.get_val()?;
        self.positions.get_val().copied()
    }
}

fn visit_cursor_frames(mut cursor: CursorProduct<'_>, mut visitor: impl FnMut(CursorProduct<'_>)) {
    let mut levels: Vec<(ByteMaskIter, FrameScanner)> =
        vec![(cursor.child_mask().iter(), FrameScanner::default())];
    while let Some((children, scanner_base)) = levels.last_mut() {
        match children.next() {
            Some(byte) => {
                let mut scanner = *scanner_base;
                match scanner.feed(byte) {
                    FrameStep::NeedMore => {
                        cursor.descend_byte(byte);
                        levels.push((cursor.child_mask().iter(), scanner));
                    }
                    FrameStep::Complete => {
                        cursor.descend_byte(byte);
                        visitor(cursor.clone());
                        cursor.ascend_byte();
                    }
                    FrameStep::Invalid => {}
                }
            }
            None => {
                levels.pop();
                if !levels.is_empty() {
                    cursor.ascend_byte();
                }
            }
        }
    }
}

struct FocusLevel<'a> {
    focus: FocusProduct<'a>,
    children: ByteMaskIter,
    scanner: FrameScanner,
}

fn visit_focus_frames(focus: FocusProduct<'_>, mut visitor: impl FnMut(FocusProduct<'_>)) {
    let mut levels = vec![FocusLevel {
        children: focus.child_mask().iter(),
        focus,
        scanner: FrameScanner::default(),
    }];
    while let Some(level) = levels.last_mut() {
        match level.children.next() {
            Some(byte) => {
                let mut scanner = level.scanner;
                match scanner.feed(byte) {
                    FrameStep::NeedMore => {
                        let focus = level.focus.child(byte);
                        levels.push(FocusLevel {
                            children: focus.child_mask().iter(),
                            focus,
                            scanner,
                        });
                    }
                    FrameStep::Complete => visitor(level.focus.child(byte)),
                    FrameStep::Invalid => {}
                }
            }
            None => {
                levels.pop();
            }
        }
    }
}

fn visit_cursor_values(mut cursor: CursorProduct<'_>, mut visitor: impl FnMut(u64)) {
    if let Some(position) = cursor.position() {
        visitor(position);
    }
    let mut levels = vec![cursor.child_mask().iter()];
    while let Some(children) = levels.last_mut() {
        match children.next() {
            Some(byte) => {
                cursor.descend_byte(byte);
                if let Some(position) = cursor.position() {
                    visitor(position);
                }
                levels.push(cursor.child_mask().iter());
            }
            None => {
                levels.pop();
                if !levels.is_empty() {
                    cursor.ascend_byte();
                }
            }
        }
    }
}

fn visit_focus_values(focus: FocusProduct<'_>, mut visitor: impl FnMut(u64)) {
    if let Some(position) = focus.position() {
        visitor(position);
    }
    let mut levels = vec![(focus.clone(), focus.child_mask().iter())];
    while let Some((focus, children)) = levels.last_mut() {
        match children.next() {
            Some(byte) => {
                let child = focus.child(byte);
                if let Some(position) = child.position() {
                    visitor(position);
                }
                levels.push((child.clone(), child.child_mask().iter()));
            }
            None => {
                levels.pop();
            }
        }
    }
}

fn exact_second_frame(second: &[u8]) -> Vec<u8> {
    let empty_first = encode_structural_sequence([&[][..]]);
    let with_second = encode_structural_sequence([&[][..], second]);
    let empty_prefix = RhoPathV1::relative_datum_structure_prefix(&empty_first);
    let full_prefix = RhoPathV1::relative_datum_structure_prefix(&with_second);
    assert!(full_prefix.starts_with(&empty_prefix));
    full_prefix[empty_prefix.len()..].to_vec()
}

struct Fixture {
    availability: PathMap<u64>,
    positions: PathMap<u64>,
    second_frame: Vec<u8>,
    expected_positions: Vec<u64>,
    expected_checksum: u64,
}

impl Fixture {
    fn new(cardinality: usize) -> Self {
        let mut availability = PathMap::new();
        let mut positions = PathMap::new();
        let target = cardinality / 2;
        let target_second = format!("class/shared-prefix/{target:08x}").into_bytes();
        for index in 0..cardinality {
            let first = format!("record/shared-prefix/{index:08x}").into_bytes();
            let second = format!("class/shared-prefix/{index:08x}").into_bytes();
            let structure = encode_structural_sequence([first.as_slice(), second.as_slice()]);
            let mut identity = b"identity/shared-prefix/".to_vec();
            identity.extend_from_slice(&(index as u64).to_be_bytes());
            let path = RhoPathV1::relative_datum_key(&structure, &identity);
            availability.insert(&path, 1);
            positions.insert(&path, index as u64);
        }
        Self {
            availability,
            positions,
            second_frame: exact_second_frame(&target_second),
            expected_positions: vec![target as u64],
            expected_checksum: target as u64 + 1,
        }
    }

    fn visit_cursor_candidates(&self, mut visitor: impl FnMut(u64)) {
        let mut root = CursorProduct::new(&self.availability, &self.positions);
        assert!(root.descend(&[1]));
        visit_cursor_frames(root, |mut first_end| {
            if first_end.descend(&self.second_frame) && first_end.descend(&[0, 0]) {
                visit_cursor_values(first_end, &mut visitor);
            }
        });
    }

    fn cursor_positions(&self) -> Vec<u64> {
        let mut positions = Vec::new();
        self.visit_cursor_candidates(|position| positions.push(position));
        positions
    }

    fn cursor_control(&self) -> u64 {
        let mut checksum = 0u64;
        self.visit_cursor_candidates(|position| {
            checksum = checksum.wrapping_add(position.wrapping_add(1));
        });
        checksum
    }

    fn visit_trie_ref_candidates(&self, mut visitor: impl FnMut(u64)) {
        let root = FocusProduct::new(&self.availability, &self.positions)
            .descendant(&[1])
            .expect("the structural frame begins with version one");
        visit_focus_frames(root, |first_end| {
            if let Some(end) = first_end
                .descendant(&self.second_frame)
                .and_then(|focus| focus.descendant(&[0, 0]))
            {
                visit_focus_values(end, &mut visitor);
            }
        });
    }

    fn trie_ref_positions(&self) -> Vec<u64> {
        let mut positions = Vec::new();
        self.visit_trie_ref_candidates(|position| positions.push(position));
        positions
    }

    fn trie_ref_treatment(&self) -> u64 {
        let mut checksum = 0u64;
        self.visit_trie_ref_candidates(|position| {
            checksum = checksum.wrapping_add(position.wrapping_add(1));
        });
        checksum
    }
}

fn bench_query_product(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("rspace_query_product_shared_prefix");
    for cardinality in [16usize, 256, 4_096] {
        let fixture = Fixture::new(cardinality);
        let control_positions = fixture.cursor_positions();
        let treatment_positions = fixture.trie_ref_positions();
        assert_eq!(control_positions, fixture.expected_positions);
        assert_eq!(treatment_positions, control_positions);
        assert_eq!(fixture.cursor_control(), fixture.expected_checksum);
        assert_eq!(fixture.trie_ref_treatment(), fixture.expected_checksum);

        // Three explicit protocol warm-ups per arm precede Criterion's timed
        // warm-up, and their results are checked rather than discarded blindly.
        for _ in 0..3 {
            assert_eq!(fixture.cursor_control(), fixture.expected_checksum);
            assert_eq!(fixture.trie_ref_treatment(), fixture.expected_checksum);
        }

        group.throughput(Throughput::Elements(1));
        group.bench_function(BenchmarkId::new("control", cardinality), |bencher| {
            bencher.iter(|| black_box(fixture.cursor_control()))
        });
        group.bench_function(BenchmarkId::new("treatment", cardinality), |bencher| {
            bencher.iter(|| black_box(fixture.trie_ref_treatment()))
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
    targets = bench_query_product
}
criterion_main!(benches);
