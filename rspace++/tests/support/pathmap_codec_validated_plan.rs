use std::thread;

use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;

use super::{
    CanonicalCompactTrieCursor, CanonicalCompactTrieIndexedTopologyCursor,
    CanonicalCompactTrieTopologyCursor, CanonicalCompactTrieTopologyEvent, CompactTrie,
    CompactTrieTopologyEvent, IndexedByteSource,
};

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
