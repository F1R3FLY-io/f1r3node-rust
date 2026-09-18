use std::ops::Range;

use pathmap::arena_compact::ArenaCompactTree;
use pathmap::PathMap;
use proptest::prelude::*;

use super::{
    inspect_canonical_snapshot_value_regions, parse_canonical_snapshot_framing,
    parse_canonical_snapshot_prefix, snapshot_route_hint, EPathMapMode, EPM_MAGIC, EPM_VERSION,
};
use crate::rust::rholang::protobuf_cursor::{CanonicalByteRange, ProtobufRegion};

fn append_length(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
}

fn prefix(mode: EPathMapMode, value_count: usize) -> Vec<u8> {
    let mut bytes = Vec::from(EPM_MAGIC.as_slice());
    bytes.extend_from_slice(&[EPM_VERSION, mode as u8]);
    if mode == EPathMapMode::Empty {
        append_length(&mut bytes, 0);
    } else {
        let mut trie = PathMap::<()>::new();
        let entries = if mode == EPathMapMode::Set {
            2
        } else {
            value_count
        };
        for index in 0..entries {
            trie.insert((index as u64).to_be_bytes(), ());
        }
        if entries == 0 {
            trie.create_path(b"value-free-topology");
        }
        let arena = ArenaCompactTree::from_zipper(trie.read_zipper(), |_| 0);
        append_length(&mut bytes, arena.get_data().len() as u64);
        bytes.extend_from_slice(arena.get_data());
    }
    append_length(&mut bytes, value_count as u64);
    bytes
}

fn construct(mode: EPathMapMode, values: &[Vec<u8>]) -> (Vec<u8>, usize, Vec<Range<usize>>) {
    let mut bytes = prefix(mode, values.len());
    let prefix_end = bytes.len();
    let mut expected = Vec::with_capacity(values.len());
    for value in values {
        append_length(&mut bytes, value.len() as u64);
        let start = bytes.len();
        bytes.extend_from_slice(value);
        expected.push(start..bytes.len());
    }
    (bytes, prefix_end, expected)
}

fn assert_selected(mode: EPathMapMode, values: &[Vec<u8>]) {
    let (bytes, prefix_end, expected) = construct(mode, values);
    let framing = parse_canonical_snapshot_framing(&bytes).unwrap();
    assert_eq!(framing.value_table_start, prefix_end);
    assert_eq!(framing.value_count, values.len());
    assert_eq!(framing.value_bodies, expected);
    let inspection = inspect_canonical_snapshot_value_regions(&bytes).unwrap();
    assert_eq!(inspection.mode, mode);
    assert_eq!(inspection.prefix, 0..prefix_end);
    assert_eq!(inspection.value_bodies, expected);

    let backing = ProtobufRegion::from([&[17, 23, 42][..], bytes.as_slice(), &[99][..]].concat());
    let payload = backing
        .subregion(CanonicalByteRange {
            start: 3,
            end: 3 + bytes.len(),
        })
        .unwrap();
    let mut previous_end = prefix_end;
    for (range, value) in inspection.value_bodies.iter().zip(values) {
        assert!(range.start > previous_end);
        assert!(range.start <= range.end && range.end <= bytes.len());
        assert_eq!(&bytes[range.clone()], value);
        let region = payload
            .subregion(CanonicalByteRange {
                start: range.start,
                end: range.end,
            })
            .unwrap();
        assert_eq!(region.absolute_range(), 3 + range.start..3 + range.end);
        assert_eq!(region.as_bytes(), value);
        assert_eq!(
            region.as_bytes().as_ptr(),
            payload.as_bytes()[range.start..].as_ptr()
        );
        previous_end = range.end;
    }
    assert_eq!(previous_end, bytes.len());
}

#[test]
fn value_region_positions_match_construction_across_header_widths_and_duplicates() {
    let values = [0, 0, 1, 127, 128, 16383, 16384, 127, 0].map(|length| vec![42; length]);
    assert_selected(EPathMapMode::Map, &values);
    let (_, _, expected) = construct(EPathMapMode::Map, &values);
    assert_ne!(expected[0], expected[1]);
    assert_ne!(expected[3], expected[7]);
}

#[test]
fn value_region_zero_count_preserves_empty_set_and_value_free_map_modes() {
    for mode in [EPathMapMode::Empty, EPathMapMode::Set, EPathMapMode::Map] {
        assert_selected(mode, &[]);
    }
}

#[test]
fn malformed_value_framing_never_publishes_a_partial_range_list() {
    for (suffix, expected_error) in [
        (vec![128], "truncated"),
        (vec![128, 0], "non-canonical"),
        (vec![2, 42], "truncated"),
        (vec![0, 99], "trailing bytes"),
        (vec![255; 10], "overflowing"),
    ] {
        let mut bytes = prefix(EPathMapMode::Map, 1);
        bytes.extend_from_slice(&suffix);
        let error = match parse_canonical_snapshot_framing(&bytes) {
            Ok(_) => panic!("malformed suffix unexpectedly accepted: {suffix:?}"),
            Err(error) => error,
        };
        assert!(error.to_string().contains(expected_error), "{error}");
        assert!(inspect_canonical_snapshot_value_regions(&bytes).is_err());
    }
    let mut short_count = prefix(EPathMapMode::Map, 2);
    short_count.extend_from_slice(&[1, 42]);
    assert!(parse_canonical_snapshot_framing(&short_count).is_err());
    let mut impossible_count = prefix(EPathMapMode::Map, 2);
    impossible_count.push(0);
    let error = match parse_canonical_snapshot_framing(&impossible_count) {
        Ok(_) => panic!("count lower bound unexpectedly accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("payload lower bound"));
    let mut overflowing_body = prefix(EPathMapMode::Map, 1);
    append_length(&mut overflowing_body, u64::MAX);
    let error = match parse_canonical_snapshot_framing(&overflowing_body) {
        Ok(_) => panic!("unrepresentable payload end unexpectedly accepted"),
        Err(error) => error,
    };
    let expected = if usize::BITS == 64 {
        "length overflow"
    } else {
        "overflows usize"
    };
    assert!(error.to_string().contains(expected), "{error}");
    assert!(inspect_canonical_snapshot_value_regions(&overflowing_body).is_err());
}

#[test]
fn topology_validation_remains_a_separate_post_framing_gate() {
    let (mut bytes, _, expected) = construct(EPathMapMode::Map, &[vec![42]]);
    bytes[EPM_MAGIC.len() + 1] = EPathMapMode::Set as u8;
    let framing = parse_canonical_snapshot_framing(&bytes).unwrap();
    assert_eq!(framing.value_bodies, expected);
    let error = match inspect_canonical_snapshot_value_regions(&bytes) {
        Ok(_) => panic!("set topology with a map value unexpectedly accepted"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("set EPathMap carries map values"));
}

struct RecordedPrefix {
    bytes: Vec<u8>,
    arena: Range<usize>,
    count_header: Range<usize>,
}

fn record_prefix(mode: EPathMapMode, arena: &[u8], count: usize) -> RecordedPrefix {
    let mut bytes = vec![69, 80, 77, 49, 1, mode as u8];
    append_length(&mut bytes, arena.len() as u64);
    let arena_start = bytes.len();
    bytes.extend_from_slice(arena);
    let arena = arena_start..bytes.len();
    let count_start = bytes.len();
    append_length(&mut bytes, count as u64);
    let count_header = count_start..bytes.len();
    bytes.resize(bytes.len() + count, 0);
    RecordedPrefix {
        bytes,
        arena,
        count_header,
    }
}

fn assert_prefix_positions(mode: EPathMapMode, arena: &[u8], count: usize) {
    let recorded = record_prefix(mode, arena, count);
    let parsed = parse_canonical_snapshot_prefix(&recorded.bytes).unwrap();
    assert_eq!(parsed.mode, mode);
    assert_eq!(parsed.arena_start, recorded.arena.start);
    assert_eq!(parsed.arena, arena);
    assert_eq!(
        parsed.arena.as_ptr(),
        recorded.bytes[recorded.arena.clone()].as_ptr()
    );
    assert_eq!(parsed.value_count, count);
    assert_eq!(parsed.value_table_start, recorded.count_header.end);
    assert_eq!(recorded.arena.end, recorded.count_header.start);
    let framing = parse_canonical_snapshot_framing(&recorded.bytes).unwrap();
    assert_eq!(framing.value_table_start, recorded.count_header.end);
    assert_eq!(framing.value_count, count);
    for (index, range) in framing.value_bodies.iter().enumerate() {
        let end = recorded.count_header.end + index + 1;
        assert_eq!(range, &(end..end));
    }
    assert_eq!(framing.value_bodies.len(), count);
}

#[test]
fn prefix_offsets_follow_append_time_oracle_across_varint_widths() {
    assert_eq!(
        (
            EPathMapMode::Empty as u8,
            EPathMapMode::Set as u8,
            EPathMapMode::Map as u8
        ),
        (0, 1, 2)
    );
    let golden = [69, 80, 77, 49, 1, 0, 0, 0];
    let empty = inspect_canonical_snapshot_value_regions(&golden).unwrap();
    assert_eq!(empty.mode, EPathMapMode::Empty);
    assert_eq!(empty.prefix, 0..8);
    assert!(empty.value_bodies.is_empty());
    for size in [0, 1, 127, 128, 16383, 16384] {
        assert_prefix_positions(EPathMapMode::Map, &vec![42; size], 0);
        assert_prefix_positions(EPathMapMode::Map, &[], size);
    }
}

fn prefix_error(bytes: &[u8]) -> String {
    match parse_canonical_snapshot_prefix(bytes) {
        Ok(_) => panic!("malformed prefix unexpectedly accepted"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn malformed_prefixes_fail_before_value_range_publication() {
    let fixed = [69, 80, 77, 49, 1, 2];
    for len in 0..fixed.len() {
        let stage = if len < 4 {
            "magic"
        } else if len == 4 {
            "version"
        } else {
            "mode"
        };
        assert!(prefix_error(&fixed[..len]).contains(stage));
    }
    for (suffix, stage) in [
        (vec![128], "truncated ACTree03 length"),
        (vec![128, 0], "non-canonical ACTree03 length"),
        (vec![255; 10], "overflowing ACTree03 length"),
        (vec![2, 42], "truncated ACTree03 payload"),
        (vec![0], "truncated map value count"),
        (vec![0, 128, 0], "non-canonical map value count"),
        (
            [&[0][..], &[255; 10][..]].concat(),
            "overflowing map value count",
        ),
        (vec![0, 2, 0], "payload lower bound"),
    ] {
        let bytes = [&fixed[..], &suffix].concat();
        let error = prefix_error(&bytes);
        assert!(error.contains(stage), "{stage}: {error}");
        assert!(parse_canonical_snapshot_framing(&bytes).is_err());
        assert!(inspect_canonical_snapshot_value_regions(&bytes).is_err());
    }
    let mut huge = fixed.to_vec();
    append_length(&mut huge, u64::MAX);
    let expected = if usize::BITS == 64 {
        "length overflow"
    } else {
        "overflows usize"
    };
    assert!(prefix_error(&huge).contains(expected));
    let mut wrong_magic = fixed.to_vec();
    wrong_magic[0] = 0;
    wrong_magic[4] = 9;
    wrong_magic[5] = 9;
    assert!(prefix_error(&wrong_magic).contains("invalid EPM1 magic"));
    let mut wrong_version = fixed.to_vec();
    wrong_version[4] = 9;
    wrong_version[5] = 9;
    assert!(prefix_error(&wrong_version).contains("unsupported EPM1 version"));
    let mut wrong_mode = fixed.to_vec();
    wrong_mode[5] = 9;
    assert!(prefix_error(&wrong_mode).contains("unknown EPathMap mode"));
}

#[test]
fn route_hints_do_not_bypass_framing_or_topology_validation() {
    for bytes in [
        vec![69, 80, 77, 49, 1, 2, 0, 1, 128],
        vec![69, 80, 77, 49, 1, 2, 1, 42, 0],
        vec![69, 80, 77, 49, 1, 0, 0, 0, 99],
    ] {
        assert!(snapshot_route_hint(&bytes).is_ok());
        assert!(inspect_canonical_snapshot_value_regions(&bytes).is_err());
    }
    assert!(parse_canonical_snapshot_framing(&[69, 80, 77, 49, 1, 2, 1, 42, 0]).is_ok());
    for (bytes, stage) in [
        (
            vec![69, 80, 77, 49, 1, 2, 1, 42, 2, 0],
            "payload lower bound",
        ),
        (
            vec![69, 80, 77, 49, 1, 2, 1, 42, 1, 128],
            "truncated map value length",
        ),
        (vec![69, 80, 77, 49, 1, 2, 1, 42, 0, 99], "trailing bytes"),
    ] {
        let error = match inspect_canonical_snapshot_value_regions(&bytes) {
            Ok(_) => panic!("paired faults unexpectedly accepted"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains(stage), "{stage}: {error}");
    }
}

proptest! {
    #[test]
    fn generated_prefix_offsets_match_independent_construction(
        arena in prop::collection::vec(any::<u8>(), 0..256),
        count in 0usize..256,
        mode in 0u8..3,
    ) {
        assert_prefix_positions(EPathMapMode::try_from(mode).unwrap(), &arena, count);
    }
}

fn framed_arena(mode: EPathMapMode, arena: &[u8], value_count: usize) -> Vec<u8> {
    let mut bytes = Vec::from(EPM_MAGIC.as_slice());
    bytes.extend_from_slice(&[EPM_VERSION, mode as u8]);
    append_length(&mut bytes, arena.len() as u64);
    bytes.extend_from_slice(arena);
    append_length(&mut bytes, value_count as u64);
    bytes.resize(bytes.len() + value_count, 0);
    bytes
}

fn check_mode_matrix(markers: &[u64], retain_unvalued_path: bool) {
    let mut trie = PathMap::<u64>::new();
    if retain_unvalued_path {
        trie.create_path(b"value-free-topology");
    }
    for (index, marker) in markers.iter().enumerate() {
        trie.insert((index as u64).to_be_bytes(), *marker);
    }
    let arena = ArenaCompactTree::from_zipper(trie.read_zipper(), |marker| *marker);
    let logically_nonempty = retain_unvalued_path || !markers.is_empty();
    let all_zero = markers.iter().all(|marker| *marker == 0);
    let mut counts = vec![0, markers.len(), markers.len() + 1];
    counts.sort_unstable();
    counts.dedup();
    for mode in [EPathMapMode::Empty, EPathMapMode::Set, EPathMapMode::Map] {
        for &value_count in &counts {
            let bytes = framed_arena(mode, arena.get_data(), value_count);
            let expected = match mode {
                EPathMapMode::Empty => false,
                EPathMapMode::Set => logically_nonempty && all_zero && value_count == 0,
                EPathMapMode::Map => logically_nonempty && all_zero && value_count == markers.len(),
            };
            let framing = parse_canonical_snapshot_framing(&bytes).unwrap();
            assert_eq!(framing.value_count, value_count);
            let actual = inspect_canonical_snapshot_value_regions(&bytes);
            assert_eq!(
                actual.is_ok(), expected,
                "mode={mode:?} markers={markers:?} unvalued={retain_unvalued_path} count={value_count}"
            );
            if let Ok(inspection) = actual {
                assert_eq!(inspection.prefix, 0..framing.value_table_start);
                assert_eq!(inspection.value_bodies, framing.value_bodies);
                assert_eq!(inspection.mode, mode);
            }
        }
    }
}

#[test]
fn mode_admission_uses_logical_topology_and_zero_endpoint_markers() {
    for markers in [vec![], vec![0], vec![0, 0, 0], vec![7], vec![0, 7, 0]] {
        for retain_unvalued_path in [false, true] {
            check_mode_matrix(&markers, retain_unvalued_path);
        }
    }
    for mode in [EPathMapMode::Empty, EPathMapMode::Set, EPathMapMode::Map] {
        let bytes = framed_arena(mode, &[], 0);
        assert!(parse_canonical_snapshot_framing(&bytes).is_ok());
        assert_eq!(
            inspect_canonical_snapshot_value_regions(&bytes).is_ok(),
            mode == EPathMapMode::Empty
        );
    }
}

proptest! {
    #[test]
    fn generated_mode_admission_matches_independent_endpoint_inventory(
        markers in prop::collection::vec(prop_oneof![Just(0u64), 1u64..16], 0..24),
        retain_unvalued_path in any::<bool>(),
    ) {
        check_mode_matrix(&markers, retain_unvalued_path);
        check_mode_matrix(&vec![0; markers.len()], retain_unvalued_path);
    }
}

proptest! {
    #[test]
    fn generated_value_regions_equal_independently_recorded_positions(
        mut values in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..256), 0..20),
    ) {
        let duplicate = values.first().cloned().unwrap_or_default();
        values.extend([Vec::new(), Vec::new(), duplicate]);
        assert_selected(EPathMapMode::Map, &values);
    }
}
