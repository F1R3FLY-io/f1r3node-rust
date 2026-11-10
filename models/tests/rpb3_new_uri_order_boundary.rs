use models::rhoapi::{New, Par};
use models::rust::par_bag::{with_component_key, ParBag, PAR_BAG_KEY_DOMAIN};
use models::rust::rholang::canonical_semantics::ParComponentTag;
use proptest::prelude::*;
use prost::Message;
use rspace_plus_plus::rspace::bag::Bag;

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

fn source_new_body(uris: &[String], complete: bool) -> Vec<u8> {
    let mut body = Vec::new();
    if complete {
        put_message(2, &[], &mut body);
    }
    for uri in uris {
        put_message(3, uri.as_bytes(), &mut body);
    }
    body
}

fn score_integer(value: i64) -> Vec<u8> {
    let mut score = vec![1, 1];
    score.extend_from_slice(&((value as u64) ^ (1u64 << 63)).to_be_bytes());
    score
}

fn score_string(value: &[u8]) -> Vec<u8> {
    let mut score = vec![1, 2];
    for &byte in value {
        score.push(byte);
        if byte == 0 {
            score.push(255);
        }
    }
    score.extend_from_slice(&[0, 0]);
    score
}

fn score_constructor(constructor: i64, children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut score = vec![2];
    score.extend(score_integer(constructor));
    for child in children {
        score.extend(child);
    }
    score.push(0);
    score
}

fn source_new_score(uris: &[String], complete: bool) -> Vec<u8> {
    if !complete {
        return score_constructor(i64::from(i32::MAX), Vec::new());
    }
    let mut children = vec![score_integer(0)];
    children.extend(uris.iter().map(|uri| score_string(uri.as_bytes())));
    children.push(score_integer(0));
    children.push(score_constructor(999, vec![score_integer(0)]));
    score_constructor(303, children)
}

fn source_new_key(body: &[u8], score: &[u8]) -> Vec<u8> {
    let mut key = PAR_BAG_KEY_DOMAIN.to_vec();
    key.push(ParComponentTag::New.byte());
    key.extend_from_slice(score);
    key.extend_from_slice(body);
    key.extend_from_slice(&(score.len() as u64).to_be_bytes());
    key
}

fn byte_native_admits(key: &[u8]) -> bool {
    let mut bag = Bag::new();
    bag.insert(key)
        .expect("the candidate fits in a singleton bag");
    ParBag::try_from_bag(bag).is_ok()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn complete_new_uri_order_accepts_equality_and_refuses_descending(
        low in b'a'..=b'm',
        high in b'n'..=b'z',
        suffix in prop::collection::vec(b'a'..=b'z', 0..8),
    ) {
        let low = String::from_utf8(std::iter::once(low).chain(suffix.iter().copied()).collect())
            .expect("generated ASCII URI");
        let high = String::from_utf8(std::iter::once(high).chain(suffix).collect())
            .expect("generated ASCII URI");
        prop_assert!(low < high);

        for (uris, complete, should_accept) in [
            (vec![low.clone(), low.clone()], true, true),
            (vec![low.clone(), high.clone()], true, true),
            (vec![high.clone(), low.clone()], true, false),
            (vec![high.clone(), low.clone()], false, true),
        ] {
            let body = source_new_body(&uris, complete);
            let score = source_new_score(&uris, complete);
            let key = source_new_key(&body, &score);
            let typed = New {
                p: complete.then_some(Par::default()),
                uri: uris,
                ..New::default()
            };
            prop_assert_eq!(New::decode(body.as_slice()).unwrap(), typed.clone());
            if should_accept {
                prop_assert_eq!(with_component_key(&typed, <[u8]>::to_vec), key.clone());
            }
            prop_assert_eq!(byte_native_admits(&key), should_accept);
        }
    }
}
