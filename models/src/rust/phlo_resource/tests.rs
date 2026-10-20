use proptest::prelude::*;

use super::*;

fn limits() -> PhloResourceLimits {
    PhloResourceLimits {
        wire: PhloWireLimits {
            total_bytes: 1_048_576,
            field_bytes: 524_288,
        },
        authority_nodes: 8192,
    }
}

fn pack(fields: &[&[u8]]) -> Vec<u8> {
    fields
        .iter()
        .flat_map(|field| {
            (field.len() as u64)
                .to_be_bytes()
                .into_iter()
                .chain(field.iter().copied())
        })
        .collect()
}

fn oracle_nodes(nodes: &[PhloAuthorityNode<'_>]) -> Vec<u8> {
    let count = (nodes.len() as u32).to_be_bytes();
    let mut output = pack(&[&count]);
    for node in nodes {
        let (tag, payload): (u8, &[u8]) = match node {
            PhloAuthorityNode::Unit => (0, &[]),
            PhloAuthorityNode::Ground(bytes) => (1, bytes),
            PhloAuthorityNode::Quote(bytes) => (2, bytes),
            PhloAuthorityNode::And => (3, &[]),
        };
        output.extend(pack(&[&pack(&[&[tag], payload])]));
    }
    output
}

fn oracle(key: &PhloResourceKeyV1<'_>) -> Vec<u8> {
    pack(&[
        PHLO_RESOURCE_V1_DOMAIN,
        key.location,
        &key.class.to_be_bytes(),
        key.acquisition_terms,
        &oracle_nodes(&key.authority),
    ])
}

fn example() -> PhloResourceKeyV1<'static> {
    PhloResourceKeyV1 {
        location: b"located-surface",
        class: 0x10203040,
        acquisition_terms: b"retained-terms",
        authority: vec![
            PhloAuthorityNode::And,
            PhloAuthorityNode::Ground(b"wallet-A"),
            PhloAuthorityNode::Quote(b"quoted-authority"),
        ],
    }
}

#[test]
fn phlo_resource_matches_independent_framing_and_roundtrips() {
    let key = example();
    let wire = key.encode(limits()).unwrap();
    assert_eq!(wire, oracle(&key));
    assert_eq!(PhloResourceKeyV1::decode(&wire, limits()).unwrap(), key);
    assert_eq!(
        PhloResourceKeyV1::decode(&wire, limits())
            .unwrap()
            .encode(limits())
            .unwrap(),
        wire
    );
}

#[test]
fn phlo_resource_preserves_all_identity_components_and_multiplicity() {
    let key = example();
    let wire = key.encode(limits()).unwrap();
    let mut variants = Vec::new();
    let mut changed = key.clone();
    changed.location = b"other";
    variants.push(changed);
    let mut changed = key.clone();
    changed.class += 1;
    variants.push(changed);
    let mut changed = key.clone();
    changed.acquisition_terms = b"other";
    variants.push(changed);
    let mut changed = key.clone();
    changed.authority.swap(1, 2);
    variants.push(changed);
    let mut changed = key.clone();
    changed.authority[1] = PhloAuthorityNode::Quote(b"wallet-A");
    variants.push(changed);
    let mut changed = key.clone();
    changed.authority[2] = PhloAuthorityNode::Ground(b"quoted-authority");
    variants.push(changed);
    let mut changed = key.clone();
    changed.authority[1] = PhloAuthorityNode::Ground(b"wallet-B");
    variants.push(changed);
    let mut changed = key.clone();
    changed.authority[2] = PhloAuthorityNode::Quote(b"different");
    variants.push(changed);
    let mut changed = key.clone();
    changed.authority = vec![PhloAuthorityNode::Unit];
    variants.push(changed);
    let mut changed = key.clone();
    changed.authority = vec![PhloAuthorityNode::Ground(b"")];
    variants.push(changed);
    let mut changed = key.clone();
    changed.authority.splice(1..1, [
        PhloAuthorityNode::And,
        PhloAuthorityNode::Ground(b"wallet-A"),
    ]);
    variants.push(changed);
    for variant in variants {
        let altered = variant.encode(limits()).unwrap();
        assert_ne!(altered, wire);
        assert_eq!(
            PhloResourceKeyV1::decode(&altered, limits()).unwrap(),
            variant
        );
    }
}

fn parse_shape<'a>(nodes: &'a [PhloAuthorityNode<'a>]) -> Option<&'a [PhloAuthorityNode<'a>]> {
    let (first, rest) = nodes.split_first()?;
    match first {
        PhloAuthorityNode::And => parse_shape(parse_shape(rest)?),
        _ => Some(rest),
    }
}

#[test]
fn phlo_resource_shape_matches_exhaustive_independent_tree_parser() {
    for count in 0..=8 {
        for mut pattern in 0..4usize.pow(count) {
            let authority = (0..count)
                .map(|_| {
                    let node = match pattern % 4 {
                        0 => PhloAuthorityNode::Unit,
                        1 => PhloAuthorityNode::Ground(b"a"),
                        2 => PhloAuthorityNode::Quote(b"q"),
                        _ => PhloAuthorityNode::And,
                    };
                    pattern /= 4;
                    node
                })
                .collect();
            let key = PhloResourceKeyV1 {
                authority,
                ..example()
            };
            let valid = parse_shape(&key.authority).is_some_and(|rest| rest.is_empty());
            assert_eq!(key.encode(limits()).is_ok(), valid);
            assert_eq!(
                PhloResourceKeyV1::decode(&oracle(&key), limits()).is_ok(),
                valid
            );
        }
    }
}

#[test]
fn phlo_resource_rejects_unknown_tags_wrong_widths_and_hidden_node_payloads() {
    let fields = |nodes: &[u8]| {
        pack(&[
            PHLO_RESOURCE_V1_DOMAIN,
            b"",
            &0u32.to_be_bytes(),
            b"",
            nodes,
        ])
    };
    for tag in 4..=255 {
        let nodes = pack(&[&1u32.to_be_bytes(), &pack(&[&[tag], b""])]);
        assert_eq!(
            PhloResourceKeyV1::decode(&fields(&nodes), limits()),
            Err(PhloResourceKeyError::UnsupportedNode)
        );
    }
    for tag in [0, 3] {
        let nodes = pack(&[&1u32.to_be_bytes(), &pack(&[&[tag], b"hidden"])]);
        assert_eq!(
            PhloResourceKeyV1::decode(&fields(&nodes), limits()),
            Err(PhloResourceKeyError::AuthorityShape)
        );
    }
    for tag in [vec![], vec![0, 0]] {
        let nodes = pack(&[&1u32.to_be_bytes(), &pack(&[&tag, b""])]);
        assert_eq!(
            PhloResourceKeyV1::decode(&fields(&nodes), limits()),
            Err(PhloResourceKeyError::FieldWidth)
        );
    }
    for width in [0, 1, 3, 5, 8] {
        let wire = pack(&[
            PHLO_RESOURCE_V1_DOMAIN,
            b"",
            &vec![0; width],
            b"",
            &oracle_nodes(&[PhloAuthorityNode::Unit]),
        ]);
        assert_eq!(
            PhloResourceKeyV1::decode(&wire, limits()),
            Err(PhloResourceKeyError::FieldWidth)
        );
    }
    let wire = pack(&[
        b"different-format",
        b"",
        &0u32.to_be_bytes(),
        b"",
        &oracle_nodes(&[PhloAuthorityNode::Unit]),
    ]);
    assert_eq!(
        PhloResourceKeyV1::decode(&wire, limits()),
        Err(PhloResourceKeyError::FormatDomain)
    );
}

#[test]
fn phlo_resource_rejects_all_truncations_and_trailing_fields_at_every_level() {
    let wire = oracle(&example());
    for length in 0..wire.len() {
        assert!(PhloResourceKeyV1::decode(&wire[..length], limits()).is_err());
    }
    let nodes = oracle_nodes(&example().authority);
    for length in 0..nodes.len() {
        let truncated = pack(&[
            PHLO_RESOURCE_V1_DOMAIN,
            b"l",
            &0u32.to_be_bytes(),
            b"t",
            &nodes[..length],
        ]);
        assert!(PhloResourceKeyV1::decode(&truncated, limits()).is_err());
    }
    let node = pack(&[&[0], b""]);
    for length in 0..node.len() {
        let nodes = pack(&[&1u32.to_be_bytes(), &node[..length]]);
        let truncated = pack(&[
            PHLO_RESOURCE_V1_DOMAIN,
            b"l",
            &0u32.to_be_bytes(),
            b"t",
            &nodes,
        ]);
        assert!(PhloResourceKeyV1::decode(&truncated, limits()).is_err());
    }
    let mut outer_extra = wire;
    outer_extra.extend(pack(&[b"extra"]));
    let mut nodes_extra = nodes;
    nodes_extra.extend(pack(&[b"extra"]));
    let mut node_extra = node;
    node_extra.extend(pack(&[b"extra"]));
    let node_list = pack(&[&1u32.to_be_bytes(), &node_extra]);
    for wire in [
        outer_extra,
        pack(&[
            PHLO_RESOURCE_V1_DOMAIN,
            b"l",
            &0u32.to_be_bytes(),
            b"t",
            &nodes_extra,
        ]),
        pack(&[
            PHLO_RESOURCE_V1_DOMAIN,
            b"l",
            &0u32.to_be_bytes(),
            b"t",
            &node_list,
        ]),
    ] {
        assert_eq!(
            PhloResourceKeyV1::decode(&wire, limits()),
            Err(PhloResourceKeyError::Wire(PhloWireError::TrailingBytes))
        );
    }
}

#[test]
fn phlo_resource_bounds_apply_before_count_sized_allocation() {
    let key = example();
    let wire = oracle(&key);
    let nested_size = oracle_nodes(&key.authority).len();
    let exact = PhloResourceLimits {
        wire: PhloWireLimits {
            total_bytes: wire.len(),
            field_bytes: nested_size,
        },
        authority_nodes: key.authority.len(),
    };
    assert_eq!(key.encode(exact), Ok(wire.clone()));
    assert_eq!(PhloResourceKeyV1::decode(&wire, exact), Ok(key.clone()));
    for reduced in [
        PhloResourceLimits {
            authority_nodes: exact.authority_nodes - 1,
            ..exact
        },
        PhloResourceLimits {
            wire: PhloWireLimits {
                total_bytes: wire.len() - 1,
                ..exact.wire
            },
            ..exact
        },
        PhloResourceLimits {
            wire: PhloWireLimits {
                field_bytes: nested_size - 1,
                ..exact.wire
            },
            ..exact
        },
    ] {
        assert!(key.encode(reduced).is_err());
        assert!(PhloResourceKeyV1::decode(&wire, reduced).is_err());
    }
    let forged = pack(&[
        PHLO_RESOURCE_V1_DOMAIN,
        b"l",
        &0u32.to_be_bytes(),
        b"t",
        &pack(&[&u32::MAX.to_be_bytes()]),
    ]);
    assert_eq!(
        PhloResourceKeyV1::decode(&forged, limits()),
        Err(PhloResourceKeyError::NodeLimit)
    );
    let unbounded_count = PhloResourceLimits {
        authority_nodes: usize::MAX,
        ..limits()
    };
    assert_eq!(
        PhloResourceKeyV1::decode(&forged, unbounded_count),
        Err(PhloResourceKeyError::Wire(PhloWireError::Truncated))
    );
}

#[test]
fn phlo_resource_supports_large_compound_authorities_without_recursive_decoding() {
    for owners in [1, 2, 3, 64, 129, 1024, 4096] {
        let mut authority = vec![PhloAuthorityNode::And; owners - 1];
        authority.extend(vec![
            PhloAuthorityNode::Ground(
                b"repeated-logical-occurrence"
            );
            owners
        ]);
        let key = PhloResourceKeyV1 {
            authority,
            ..example()
        };
        let wire = key.encode(limits()).unwrap();
        assert_eq!(PhloResourceKeyV1::decode(&wire, limits()).unwrap(), key);
    }
}

#[derive(Clone, Debug)]
enum Tree {
    Unit,
    Ground(Vec<u8>),
    Quote(Vec<u8>),
    And(Box<Tree>, Box<Tree>),
}

impl Tree {
    fn nodes<'a>(&'a self, output: &mut Vec<PhloAuthorityNode<'a>>) {
        match self {
            Self::Unit => output.push(PhloAuthorityNode::Unit),
            Self::Ground(bytes) => output.push(PhloAuthorityNode::Ground(bytes)),
            Self::Quote(bytes) => output.push(PhloAuthorityNode::Quote(bytes)),
            Self::And(left, right) => {
                output.push(PhloAuthorityNode::And);
                left.nodes(output);
                right.nodes(output);
            }
        }
    }
}

fn tree() -> impl Strategy<Value = Tree> {
    prop_oneof![
        Just(Tree::Unit),
        prop::collection::vec(any::<u8>(), 0..40).prop_map(Tree::Ground),
        prop::collection::vec(any::<u8>(), 0..40).prop_map(Tree::Quote)
    ]
    .prop_recursive(8, 256, 2, |inner| {
        (inner.clone(), inner).prop_map(|(left, right)| Tree::And(Box::new(left), Box::new(right)))
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn phlo_resource_generated_trees_preserve_full_key_and_exact_bytes(
        tree in tree(), class in any::<u32>(), location in prop::collection::vec(any::<u8>(), 0..64),
        terms in prop::collection::vec(any::<u8>(), 0..64), mutation in any::<usize>(),
    ) {
        let mut authority = Vec::new(); tree.nodes(&mut authority);
        let key = PhloResourceKeyV1 { location: &location, class, acquisition_terms: &terms, authority };
        let wire = key.encode(limits()).unwrap();
        let prepared = key.prepare_encoding(limits()).unwrap();
        prop_assert_eq!(prepared.size, wire.len());
        let mut direct = PhloWireEncoder::with_capacity(limits().wire, prepared.size).unwrap();
        let original_pointer = direct.as_bytes().as_ptr();
        prepared.write_to(&mut direct).unwrap();
        prop_assert_eq!(direct.as_bytes().as_ptr(), original_pointer);
        prop_assert_eq!(direct.into_bytes(), wire.clone());
        prop_assert_eq!(&wire, &oracle(&key));
        let decoded = PhloResourceKeyV1::decode(&wire, limits()).unwrap();
        prop_assert_eq!(&decoded, &key);
        prop_assert_eq!(decoded.encode(limits()).unwrap(), wire.clone());
        let mut changed = wire.clone();
        let index = mutation % changed.len(); changed[index] ^= 1;
        if let Ok(altered) = PhloResourceKeyV1::decode(&changed, limits()) {
            prop_assert_ne!(&altered, &key);
            prop_assert_eq!(altered.encode(limits()).unwrap(), changed);
        }
    }
}
