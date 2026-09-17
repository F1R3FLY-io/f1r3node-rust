use proptest::prelude::*;

use super::*;
use crate::rust::phlo_resource::PhloAuthorityNode;

fn limits() -> PhloObligationKeyLimits {
    PhloObligationKeyLimits {
        wire: PhloWireLimits {
            total_bytes: 16_384,
            field_bytes: 8192,
        },
        authority_nodes: 256,
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

fn resource() -> PhloResourceKeyV1<'static> {
    PhloResourceKeyV1 {
        location: b"slot",
        class: u32::MAX,
        acquisition_terms: b"captured-terms",
        authority: vec![
            PhloAuthorityNode::And,
            PhloAuthorityNode::Ground(b"alice"),
            PhloAuthorityNode::Quote(b"bob"),
        ],
    }
}

#[test]
fn obligation_kind_has_exact_framing_and_roundtrips() {
    let fee = PhloObligationKeyV1::Fee.encode(limits()).unwrap();
    assert_eq!(fee, pack(&[PHLO_OBLIGATION_V1_DOMAIN, &[0], &[]]));
    let key = resource();
    let payload = key.encode(limits().resource(8192)).unwrap();
    let wire = PhloObligationKeyV1::Resource(key.clone())
        .encode(limits())
        .unwrap();
    assert_eq!(wire, pack(&[PHLO_OBLIGATION_V1_DOMAIN, &[1], &payload]));
    assert!(fee < wire);
    assert_eq!(
        PhloObligationKeyV1::decode(&fee, limits()).unwrap(),
        PhloObligationKeyV1::Fee
    );
    assert_eq!(
        PhloObligationKeyV1::decode(&wire, limits()).unwrap(),
        PhloObligationKeyV1::Resource(key)
    );
    assert_eq!(
        PhloObligationKeyV1::decode(&wire, limits())
            .unwrap()
            .encode(limits())
            .unwrap(),
        wire
    );
}

#[test]
fn unknown_domains_kind_widths_and_fee_payloads_reject() {
    for kind in [&[][..], &[2], &[0, 0], &[1, 0]] {
        assert_eq!(
            PhloObligationKeyV1::decode(&pack(&[PHLO_OBLIGATION_V1_DOMAIN, kind, &[]]), limits()),
            Err(PhloObligationKeyError::InvalidKind)
        );
    }
    assert_eq!(
        PhloObligationKeyV1::decode(&pack(&[b"other-domain", &[0], &[]]), limits()),
        Err(PhloObligationKeyError::FormatDomain)
    );
    assert_eq!(
        PhloObligationKeyV1::decode(
            &pack(&[PHLO_OBLIGATION_V1_DOMAIN, &[0], b"resource"]),
            limits()
        ),
        Err(PhloObligationKeyError::InvalidKind)
    );
    assert!(
        PhloObligationKeyV1::decode(&pack(&[PHLO_OBLIGATION_V1_DOMAIN, &[1], &[]]), limits())
            .is_err()
    );
}

#[test]
fn every_truncation_and_trailing_suffix_rejects() {
    for key in [
        PhloObligationKeyV1::Fee,
        PhloObligationKeyV1::Resource(resource()),
    ] {
        let wire = key.encode(limits()).unwrap();
        for end in 0..wire.len() {
            assert!(PhloObligationKeyV1::decode(&wire[..end], limits()).is_err());
        }
        let mut extra = wire;
        extra.push(0);
        assert_eq!(
            PhloObligationKeyV1::decode(&extra, limits()),
            Err(PhloObligationKeyError::Wire(PhloWireError::TrailingBytes))
        );
    }
}

#[test]
fn total_byte_and_authority_limits_apply_to_nested_payloads() {
    let key = PhloObligationKeyV1::Resource(resource());
    let wire = key.encode(limits()).unwrap();
    for size in 0..=wire.len() {
        let mut bound = limits();
        bound.wire.total_bytes = size;
        if size == wire.len() {
            assert_eq!(key.encode(bound).unwrap(), wire);
            assert_eq!(PhloObligationKeyV1::decode(&wire, bound).unwrap(), key);
        } else {
            assert!(key.encode(bound).is_err());
            assert!(PhloObligationKeyV1::decode(&wire, bound).is_err());
        }
    }
    let mut bound = limits();
    bound.authority_nodes = 2;
    assert!(key.encode(bound).is_err());
    assert!(PhloObligationKeyV1::decode(&wire, bound).is_err());
    bound.authority_nodes = 0;
    assert!(PhloObligationKeyV1::Fee.encode(bound).is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn typed_obligation_roundtrip_preserves_each_resource_component(
        location in prop::collection::vec(any::<u8>(), 0..64),
        terms in prop::collection::vec(any::<u8>(), 0..64),
        owner in prop::collection::vec(any::<u8>(), 0..64),
        class in any::<u32>(), quoted in any::<bool>(),
    ) {
        let authority = if quoted { PhloAuthorityNode::Quote(&owner) } else { PhloAuthorityNode::Ground(&owner) };
        let key = PhloObligationKeyV1::Resource(PhloResourceKeyV1 { location: &location, class, acquisition_terms: &terms, authority: vec![authority] });
        let wire = key.encode(limits()).unwrap();
        let prepared = key.prepare_encoding(limits()).unwrap();
        prop_assert_eq!(prepared.encoded_len(), wire.len());
        prop_assert_eq!(prepared.encode().unwrap(), wire.clone());
        prop_assert_eq!(PhloObligationKeyV1::decode(&wire, limits()).unwrap(), key);
        prop_assert!(PhloObligationKeyV1::Fee.encode(limits()).unwrap() < wire);
        let other = PhloObligationKeyV1::Resource(PhloResourceKeyV1 { location: &location, class: class ^ 1, acquisition_terms: &terms, authority: vec![authority] });
        prop_assert_ne!(other.encode(limits()).unwrap(), wire);
    }
}
