use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par, TaggedContinuation};
use models::rholang_scala_rust_types::{ConsumeResultReturn, EvaluateResultProto};
use models::rspace_plus_plus_types::{
    ChannelsProto, CheckpointProto, DatumProto, DatumsProto, EventProto, HotStoreStateProto,
    JoinProto, JoinsProto, SoftCheckpointProto, StoreStateContMapEntry, StoreStateDataMapEntry,
    StoreStateInstalledContMapEntry, StoreStateInstalledJoinsMapEntry, StoreStateJoinsMapEntry,
    StoreToMapEntry, StoreToMapResult, StoreToMapValue, WaitingContinuationProto,
    WaitingContinuationsProto,
};
use models::rust::par_bag::ParProtobufProjectionPreparation;
use models::rust::rholang::protobuf_schema::{
    CertifiedEgressEncodeError, CertifiedEgressError, CertifiedEgressNode,
    CertifiedEgressPreflight, CertifiedEgressReplay, ProtobufDescent, ProtobufField, ProtobufNode,
};
use prost::bytes::{Buf, BufMut};
use prost::encoding::{DecodeContext, WireType};
use prost::{DecodeError, Message};
use rholang::rust::protobuf_egress::{
    encode_frame, encode_frame_native, encode_frame_native_with_statistics,
    try_visit_consume_result_par_slots, try_visit_hot_changes_par_slots,
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

#[test]
fn native_frame_reuses_only_actual_cow_owners_and_preserves_root_metadata() {
    let shared = integer(17);
    let metadata_variant = shared
        .with_locally_free(vec![0x81, 0x01])
        .with_connective_used(true);
    assert!(
        shared
            .components()
            .shares_storage_with(metadata_variant.components()),
        "root metadata changes must retain the component owner"
    );
    let independently_equal = integer(17);
    assert!(!shared
        .components()
        .shares_storage_with(independently_equal.components()));

    let message = ChannelsProto {
        channels: vec![
            shared.clone(),
            metadata_variant.clone(),
            shared,
            independently_equal,
        ],
    };
    let expected = encode_frame(&message).unwrap();
    let (actual, statistics) = encode_frame_native_with_statistics(&message).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(ChannelsProto::decode(&actual[4..]).unwrap(), message);
    assert_eq!(statistics.census.unique_owners, 2);
    assert_eq!(statistics.census.repeated_owners, 1);
    assert_eq!(statistics.direct_only_occurrences, 1);
    assert_eq!(statistics.range_recorded_owners, 1);
    assert_eq!(statistics.range_copied_occurrences, 2);
    assert!(statistics.direct_only_component_bytes > 0);
    assert!(statistics.range_recorded_component_bytes > 0);
    assert_eq!(
        statistics.range_copied_component_bytes,
        statistics.range_recorded_component_bytes * 2
    );
}

#[test]
fn native_frame_matches_prost_for_complete_outer_rspace_graphs() {
    let shared = integer(29);
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
    assert_eq!(
        encode_frame_native(&checkpoint).unwrap(),
        encode_frame(&checkpoint).unwrap()
    );
}

fn assert_default_certified_root<T>()
where T: CertifiedEgressNode + Message + Default {
    let message = T::default();
    assert_eq!(
        encode_frame_native(&message).unwrap(),
        encode_frame(&message).unwrap()
    );
}

#[test]
fn every_production_publish_root_executes_through_the_certified_boundary() {
    assert_default_certified_root::<Par>();
    assert_default_certified_root::<EvaluateResultProto>();
    assert_default_certified_root::<SoftCheckpointProto>();
    assert_default_certified_root::<CheckpointProto>();
    assert_default_certified_root::<ConsumeResultReturn>();
    assert_default_certified_root::<DatumsProto>();
    assert_default_certified_root::<JoinsProto>();
    assert_default_certified_root::<WaitingContinuationsProto>();
    assert_default_certified_root::<StoreToMapResult>();
}

fn assert_owner_shape(frequencies: &[usize], empty_occurrences: usize) {
    let mut channels = vec![Par::default(); empty_occurrences];
    for (owner, frequency) in frequencies.iter().copied().enumerate() {
        let value = integer(owner as i64 + 1);
        channels.extend(std::iter::repeat(value).take(frequency));
    }
    let message = ChannelsProto { channels };
    let expected = encode_frame(&message).unwrap();
    let (actual, statistics) = encode_frame_native_with_statistics(&message).unwrap();
    assert_eq!(actual, expected);

    let eligible_occurrences: usize = frequencies.iter().sum();
    let unique_owners = frequencies.len();
    let repeated_owners = frequencies
        .iter()
        .filter(|frequency| **frequency > 1)
        .count();
    let repeated_occurrences: usize = frequencies
        .iter()
        .map(|frequency| frequency.saturating_sub(1))
        .sum();
    let direct_only_occurrences = empty_occurrences
        + frequencies
            .iter()
            .filter(|frequency| **frequency == 1)
            .count();

    assert_eq!(
        statistics.census.par_occurrences,
        eligible_occurrences + empty_occurrences
    );
    assert_eq!(
        statistics.census.component_empty_occurrences,
        empty_occurrences
    );
    assert_eq!(statistics.census.eligible_occurrences, eligible_occurrences);
    assert_eq!(statistics.census.unique_owners, unique_owners);
    assert_eq!(statistics.census.repeated_owners, repeated_owners);
    assert_eq!(statistics.census.repeated_occurrences, repeated_occurrences);
    assert_eq!(statistics.direct_only_occurrences, direct_only_occurrences);
    assert_eq!(statistics.range_recorded_owners, repeated_owners);
    assert_eq!(statistics.range_copied_occurrences, repeated_occurrences);
    assert_eq!(statistics.census_owner_spilled, unique_owners > 1);
    assert_eq!(
        statistics.transcript_events,
        eligible_occurrences + empty_occurrences
    );
    assert_eq!(
        statistics.transcript_spilled,
        eligible_occurrences + empty_occurrences > 16
    );
    assert_eq!(statistics.memo_owner_spilled, repeated_owners > 1);
    assert_eq!(
        statistics.direct_only_component_bytes + statistics.range_recorded_component_bytes,
        statistics.census.unique_component_bytes
    );
    assert_eq!(
        statistics.range_recorded_component_bytes,
        statistics.census.reusable_component_bytes
    );
    assert_eq!(
        statistics.range_copied_component_bytes,
        statistics.census.repeated_component_bytes
    );
}

#[test]
fn certified_owner_policy_covers_empty_singleton_skewed_and_dense_reuse_shapes() {
    assert_owner_shape(&[], 0); // N0
    assert_owner_shape(&[], 16); // component-empty only
    assert_owner_shape(&[1], 0); // N1/U1
    assert_owner_shape(&[2], 0); // N2/U1
    assert_owner_shape(&[1, 1], 0); // N2/U2
    assert_owner_shape(&[16], 0); // N16/U1
    assert_owner_shape(&[2; 8], 0); // N16/U8
    assert_owner_shape(&[2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1], 0); // N16/U15
    assert_owner_shape(&[8, 8], 0); // N16/U2 balanced
    assert_owner_shape(&[15, 1], 0); // N16/U2 skewed
    assert_owner_shape(&[1; 16], 0); // N16/U16
    assert_owner_shape(&[2, 2, 1, 1, 1, 1, 1, 1], 8); // mixed empty/repeated/singleton
    assert_owner_shape(&[256], 0); // N256/U1
    assert_owner_shape(&[16; 16], 0); // N256/U16
    assert_owner_shape(&[1; 256], 0); // N256/U256
}

#[derive(Clone, Copy)]
enum SyntheticCertifiedFault {
    Oversize,
    PartialStructuralError,
    ReplayRemaining,
    ReplayExhausted,
    BodyLengthMismatch,
}

struct SyntheticCertifiedNode {
    fault: SyntheticCertifiedFault,
    par: Par,
}

impl SyntheticCertifiedNode {
    fn new(fault: SyntheticCertifiedFault) -> Self {
        Self {
            fault,
            par: integer(97),
        }
    }
}

impl ProtobufNode for SyntheticCertifiedNode {
    fn protobuf_program(&self) -> &'static [ProtobufField] { &[] }

    fn protobuf_len_step(&self, _from: usize) -> (u64, ProtobufDescent<'_>) {
        (0, ProtobufDescent::Done)
    }

    fn protobuf_emit(&self, _from: usize, _out: &mut dyn BufMut) -> ProtobufDescent<'_> {
        ProtobufDescent::Done
    }
}

impl CertifiedEgressNode for SyntheticCertifiedNode {
    const MAX_SCHEMA_DEPTH: usize = 1;

    fn certified_egress_encoded_len(
        &self,
        preflight: &mut CertifiedEgressPreflight,
    ) -> Result<usize, CertifiedEgressError> {
        match self.fault {
            SyntheticCertifiedFault::Oversize => Ok(u32::MAX as usize + 1),
            SyntheticCertifiedFault::PartialStructuralError
            | SyntheticCertifiedFault::BodyLengthMismatch => Ok(1),
            SyntheticCertifiedFault::ReplayRemaining => preflight.par_encoded_len(&self.par),
            SyntheticCertifiedFault::ReplayExhausted => Ok(0),
        }
    }

    fn try_encode_certified_egress<B, E, F>(
        &self,
        output: &mut B,
        replay: &mut CertifiedEgressReplay<'_>,
        _emit_par: &mut F,
    ) -> Result<(), CertifiedEgressEncodeError<E>>
    where
        B: BufMut,
        F: FnMut(&Par, usize, &mut B) -> Result<(), E>,
    {
        match self.fault {
            SyntheticCertifiedFault::Oversize => {
                panic!("oversize input must be rejected before emission")
            }
            SyntheticCertifiedFault::PartialStructuralError => {
                output.put_u8(0xaa);
                Err(CertifiedEgressError::LengthOverflow {
                    context: "synthetic post-prefix failure",
                }
                .into())
            }
            SyntheticCertifiedFault::ReplayRemaining => Ok(()),
            SyntheticCertifiedFault::ReplayExhausted => {
                let _ = replay.next_par_encoded_len()?;
                Ok(())
            }
            SyntheticCertifiedFault::BodyLengthMismatch => {
                output.put_slice(&[0xaa, 0xbb]);
                Ok(())
            }
        }
    }
}

#[test]
fn certified_frame_never_returns_a_partial_or_incompletely_replayed_buffer() {
    assert!(matches!(
        encode_frame_native(&SyntheticCertifiedNode::new(SyntheticCertifiedFault::Oversize)),
        Err(ProtobufEgressError::BodyLengthOverflow { body_bytes })
            if body_bytes == u32::MAX as usize + 1
    ));
    assert!(matches!(
        encode_frame_native(&SyntheticCertifiedNode::new(
            SyntheticCertifiedFault::PartialStructuralError
        )),
        Err(ProtobufEgressError::Certified(
            CertifiedEgressError::LengthOverflow {
                context: "synthetic post-prefix failure"
            }
        ))
    ));
    assert!(matches!(
        encode_frame_native(&SyntheticCertifiedNode::new(
            SyntheticCertifiedFault::ReplayRemaining
        )),
        Err(ProtobufEgressError::Certified(
            CertifiedEgressError::TranscriptRemaining { remaining: 1 }
        ))
    ));
    assert!(matches!(
        encode_frame_native(&SyntheticCertifiedNode::new(
            SyntheticCertifiedFault::ReplayExhausted
        )),
        Err(ProtobufEgressError::Certified(
            CertifiedEgressError::TranscriptExhausted { .. }
        ))
    ));
    assert!(matches!(
        encode_frame_native(&SyntheticCertifiedNode::new(
            SyntheticCertifiedFault::BodyLengthMismatch
        )),
        Err(ProtobufEgressError::EncodedLengthMismatch {
            expected: 1,
            actual: 2
        })
    ));
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

fn assert_populated_certified_root<T>(message: T)
where T: CertifiedEgressNode + Message + Default + PartialEq + std::fmt::Debug {
    let expected = encode_frame(&message).unwrap();
    let actual = encode_frame_native(&message).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(T::decode(&actual[4..]).unwrap(), message);
}

#[test]
fn every_production_publish_root_matches_prost_for_populated_fixtures() {
    let shared = integer(101);
    assert_populated_certified_root(shared.clone());
    assert_populated_certified_root(EvaluateResultProto {
        errors: vec!["certified egress fixture".to_string()],
        mergeable: vec![shared.clone(), shared.clone()],
        ..EvaluateResultProto::default()
    });
    assert_populated_certified_root(SoftCheckpointProto {
        cache_snapshot: Some(HotStoreStateProto {
            continuations: vec![StoreStateContMapEntry {
                key: vec![shared.clone()],
                value: vec![waiting(&shared)],
            }],
            data: vec![StoreStateDataMapEntry {
                key: Some(shared.clone()),
                value: vec![datum(&shared)],
            }],
            ..HotStoreStateProto::default()
        }),
        ..SoftCheckpointProto::default()
    });
    assert_populated_certified_root(CheckpointProto {
        root: vec![0x41; 32],
        log: vec![EventProto::default()],
    });
    assert_populated_certified_root(ConsumeResultReturn {
        tagged_cont: Some(TaggedContinuation {
            guard: Some(shared.clone()),
            ..TaggedContinuation::default()
        }),
        datums: vec![ListParWithRandom {
            pars: vec![shared.clone()],
            ..ListParWithRandom::default()
        }],
    });
    assert_populated_certified_root(DatumsProto {
        datums: vec![datum(&shared)],
    });
    assert_populated_certified_root(JoinsProto {
        joins: vec![JoinProto {
            join: vec![shared.clone(), shared.clone()],
        }],
    });
    assert_populated_certified_root(WaitingContinuationsProto {
        wks: vec![waiting(&shared)],
    });
    assert_populated_certified_root(StoreToMapResult {
        map_entries: vec![StoreToMapEntry {
            key: vec![shared.clone()],
            value: Some(StoreToMapValue {
                data: vec![datum(&shared)],
                wks: vec![waiting(&shared)],
            }),
        }],
    });
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
