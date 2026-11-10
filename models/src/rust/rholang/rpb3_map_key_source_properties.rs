use proptest::prelude::*;

use super::*;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;

fn key_case(kind: u8, ascii: u8, prefix: bool, suffix: bool) -> Vec<u8> {
    let mut key = match kind {
        0 => vec![ascii],
        1 => vec![0xc2, 0x80],
        2 => vec![0xc1, 0xbf],
        3 => vec![0xe0, 0xa0, 0x80],
        4 => vec![0xe0, 0x9f, 0xbf],
        5 => vec![0xed, 0x9f, 0xbf],
        6 => vec![0xed, 0xa0, 0x80],
        7 => vec![0xf0, 0x90, 0x80, 0x80],
        8 => vec![0xf0, 0x8f, 0xbf, 0xbf],
        9 => vec![0xf4, 0x8f, 0xbf, 0xbf],
        10 => vec![0xf4, 0x90, 0x80, 0x80],
        11 => vec![0x80],
        12 => vec![0xe2, 0x82],
        13 => vec![0xf1, 0xbf, 0xbf, 0xbf],
        14 => vec![0xf5, 0x80, 0x80, 0x80],
        15 => Vec::new(),
        _ => unreachable!(),
    };
    if !key.is_empty() {
        if prefix {
            key.insert(0, ascii);
        }
        if suffix {
            key.push(ascii);
        }
    }
    key
}

fn assert_map_key_step(
    region: IndexedByteRegion<'_>,
    key_bytes: &[u8],
    anchors: &[CanonicalByteProvenance],
    end: CanonicalByteProvenance,
) {
    let entry = region.bytes().collect::<Vec<_>>();
    assert_eq!(entry.len(), key_bytes.len() + 2);
    assert_eq!(entry[0], 0x0a);
    assert_eq!(usize::from(entry[1]), key_bytes.len());
    assert_eq!(&entry[2..], key_bytes);
    assert_eq!(anchors.len(), entry.len());
    for (index, anchor) in anchors.iter().enumerate() {
        assert_eq!(region.position_at(index).unwrap().physical, *anchor);
    }
    assert_eq!(region.end_position().physical, end);

    let selected_payload = region
        .subview(CanonicalByteRange {
            start: 2,
            end: entry.len(),
        })
        .unwrap();
    let expected_utf8 = std::str::from_utf8(key_bytes).is_ok();
    let mut frame = MapFrame::from_map_region(
        &NEW_RHO_MATCH_PROGRAM,
        4,
        PAR_PROTOBUF_REGION_PROGRAM.type_id,
        region.clone(),
    );
    let request = FrameRequest::Map {
        region: region.clone(),
        owner: &NEW_RHO_MATCH_PROGRAM,
        field: 4,
        value_type_id: PAR_PROTOBUF_REGION_PROGRAM.type_id,
    };
    let mut fresh_probe = Frame::Map(MapFrame::from_map_region(
        &NEW_RHO_MATCH_PROGRAM,
        4,
        PAR_PROTOBUF_REGION_PROGRAM.type_id,
        region.clone(),
    ));
    assert!(request.matches_pushed_frame(&fresh_probe));
    let Frame::Map(probe) = &mut fresh_probe else {
        unreachable!()
    };
    probe.saw_value = true;
    assert!(!request.matches_pushed_frame(&fresh_probe));
    let Frame::Map(probe) = &mut fresh_probe else {
        unreachable!()
    };
    probe.saw_value = false;
    probe.key = Some(selected_payload.clone());
    assert!(!request.matches_pushed_frame(&fresh_probe));
    let mut stats = CanonicalProtobufAdmissionStats::default();
    assert_eq!(frame.cursor.logical_offset(), 0);
    assert_eq!(
        frame.cursor.stats(),
        IndexedCanonicalFieldCursorStats::default()
    );
    assert!(frame.key.is_none());
    assert!(!frame.saw_value);
    assert!(!frame.waiting_value);

    let action = map_step(&mut frame, &mut stats);
    if expected_utf8 && !key_bytes.is_empty() {
        assert!(matches!(action, Ok(FrameAction::Continue)));
        let selected_key = frame.key.as_ref().expect("accepted map key is retained");
        assert!(selected_key.same_view(&selected_payload));
        assert_eq!(selected_key.bytes().collect::<Vec<_>>(), key_bytes);
        assert_eq!(selected_key.position_at(0).unwrap().physical, anchors[2]);
        assert_eq!(selected_key.end_position().physical, end);
        for index in 0..key_bytes.len() {
            assert_eq!(
                selected_key.position_at(index).unwrap().physical,
                anchors[index + 2]
            );
        }
        assert_eq!(frame.key_position, Some(anchors[0]));
    } else {
        let expected_detail = if key_bytes.is_empty() {
            "explicit protobuf map key equals the omitted default"
        } else {
            "protobuf map key is not UTF-8"
        };
        assert!(
            matches!(action, Err(CanonicalProtobufAdmissionError::Schema {
            type_name: "protobuf map entry",
            field: 1,
            position,
            detail,
        }) if position == anchors[0] && detail == expected_detail)
        );
        assert!(frame.key.is_none());
        assert!(frame.key_position.is_none());
    }
    assert!(frame.region.same_view(&region));
    assert_eq!(frame.region_start, anchors[0]);
    assert_eq!(frame.cursor.logical_offset(), entry.len());
    assert_eq!(frame.cursor.stats().fields_read, 1);
    assert_eq!(frame.cursor.stats().bytes_read, entry.len());
    assert_eq!(stats.fields, 1);
    assert_eq!(stats.protobuf_bytes_read, 0);
    assert_eq!(stats.protobuf_bytes_traversed, 0);
    assert!(!frame.saw_value);
    assert!(frame.value_position.is_none());
    assert!(frame.value.is_none());
    assert!(!frame.waiting_value);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn generated_map_key_utf8_source_matches_indexed_views(
        ascii in 0x21u8..0x7f,
        prefix in any::<bool>(),
        suffix in any::<bool>(),
        cut_seed in any::<u8>(),
        source in 100u32..1000,
        base in 0usize..100_000,
    ) {
      for kind in 0u8..16 {
        let key = key_case(kind, ascii, prefix, suffix);
        let mut entry = vec![0x0a, key.len() as u8];
        entry.extend_from_slice(&key);

        let borrowed_anchors = (0..entry.len())
            .map(|at| CanonicalByteProvenance::new(source, base + at))
            .collect::<Vec<_>>();
        let borrowed_end = CanonicalByteProvenance::new(source, base + entry.len());
        assert_map_key_step(
            IndexedByteRegion::from_borrowed(&entry, source, base),
            &key,
            &borrowed_anchors,
            borrowed_end,
        );

        let frozen_anchors = (0..entry.len())
            .map(|at| CanonicalByteProvenance::new(source + 1 + at as u32 % 13, base + 17 * at))
            .collect::<Vec<_>>();
        let frozen_end = CanonicalByteProvenance::new(source + 100, base + 10_000);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in entry.iter().zip(&frozen_anchors) {
            handle = arena.append(handle, CanonicalLocatedByte::new(byte, anchor)).unwrap();
        }
        assert_map_key_step(
            IndexedByteRegion::from_frozen(arena.freeze(handle), frozen_end),
            &key,
            &frozen_anchors,
            frozen_end,
        );

        let cut = if key.len() > 1 {
            3 + usize::from(cut_seed) % (key.len() - 1)
        } else {
            1
        };
        let right_source = source + 30;
        let right_base = base + 20_000;
        let left = SourceTreeWindow::from_borrowed(&entry[..cut], source, base).unwrap();
        let right = SourceTreeWindow::from_borrowed(
            &entry[cut..], right_source, right_base,
        ).unwrap();
        let composed_anchors = (0..entry.len())
            .map(|at| if at < cut {
                CanonicalByteProvenance::new(source, base + at)
            } else {
                CanonicalByteProvenance::new(right_source, right_base + at - cut)
            })
            .collect::<Vec<_>>();
        let composed_end = CanonicalByteProvenance::new(source + 200, base + 30_000);
        assert_map_key_step(
            IndexedByteRegion::from_source(left.concat(&right).unwrap(), composed_end),
            &key,
            &composed_anchors,
            composed_end,
        );
      }
    }
}
