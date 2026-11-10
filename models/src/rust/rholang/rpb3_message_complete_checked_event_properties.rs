//! Generated Message completion events retain the pre-pop source and the
//! actual parent acceptance effect through the checked driver transition.

use proptest::prelude::*;

use super::*;
use crate::rhoapi::{Par, Send};
use crate::rust::rholang::protobuf_encoder;

fn checked_message_complete<'src>(
    before: &DriverStepSource<'src>,
    frames: &[Frame<'src>],
    scores: &CanonicalScoreArena<'src>,
    eof: &MessageEofSourceReceipt<'src>,
    outcome: &CompleteAdmissionOutcome<'src>,
    source: &DriverCompleteSource<'src>,
    effect: Option<&FrameAcceptEffect<'src>>,
    stats: &CanonicalProtobufAdmissionStats,
) -> bool {
    emit_checked_admission_event(CheckedAdmissionEvent {
        before,
        after_frames: frames,
        after_scores: scores,
        action: CheckedAdmissionAction::MessageComplete {
            eof,
            outcome,
            source,
            effect,
            stats,
        },
        map_source: None,
    })
    .is_ok()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn generated_send_root_completion_binds_source_score_summary_and_stats(
        source_id in 1u32..10_000,
        offset in 0usize..100_000,
    ) {
        let bytes = protobuf_encoder::encode_to_vec(&Send::default());
        let region = IndexedByteRegion::from_borrowed(&bytes, source_id, offset);
        let mut frames = Vec::new();
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            input_bytes: bytes.len(),
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            byte_native_validations: 1,
            ..CanonicalProtobufAdmissionStats::default()
        };
        push_admission_frame(
            &mut frames,
            FrameRequest::Message { region, program: &SEND_RHO_MATCH_PROGRAM },
            &mut stats,
        ).unwrap();
        let mut before = DriverStepSource::before(&frames, &scores);
        let FrameStep::MessageComplete { summary, mut eof } =
            frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap()
        else {
            panic!("generated empty Send must complete");
        };
        let (outcome, mut completion_source, effect) = complete_admission_step_with_source(
            &before, &mut frames, summary, Some(&eof), &scores, &mut stats, None,
        ).unwrap();
        prop_assert!(matches!(&outcome, CompleteAdmissionOutcome::Root(_)));
        prop_assert!(effect.is_none());
        prop_assert!(checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &stats,
        ));

        completion_source.completed_program = Some(&PAR_PROTOBUF_REGION_PROGRAM);
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &stats,
        ));
        completion_source.completed_program = Some(&SEND_RHO_MATCH_PROGRAM);
        completion_source.score_len_before += 1;
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &stats,
        ));
        completion_source.score_len_before -= 1;
        before.score_entries += 1;
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &stats,
        ));
        before.score_entries -= 1;

        let wrong_effect = FrameAcceptEffect::Stored;
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            Some(&wrong_effect), &stats,
        ));
        let actual_summary = std::mem::replace(&mut eof.summary_source, SemanticSummarySource::Unit);
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &stats,
        ));
        eof.summary_source = actual_summary;
        let wrong_outcome = CompleteAdmissionOutcome::Root(FrameSummary::Message(MessageSummary {
            semantic: SemanticSummary::Unit,
        }));
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &wrong_outcome, &completion_source,
            effect.as_ref(), &stats,
        ));
        let mut wrong_stats = stats;
        wrong_stats.canonical_path_resolutions += 1;
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &wrong_stats,
        ));
    }

    #[test]
    fn generated_send_channel_return_binds_accepted_child_source_effect_and_summary(
        source_id in 1u32..10_000,
        offset in 0usize..100_000,
        persistent in any::<bool>(),
    ) {
        let send = Send {
            chan: Some(Par::default()),
            persistent,
            ..Send::default()
        };
        let bytes = protobuf_encoder::encode_to_vec(&send);
        let region = IndexedByteRegion::from_borrowed(&bytes, source_id, offset);
        let mut frames = Vec::new();
        let mut scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            input_bytes: bytes.len(),
            frame_variant_bytes: std::mem::size_of::<Frame>(),
            byte_native_validations: 1,
            ..CanonicalProtobufAdmissionStats::default()
        };
        push_admission_frame(
            &mut frames,
            FrameRequest::Message { region, program: &SEND_RHO_MATCH_PROGRAM },
            &mut stats,
        ).unwrap();
        let FrameStep::MessagePush { request, .. } =
            frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap()
        else {
            panic!("generated Send channel must push a Par child");
        };
        push_admission_frame(&mut frames, request, &mut stats).unwrap();
        let before = DriverStepSource::before(&frames, &scores);
        let FrameStep::MessageComplete { summary, mut eof } =
            frames.last_mut().unwrap().step(&mut stats, &mut scores).unwrap()
        else {
            panic!("generated empty Par channel must complete");
        };
        let (outcome, mut completion_source, effect) = complete_admission_step_with_source(
            &before, &mut frames, summary, Some(&eof), &scores, &mut stats, None,
        ).unwrap();
        prop_assert!(matches!(&outcome, CompleteAdmissionOutcome::AcceptedMessage(_)));
        prop_assert!(matches!(&effect, Some(FrameAcceptEffect::Stored)));
        prop_assert!(checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &stats,
        ));

        completion_source.completed_program = Some(&SEND_RHO_MATCH_PROGRAM);
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &stats,
        ));
        completion_source.completed_program = Some(&PAR_PROTOBUF_REGION_PROGRAM);
        completion_source.score_len_after_finish += 1;
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &stats,
        ));
        completion_source.score_len_after_finish -= 1;
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            None, &stats,
        ));
        let actual_summary = std::mem::replace(&mut eof.summary_source, SemanticSummarySource::Unit);
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &stats,
        ));
        eof.summary_source = actual_summary;
        let mut wrong_stats = stats;
        wrong_stats.canonical_path_resolutions += 1;
        prop_assert!(!checked_message_complete(
            &before, &frames, &scores, &eof, &outcome, &completion_source,
            effect.as_ref(), &wrong_stats,
        ));
    }
}
