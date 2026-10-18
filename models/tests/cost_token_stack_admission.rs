use models::rhoapi::cost_signature_atom::Form;
use models::rhoapi::{CostBoundSignature, CostSignature, CostSignatureAtom, CostTokenStack};
use models::rust::par_bag::{
    with_component_key, ParBag, ParBagCodecError, ParComponentTag, PAR_BAG_KEY_DOMAIN,
};
use models::rust::rholang::{protobuf_decoder, protobuf_encoder};
use proptest::prelude::*;
use prost::Message as _;
use rspace_plus_plus::rspace::bag::Bag;

fn integer(value: i64) -> Vec<u8> {
    let mut bytes = vec![1, 1];
    bytes.extend_from_slice(&((value as u64) ^ (1u64 << 63)).to_be_bytes());
    bytes
}

fn bytes(value: &[u8]) -> Vec<u8> {
    let mut encoded = vec![1, 3];
    for &byte in value {
        encoded.push(byte);
        if byte == 0 {
            encoded.push(0xff);
        }
    }
    encoded.extend_from_slice(&[0, 0]);
    encoded
}

fn constructor(tag: i64, children: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    let mut encoded = vec![2];
    encoded.extend(integer(tag));
    for child in children {
        encoded.extend(child);
    }
    encoded.push(0);
    encoded
}

fn semantic_score(stack: &CostTokenStack) -> Vec<u8> {
    let layers = stack.layers.iter().map(|layer| {
        constructor(
            501,
            layer.atoms.iter().map(|atom| match &atom.form {
                Some(Form::Ground(value)) => constructor(502, [bytes(value)]),
                Some(Form::Quote(value)) => constructor(503, [bytes(value)]),
                Some(Form::Bound(value)) => constructor(504, [integer(i64::from(value.index))]),
                Some(Form::RuntimeName(value)) => constructor(507, [bytes(value)]),
                None => constructor(505, []),
            }),
        )
    });
    constructor(508, layers)
}

fn key(score: &[u8], body: &[u8]) -> Vec<u8> {
    let mut key = PAR_BAG_KEY_DOMAIN.to_vec();
    key.push(ParComponentTag::CostTokenStack.byte());
    key.extend_from_slice(score);
    key.extend_from_slice(body);
    key.extend_from_slice(&(score.len() as u64).to_be_bytes());
    key
}

fn admit(key: &[u8]) -> Result<ParBag, ParBagCodecError> {
    let mut raw = Bag::new();
    raw.insert(key).expect("the raw Bag accepts a byte path");
    ParBag::try_from_bag(raw)
}

fn atom(form: Option<Form>) -> CostSignatureAtom { CostSignatureAtom { form } }

fn layer(atoms: Vec<CostSignatureAtom>) -> CostSignature { CostSignature { atoms } }

fn nested_atom_body(atom_body: &[u8]) -> Vec<u8> {
    assert!(atom_body.len() < 128);
    let mut layer_body = vec![0x0a, atom_body.len() as u8];
    layer_body.extend_from_slice(atom_body);
    let mut stack_body = vec![0x0a, layer_body.len() as u8];
    stack_body.extend_from_slice(&layer_body);
    stack_body
}

#[test]
fn canonical_layers_match_frozen_and_semantic_score_oracles() {
    let empty = CostTokenStack::default();
    assert_eq!(semantic_score(&empty), [
        2, 1, 1, 0x80, 0, 0, 0, 0, 0, 1, 0xfc, 0
    ]);
    assert!(empty.encode_to_vec().is_empty());

    let single_ground = CostTokenStack {
        layers: vec![layer(vec![atom(Some(Form::Ground(b"a".to_vec())))])],
    };
    assert_eq!(single_ground.encode_to_vec(), [
        0x0a, 0x05, 0x0a, 0x03, 0x0a, 0x01, b'a'
    ]);

    let cases = [
        empty,
        single_ground,
        CostTokenStack {
            layers: vec![layer(vec![])],
        },
        CostTokenStack {
            layers: vec![
                layer(vec![
                    atom(Some(Form::Ground(vec![0, 0xff, 3]))),
                    atom(Some(Form::Quote(vec![9, 0, 8]))),
                    atom(Some(Form::Bound(CostBoundSignature { index: -7 }))),
                    atom(Some(Form::RuntimeName(vec![4, 5]))),
                    atom(None),
                ]),
                layer(vec![atom(Some(Form::Ground(b"second".to_vec())))]),
                layer(vec![]),
            ],
        },
    ];

    for stack in cases {
        let body = stack.encode_to_vec();
        assert_eq!(protobuf_encoder::encode_to_vec(&stack), body);
        let score = semantic_score(&stack);
        let independent = key(&score, &body);
        assert_eq!(with_component_key(&stack, <[u8]>::to_vec), independent);
        let imported =
            admit(&independent).expect("canonical layers must pass byte-native admission");
        assert_eq!(imported.count::<CostTokenStack>(&stack), 1);
    }
}

#[test]
fn canonical_layer_body_rejects_a_different_valid_semantic_score() {
    let ground = CostTokenStack {
        layers: vec![layer(vec![atom(Some(Form::Ground(b"ground".to_vec())))])],
    };
    let quote = CostTokenStack {
        layers: vec![layer(vec![atom(Some(Form::Quote(b"ground".to_vec())))])],
    };
    let body = ground.encode_to_vec();
    let wrong_score = semantic_score(&quote);
    assert_ne!(semantic_score(&ground), wrong_score);
    assert!(matches!(
        admit(&key(&wrong_score, &body)),
        Err(ParBagCodecError::NonCanonicalComponent {
            tag: ParComponentTag::CostTokenStack
        })
    ));
}

#[test]
fn nested_layer_wire_aliases_fail_admission_even_with_their_decoded_score() {
    let malformed_atoms: [&[u8]; 3] = [
        &[0x0a, 0x01, b'a', 0x28, 0x01],
        &[0x0a, 0x01, b'a', 0x12, 0x01, b'b'],
        &[0x1a, 0x02, 0x08, 0x00],
    ];

    for atom_body in malformed_atoms {
        let body = nested_atom_body(atom_body);
        let decoded = CostTokenStack::decode(body.as_slice())
            .expect("Prost accepts the noncanonical nested layer");
        assert_ne!(decoded.encode_to_vec(), body);
        let score = semantic_score(&decoded);
        assert!(matches!(
            admit(&key(&score, &body)),
            Err(ParBagCodecError::NonCanonicalComponent {
                tag: ParComponentTag::CostTokenStack
            })
        ));
    }
}

fn arbitrary_atom() -> impl Strategy<Value = CostSignatureAtom> {
    prop_oneof![
        Just(atom(None)),
        prop::collection::vec(any::<u8>(), 0..12).prop_map(|value| atom(Some(Form::Ground(value)))),
        prop::collection::vec(any::<u8>(), 0..12).prop_map(|value| atom(Some(Form::Quote(value)))),
        any::<i16>().prop_map(|index| atom(Some(Form::Bound(CostBoundSignature {
            index: i32::from(index),
        })))),
        prop::collection::vec(any::<u8>(), 0..12)
            .prop_map(|value| atom(Some(Form::RuntimeName(value)))),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_layers_match_admission_and_reject_wrong_score(
        layers in prop::collection::vec(
            prop::collection::vec(arbitrary_atom(), 0..5), 0..5,
        ),
    ) {
        let stack = CostTokenStack {
            layers: layers.into_iter().map(layer).collect(),
        };
        let body = protobuf_encoder::encode_to_vec(&stack);
        assert_eq!(body, stack.encode_to_vec());
        assert_eq!(
            protobuf_decoder::decode_cost_token_stack(body.as_slice())
                .expect("generated CostTokenStack decoder"),
            stack,
        );
        let score = semantic_score(&stack);
        let canonical_key = key(&score, &body);
        assert_eq!(with_component_key(&stack, <[u8]>::to_vec), canonical_key);
        let imported = admit(&canonical_key).expect("generated canonical stack admits");
        assert_eq!(imported.count::<CostTokenStack>(&stack), 1);

        let mut altered = stack.clone();
        altered.layers.push(layer(vec![]));
        let wrong_score = semantic_score(&altered);
        assert_ne!(score, wrong_score);
        assert!(matches!(
            admit(&key(&wrong_score, &body)),
            Err(ParBagCodecError::NonCanonicalComponent {
                tag: ParComponentTag::CostTokenStack,
            })
        ));
    }

    #[test]
    fn generated_nested_unknown_field_alias_is_rejected(
        ground in prop::collection::vec(any::<u8>(), 0..32),
    ) {
        let mut atom_body = vec![0x0a, ground.len() as u8];
        atom_body.extend_from_slice(&ground);
        atom_body.extend_from_slice(&[0x28, 0x01]);
        let body = nested_atom_body(&atom_body);
        let decoded = CostTokenStack::decode(body.as_slice())
            .expect("Prost accepts generated nested unknown field");
        assert_ne!(decoded.encode_to_vec(), body);
        assert!(matches!(
            admit(&key(&semantic_score(&decoded), &body)),
            Err(ParBagCodecError::NonCanonicalComponent {
                tag: ParComponentTag::CostTokenStack,
            })
        ));
    }
}
