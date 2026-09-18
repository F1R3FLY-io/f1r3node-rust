use super::*;
use proptest::prelude::*;
use models::rust::rholang::protobuf_encoder;

#[derive(Debug, PartialEq, Eq)]
enum ObservedJob {
    Substitute(usize, SubCtx),
    Preserve(usize),
    Missing(SubCtx),
    Empty(PlannedPar),
}

fn retained_plan(count: usize) -> ProtobufCutPlan {
    let data = (0..count).map(|index| {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(index as i64 + 1)),
        }])
    }).collect();
    let send = Send { data, ..Send::default() };
    cut_at_par(
        ProtobufRegion::from(protobuf_encoder::encode_to_vec(&send)),
        &SEND_RHO_MATCH_PROGRAM,
    ).unwrap()
}

fn region_index(plan: &ProtobufCutPlan, region: &ProtobufRegion) -> usize {
    let index = plan.cuts().iter().position(|cut| {
        cut.region().absolute_range() == region.absolute_range()
    }).expect("job references a retained cut");
    let original = plan.cuts()[index].region();
    assert_eq!(original.as_bytes(), region.as_bytes());
    assert_eq!(original.as_bytes().as_ptr(), region.as_bytes().as_ptr());
    index
}

fn verify_selection(requests: &[u8], preserve: bool) {
    let consumed = requests.iter().filter(|&&request| request < 3).count();
    let plan = retained_plan(consumed);
    assert_eq!(plan.cuts().len(), consumed);
    let mut cursor = RegionCutCursor::new(&plan);
    let mut construction = ParConstruction::new();
    let empty = construction.empty_par().unwrap();
    let mut children = Vec::new();
    let mut expected = Vec::new();
    let mut ordinal = 0;
    for (index, &request) in requests.iter().enumerate() {
        let ctx = SubCtx { depth: index as i32, shift_delta: index as i32 + 2 };
        match request {
            0 => {
                children.push(cursor.required_mode(true, ctx, preserve).unwrap());
                expected.push(if preserve { ObservedJob::Preserve(ordinal) }
                    else { ObservedJob::Substitute(ordinal, ctx) });
                ordinal += 1;
            }
            1 => {
                children.push(cursor.preserved().unwrap());
                expected.push(ObservedJob::Preserve(ordinal));
                ordinal += 1;
            }
            2 => {
                assert_eq!(region_index(&plan, &cursor.take().unwrap()), ordinal);
                ordinal += 1;
            }
            3 => {
                children.push(cursor.required_mode(false, ctx, preserve).unwrap());
                expected.push(ObservedJob::Missing(ctx));
            }
            4 => {
                children.push(cursor.optional_mode(false, ctx, empty, preserve).unwrap());
                expected.push(ObservedJob::Empty(empty));
            }
            _ => unreachable!(),
        }
        assert_eq!(cursor.at, ordinal);
    }
    assert_eq!(ordinal, consumed);
    assert!(cursor.take().is_err());
    assert_eq!(cursor.at, consumed);
    cursor.finish().unwrap();
    let mut work = vec![SubWork::PlannedPar(empty)];
    schedule_region_component(
        RegionComponentShell::Send(Send::default()), Multiplicity::ONE,
        children, Vec::new(), preserve, &mut work,
    );
    let job_count = expected.len();
    for expected in expected {
        let actual = match work.pop().unwrap() {
            SubWork::ParRegion(region, ctx) =>
                ObservedJob::Substitute(region_index(&plan, &region), ctx),
            SubWork::ImportParRegion(region) => ObservedJob::Preserve(region_index(&plan, &region)),
            SubWork::Par(None, ctx) => ObservedJob::Missing(ctx),
            SubWork::PlannedPar(value) => ObservedJob::Empty(value),
            _ => panic!("continuation or unrelated work ran before the selected child"),
        };
        assert_eq!(actual, expected);
    }
    let Some(SubWork::Combine(SubKont::RegionComponentK(kont))) = work.pop() else {
        panic!("selected jobs must be followed by exactly one component continuation");
    };
    assert_eq!(kont.child_count, job_count);
    assert_eq!(kont.multiplicity, Multiplicity::ONE);
    assert!(kont.wire_order.is_empty());
    assert_eq!(kont.preserve, preserve);
    assert!(matches!(work.pop(), Some(SubWork::PlannedPar(value)) if value == empty));
    assert!(work.is_empty());
}

#[test]
fn dropped_and_synthetic_slots_are_not_a_permutation_of_retained_cuts() {
    verify_selection(&[2, 4, 1], false);
    verify_selection(&[3, 0, 4, 2, 1], false);
    verify_selection(&[3, 0, 4, 2, 1], true);
    verify_selection(&[], false);
}

#[test]
fn region_cut_cursor_rejects_unconsumed_and_exhausted_plans() {
    let plan = retained_plan(2);
    let mut cursor = RegionCutCursor::new(&plan);
    cursor.take().unwrap();
    assert!(cursor.finish().is_err());
    let plan = retained_plan(0);
    let mut cursor = RegionCutCursor::new(&plan);
    assert!(cursor.take().is_err());
    assert_eq!(cursor.at, 0);
    cursor.finish().unwrap();
}

proptest! {
    #[test]
    fn generated_slot_selection_preserves_prefix_identity_and_semantic_order(
        requests in prop::collection::vec(0u8..5, 0..48),
        preserve in any::<bool>(),
    ) {
        verify_selection(&requests, preserve);
    }
}

