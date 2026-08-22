mod par_corpus;

use models::rhoapi::{Bundle, Connective, Expr, GUnforgeable, If, Match, New, Par, Receive, Send};
use models::rust::par_bag::ParBagComponent;
use models::rust::rholang::match_schema::RhoMatchProgram;
use models::rust::rholang::match_schema_tables::{
    BUNDLE_RHO_MATCH_PROGRAM, CONNECTIVE_RHO_MATCH_PROGRAM, EXPR_RHO_MATCH_PROGRAM,
    GUNFORGEABLE_RHO_MATCH_PROGRAM, IF_RHO_MATCH_PROGRAM, MATCH_RHO_MATCH_PROGRAM,
    NEW_RHO_MATCH_PROGRAM, PAR_PROTOBUF_REGION_PROGRAM, RECEIVE_RHO_MATCH_PROGRAM,
    SEND_RHO_MATCH_PROGRAM,
};
use models::rust::rholang::protobuf_cursor::ProtobufRegion;
use models::rust::rholang::protobuf_encoder;
use models::rust::rholang::protobuf_region::{cut_at_par, ProtobufRegionStepKind};

fn verify_family<T: ParBagComponent>(
    par: &Par,
    program: &'static RhoMatchProgram,
    decode: impl Fn(&[u8]) -> Result<T, prost::DecodeError>,
) {
    par.visit_encoded::<T>(|entry| {
        let input = ProtobufRegion::copy_from_slice(entry.payload());
        let plan = cut_at_par(input, program).expect("canonical component region cuts");
        assert_eq!(plan.reconstructed_bytes().as_ref(), entry.payload());
        decode(plan.shallow_bytes().as_ref()).expect("the generated decoder accepts the shell");
        let stats = plan.stats();
        assert!(stats.message_nodes <= stats.field_visits + 1);
        assert!(stats.field_visits <= entry.payload().len());
        assert_eq!(stats.par_cuts, plan.cuts().len());
        for index in 0..plan.cuts().len() {
            let path = plan.cut_path(index).expect("every retained cut has a path");
            assert!(!path.is_empty());
        }
    });
}

fn verify_all_families(par: &Par) {
    use models::rust::rholang::protobuf_decoder;

    verify_family::<Send>(par, &SEND_RHO_MATCH_PROGRAM, |bytes| {
        protobuf_decoder::decode_send(bytes)
    });
    verify_family::<Receive>(par, &RECEIVE_RHO_MATCH_PROGRAM, |bytes| {
        protobuf_decoder::decode_receive(bytes)
    });
    verify_family::<New>(par, &NEW_RHO_MATCH_PROGRAM, |bytes| {
        protobuf_decoder::decode_new(bytes)
    });
    verify_family::<Expr>(par, &EXPR_RHO_MATCH_PROGRAM, |bytes| {
        protobuf_decoder::decode_expr(bytes)
    });
    verify_family::<Match>(par, &MATCH_RHO_MATCH_PROGRAM, |bytes| {
        protobuf_decoder::decode_match(bytes)
    });
    verify_family::<GUnforgeable>(par, &GUNFORGEABLE_RHO_MATCH_PROGRAM, |bytes| {
        protobuf_decoder::decode_g_unforgeable(bytes)
    });
    verify_family::<Connective>(par, &CONNECTIVE_RHO_MATCH_PROGRAM, |bytes| {
        protobuf_decoder::decode_connective(bytes)
    });
    verify_family::<Bundle>(par, &BUNDLE_RHO_MATCH_PROGRAM, |bytes| {
        protobuf_decoder::decode_bundle(bytes)
    });
    verify_family::<If>(par, &IF_RHO_MATCH_PROGRAM, |bytes| {
        protobuf_decoder::decode_if(bytes)
    });
}

#[test]
fn every_component_family_reconstructs_exactly_and_decodes_shallowly() {
    verify_all_families(&par_corpus::all_par_fields());
    for (_, par) in par_corpus::par_corpus() {
        verify_all_families(&par);
    }
}

#[test]
fn raw_par_program_reconstructs_the_complete_external_wire_view() {
    for (_, par) in par_corpus::par_corpus() {
        let bytes = protobuf_encoder::encode_to_vec(&par);
        let plan = cut_at_par(
            ProtobufRegion::copy_from_slice(&bytes),
            &PAR_PROTOBUF_REGION_PROGRAM,
        )
        .expect("canonical raw Par region cuts");
        assert_eq!(plan.reconstructed_bytes().as_ref(), bytes);
        models::rust::rholang::protobuf_decoder::decode_par(plan.shallow_bytes())
            .expect("the generated decoder accepts the raw Par shell");
    }
}

#[test]
fn present_empty_par_fields_remain_distinct_from_absence() {
    let absent = Send::default();
    let present = Send {
        chan: Some(Par::default()),
        data: vec![Par::default()],
        ..Send::default()
    };
    let absent_bytes = protobuf_encoder::encode_to_vec(&absent);
    let present_bytes = protobuf_encoder::encode_to_vec(&present);
    let absent_plan = cut_at_par(
        ProtobufRegion::from(absent_bytes.clone()),
        &SEND_RHO_MATCH_PROGRAM,
    )
    .expect("absent Par fields cut");
    let present_plan = cut_at_par(
        ProtobufRegion::from(present_bytes.clone()),
        &SEND_RHO_MATCH_PROGRAM,
    )
    .expect("present-empty Par fields cut");
    assert!(absent_plan.cuts().is_empty());
    assert_eq!(present_plan.cuts().len(), 2);
    assert!(present_plan
        .cuts()
        .iter()
        .all(|cut| cut.region().is_empty()));
    assert_eq!(absent_plan.reconstructed_bytes().as_ref(), absent_bytes);
    assert_eq!(present_plan.reconstructed_bytes().as_ref(), present_bytes);

    let shell = models::rust::rholang::protobuf_decoder::decode_send(present_plan.shallow_bytes())
        .expect("present-empty shell decodes");
    assert_eq!(shell.chan, Some(Par::default()));
    assert_eq!(shell.data, vec![Par::default()]);
}

#[test]
fn epathmap_values_are_cut_without_materializing_keys_or_pars() {
    let map = models::rust::rhoapi_ext::EPathMap::new_map(
        [
            (par_corpus::gint(1), par_corpus::gint(11)),
            (par_corpus::gint(2), par_corpus::gint(22)),
        ],
        Vec::new(),
        false,
        None,
    );
    let par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::EPathmapBody(map)),
    }]);
    let mut saw_values = false;
    par.visit_encoded::<Expr>(|entry| {
        let plan = cut_at_par(
            ProtobufRegion::copy_from_slice(entry.payload()),
            &EXPR_RHO_MATCH_PROGRAM,
        )
        .expect("canonical EPathMap expression cuts");
        assert_eq!(plan.reconstructed_bytes().as_ref(), entry.payload());
        assert_eq!(plan.epathmaps().len(), 1);
        let group = &plan.epathmaps()[0];
        assert_eq!(
            group.mode(),
            models::rust::epathmap_trie_codec::EPathMapMode::Map
        );
        assert_eq!(group.cut_range().len(), 2);
        let group_path = plan.epathmap_path(0).expect("EPathMap group path");
        assert_eq!(
            group_path.last().map(|step| step.kind),
            Some(ProtobufRegionStepKind::EPathMapSnapshot)
        );
        for index in 0..plan.cuts().len() {
            let path = plan.cut_path(index).expect("EPathMap value cut path");
            if path
                .iter()
                .any(|step| step.kind == ProtobufRegionStepKind::EPathMapValue)
            {
                saw_values = true;
            }
        }
        models::rust::rholang::protobuf_decoder::decode_expr(plan.shallow_bytes())
            .expect("the generated decoder accepts the topology-preserving shell");
    });
    assert!(saw_values, "map-mode EPathMap must expose value regions");
}

#[test]
fn depth_4096_walk_reuses_one_backing_and_keeps_machine_depth_bounded() {
    std::thread::Builder::new()
        .name("protobuf-region-depth".to_string())
        .stack_size(256 * 1024)
        .spawn(|| {
            let deep = par_corpus::deep_par(4_096);
            let bytes = protobuf_encoder::encode_to_vec(&deep);
            let backing_start = bytes.as_ptr() as usize;
            let backing_end = backing_start + bytes.len();
            let mut current = ProtobufRegion::from(bytes);
            let mut levels = 0usize;
            loop {
                let plan = cut_at_par(current, &PAR_PROTOBUF_REGION_PROGRAM)
                    .expect("each nested canonical Par cuts");
                assert!(plan.stats().max_frames < 32);
                if levels == 0 {
                    let cloned = plan.clone();
                    assert_eq!(cloned.shallow_bytes(), plan.shallow_bytes());
                    assert_eq!(cloned.reconstructed_bytes(), plan.reconstructed_bytes());
                }
                let Some(cut) = plan.cuts().first() else {
                    break;
                };
                let next = cut.region().clone();
                let next_start = next.as_bytes().as_ptr() as usize;
                assert!(next_start >= backing_start);
                assert!(next_start + next.len() <= backing_end);
                current = next;
                levels += 1;
            }
            assert_eq!(levels, 4_096);
        })
        .expect("spawn small-stack protobuf-region worker")
        .join()
        .expect("protobuf-region worker completes");
}
