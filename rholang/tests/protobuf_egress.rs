use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par, TaggedContinuation};
use models::rholang_scala_rust_types::ConsumeResultReturn;
use models::rspace_plus_plus_types::{
    ChannelsProto, DatumProto, HotStoreStateProto, JoinProto, SoftCheckpointProto,
    StoreStateContMapEntry, StoreStateDataMapEntry, StoreStateInstalledContMapEntry,
    StoreStateInstalledJoinsMapEntry, StoreStateJoinsMapEntry, StoreToMapEntry, StoreToMapResult,
    StoreToMapValue, WaitingContinuationProto,
};
use models::rust::par_bag::ParProtobufProjectionPreparation;
use prost::bytes::{Buf, BufMut};
use prost::encoding::{DecodeContext, WireType};
use prost::{DecodeError, Message};
use rholang::rust::protobuf_egress::{
    encode_frame, try_visit_consume_result_par_slots, try_visit_hot_changes_par_slots,
    try_visit_soft_checkpoint_par_slots, ProjectionPreparationBudget, ProtobufEgressError,
};

fn integer(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

#[test]
fn frame_is_exactly_the_legacy_prefix_and_protobuf_body_in_one_result() {
    let message = ChannelsProto {
        channels: vec![integer(7), integer(11)],
    };
    let body = message.encode_to_vec();
    let mut expected = Vec::with_capacity(4 + body.len());
    expected.extend_from_slice(&(body.len() as u32).to_le_bytes());
    expected.extend_from_slice(&body);

    let frame = encode_frame(&message).unwrap();
    assert_eq!(frame, expected);
    assert_eq!(ChannelsProto::decode(&frame[4..]).unwrap(), message);
}

#[derive(Debug, Default)]
struct OversizedMessage;

impl Message for OversizedMessage {
    fn encode_raw(&self, _buf: &mut impl BufMut) {
        panic!("an unrepresentable body must be refused before emission")
    }

    fn merge_field(
        &mut self,
        _tag: u32,
        _wire_type: WireType,
        _buf: &mut impl Buf,
        _ctx: DecodeContext,
    ) -> Result<(), DecodeError> {
        unreachable!("the egress test never decodes the synthetic message")
    }

    fn encoded_len(&self) -> usize { u32::MAX as usize + 1 }

    fn clear(&mut self) {}
}

#[test]
fn frame_rejects_an_unrepresentable_body_before_emission() {
    assert!(matches!(
        encode_frame(&OversizedMessage),
        Err(ProtobufEgressError::BodyLengthOverflow { body_bytes })
            if body_bytes == u32::MAX as usize + 1
    ));
}

#[derive(Debug, Default)]
struct LengthMismatchedMessage;

impl Message for LengthMismatchedMessage {
    fn encode_raw(&self, buf: &mut impl BufMut) {
        buf.put_u8(0xaa);
        buf.put_u8(0xbb);
    }

    fn merge_field(
        &mut self,
        _tag: u32,
        _wire_type: WireType,
        _buf: &mut impl Buf,
        _ctx: DecodeContext,
    ) -> Result<(), DecodeError> {
        unreachable!("the egress test never decodes the synthetic message")
    }

    fn encoded_len(&self) -> usize { 1 }

    fn clear(&mut self) {}
}

#[test]
fn frame_rejects_a_length_mismatch_without_returning_partial_bytes() {
    assert!(matches!(
        encode_frame(&LengthMismatchedMessage),
        Err(ProtobufEgressError::EncodedLengthMismatch {
            expected: 1,
            actual: 2
        })
    ));
}

#[test]
fn aggregate_budget_charges_only_new_projection_retention() {
    let refused_value = integer(1);
    let mut zero = ProjectionPreparationBudget::new(0, usize::MAX);
    let projected_bytes = match zero.prepare(&refused_value).unwrap() {
        ParProtobufProjectionPreparation::TooLarge {
            projected_bytes,
            max_retained_bytes: 0,
        } => projected_bytes,
        outcome => panic!("zero cap unexpectedly produced {outcome:?}"),
    };
    assert!(projected_bytes > 0);
    assert_eq!(zero.remaining_new_retention_bytes(), 0);
    assert_eq!(zero.statistics().refused_too_large, 1);
    assert_eq!(zero.statistics().newly_retained_bytes, 0);

    let prepared_value = integer(2);
    let shared_clone = prepared_value.clone();
    let unshared_value = integer(3);
    let mut budget = ProjectionPreparationBudget::new(projected_bytes, projected_bytes);
    assert_eq!(
        budget.prepare(&prepared_value).unwrap(),
        ParProtobufProjectionPreparation::Prepared {
            retained_bytes: projected_bytes
        }
    );
    assert_eq!(budget.remaining_new_retention_bytes(), 0);

    assert_eq!(
        budget.prepare(&shared_clone).unwrap(),
        ParProtobufProjectionPreparation::AlreadyPrepared {
            retained_bytes: projected_bytes
        }
    );
    assert!(matches!(
        budget.prepare(&unshared_value).unwrap(),
        ParProtobufProjectionPreparation::TooLarge {
            max_retained_bytes: 0,
            ..
        }
    ));

    let statistics = budget.statistics();
    assert_eq!(statistics.attempts, 3);
    assert_eq!(statistics.prepared, 1);
    assert_eq!(statistics.already_prepared, 1);
    assert_eq!(statistics.refused_too_large, 1);
    assert_eq!(statistics.newly_retained_bytes, projected_bytes);
    assert_eq!(
        budget.initial_new_retention_bytes() - budget.remaining_new_retention_bytes(),
        statistics.newly_retained_bytes
    );
}

#[test]
fn per_value_cap_cannot_be_bypassed_by_a_larger_aggregate_budget() {
    let value = integer(4);
    let mut probe = ProjectionPreparationBudget::new(0, 0);
    let projected_bytes = match probe.prepare(&value).unwrap() {
        ParProtobufProjectionPreparation::TooLarge {
            projected_bytes, ..
        } => projected_bytes,
        outcome => panic!("zero cap unexpectedly produced {outcome:?}"),
    };
    let fresh = integer(5);
    let mut budget = ProjectionPreparationBudget::new(projected_bytes * 2, projected_bytes - 1);
    assert!(matches!(
        budget.prepare(&fresh).unwrap(),
        ParProtobufProjectionPreparation::TooLarge {
            projected_bytes: actual,
            max_retained_bytes: limit,
        } if actual == projected_bytes && limit == projected_bytes - 1
    ));
    assert_eq!(
        budget.remaining_new_retention_bytes(),
        budget.initial_new_retention_bytes()
    );
}

fn waiting(shared: &Par) -> WaitingContinuationProto {
    WaitingContinuationProto {
        patterns: vec![BindPattern {
            patterns: vec![shared.clone()],
            ..BindPattern::default()
        }],
        continuation: Some(TaggedContinuation {
            guard: Some(shared.clone()),
            ..TaggedContinuation::default()
        }),
        ..WaitingContinuationProto::default()
    }
}

fn datum(shared: &Par) -> DatumProto {
    DatumProto {
        a: Some(ListParWithRandom {
            pars: vec![shared.clone()],
            ..ListParWithRandom::default()
        }),
        ..DatumProto::default()
    }
}

#[test]
fn outer_rspace_visitors_cover_every_par_slot_and_preserve_cow_owners() {
    let shared = integer(23);
    let checkpoint = SoftCheckpointProto {
        cache_snapshot: Some(HotStoreStateProto {
            continuations: vec![StoreStateContMapEntry {
                key: vec![shared.clone()],
                value: vec![waiting(&shared)],
            }],
            installed_continuations: vec![StoreStateInstalledContMapEntry {
                key: vec![shared.clone()],
                value: Some(waiting(&shared)),
            }],
            data: vec![StoreStateDataMapEntry {
                key: Some(shared.clone()),
                value: vec![datum(&shared)],
            }],
            joins: vec![StoreStateJoinsMapEntry {
                key: Some(shared.clone()),
                value: vec![JoinProto {
                    join: vec![shared.clone()],
                }],
            }],
            installed_joins: vec![StoreStateInstalledJoinsMapEntry {
                key: Some(shared.clone()),
                value: vec![JoinProto {
                    join: vec![shared.clone()],
                }],
            }],
        }),
        ..SoftCheckpointProto::default()
    };

    let mut checkpoint_slots = 0usize;
    try_visit_soft_checkpoint_par_slots(&checkpoint, |value| {
        assert!(value.components().shares_storage_with(shared.components()));
        checkpoint_slots += 1;
        Ok::<_, std::convert::Infallible>(())
    })
    .unwrap();
    assert_eq!(checkpoint_slots, 12);

    let mut budget = ProjectionPreparationBudget::new(usize::MAX, usize::MAX);
    try_visit_soft_checkpoint_par_slots(&checkpoint, |value| budget.prepare(value).map(|_| ()))
        .unwrap();
    assert_eq!(budget.statistics().prepared, 1);
    assert_eq!(budget.statistics().already_prepared, 11);
    assert_eq!(
        budget.statistics().newly_retained_bytes,
        budget.initial_new_retention_bytes() - budget.remaining_new_retention_bytes()
    );

    let hot_changes = StoreToMapResult {
        map_entries: vec![StoreToMapEntry {
            key: vec![shared.clone()],
            value: Some(StoreToMapValue {
                data: vec![datum(&shared)],
                wks: vec![waiting(&shared)],
            }),
        }],
    };
    let mut hot_slots = 0usize;
    try_visit_hot_changes_par_slots(&hot_changes, |value| {
        assert!(value.components().shares_storage_with(shared.components()));
        hot_slots += 1;
        Ok::<_, std::convert::Infallible>(())
    })
    .unwrap();
    assert_eq!(hot_slots, 4);

    let consume = ConsumeResultReturn {
        tagged_cont: Some(TaggedContinuation {
            guard: Some(shared.clone()),
            ..TaggedContinuation::default()
        }),
        datums: vec![ListParWithRandom {
            pars: vec![shared.clone()],
            ..ListParWithRandom::default()
        }],
    };
    let mut consume_slots = 0usize;
    try_visit_consume_result_par_slots(&consume, |value| {
        assert!(value.components().shares_storage_with(shared.components()));
        consume_slots += 1;
        Ok::<_, std::convert::Infallible>(())
    })
    .unwrap();
    assert_eq!(consume_slots, 2);
}
