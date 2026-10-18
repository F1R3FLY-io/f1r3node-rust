use std::collections::BTreeSet;

use models::rhoapi::cost_signature_atom::Form;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostBoundSignature, CostClauseRegion, CostRegion, CostSignature,
    CostSignatureAtom, CostTokenStack, ListParWithRandom, Par, ParWithRandom, Receive, ReceiveBind,
    Send, TaggedContinuation,
};
use models::rust::rholang::bincode_encoder::ColdStoreEncode;
use models::rust::rholang::sorter::receive_sort_matcher::ReceiveSortMatcher;
use models::rust::rholang::sorter::send_sort_matcher::SendSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError,
};
use rspace_plus_plus::rspace::serializers::serializers::{
    decode_continuations, decode_datums, encode_continuations, encode_datums,
};

#[test]
fn signed_components_survive_cold_store_round_trip() {
    let region = CostRegion {
        version: 1,
        start_line: 1,
        start_column: 2,
        end_line: 1,
        end_column: 20,
        signer: Some(CostSignature {
            atoms: vec![CostSignatureAtom {
                form: Some(Form::Bound(CostBoundSignature { index: 0 })),
            }],
        }),
        ordinal: 0,
    };
    let send = Send {
        chan: Some(Par::default()),
        cost_region: Some(region.clone()),
        ..Default::default()
    };
    let receive = Receive {
        binds: vec![ReceiveBind {
            source: Some(Par::default()),
            cost_region: Some(region.clone()),
            ..Default::default()
        }],
        body: Some(Par::default()),
        cost_region: Some(region),
        ..Default::default()
    };
    let par = Par::default()
        .with_sends(vec![send])
        .with_receives(vec![receive]);
    let bytes = bincode::serialize(&par).unwrap();
    let restored = Par::cold_decode(&bytes).unwrap();
    assert_eq!(restored, par);
}

#[test]
fn signed_authority_changes_canonical_identity_and_wire_image() {
    let region = |name: &[u8]| CostRegion {
        version: 1,
        start_line: 1,
        start_column: 2,
        end_line: 1,
        end_column: 20,
        signer: Some(CostSignature {
            atoms: vec![CostSignatureAtom {
                form: Some(Form::Ground(name.to_vec())),
            }],
        }),
        ordinal: 0,
    };
    let send = |name: &[u8]| Send {
        chan: Some(Par::default()),
        cost_region: Some(region(name)),
        ..Default::default()
    };
    let a = send(b"a");
    let b = send(b"b");
    assert_ne!(
        SendSortMatcher::sort_match(&a).score,
        SendSortMatcher::sort_match(&b).score
    );

    let receive = |name: &[u8]| Receive {
        binds: vec![ReceiveBind {
            source: Some(Par::default()),
            cost_region: Some(region(name)),
            ..Default::default()
        }],
        body: Some(Par::default()),
        cost_region: Some(region(name)),
        ..Default::default()
    };
    let ra = receive(b"a");
    let rb = receive(b"b");
    assert_ne!(
        ReceiveSortMatcher::sort_match(&ra).score,
        ReceiveSortMatcher::sort_match(&rb).score
    );

    let par = Par::default()
        .with_sends(vec![a, b])
        .with_receives(vec![ra, rb]);
    assert_eq!(par.family_distinct_len::<Send>(), 2);
    assert_eq!(par.family_distinct_len::<Receive>(), 2);
    let wire = par.encode_to_vec();
    assert_eq!(Par::decode(wire.as_slice()).unwrap(), par);
    assert_eq!(
        Par::cold_decode(&bincode::serialize(&par).unwrap()).unwrap(),
        par
    );
}

#[test]
fn resolved_name_remains_distinct_from_quoted_principal() {
    let make_send = |form| Send {
        chan: Some(Par::default()),
        cost_region: Some(CostRegion {
            version: 1,
            signer: Some(CostSignature {
                atoms: vec![CostSignatureAtom { form: Some(form) }],
            }),
            ..CostRegion::default()
        }),
        ..Send::default()
    };
    let runtime = make_send(Form::RuntimeName(vec![9, 8, 7]));
    let quote = make_send(Form::Quote(vec![9, 8, 7]));
    assert_ne!(
        SendSortMatcher::sort_match(&runtime).score,
        SendSortMatcher::sort_match(&quote).score
    );
    let par = Par::default().with_sends(vec![runtime, quote]);
    assert_eq!(par.family_distinct_len::<Send>(), 2);
    assert_eq!(Par::decode(par.encode_to_vec().as_slice()).unwrap(), par);
    assert_eq!(
        Par::cold_decode(&bincode::serialize(&par).unwrap()).unwrap(),
        par
    );
}

#[test]
fn ordered_token_stacks_keep_order_and_multiplicity_across_wires() {
    let layer = |name: &[u8]| CostSignature {
        atoms: vec![CostSignatureAtom {
            form: Some(Form::Ground(name.to_vec())),
        }],
    };
    let ab = CostTokenStack {
        layers: vec![layer(b"a"), layer(b"b")],
    };
    let ba = CostTokenStack {
        layers: vec![layer(b"b"), layer(b"a")],
    };
    let par = Par::default().with_cost_token_stacks(vec![ab.clone(), ba.clone(), ab]);
    assert_eq!(par.family_distinct_len::<CostTokenStack>(), 2);
    assert_eq!(par.family_cardinality::<CostTokenStack>(), 3);
    assert_eq!(Par::decode(par.encode_to_vec().as_slice()).unwrap(), par);
    assert_eq!(
        Par::cold_decode(&bincode::serialize(&par).unwrap()).unwrap(),
        par
    );
}

#[test]
fn stored_comm_authority_survives_wire_and_cold_store_boundaries() {
    let region = |ordinal| CostRegion {
        version: 1,
        signer: Some(CostSignature {
            atoms: vec![CostSignatureAtom {
                form: Some(Form::RuntimeName(vec![1, 2, 3, ordinal as u8])),
            }],
        }),
        ordinal,
        ..Default::default()
    };
    let guard = Par::default().with_sends(vec![Send {
        chan: Some(Par::default()),
        ..Default::default()
    }]);
    let continuation = TaggedContinuation {
        guard: Some(guard.clone()),
        cost_region: Some(region(0)),
        clause_regions: vec![
            CostClauseRegion {
                bind_index: 0,
                region: Some(region(1)),
            },
            CostClauseRegion {
                bind_index: 2,
                region: Some(region(2)),
            },
        ],
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(guard.clone()),
            random_state: vec![4, 5, 6],
        })),
    };
    let datum = ListParWithRandom {
        pars: vec![guard],
        random_state: vec![7, 8, 9],
        cost_region: Some(region(3)),
    };

    assert_eq!(
        TaggedContinuation::decode(continuation.encode_to_vec().as_slice()).unwrap(),
        continuation
    );
    assert_eq!(
        ListParWithRandom::decode(datum.encode_to_vec().as_slice()).unwrap(),
        datum
    );
    let continuation_bytes = continuation.cold_encode();
    let datum_bytes = datum.cold_encode();
    let (restored_continuation, consumed_continuation) =
        TaggedContinuation::cold_decode_prefix(&[continuation_bytes.as_slice(), &[0xa5]].concat())
            .unwrap();
    let (restored_datum, consumed_datum) =
        ListParWithRandom::cold_decode_prefix(&[datum_bytes.as_slice(), &[0xa5]].concat()).unwrap();
    assert_eq!(restored_continuation, continuation);
    assert_eq!(restored_datum, datum);
    assert_eq!(consumed_continuation, continuation_bytes.len());
    assert_eq!(consumed_datum, datum_bytes.len());
}

#[test]
fn authority_bearing_rspace_leaves_survive_cold_store_round_trip() {
    let region = |ordinal| CostRegion {
        version: 1,
        signer: Some(CostSignature {
            atoms: vec![CostSignatureAtom {
                form: Some(Form::RuntimeName(vec![1, 2, 3, ordinal as u8])),
            }],
        }),
        ordinal,
        ..Default::default()
    };
    let channel = Par::default();
    let datum = Datum::create(
        &channel,
        ListParWithRandom {
            pars: vec![Par::default()],
            random_state: vec![5, 6],
            cost_region: Some(region(1)),
        },
        true,
    );
    let datum_leaf = encode_datums(&vec![datum.clone()]);
    assert_eq!(
        decode_datums::<ListParWithRandom>(&datum_leaf).unwrap(),
        vec![datum.clone()]
    );

    let continuation = TaggedContinuation {
        guard: Some(Par::default()),
        tagged_cont: Some(TaggedCont::ScalaBodyRef(7)),
        cost_region: Some(region(2)),
        clause_regions: vec![CostClauseRegion {
            bind_index: 0,
            region: Some(region(3)),
        }],
    };
    let waiting = WaitingContinuation::create(
        &vec![channel.clone()],
        &vec![BindPattern::default()],
        &continuation,
        true,
        BTreeSet::new(),
    );
    let continuation_leaf = encode_continuations(&vec![waiting.clone()]);
    assert_eq!(
        decode_continuations::<BindPattern, TaggedContinuation>(&continuation_leaf).unwrap(),
        vec![waiting.clone()]
    );

    let mut other_datum = datum.a.as_ref().clone();
    other_datum.cost_region = Some(region(4));
    let other_datum = Datum::create(&channel, other_datum, true);
    assert_ne!(datum.source.hash, other_datum.source.hash);

    let mut other_continuation = continuation;
    other_continuation.cost_region = Some(region(4));
    let other_waiting = WaitingContinuation::create(
        &vec![channel],
        &vec![BindPattern::default()],
        &other_continuation,
        true,
        BTreeSet::new(),
    );
    assert_ne!(waiting.source.hash, other_waiting.source.hash);
}

#[test]
fn unsupported_authority_record_versions_are_rejected() {
    let region = CostRegion {
        version: 1,
        signer: Some(CostSignature {
            atoms: vec![CostSignatureAtom {
                form: Some(Form::Ground(b"payer".to_vec())),
            }],
        }),
        ..Default::default()
    };
    let mut datum = ListParWithRandom {
        cost_region: Some(region.clone()),
        ..Default::default()
    }
    .cold_encode();
    datum[8] = 2;
    assert!(matches!(
        ListParWithRandom::cold_decode(&datum),
        Err(ColdStoreDecodeError::UnsupportedCostAuthorityVersion(2))
    ));

    let mut continuation = TaggedContinuation {
        cost_region: Some(region),
        ..Default::default()
    }
    .cold_encode();
    continuation[1] = 2;
    assert!(matches!(
        TaggedContinuation::cold_decode(&continuation),
        Err(ColdStoreDecodeError::UnsupportedCostAuthorityVersion(2))
    ));
}
