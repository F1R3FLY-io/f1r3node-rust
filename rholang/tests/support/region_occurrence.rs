use models::rust::rholang::match_schema_tables::RECEIVEBIND_RHO_MATCH_PROGRAM;

use super::{
    pair_receive_match_cuts, receive_cut_address, ConcreteCut, ConcreteParEquation,
    ConcreteRegionTask, ProtobufRegion, ProtobufRegionStep, ProtobufRegionStepKind,
    RECEIVE_RHO_MATCH_PROGRAM,
};

fn cut(bind: usize, occurrence: usize, byte: u8) -> ConcreteCut {
    ConcreteCut {
        path: vec![
            ProtobufRegionStep {
                owner_type_id: RECEIVE_RHO_MATCH_PROGRAM.type_id,
                field_tag: 1,
                occurrence: bind,
                kind: ProtobufRegionStepKind::Message,
            },
            ProtobufRegionStep {
                owner_type_id: RECEIVEBIND_RHO_MATCH_PROGRAM.type_id,
                field_tag: 1,
                occurrence,
                kind: ProtobufRegionStepKind::Message,
            },
        ],
        region: ProtobufRegion::from(vec![byte]),
        equation: ConcreteParEquation::MatchPars,
    }
}

#[test]
fn receive_cut_addresses_retain_full_native_ordinals() {
    for bind in [0, 1, u32::MAX as usize, usize::MAX] {
        for occurrence in [0, 1, u32::MAX as usize, usize::MAX] {
            let address = receive_cut_address(&cut(bind, occurrence, 7)).unwrap();
            assert_eq!(address.bind, bind);
            assert_eq!(address.occurrence, occurrence);
        }
    }
}

#[cfg(target_pointer_width = "64")]
#[test]
fn receive_pairing_does_not_alias_ordinals_at_u32_boundary() {
    let wide = u32::MAX as usize + 1;
    assert!(receive_cut_address(&cut(0, 0, 1)) < receive_cut_address(&cut(wide, 0, 2)));
    assert!(receive_cut_address(&cut(0, 0, 1)) < receive_cut_address(&cut(0, wide, 2)));
    for (target, pattern) in [
        (cut(0, 0, 1), cut(wide, 0, 2)),
        (cut(0, 0, 1), cut(0, wide, 2)),
    ] {
        assert!(pair_receive_match_cuts(vec![target], vec![pattern])
            .unwrap()
            .is_empty());
    }
    let paired = pair_receive_match_cuts(
        vec![cut(0, 0, 11), cut(wide, wide, 12), cut(wide + 1, 0, 13)],
        vec![cut(wide, wide, 21)],
    )
    .unwrap();
    assert_eq!(paired.len(), 1);
    match &paired[0] {
        ConcreteRegionTask::Par {
            target, pattern, ..
        } => {
            assert_eq!(target.as_bytes(), &[12]);
            assert_eq!(pattern.as_bytes(), &[21]);
        }
        _ => panic!("paired receive patterns must remain Par obligations"),
    }
}
