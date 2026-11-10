use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ops::Range;
use std::thread;

use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;
use proptest::prelude::*;

use super::{
    AsRefIndexedByteSource, CanonicalCompactTrieCursor, CanonicalCompactTrieIndexedTopologyCursor,
    CanonicalCompactTrieTopologyCursor, CanonicalCompactTrieTopologyEvent, CompactTrie,
    CompactTrieTopologyEvent, IndexedByteSource, direct_semantic_node, new_act_work_counters,
    validate_direct_canonical_act,
};

#[derive(Clone)]
struct FragmentedSource {
    chunks: Vec<Vec<u8>>,
    starts: Vec<usize>,
    len: usize,
}

impl FragmentedSource {
    fn new(bytes: &[u8], mut state: u64) -> Self {
        let mut chunks = Vec::new();
        let mut starts = Vec::new();
        let mut position = 0;
        while position < bytes.len() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = ((state as usize) % 17 + 1).min(bytes.len() - position);
            starts.push(position);
            chunks.push(bytes[position..position + len].to_vec());
            position += len;
        }
        Self {
            chunks,
            starts,
            len: bytes.len(),
        }
    }
}

impl IndexedByteSource for FragmentedSource {
    fn len(&self) -> usize { self.len }

    fn byte_at(&self, index: usize) -> Option<u8> {
        if index >= self.len {
            return None;
        }
        let chunk = self.starts.partition_point(|start| *start <= index) - 1;
        self.chunks[chunk].get(index - self.starts[chunk]).copied()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SourceAccess {
    Byte(usize),
    Copy { start: usize, len: usize },
    Contiguous(Range<usize>),
    Equal(Range<usize>, Range<usize>),
}

struct ObservedSource<S> {
    inner: S,
    accesses: RefCell<Vec<SourceAccess>>,
}

impl<S> ObservedSource<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            accesses: RefCell::new(Vec::new()),
        }
    }

    fn take_accesses(&self) -> Vec<SourceAccess> {
        std::mem::take(&mut *self.accesses.borrow_mut())
    }
}

impl<S: IndexedByteSource> IndexedByteSource for ObservedSource<S> {
    fn len(&self) -> usize { self.inner.len() }

    fn byte_at(&self, index: usize) -> Option<u8> {
        self.accesses.borrow_mut().push(SourceAccess::Byte(index));
        self.inner.byte_at(index)
    }

    fn contiguous_range(&self, range: Range<usize>) -> Option<&[u8]> {
        self.accesses
            .borrow_mut()
            .push(SourceAccess::Contiguous(range.clone()));
        self.inner.contiguous_range(range)
    }

    fn copy_exact(&self, start: usize, output: &mut [u8]) -> bool {
        self.accesses.borrow_mut().push(SourceAccess::Copy {
            start,
            len: output.len(),
        });
        self.inner.copy_exact(start, output)
    }

    fn ranges_equal(&self, left: Range<usize>, right: Range<usize>) -> bool {
        self.accesses
            .borrow_mut()
            .push(SourceAccess::Equal(left.clone(), right.clone()));
        self.inner.ranges_equal(left, right)
    }
}

fn legacy_event_access_oracle(bytes: &[u8]) -> Vec<(CompactTrieTopologyEvent, Option<usize>)> {
    enum Work {
        Visit(usize),
        Edge(u8),
        Leave(usize),
    }
    let trie = CompactTrie::parse_canonical(bytes).unwrap();
    let mut events = Vec::new();
    let mut work = vec![Work::Visit(trie.parsed.root_id)];
    while let Some(next) = work.pop() {
        match next {
            Work::Visit(id) => {
                let node = &trie.parsed.semantic_nodes[&id];
                let line = node
                    .line
                    .as_ref()
                    .map(|line| line.suffix.clone())
                    .unwrap_or(0..0);
                events.push((
                    CompactTrieTopologyEvent::Value(line.is_empty() && node.value.is_some()),
                    None,
                ));
                for offset in line.clone() {
                    events.push((CompactTrieTopologyEvent::Edge(bytes[offset]), Some(offset)));
                    events.push((
                        CompactTrieTopologyEvent::Value(
                            offset + 1 == line.end && node.value.is_some(),
                        ),
                        None,
                    ));
                }
                work.push(Work::Leave(line.len() + 1));
                for &(byte, child) in node.children.iter().rev() {
                    work.push(Work::Visit(child));
                    work.push(Work::Edge(byte));
                }
            }
            Work::Edge(byte) => events.push((CompactTrieTopologyEvent::Edge(byte), None)),
            Work::Leave(count) => {
                events.extend((0..count).map(|_| (CompactTrieTopologyEvent::Leave, None)));
            }
        }
    }
    assert_eq!(events.iter().map(|(event, _)| *event).collect::<Vec<_>>(), old_topology(bytes));
    events
}

#[derive(Clone, Copy)]
enum AccessMutation {
    None,
    Metadata(usize),
    DuplicateSuffix,
}

fn check_observed_event<S: IndexedByteSource>(
    cursor: &mut CanonicalCompactTrieIndexedTopologyCursor<ObservedSource<S>>,
    expected: Option<&(CompactTrieTopologyEvent, Option<usize>)>,
    mutation: AccessMutation,
) -> Result<(), String> {
    match mutation {
        AccessMutation::None => {}
        AccessMutation::Metadata(offset) => {
            let _ = cursor.source().byte_at(offset);
        }
        AccessMutation::DuplicateSuffix => {
            let offset = expected
                .and_then(|(_, offset)| *offset)
                .ok_or("duplicate control requires a line edge")?;
            let _ = cursor.source().byte_at(offset);
        }
    }
    let actual = cursor.next_event().map_err(|error| error.to_string())?;
    let accesses = cursor.source().take_accesses();
    let expected_accesses = expected
        .and_then(|(_, offset)| *offset)
        .map(SourceAccess::Byte)
        .into_iter()
        .collect::<Vec<_>>();
    if accesses != expected_accesses {
        return Err(format!(
            "source access mismatch: expected {expected_accesses:?}, actual {accesses:?}"
        ));
    }
    if let Some((_, Some(offset))) = expected {
        if !matches!(actual, Some(CanonicalCompactTrieTopologyEvent::Edge { source_offset, .. }) if source_offset == *offset)
        {
            return Err(format!("line provenance mismatch at {offset}: {actual:?}"));
        }
    }
    let erased = actual.map(|event| match event {
        CanonicalCompactTrieTopologyEvent::Value { value, .. } => {
            CompactTrieTopologyEvent::Value(value.is_some())
        }
        CanonicalCompactTrieTopologyEvent::Edge { byte, .. } => {
            CompactTrieTopologyEvent::Edge(byte)
        }
        CanonicalCompactTrieTopologyEvent::Leave { .. } => CompactTrieTopologyEvent::Leave,
    });
    if erased != expected.map(|(event, _)| *event) {
        return Err(format!("event mismatch: expected {expected:?}, actual {actual:?}"));
    }
    Ok(())
}

fn check_observed_rewind<S: IndexedByteSource>(
    cursor: &mut CanonicalCompactTrieIndexedTopologyCursor<ObservedSource<S>>,
) -> Result<(), String> {
    cursor.rewind();
    let accesses = cursor.source().take_accesses();
    if !accesses.is_empty() {
        return Err(format!("rewind accessed source: {accesses:?}"));
    }
    Ok(())
}

fn check_independent_source_access<S: IndexedByteSource>(
    bytes: &[u8],
    source: S,
) -> Result<(), String> {
    let expected = legacy_event_access_oracle(bytes);
    let mut cursor =
        CanonicalCompactTrieIndexedTopologyCursor::from_source(ObservedSource::new(source))
            .map_err(|error| error.to_string())?;
    if cursor.source().take_accesses().is_empty() {
        return Err("admission made no observed source accesses".to_owned());
    }
    let partial_line = expected
        .iter()
        .position(|(_, offset)| offset.is_some())
        .map_or(1, |index| index + 1);
    for partial in [None, Some(expected.len() / 2), Some(partial_line)] {
        if let Some(count) = partial {
            for event in expected.iter().take(count) {
                check_observed_event(&mut cursor, Some(event), AccessMutation::None)?;
            }
            check_observed_rewind(&mut cursor)?;
        }
        for event in &expected {
            check_observed_event(&mut cursor, Some(event), AccessMutation::None)?;
        }
        for _ in 0..3 {
            check_observed_event(&mut cursor, None, AccessMutation::None)?;
        }
        check_observed_rewind(&mut cursor)?;
    }
    Ok(())
}

fn assert_independent_source_access(bytes: &[u8]) {
    check_independent_source_access(bytes, AsRefIndexedByteSource::new(bytes)).unwrap();
    check_independent_source_access(bytes, FragmentedSource::new(bytes, 0x2ffd_72db_d01a_dfb7))
        .unwrap();
}

#[test]
fn independent_source_witness_records_all_access_methods_without_double_counting() {
    let source = ObservedSource::new(AsRefIndexedByteSource::new(b"aba"));
    assert_eq!(source.byte_at(0), Some(b'a'));
    assert_eq!(source.contiguous_range(0..2), Some(b"ab".as_slice()));
    assert!(source.copy_exact(1, &mut [0; 2]));
    assert!(source.ranges_equal(0..1, 2..3));
    assert_eq!(source.take_accesses(), vec![
        SourceAccess::Byte(0),
        SourceAccess::Contiguous(0..2),
        SourceAccess::Copy { start: 1, len: 2 },
        SourceAccess::Equal(0..1, 2..3)
    ]);
    assert!(!source.copy_exact(usize::MAX, &mut [0; 2]));
    assert_eq!(source.take_accesses(), vec![SourceAccess::Copy {
        start: usize::MAX,
        len: 2
    }]);
}

#[test]
fn independent_source_witness_checks_empty_sparse_dense_and_compressed_rewinds() {
    for operations in [
        vec![],
        vec![(vec![1], Some(0)), (vec![7], Some(1)), (vec![200], None)],
        (0u16..=255)
            .map(|byte| (vec![byte as u8], Some(u64::from(byte))))
            .collect(),
        vec![
            (b"compressed".to_vec(), Some(0)),
            (b"compressed/prefix".to_vec(), Some(1)),
            (b"compressed/prefix/tail".to_vec(), None),
        ],
    ] {
        assert_independent_source_access(&writer_image_from_operations(&operations));
    }
}

#[test]
fn independent_source_witness_rejects_metadata_and_duplicate_suffix_reads() {
    let bytes = writer_image_from_operations(&[(b"compressed/suffix".to_vec(), Some(7))]);
    let expected = legacy_event_access_oracle(&bytes);
    let root = CompactTrie::parse_canonical(&bytes).unwrap().parsed.root_id;
    let mut cursor = CanonicalCompactTrieIndexedTopologyCursor::from_source(ObservedSource::new(
        AsRefIndexedByteSource::new(bytes.as_slice()),
    ))
    .unwrap();
    cursor.source().take_accesses();
    let error = check_observed_event(&mut cursor, expected.first(), AccessMutation::Metadata(root))
        .unwrap_err();
    assert!(error.starts_with("source access mismatch:"), "{error}");
    check_observed_rewind(&mut cursor).unwrap();
    for event in &expected {
        if event.1.is_some() {
            let error =
                check_observed_event(&mut cursor, Some(event), AccessMutation::DuplicateSuffix)
                    .unwrap_err();
            assert!(error.starts_with("source access mismatch:"), "{error}");
            return;
        }
        check_observed_event(&mut cursor, Some(event), AccessMutation::None).unwrap();
    }
    panic!("duplicate control never reached a compressed suffix");
}

fn canonical_image(entries: impl IntoIterator<Item = (Vec<u8>, u64)>) -> Vec<u8> {
    let mut map = PathMap::new();
    for (path, value) in entries {
        map.set_val_at(path, value);
    }
    ArenaCompactTree::from_zipper(map.read_zipper(), |value| *value)
        .get_data()
        .to_vec()
}

fn old_topology(bytes: &[u8]) -> Vec<CompactTrieTopologyEvent> {
    let trie = CompactTrie::parse_canonical(bytes).expect("PathMap image passes oracle admission");
    let mut events = Vec::new();
    trie.visit_topology(|event| {
        events.push(event);
        Ok::<_, ()>(())
    })
    .unwrap();
    events
}

fn new_topology(bytes: &[u8]) -> Vec<CanonicalCompactTrieTopologyEvent> {
    let mut cursor = CanonicalCompactTrieTopologyCursor::new(bytes).expect("canonical topology");
    std::iter::from_fn(|| cursor.next_event()).collect()
}

fn erase_ordinals(events: &[CanonicalCompactTrieTopologyEvent]) -> Vec<CompactTrieTopologyEvent> {
    events
        .iter()
        .map(|event| match event {
            CanonicalCompactTrieTopologyEvent::Value { value, .. } => {
                CompactTrieTopologyEvent::Value(value.is_some())
            }
            CanonicalCompactTrieTopologyEvent::Edge { byte, .. } => {
                CompactTrieTopologyEvent::Edge(*byte)
            }
            CanonicalCompactTrieTopologyEvent::Leave { .. } => CompactTrieTopologyEvent::Leave,
        })
        .collect()
}

fn oracle_endpoints(bytes: &[u8]) -> Vec<(Vec<u8>, Option<u64>)> {
    let trie = CompactTrie::parse_canonical(bytes).unwrap();
    let mut endpoints = Vec::new();
    trie.visit(|path, value| {
        endpoints.push((path.to_vec(), value));
        Ok::<_, ()>(())
    })
    .unwrap();
    endpoints
}

fn planned_endpoints(bytes: &[u8]) -> Vec<(Vec<u8>, Option<u64>)> {
    let mut cursor = CanonicalCompactTrieCursor::new(bytes).unwrap();
    let mut endpoints = Vec::new();
    while let Some(endpoint) = cursor.next_endpoint() {
        endpoints.push((endpoint.path.to_vec(), endpoint.value));
    }
    endpoints
}

/// Mechanical refinement witness for the production optimization that stores
/// semantic nodes in forward preorder instead of the Rocq model's
/// reverse-preorder environment plus identifier lookup.
fn assert_forward_preorder_plan_is_exact(bytes: &[u8]) {
    let source = AsRefIndexedByteSource::new(bytes);
    let plan = validate_direct_canonical_act(&source).expect("canonical writer image has a plan");
    assert!(!plan.records.is_empty(), "a canonical ACT always has a root record");

    let mut physical_ids = BTreeSet::new();
    let mut remaining_children = Vec::<usize>::new();
    for (index, record) in plan.records.iter().enumerate() {
        assert!(
            physical_ids.insert(record.node_id),
            "each discovered semantic node has one physical identity",
        );
        assert!(record.node_id < bytes.len(), "record identity is a source offset");

        if index == 0 {
            assert!(record.incoming().is_none(), "the root has no incoming edge");
        } else {
            while remaining_children.last() == Some(&0) {
                remaining_children.pop();
            }
            let parent = remaining_children
                .last_mut()
                .expect("every non-root preorder record has a pending parent");
            *parent -= 1;
            assert!(record.incoming().is_some(), "every non-root has one incoming edge");
        }

        let mut branch_edges = Vec::new();
        let mut counters = new_act_work_counters();
        let semantic =
            direct_semantic_node(&source, record.node_id, &mut branch_edges, &mut counters)
                .expect("a validated record reparses as its exact source semantic node");
        assert_eq!(record.value(), semantic.value);
        assert_eq!(record.line_suffix(), semantic.line.as_ref().map(|line| line.suffix.clone()),);
        assert_eq!(record.child_count(), semantic.children.len());
        remaining_children.push(record.child_count());
    }
    while remaining_children.last() == Some(&0) {
        remaining_children.pop();
    }
    assert!(
        remaining_children.is_empty(),
        "the implicit child-count schedule consumes every record exactly once",
    );
    assert_eq!(physical_ids.len(), plan.records.len());
}

#[derive(Clone, Copy)]
struct WriterImagePolicy;

impl crate::rspace::guarded_pathmap::DisposeValue<u64> for WriterImagePolicy {
    fn dispose(&self, _value: u64) {}
}

impl crate::rspace::guarded_pathmap::TeardownBarrier<u64> for WriterImagePolicy {
    fn make_barrier() -> u64 { 0 }
}

fn writer_image_from_operations(operations: &[(Vec<u8>, Option<u64>)]) -> Vec<u8> {
    let mut map = crate::rspace::guarded_pathmap::GuardedPathMap::<
        u64,
        WriterImagePolicy,
        WriterImagePolicy,
    >::new_with_barrier(WriterImagePolicy);
    for (path, value) in operations {
        match value {
            Some(value) => {
                map.set_val_at(path, *value);
            }
            None => {
                map.create_path(path);
            }
        }
    }
    map.compact_u64_bytes(|value| *value)
}

fn check_forced_collision_history(
    lines: &[Vec<u8>],
) -> Result<(), proptest::test_runner::TestCaseError> {
    let mut source_bytes = Vec::new();
    let mut latest: Option<(&[u8], super::ActLineData)> = None;
    let mut expected = Vec::new();
    let mut collisions = 0;
    for line in lines {
        prop_assert!(!line.is_empty());
        prop_assert!(line.len() <= usize::from(super::ACT_VARINT_BIAS));
        let descriptor = match &latest {
            Some((content, descriptor)) if *content == line.as_slice() => descriptor.clone(),
            previous => {
                collisions += usize::from(previous.is_some());
                let start = source_bytes.len();
                source_bytes.push(line.len() as u8);
                let suffix_start = source_bytes.len();
                source_bytes.extend_from_slice(line);
                super::ActLineData {
                    suffix: suffix_start..source_bytes.len(),
                    encoded: start..source_bytes.len(),
                }
            }
        };
        expected.push((descriptor.clone(), source_bytes.len(), collisions));
        latest = Some((line.as_slice(), descriptor));
    }

    let source = AsRefIndexedByteSource::new(source_bytes.as_slice());
    let mut replay = super::DirectCanonicalLineReplay::new(&source);
    replay.force_hash(7);
    let mut counters = new_act_work_counters();
    let mut position = 0;
    for (ordinal, (descriptor, expected_position, expected_collisions)) in
        expected.iter().enumerate()
    {
        let id = replay
            .expected_line_id(descriptor, &mut position, &mut counters)
            .map_err(|error| proptest::test_runner::TestCaseError::fail(error.to_string()))?;
        prop_assert_eq!(id, descriptor.encoded.start, "line ordinal {}", ordinal);
        prop_assert_eq!(position, *expected_position, "line ordinal {}", ordinal);
        prop_assert_eq!(
            counters.line_hash_collisions,
            *expected_collisions,
            "line ordinal {}",
            ordinal
        );
    }
    Ok(())
}

#[test]
fn forced_collision_history_covers_reuse_aba_and_unequal_lengths() {
    check_forced_collision_history(&[
        b"a".to_vec(),
        b"a".to_vec(),
        b"b".to_vec(),
        b"a".to_vec(),
        b"a".to_vec(),
        b"longer".to_vec(),
        b"longer".to_vec(),
        b"a".to_vec(),
    ])
    .unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn forced_collision_histories_match_latest_content_and_physical_writer_order(
        lines in proptest::collection::vec(
            prop_oneof![
                Just(b"a".to_vec()),
                Just(b"b".to_vec()),
                Just(b"aa".to_vec()),
                proptest::collection::vec(any::<u8>(), 1..65),
            ],
            0..48,
        ),
    ) {
        check_forced_collision_history(&lines)?;
    }

    #[test]
    fn arbitrary_pathmap_writer_images_are_accepted_with_exact_preorder_plans(
        operations in proptest::collection::vec(
            (
                proptest::collection::vec(any::<u8>(), 1..64),
                prop_oneof![Just(None), any::<u64>().prop_map(Some)],
            ),
            0..48,
        ),
    ) {
        let bytes = writer_image_from_operations(&operations);
        prop_assert!(CompactTrie::parse_canonical(&bytes).is_ok());
        prop_assert!(CanonicalCompactTrieTopologyCursor::new(bytes.as_slice()).is_ok());
        prop_assert_eq!(erase_ordinals(&new_topology(&bytes)), old_topology(&bytes));
        prop_assert_eq!(planned_endpoints(&bytes), oracle_endpoints(&bytes));
        assert_forward_preorder_plan_is_exact(&bytes);
        assert_independent_source_access(&bytes);
        assert_summary_inventory_and_cached_access(&bytes);
    }
}

#[test]
fn unequal_children_wrappers_and_reused_lines_preserve_exact_plan_layout() {
    let repeated = b"/identical/compressed/suffix";
    let mut first = vec![0x10];
    first.extend_from_slice(repeated);
    let mut second = vec![0x20];
    second.extend_from_slice(repeated);
    let mut first_child = first.clone();
    first_child.extend_from_slice(b"/x");
    let mut second_child = second.clone();
    second_child.extend_from_slice(b"/a-much-longer-descendant");
    let mut value_free = vec![0x30];
    value_free.extend_from_slice(repeated);

    let operations = vec![
        (first, Some(1)),
        (first_child, Some(u64::MAX)),
        (second, Some(2)),
        (second_child, Some(3)),
        (value_free, None),
    ];
    let bytes = writer_image_from_operations(&operations);
    assert_eq!(erase_ordinals(&new_topology(&bytes)), old_topology(&bytes));
    assert_eq!(planned_endpoints(&bytes), oracle_endpoints(&bytes));
    assert_forward_preorder_plan_is_exact(&bytes);

    let source = FragmentedSource::new(&bytes, 0xd131_0ba6_98df_b5ac);
    let mut state = CanonicalCompactTrieIndexedTopologyCursor::from_source(source).unwrap();
    while state.next_event().unwrap().is_some() {}
    let counters = state.work_counters();
    assert!(counters.wrapper_node_parses >= 2);
    assert!(counters.line_equality_bytes >= repeated.len());
    assert_independent_source_access(&bytes);
    assert_eq!(counters.semantic_node_parses, counters.plan_records);
    assert_eq!(counters.node_reparses, 0);
}

#[test]
fn endpoint_provenance_borrows_only_complete_single_line_paths() {
    let path = b"one/prefix-compressed/path";
    let bytes = canonical_image([(path.to_vec(), 7)]);
    let mut cursor = CanonicalCompactTrieCursor::new(bytes.as_slice()).unwrap();
    let endpoint = cursor.next_value_endpoint().expect("one value endpoint");
    let range = endpoint
        .contiguous_path_range
        .clone()
        .expect("one complete ACT line remains source-relative");
    assert_eq!(endpoint.path, path);
    assert_eq!(&bytes[range], path);
    assert!(cursor.next_value_endpoint().is_none());

    let bytes = canonical_image([(b"shared/a".to_vec(), 0), (b"shared/b".to_vec(), 1)]);
    let mut cursor = CanonicalCompactTrieCursor::new(bytes.as_slice()).unwrap();
    while let Some(endpoint) = cursor.next_value_endpoint() {
        assert!(
            endpoint.contiguous_path_range.is_none(),
            "a path assembled across a branch edge must not claim one source range"
        );
    }
}

fn assert_old_new_admission_equivalent(bytes: &[u8], case: impl std::fmt::Display) {
    let old = CompactTrie::parse_canonical(bytes)
        .map(|_| ())
        .map_err(|error| (error.to_string(), error.source_offset()));
    let new = CanonicalCompactTrieTopologyCursor::new(bytes)
        .map(|_| ())
        .map_err(|error| (error.to_string(), error.source_offset()));
    let indexed = CanonicalCompactTrieIndexedTopologyCursor::from_source(FragmentedSource::new(
        bytes,
        0x517c_c1b7_2722_0a95 ^ bytes.len() as u64,
    ))
    .map(|_| ())
    .map_err(|error| (error.to_string(), error.source_offset()));
    assert_eq!(indexed, new, "fragmented provenance differs: {case}");

    match (old, new) {
        (Ok(()), Ok(())) => {}
        (Err((old_message, old_offset)), Err((new_message, new_offset))) => {
            assert_eq!(new_message, old_message, "{case}");
            if let Some(old_offset) = old_offset {
                assert_eq!(new_offset, Some(old_offset), "{case}");
            } else if let Some(new_offset) = new_offset {
                assert!(
                    new_offset <= bytes.len(),
                    "new provenance lies beyond the source/end boundary: {case}",
                );
                if new_offset == bytes.len() {
                    assert!(
                        new_message.contains("truncated"),
                        "only truncation may point at the end boundary: {case}",
                    );
                }
            }
        }
        (old, new) => panic!("acceptance differs for {case}: old={old:?}, new={new:?}"),
    }
}

#[test]
fn validated_plan_preserves_old_topology_endpoints_rewind_and_fragmented_provenance() {
    let bytes = canonical_image([
        (b"shared/prefix/a".to_vec(), 0),
        (b"shared/prefix/b".to_vec(), 1),
        (b"shared/prefix/b/tail".to_vec(), 2),
        (b"wide/x".to_vec(), 3),
    ]);
    let expected = old_topology(&bytes);
    let first = new_topology(&bytes);
    assert_eq!(erase_ordinals(&first), expected);
    assert_eq!(planned_endpoints(&bytes), oracle_endpoints(&bytes));

    let fragmented = FragmentedSource::new(&bytes, 0x243f_6a88_85a3_08d3);
    let mut state = CanonicalCompactTrieIndexedTopologyCursor::from_source(fragmented).unwrap();
    let mut fragmented_first = Vec::new();
    while let Some(event) = state.next_event().unwrap() {
        fragmented_first.push(event);
    }
    assert_eq!(fragmented_first, first);

    state.rewind();
    let mut replay = Vec::new();
    while let Some(event) = state.next_event().unwrap() {
        replay.push(event);
    }
    assert_eq!(replay, first);
    let counters = state.work_counters();
    assert_eq!(counters.semantic_node_parses, counters.plan_records);
    assert_eq!(counters.node_reparses, 0);
    assert_eq!(counters.event_metadata_source_reads, 0);
    assert!(counters.wrapper_node_parses > 0);
    assert!(counters.event_line_source_reads > 0);
    assert!(counters.plan_bytes >= counters.plan_records);
    #[cfg(target_pointer_width = "64")]
    assert!(counters.plan_record_bytes <= 56);
    assert_eq!(counters.topology_cursor_bytes, std::mem::size_of_val(&state));
    // The instrumented build deliberately carries two complete counter sets;
    // its cursor/state layout is measurement overhead, not a production ABI or
    // memory budget. Ordinary builds compile those fields out entirely.
    assert!(
        counters.validation_branch_capacity_bytes_peak >= counters.validation_branch_bytes_peak
    );
    assert!(
        counters.validation_record_capacity_bytes_peak >=
            counters.plan_records * counters.plan_record_bytes
    );
    assert!(counters.validation_seen_bitmap_bytes <= bytes.len().div_ceil(8));
}

#[test]
fn sparse_and_dense_branches_are_streamed_once_into_the_plan() {
    let sparse = canonical_image([(vec![1], 0), (vec![7], 1), (vec![200], 2)]);
    let sparse_source = FragmentedSource::new(&sparse, 1);
    let mut sparse_state =
        CanonicalCompactTrieIndexedTopologyCursor::from_source(sparse_source).unwrap();
    while sparse_state.next_event().unwrap().is_some() {}
    let sparse_counters = sparse_state.work_counters();
    assert!(sparse_counters.sparse_branch_bytes >= 3);
    assert_eq!(sparse_counters.dense_mask_bytes, 0);
    assert_eq!(sparse_counters.node_reparses, 0);
    assert_eq!(sparse_counters.event_metadata_source_reads, 0);

    let dense = canonical_image((0u16..=255).map(|byte| (vec![byte as u8], byte as u64)));
    let dense_source = FragmentedSource::new(&dense, 2);
    let mut dense_state =
        CanonicalCompactTrieIndexedTopologyCursor::from_source(dense_source).unwrap();
    while dense_state.next_event().unwrap().is_some() {}
    let dense_counters = dense_state.work_counters();
    assert_eq!(dense_counters.dense_mask_bytes, 32);
    assert_eq!(dense_counters.node_reparses, 0);
    assert_eq!(dense_counters.event_metadata_source_reads, 0);
    assert_eq!(erase_ordinals(&new_topology(&dense)), old_topology(&dense));
}

fn legacy_value_inventory(bytes: &[u8]) -> Vec<(usize, u64)> {
    let trie = CompactTrie::parse_canonical(bytes).unwrap();
    let mut work = vec![trie.parsed.root_id];
    let mut values = Vec::new();
    while let Some(id) = work.pop() {
        let node = &trie.parsed.semantic_nodes[&id];
        if let Some(value) = node.value {
            values.push((node.wrapper.unwrap_or(id), value));
        }
        work.extend(node.children.iter().rev().map(|(_, child)| *child));
    }
    values
}

fn assert_cached_summary<S: IndexedByteSource>(
    cursor: &CanonicalCompactTrieIndexedTopologyCursor<ObservedSource<S>>,
    values: &[(usize, u64)],
) {
    let first_nonzero = values.iter().copied().find(|(_, value)| *value != 0);
    let first_nonsequential = values
        .iter()
        .enumerate()
        .find(|(ordinal, (_, value))| *value != *ordinal as u64)
        .map(|(ordinal, (offset, value))| (*offset, *value, ordinal as u64));
    cursor.source().take_accesses();
    let before = cursor.work_counters();
    for _ in 0..3 {
        let actual = cursor.value_summary();
        assert_eq!(actual.value_count(), values.len());
        assert_eq!(actual.first_nonzero(), first_nonzero);
        assert_eq!(actual.first_nonsequential(), first_nonsequential);
    }
    assert!(cursor.source().take_accesses().is_empty());
    assert_eq!(cursor.work_counters(), before);
}

fn assert_summary_inventory_and_cached_access(bytes: &[u8]) {
    let values = legacy_value_inventory(bytes);
    let contiguous = CanonicalCompactTrieTopologyCursor::new(bytes).unwrap();
    let source = ObservedSource::new(FragmentedSource::new(bytes, 0x51a7_4131));
    let mut indexed = CanonicalCompactTrieIndexedTopologyCursor::from_source(source).unwrap();
    assert_eq!(contiguous.value_summary(), indexed.value_summary());
    assert_cached_summary(&indexed, &values);
    let mut event_count = 0usize;
    while indexed.next_event().unwrap().is_some() {
        event_count += 1;
        if event_count % 7 == 0 {
            assert_cached_summary(&indexed, &values);
        }
    }
    assert_cached_summary(&indexed, &values);
    indexed.rewind();
    assert_cached_summary(&indexed, &values);
    assert!(indexed.next_event().unwrap().is_some());
    assert_cached_summary(&indexed, &values);
}

#[test]
fn cached_value_summary_matches_independent_preorder_offsets_and_latches() {
    for entries in [
        vec![],
        vec![(Vec::new(), 0)],
        vec![(Vec::new(), 9), (b"a".to_vec(), 8), (b"b".to_vec(), 7)],
        vec![(b"a".to_vec(), 0), (b"b".to_vec(), 1), (b"c".to_vec(), 2)],
        vec![(b"a".to_vec(), 0), (b"b".to_vec(), 0), (b"c".to_vec(), 0)],
        vec![(b"a".to_vec(), 0), (b"b".to_vec(), 1), (b"c".to_vec(), 7), (b"d".to_vec(), 8)],
    ] {
        assert_summary_inventory_and_cached_access(&canonical_image(entries));
    }
    let wrapped = canonical_image([
        (Vec::new(), 9),
        (b"a".to_vec(), 1),
        (b"shared-prefix".to_vec(), 7),
        (b"shared-prefix/a".to_vec(), 0),
        (b"shared-prefix/b".to_vec(), 3),
        (b"z".to_vec(), 99),
    ]);
    let oracle = CompactTrie::parse_canonical(&wrapped).unwrap();
    assert!(oracle.parsed.semantic_nodes.iter().any(|(id, node)| {
        node.value.is_some() &&
            node.line.is_some() &&
            !node.children.is_empty() &&
            node.wrapper.is_some_and(|wrapper| wrapper != *id)
    }));
    let values = legacy_value_inventory(&wrapped);
    assert!(values.windows(2).any(|pair| pair[0].0 > pair[1].0));
    assert_summary_inventory_and_cached_access(&wrapped);
    let first_wrapped = canonical_image([
        (b"long-prefix".to_vec(), 7),
        (b"long-prefix/a".to_vec(), 0),
        (b"long-prefix/b".to_vec(), 8),
        (b"z".to_vec(), 0),
    ]);
    let oracle = CompactTrie::parse_canonical(&first_wrapped).unwrap();
    let (id, node) = oracle
        .parsed
        .semantic_nodes
        .iter()
        .find(|(_, node)| node.value == Some(7))
        .unwrap();
    let wrapper = node.wrapper.expect("first offending value must be wrapped");
    assert_ne!(wrapper, *id);
    let summary = CanonicalCompactTrieTopologyCursor::new(first_wrapped.as_slice())
        .unwrap()
        .value_summary();
    assert_eq!(summary.first_nonzero(), Some((wrapper, 7)));
    assert_eq!(summary.first_nonsequential(), Some((wrapper, 7, 0)));
    assert_summary_inventory_and_cached_access(&first_wrapped);
    assert_summary_inventory_and_cached_access(&writer_image_from_operations(&[
        (b"unvalued-prefix/a".to_vec(), None),
        (b"unvalued-prefix/b".to_vec(), None),
    ]));
}

#[test]
fn value_summary_retains_preorder_ordinal_diagnostics_without_an_event_prepass() {
    let sequential = canonical_image([(b"a".to_vec(), 0), (b"b".to_vec(), 1), (b"c".to_vec(), 2)]);
    let source = FragmentedSource::new(&sequential, 3);
    let state = CanonicalCompactTrieIndexedTopologyCursor::from_source(source).unwrap();
    let summary = state.value_summary();
    assert_eq!(summary.value_count(), 3);
    let (_, first_nonzero) = summary.first_nonzero().unwrap();
    assert_eq!(first_nonzero, 1);
    assert_eq!(summary.first_nonsequential(), None);

    let nonsequential =
        canonical_image([(b"a".to_vec(), 0), (b"b".to_vec(), 7), (b"c".to_vec(), 2)]);
    let source = FragmentedSource::new(&nonsequential, 4);
    let state = CanonicalCompactTrieIndexedTopologyCursor::from_source(source).unwrap();
    let summary = state.value_summary();
    assert_eq!(summary.value_count(), 3);
    let (_, actual, expected) = summary.first_nonsequential().unwrap();
    assert_eq!((actual, expected), (7, 1));
}

#[test]
fn new_and_independent_oracle_agree_for_all_truncations_and_single_bit_mutations() {
    let valid =
        canonical_image([(b"shared/a".to_vec(), 0), (b"shared/b".to_vec(), 1), (b"z".to_vec(), 2)]);

    for end in 0..=valid.len() {
        let candidate = &valid[..end];
        assert_old_new_admission_equivalent(candidate, format_args!("truncation at {end}"));
    }

    for index in 0..valid.len() {
        for bit in 0..8 {
            let mut candidate = valid.clone();
            candidate[index] ^= 1 << bit;
            assert_old_new_admission_equivalent(
                candidate.as_slice(),
                format_args!("mutation at byte {index}, bit {bit}"),
            );
        }
    }

    let mut with_gap = valid.clone();
    with_gap.insert(with_gap.len() - 8, 0);
    assert!(CanonicalCompactTrieTopologyCursor::new(with_gap.as_slice()).is_err());

    let mut bad_root_order = valid.clone();
    let root = u64::from_le_bytes(bad_root_order[8..16].try_into().unwrap());
    bad_root_order[8..16].copy_from_slice(&(root - 1).to_le_bytes());
    assert!(CanonicalCompactTrieTopologyCursor::new(bad_root_order.as_slice()).is_err());
}

#[test]
fn malformed_errors_match_between_contiguous_and_fragmented_sources() {
    let valid = canonical_image([(b"prefix/a".to_vec(), 0), (b"prefix/b".to_vec(), 1)]);
    let mut cases = Vec::new();
    cases.push(Vec::new());
    cases.extend((0..valid.len()).map(|end| valid[..end].to_vec()));
    for index in 0..valid.len() {
        let mut mutation = valid.clone();
        mutation[index] ^= 0x80;
        cases.push(mutation);
    }

    for (index, bytes) in cases.iter().enumerate() {
        let contiguous = CanonicalCompactTrieTopologyCursor::new(bytes.as_slice()).err();
        let fragmented = FragmentedSource::new(bytes, index as u64 + 5);
        let indexed = CanonicalCompactTrieIndexedTopologyCursor::from_source(fragmented).err();
        assert_eq!(contiguous.is_some(), indexed.is_some(), "case {index}");
        if let (Some(contiguous), Some(indexed)) = (contiguous, indexed) {
            assert_eq!(contiguous.to_string(), indexed.to_string(), "case {index}");
            assert_eq!(contiguous.source_offset(), indexed.source_offset(), "case {index}");
        }
    }
}

#[test]
fn deeply_nested_value_prefixes_are_stack_safe_on_a_small_native_stack() {
    let depth = 2_048usize;
    let mut path = Vec::with_capacity(depth);
    let mut entries = Vec::with_capacity(depth);
    for ordinal in 0..depth {
        path.push(0x61);
        entries.push((path.clone(), ordinal as u64));
    }
    let bytes = canonical_image(entries);

    thread::Builder::new()
        .name("validated-act-plan-deep".to_owned())
        .stack_size(64 * 1024)
        .spawn(move || {
            let fragmented = FragmentedSource::new(&bytes, 0x9e37_79b9_7f4a_7c15);
            let mut state =
                CanonicalCompactTrieIndexedTopologyCursor::from_source(fragmented).unwrap();
            let mut values = 0usize;
            while let Some(event) = state.next_event().unwrap() {
                if matches!(event, CanonicalCompactTrieTopologyEvent::Value { value: Some(_), .. })
                {
                    values += 1;
                }
            }
            assert_eq!(values, depth);
            let counters = state.work_counters();
            assert_eq!(counters.semantic_node_parses, counters.plan_records);
            assert_eq!(counters.node_reparses, 0);
            assert_eq!(counters.event_metadata_source_reads, 0);
            assert!(counters.max_validation_depth >= depth);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn wide_dense_topology_is_stack_safe_on_a_small_native_stack() {
    let bytes = canonical_image((0u16..=255).map(|byte| (vec![byte as u8], byte as u64)));
    thread::Builder::new()
        .name("validated-act-plan-wide".to_owned())
        .stack_size(64 * 1024)
        .spawn(move || {
            let source = FragmentedSource::new(&bytes, 0xa409_3822_299f_31d0);
            let mut cursor =
                CanonicalCompactTrieIndexedTopologyCursor::from_source(source).unwrap();
            let mut values = 0;
            while let Some(event) = cursor.next_event().unwrap() {
                values += usize::from(matches!(event, CanonicalCompactTrieTopologyEvent::Value {
                    value: Some(_),
                    ..
                }));
            }
            assert_eq!(values, 256);
            let counters = cursor.work_counters();
            assert_eq!(counters.dense_mask_bytes, 32);
            assert_eq!(counters.node_reparses, 0);
            assert_eq!(counters.event_metadata_source_reads, 0);
        })
        .unwrap()
        .join()
        .unwrap();
}

mod checked_line_transport {
    use super::super::{
        CanonicalCompactTrieLineDirection as Direction, CanonicalCompactTrieLineSpan,
        DirectTopologyStage, ValidatedActPlan, ValidatedCompactTrieTopologyState,
    };
    use super::*;

    type Cursor = CanonicalCompactTrieIndexedTopologyCursor<FragmentedSource>;

    fn fork(cursor: &Cursor) -> Cursor {
        Cursor {
            source: cursor.source.clone(),
            state: ValidatedCompactTrieTopologyState {
                plan: ValidatedActPlan {
                    empty: cursor.state.plan.empty,
                    topology_event_count: cursor.state.plan.topology_event_count,
                    records: cursor.state.plan.records.clone(),
                    value_summary: cursor.state.plan.value_summary,
                    audit_finish_node_ids: cursor.state.plan.audit_finish_node_ids.clone(),
                    audit_line_hashes: cursor.state.plan.audit_line_hashes.clone(),
                    counters: cursor.state.plan.counters,
                },
                next_record: cursor.state.next_record,
                stack: cursor.state.stack.clone(),
                event_counters: cursor.state.event_counters,
            },
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct State {
        empty: bool,
        next_record: usize,
        records: Vec<(usize, u64, usize, usize, usize, usize, u16, u8, u8)>,
        frames: Vec<(usize, usize, usize, usize, u8)>,
        summary: super::super::CanonicalCompactTrieValueSummary,
    }

    fn state(cursor: &Cursor) -> State {
        State {
            empty: cursor.state.plan.empty,
            next_record: cursor.state.next_record,
            records: cursor
                .state
                .plan
                .records
                .iter()
                .map(|record| {
                    (
                        record.node_id,
                        record.value,
                        record.value_source_offset,
                        record.line_start,
                        record.line_len,
                        record.incoming_source_offset,
                        record.child_count,
                        record.incoming_byte,
                        record.flags,
                    )
                })
                .collect(),
            frames: cursor
                .state
                .stack
                .iter()
                .map(|frame| {
                    (
                        frame.record,
                        frame.remaining_children,
                        frame.line_index,
                        frame.leave_index,
                        frame.stage as u8,
                    )
                })
                .collect(),
            summary: cursor.value_summary(),
        }
    }

    fn check_every_prefix(bytes: &[u8], seed: u64) -> (usize, usize) {
        let mut cursor = Cursor::from_source(FragmentedSource::new(bytes, seed)).unwrap();
        let mut forward = 0;
        let mut unwind = 0;
        loop {
            let before = state(&cursor);
            let counters = cursor.work_counters();
            let offer = cursor
                .line_span()
                .map(|span| (span.source_range(), span.direction()));
            assert_eq!(state(&cursor), before, "offering and dropping is nonmutating");
            assert_eq!(cursor.work_counters(), counters);
            if let Some((range, direction)) = offer {
                assert!(!range.is_empty());
                for invalid in [0, range.len() + 1, usize::MAX] {
                    assert!(cursor.line_span().unwrap().commit(invalid).is_none());
                    assert!(cursor.line_span().unwrap().prepare(invalid).is_none());
                    assert_eq!(state(&cursor), before);
                    assert_eq!(cursor.work_counters(), counters);
                }
                for width in 1..=range.len() {
                    let mut fast = fork(&cursor);
                    let mut scalar = fork(&cursor);
                    let mut prepared_fast = fork(&cursor);
                    let prepared_source = &prepared_fast.source as *const FragmentedSource;
                    let prepared = prepared_fast.line_span().unwrap().prepare(width).unwrap();
                    assert_eq!(prepared.source() as *const FragmentedSource, prepared_source);
                    drop(prepared);
                    assert_eq!(
                        state(&prepared_fast),
                        before,
                        "dropping preparation is nonmutating"
                    );
                    assert_eq!(prepared_fast.work_counters(), counters);
                    let prepared = prepared_fast.line_span().unwrap().prepare(width).unwrap();
                    let prepared_range = prepared.source_range();
                    let prepared_direction = prepared.direction();
                    let prepared_events = prepared.expanded_events();
                    let prepared_last = prepared.last_source_offset();
                    let prepared_commit = prepared.commit();
                    assert_eq!(prepared_commit.source_range(), prepared_range);
                    assert_eq!(prepared_commit.direction(), prepared_direction);
                    assert_eq!(prepared_commit.expanded_events(), prepared_events);
                    assert_eq!(prepared_commit.last_source_offset(), prepared_last);
                    let records = fast.state.plan.records.as_ptr();
                    let source = &fast.source as *const FragmentedSource;
                    let span = fast.line_span().unwrap();
                    assert_eq!(span.source() as *const FragmentedSource, source);
                    let committed = span.commit(width).unwrap();
                    assert_eq!(prepared_commit, committed);
                    assert_eq!(state(&prepared_fast), state(&fast));
                    assert_eq!(prepared_fast.work_counters(), counters);
                    assert_eq!(committed.direction, direction);
                    let expanded = match direction {
                        Direction::Forward => {
                            forward += 1;
                            let expected_range = range.start..range.start + width;
                            assert_eq!(committed.source_range, expected_range);
                            for offset in expected_range {
                                assert_eq!(
                                    scalar.next_event().unwrap(),
                                    Some(CanonicalCompactTrieTopologyEvent::Edge {
                                        byte: bytes[offset],
                                        source_offset: offset,
                                    },)
                                );
                                assert!(
                                    scalar.line_span().is_none(),
                                    "odd prefix retains LineValue"
                                );
                                assert_eq!(
                                    scalar.next_event().unwrap(),
                                    Some(CanonicalCompactTrieTopologyEvent::Value {
                                        value: None,
                                        source_offset: offset,
                                    },)
                                );
                            }
                            assert_eq!(committed.last_source_offset(), range.start + width - 1);
                            width * 2
                        }
                        Direction::Unwind => {
                            unwind += 1;
                            let expected_range = range.end - width..range.end;
                            assert_eq!(committed.source_range, expected_range);
                            for offset in expected_range.rev() {
                                assert_eq!(
                                    scalar.next_event().unwrap(),
                                    Some(CanonicalCompactTrieTopologyEvent::Leave {
                                        source_offset: offset
                                    },)
                                );
                            }
                            assert_eq!(committed.last_source_offset(), range.end - width);
                            width
                        }
                    };
                    assert_eq!(committed.expanded_events, expanded);
                    assert_eq!(fast.state.plan.records.as_ptr(), records);
                    assert_eq!(state(&fast), state(&scalar));
                    assert_eq!(fast.work_counters(), counters, "commit performs no byte reads");
                    let mut expected_counters = counters;
                    if direction == Direction::Forward {
                        expected_counters.event_line_source_reads += width;
                    }
                    assert_eq!(scalar.work_counters(), expected_counters);
                    assert_eq!(fast.next_event().unwrap(), scalar.next_event().unwrap());
                    assert_eq!(state(&fast), state(&scalar), "next scalar boundary remains exact");
                }
            }
            if cursor.next_event().unwrap().is_none() {
                assert!(cursor.line_span().is_none());
                break;
            }
        }
        (forward, unwind)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn arbitrary_branched_fragmented_images_match_every_scalar_prefix(
            operations in proptest::collection::vec((
                proptest::collection::vec(any::<u8>(), 1..25),
                prop_oneof![Just(None), any::<u64>().prop_map(Some)],
            ), 0..12),
            seed in 1u64..u64::MAX,
        ) {
            check_every_prefix(&writer_image_from_operations(&operations), seed);
        }
    }

    #[test]
    fn shared_lines_values_and_valueless_branches_preserve_all_boundaries() {
        let bytes = writer_image_from_operations(&[
            (b"shared/branch/a/tail".to_vec(), Some(3)),
            (b"shared/branch/b/tail".to_vec(), None),
            (b"shared/branch".to_vec(), Some(7)),
            (b"other/prefix/a".to_vec(), Some(11)),
            (b"other/prefix/b".to_vec(), None),
        ]);
        let (forward, unwind) = check_every_prefix(&bytes, 17);
        assert!(forward > 0 && unwind > 0);
    }

    #[test]
    fn empty_and_one_byte_lines_never_skip_final_values_or_node_leaves() {
        check_every_prefix(&canonical_image([]), 13);
        let (forward, unwind) = check_every_prefix(&canonical_image([(vec![19], 7)]), 19);
        assert_eq!(forward, 0);
        assert_eq!(unwind, 1);
    }

    #[test]
    fn dropped_offer_allows_rewind_and_a_fresh_exact_traversal() {
        let bytes = canonical_image([(b"abcdef".to_vec(), 7)]);
        let mut cursor = Cursor::from_source(FragmentedSource::new(&bytes, 11)).unwrap();
        let before = new_topology(&bytes);
        cursor.next_event().unwrap();
        let offered = cursor.line_span().unwrap().source_range();
        assert!(!offered.is_empty());
        drop(cursor.line_span().unwrap().prepare(1).unwrap());
        cursor.rewind();
        let actual: Vec<_> = std::iter::from_fn(|| cursor.next_event().unwrap()).collect();
        assert_eq!(actual, before);
    }

    #[test]
    fn repeated_partial_commits_keep_endpoint_and_parent_leave_scalar() {
        let bytes = canonical_image([(vec![31; 4096], 23)]);
        let mut cursor = Cursor::from_source(FragmentedSource::new(&bytes, 7)).unwrap();
        let expected = new_topology(&bytes);
        let mut events = Vec::new();
        let mut skipped = 0;
        loop {
            if let Some(span) = cursor.line_span() {
                let width = span.source_range().len().min(17);
                let direction = span.direction();
                let committed = span.prepare(width).unwrap().commit();
                skipped += committed.expanded_events;
                match direction {
                    Direction::Forward => {
                        for offset in committed.source_range {
                            events.push(CanonicalCompactTrieTopologyEvent::Edge {
                                byte: bytes[offset],
                                source_offset: offset,
                            });
                            events.push(CanonicalCompactTrieTopologyEvent::Value {
                                value: None,
                                source_offset: offset,
                            });
                        }
                    }
                    Direction::Unwind => {
                        for offset in committed.source_range.rev() {
                            events.push(CanonicalCompactTrieTopologyEvent::Leave {
                                source_offset: offset,
                            });
                        }
                    }
                }
            } else if let Some(event) = cursor.next_event().unwrap() {
                events.push(event);
            } else {
                break;
            }
        }
        assert_eq!(events, expected);
        assert!(skipped >= 2 * 4095);
        assert_eq!(cursor.work_counters().event_line_source_reads, 1);
        assert!(cursor.state.stack.is_empty());
    }

    #[test]
    fn boundary_phases_and_pending_children_never_offer_a_span() {
        let bytes = canonical_image([(b"ab".to_vec(), 7)]);
        let mut cursor = Cursor::from_source(FragmentedSource::new(&bytes, 3)).unwrap();
        loop {
            let stage = cursor.state.stack.last().map(|frame| frame.stage);
            if matches!(
                stage,
                None | Some(DirectTopologyStage::Value | DirectTopologyStage::LineValue)
            ) {
                assert!(cursor.line_span().is_none());
            }
            if matches!(stage, Some(DirectTopologyStage::Children)) {
                let pending = cursor.state.stack.last().unwrap().remaining_children;
                if pending > 0 {
                    assert!(cursor.line_span().is_none());
                }
            }
            if cursor.next_event().unwrap().is_none() {
                break;
            }
        }
    }

    #[test]
    fn prepared_arithmetic_and_missing_frame_refusals_publish_nothing() {
        let bytes = canonical_image([(b"abcdef".to_vec(), 7)]);
        let mut cursor = Cursor::from_source(FragmentedSource::new(&bytes, 11)).unwrap();
        cursor.next_event().unwrap();
        let before = state(&cursor);
        let counters = cursor.work_counters();
        assert!(
            CanonicalCompactTrieLineSpan {
                cursor: &mut cursor,
                source_range: 0..usize::MAX,
                direction: Direction::Forward,
            }
            .prepare(usize::MAX / 2 + 1)
            .is_none()
        );
        assert_eq!(state(&cursor), before);
        assert_eq!(cursor.work_counters(), counters);

        cursor.state.stack.last_mut().unwrap().line_index = usize::MAX;
        let before = state(&cursor);
        assert!(
            CanonicalCompactTrieLineSpan {
                cursor: &mut cursor,
                source_range: 0..1,
                direction: Direction::Forward,
            }
            .prepare(1)
            .is_none()
        );
        assert_eq!(state(&cursor), before);
        assert_eq!(cursor.work_counters(), counters);

        cursor.state.stack.last_mut().unwrap().leave_index = usize::MAX;
        let before = state(&cursor);
        assert!(
            CanonicalCompactTrieLineSpan {
                cursor: &mut cursor,
                source_range: 0..1,
                direction: Direction::Unwind,
            }
            .prepare(1)
            .is_none()
        );
        assert_eq!(state(&cursor), before);
        assert_eq!(cursor.work_counters(), counters);

        cursor.state.stack.clear();
        let before = state(&cursor);
        assert!(
            CanonicalCompactTrieLineSpan {
                cursor: &mut cursor,
                source_range: 0..1,
                direction: Direction::Forward,
            }
            .prepare(1)
            .is_none()
        );
        assert_eq!(state(&cursor), before);
        assert_eq!(cursor.work_counters(), counters);
    }
}
