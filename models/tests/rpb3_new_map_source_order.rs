use std::collections::BTreeMap;

use models::rhoapi::{New, Par};
use models::rust::par_bag::{with_component_key, ParBag, PAR_BAG_KEY_DOMAIN};
use proptest::prelude::*;
use prost::Message;
use rspace_plus_plus::rspace::bag::Bag;

#[derive(Clone, Debug)]
enum MapValue {
    Absent,
    PresentEmpty,
    Nested(Vec<MapEntry>),
}

#[derive(Clone, Debug)]
struct MapEntry {
    key: String,
    value: MapValue,
}

fn put_varint(mut value: usize, output: &mut Vec<u8>) {
    while value >= 0x80 {
        output.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn put_message(tag: usize, body: &[u8], output: &mut Vec<u8>) {
    put_varint((tag << 3) | 2, output);
    put_varint(body.len(), output);
    output.extend_from_slice(body);
}

fn encode_new(entries: &[MapEntry]) -> Vec<u8> {
    let mut output = Vec::new();
    put_message(2, &[], &mut output);
    for entry in entries {
        let mut map_entry = Vec::new();
        put_message(1, entry.key.as_bytes(), &mut map_entry);
        match &entry.value {
            MapValue::Absent => {}
            MapValue::PresentEmpty => put_message(2, &[], &mut map_entry),
            MapValue::Nested(nested) => {
                let mut value = Vec::new();
                put_message(4, &encode_new(nested), &mut value);
                put_message(2, &value, &mut map_entry);
            }
        }
        put_message(4, &map_entry, &mut output);
    }
    output
}

fn typed_new(entries: &[MapEntry]) -> New {
    let mut injections = BTreeMap::new();
    for entry in entries {
        let value = match &entry.value {
            MapValue::Absent | MapValue::PresentEmpty => Par::default(),
            MapValue::Nested(nested) => Par::default().with_news(vec![typed_new(nested)]),
        };
        injections.insert(entry.key.clone(), value);
    }
    New {
        p: Some(Par::default()),
        injections,
        ..New::default()
    }
}

fn strictly_ordered(entries: &[MapEntry]) -> bool {
    entries
        .windows(2)
        .all(|pair| pair[0].key.as_bytes() < pair[1].key.as_bytes())
        && entries.iter().all(|entry| match &entry.value {
            MapValue::Nested(nested) => strictly_ordered(nested),
            MapValue::Absent | MapValue::PresentEmpty => true,
        })
}

fn canonical_value_presence(entries: &[MapEntry]) -> bool {
    entries.iter().all(|entry| match &entry.value {
        MapValue::Absent => true,
        MapValue::PresentEmpty => false,
        MapValue::Nested(nested) => canonical_value_presence(nested),
    })
}

fn byte_native_admits(body: &[u8], typed: &New) -> bool {
    let reference = with_component_key(typed, |key| key.to_vec());
    let trailer_start = reference.len() - 8;
    let score_len = usize::try_from(u64::from_be_bytes(
        reference[trailer_start..].try_into().unwrap(),
    ))
    .unwrap();
    let score_end = PAR_BAG_KEY_DOMAIN.len() + 1 + score_len;
    assert!(score_end <= trailer_start);
    let mut candidate = Vec::with_capacity(score_end + body.len() + 8);
    candidate.extend_from_slice(&reference[..score_end]);
    candidate.extend_from_slice(body);
    candidate.extend_from_slice(&reference[trailer_start..]);
    let mut bag = Bag::new();
    bag.insert(&candidate)
        .expect("candidate fits in a singleton bag");
    ParBag::try_from_bag(bag).is_ok()
}

fn value(mode: u8, nested: &[MapEntry]) -> MapValue {
    match mode {
        0 => MapValue::Absent,
        1 => MapValue::PresentEmpty,
        _ => MapValue::Nested(nested.to_vec()),
    }
}

fn entries(
    low: u8,
    high: u8,
    reverse: bool,
    duplicate: bool,
    first: MapValue,
    second: MapValue,
) -> Vec<MapEntry> {
    let mut entries = vec![
        MapEntry {
            key: (low as char).to_string(),
            value: first,
        },
        MapEntry {
            key: ((if duplicate { low } else { high }) as char).to_string(),
            value: second,
        },
    ];
    if reverse {
        entries.reverse();
    }
    entries
}

fn check_case(entries: &[MapEntry]) {
    let body = encode_new(entries);
    let typed = typed_new(entries);
    assert_eq!(New::decode(body.as_slice()).unwrap(), typed);
    assert_eq!(
        byte_native_admits(&body, &typed),
        strictly_ordered(entries) && canonical_value_presence(entries)
    );
}

#[test]
fn strict_new_map_order_covers_values_duplicates_and_nested_maps() {
    for first in [MapValue::Absent, MapValue::PresentEmpty] {
        for second in [MapValue::Absent, MapValue::PresentEmpty] {
            for reverse in [false, true] {
                for duplicate in [false, true] {
                    check_case(&entries(
                        b'a',
                        b'b',
                        reverse,
                        duplicate,
                        first.clone(),
                        second.clone(),
                    ));
                }
            }
        }
    }
    for reverse in [false, true] {
        let nested = entries(
            b'c',
            b'd',
            reverse,
            false,
            MapValue::Absent,
            MapValue::PresentEmpty,
        );
        check_case(&entries(
            b'a',
            b'b',
            false,
            false,
            MapValue::Nested(nested),
            MapValue::PresentEmpty,
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_new_map_order_matches_byte_native_admission(
        low in b'a'..=b'm',
        high in b'n'..=b'z',
        reverse in any::<bool>(),
        duplicate in any::<bool>(),
        first_mode in 0u8..3,
        second_mode in 0u8..3,
        nested_reverse in any::<bool>(),
        nested_duplicate in any::<bool>(),
    ) {
        let nested = entries(
            b'r',
            b's',
            nested_reverse,
            nested_duplicate,
            MapValue::Absent,
            MapValue::PresentEmpty,
        );
        let entries = entries(
            low,
            high,
            reverse,
            duplicate,
            value(first_mode, &nested),
            value(second_mode, &nested),
        );
        let body = encode_new(&entries);
        let typed = typed_new(&entries);
        prop_assert_eq!(New::decode(body.as_slice()).unwrap(), typed.clone());
        prop_assert_eq!(
            byte_native_admits(&body, &typed),
            strictly_ordered(&entries) && canonical_value_presence(&entries)
        );
    }
}
