use verified_score_store::admission_event_checker::AdmissionMonitor;

use super::*;

const MONITOR_INITIAL_FRAME_CAPACITY: usize = 16;

pub(super) struct AdmissionLiveCore<'src> {
    frames: Vec<Frame<'src>>,
    scores: CanonicalScoreArena<'src>,
    stats: CanonicalProtobufAdmissionStats,
    scratch: Box<AdmissionScratch<'src>>,
    monitor: Box<AdmissionMonitor>,
    iterations: u64,
}

struct MessagePreStepTicket<'src> {
    program: &'static RhoMatchProgram,
    body: AdmissionRegion<'src>,
    region_start: CanonicalByteProvenance,
    cursor: IndexedCanonicalFieldCursorStats,
    cursor_source: Option<IndexedMessagePreReadSource>,
    last_slot: Option<usize>,
    stack_len: usize,
    score_entries: usize,
    protobuf_bytes_read: usize,
    protobuf_bytes_traversed: usize,
}

struct MapPreStepTicket<'src> {
    source: MapEofObservation<'src>,
    waiting_value: bool,
}

struct EPathMapPreStepTicket<'src> {
    snapshot: AdmissionRegion<'src>,
    source: EPathMapLoopStateSource,
}

enum ActivePreStepTicket<'src> {
    Message(MessagePreStepTicket<'src>),
    Map(MapPreStepTicket<'src>),
    EPathMap(EPathMapPreStepTicket<'src>),
    CanonicalPath,
}

pub(super) struct PreStepTicket<'src> {
    active: ActivePreStepTicket<'src>,
    stack_len: usize,
    score_checkpoint: CanonicalScoreAppendCheckpoint,
}

impl<'src> PreStepTicket<'src> {
    pub(super) fn before(
        frames: &[Frame<'src>],
        scores: &CanonicalScoreArena<'src>,
        stats: &CanonicalProtobufAdmissionStats,
    ) -> Result<Self, CanonicalProtobufAdmissionError> {
        let score_checkpoint = scores.append_checkpoint();
        let active = match frames
            .last()
            .expect("canonical protobuf admission retains its root frame")
        {
            Frame::Message(frame) => ActivePreStepTicket::Message(MessagePreStepTicket {
                program: frame.program,
                body: frame.body.clone(),
                region_start: frame.region_start,
                cursor: frame.cursor.stats(),
                cursor_source: frame.cursor.message_pre_read_source(&frame.body),
                last_slot: frame.slots.last_slot(),
                stack_len: frames.len(),
                score_entries: scores.source_len(),
                protobuf_bytes_read: stats.protobuf_bytes_read,
                protobuf_bytes_traversed: stats.protobuf_bytes_traversed,
            }),
            Frame::Map(frame) => ActivePreStepTicket::Map(MapPreStepTicket {
                source: MapEofObservation::from_frame(frame),
                waiting_value: frame.waiting_value,
            }),
            Frame::EPathMap(frame) => ActivePreStepTicket::EPathMap(EPathMapPreStepTicket {
                snapshot: frame.snapshot.clone(),
                source: EPathMapLoopStateSource::capture(frame)?,
            }),
            Frame::CanonicalPath(_) => ActivePreStepTicket::CanonicalPath,
        };
        Ok(Self {
            active,
            stack_len: frames.len(),
            score_checkpoint,
        })
    }

    pub(super) fn matches_field_preimage(&self, image: &MessageFieldPreStepImage<'src>) -> bool {
        let ActivePreStepTicket::Message(before) = &self.active else {
            return false;
        };
        image
            .program
            .is_some_and(|program| std::ptr::eq(program, before.program))
            && image
                .body
                .as_ref()
                .is_some_and(|body| body.same_view(&before.body))
            && image.cursor == before.cursor
            && before.cursor_source.is_some()
            && image.cursor_source == before.cursor_source
            && image.last_slot == before.last_slot
            && image.seen.len() == before.program.fields.len()
            && image.slot_lengths.len() == before.program.fields.len()
            && before.stack_len == self.stack_len
    }

    fn matches_message_complete(
        &self,
        scratch: &AdmissionScratch<'src>,
        scores: &CanonicalScoreArena<'src>,
        stats: &CanonicalProtobufAdmissionStats,
    ) -> bool {
        let (ActivePreStepTicket::Message(before), Some(eof), Some(source)) = (
            &self.active,
            scratch.eof.as_ref(),
            scratch.complete_source.as_ref(),
        ) else {
            return false;
        };
        let driver = scratch.driver_source();
        std::ptr::eq(before.program, eof.program)
            && driver
                .top_program
                .is_some_and(|program| std::ptr::eq(program, before.program))
            && before.body.same_view(&eof.body)
            && before.region_start == eof.region_start
            && before.cursor == eof.cursor_after
            && before.stack_len == driver.stack_len
            && before.stack_len == source.stack_len_before
            && before.score_entries == driver.score_entries
            && before.score_entries == self.score_checkpoint.source_len()
            && before.score_entries == eof.score_len_before
            && before.score_entries == source.score_len_before
            && eof.score_len_after == scores.source_len()
            && source.score_len_after_finish == scores.source_len()
            && scores.certified_append_since(&self.score_checkpoint)
            && before
                .protobuf_bytes_read
                .checked_add(eof.cursor_after.bytes_read)
                == Some(stats.protobuf_bytes_read)
            && before
                .protobuf_bytes_traversed
                .checked_add(eof.cursor_after.bytes_traversed)
                == Some(stats.protobuf_bytes_traversed)
    }

    pub(super) fn matches_checked_event(
        &self,
        event: &CheckedAdmissionEvent<'_, 'src>,
        epm_step_start: Option<&EPathMapLoopStateSource>,
    ) -> bool {
        if self.stack_len != event.before.stack_len
            || self.score_checkpoint.source_len() != event.before.score_entries
            || !event
                .after_scores
                .certified_append_since(&self.score_checkpoint)
            || match &self.active {
                ActivePreStepTicket::Message(before) => !event
                    .before
                    .top_program
                    .is_some_and(|program| std::ptr::eq(program, before.program)),
                _ => event.before.top_program.is_some(),
            }
        {
            return false;
        }
        let complete_shape = match &event.action {
            CheckedAdmissionAction::MessageComplete { outcome, .. }
            | CheckedAdmissionAction::OtherComplete { outcome, .. } => match outcome {
                CompleteAdmissionOutcome::Root(summary) => {
                    event.after_frames.is_empty() && summary.request_kind() == event.before.top_kind
                }
                CompleteAdmissionOutcome::AcceptedMessage(receipt) => {
                    matches!(event.after_frames.last(), Some(Frame::Message(_)))
                        && receipt.before.child_kind == event.before.top_kind
                        && matches!(
                            (event.before.top_kind, &receipt.before.returned),
                            (FrameRequestKind::Message, FrameSummarySource::Message(_))
                                | (FrameRequestKind::Map, FrameSummarySource::Map(_))
                                | (FrameRequestKind::EPathMap, FrameSummarySource::EPathMap(_))
                        )
                }
                CompleteAdmissionOutcome::AcceptedOther => matches!(
                    event.after_frames.last(),
                    Some(Frame::Map(_) | Frame::EPathMap(_) | Frame::CanonicalPath(_))
                ),
            },
            _ => true,
        };
        complete_shape
            && match (&self.active, &event.action) {
                (
                    ActivePreStepTicket::Message(before),
                    CheckedAdmissionAction::MessageField { field, .. },
                ) => {
                    std::ptr::eq(before.program, field.program)
                        && before.body.same_view(&field.body)
                        && before.region_start == field.region_start
                        && before.cursor == field.cursor_before
                        && before.last_slot == field.last_slot_before
                        && event.before.top_kind == FrameRequestKind::Message
                        && event.map_source.is_none()
                }
                (
                    ActivePreStepTicket::Message(before),
                    CheckedAdmissionAction::MessageComplete { eof, .. },
                ) => {
                    std::ptr::eq(before.program, eof.program)
                        && before.body.same_view(&eof.body)
                        && before.region_start == eof.region_start
                        && before.cursor == eof.cursor_after
                        && event.before.top_kind == FrameRequestKind::Message
                        && event.map_source.is_none()
                }
                (ActivePreStepTicket::Map(before), CheckedAdmissionAction::OtherContinue)
                | (ActivePreStepTicket::Map(before), CheckedAdmissionAction::OtherPush { .. }) => {
                    let Some(MapStepSource::Field(field)) = event.map_source else {
                        return false;
                    };
                    std::ptr::eq(before.source.owner, field.owner)
                        && before.source.outer_field == field.outer_field
                        && before.source.value_type_id == field.value_type_id
                        && before.source.body.same_view(&field.body)
                        && before.source.region_start == field.region_start
                        && before.source.cursor_after == field.cursor_before
                        && before.source.key_present == field.key_before
                        && before.source.key_position == field.key_position_before
                        && before.source.saw_value == field.saw_value_before
                        && before.source.value_position == field.value_position_before
                        && before.source.summary.value.is_none() == field.value_absent_before
                        && before.waiting_value == field.waiting_before
                        && event.before.top_kind == FrameRequestKind::Map
                        && self.stack_len.checked_sub(1).and_then(|index| event.after_frames.get(index)).is_some_and(|parent| {
                            matches!(parent, Frame::Map(frame)
                                if !before.source.key_present || frame.key.as_ref().is_some_and(|key|
                                    before.source.summary.key.same_view(key)))
                        })
                }
                (
                    ActivePreStepTicket::Map(before),
                    CheckedAdmissionAction::OtherComplete { .. },
                ) => {
                    matches!(event.map_source, Some(MapStepSource::Eof(eof))
                    if before.source.matches_source(eof))
                        && !before.waiting_value
                        && event.before.top_kind == FrameRequestKind::Map
                }
                (
                    ActivePreStepTicket::EPathMap(before),
                    CheckedAdmissionAction::OtherPush { .. },
                ) => {
                    epm_step_start.is_some_and(|start| before.source.matches_source(start))
                        && event.before.top_kind == FrameRequestKind::EPathMap
                        && event.map_source.is_none()
                        && event.before.score_entries == event.after_scores.source_len()
                        && self
                            .stack_len
                            .checked_sub(1)
                            .and_then(|index| event.after_frames.get(index))
                            .is_some_and(|parent| {
                                matches!(parent, Frame::EPathMap(frame)
                                if before.snapshot.same_view(&frame.snapshot))
                            })
                }
                (
                    ActivePreStepTicket::EPathMap(before),
                    CheckedAdmissionAction::OtherComplete { source, .. },
                ) => {
                    epm_step_start.is_some_and(|start| before.source.matches_source(start))
                        && event.before.top_kind == FrameRequestKind::EPathMap
                        && event.map_source.is_none()
                        && event.before.score_entries == event.after_scores.source_len()
                        && matches!(&source.terminal,
                            Some(terminal) if matches!(terminal.as_ref(),
                                CompleteTerminalSource::EPathMap { snapshot, .. }
                                    if before.snapshot.same_view(snapshot)))
                }
                (
                    ActivePreStepTicket::CanonicalPath,
                    CheckedAdmissionAction::OtherPush { .. }
                    | CheckedAdmissionAction::OtherComplete { .. },
                ) => {
                    event.before.top_kind == FrameRequestKind::CanonicalPath
                        && event.map_source.is_none()
                        && event.before.score_entries == event.after_scores.source_len()
                }
                _ => false,
            }
    }
}

impl<'src> AdmissionLiveCore<'src> {
    pub(super) fn new(
        initial: FrameRequest<'src>,
        input_bytes: usize,
    ) -> Result<Self, CanonicalProtobufAdmissionError> {
        let frame_variant_bytes = std::mem::size_of::<Frame>();
        let scores = CanonicalScoreArena::new();
        let mut stats = CanonicalProtobufAdmissionStats {
            input_bytes,
            frame_variant_bytes,
            byte_native_validations: 1,
            ..CanonicalProtobufAdmissionStats::default()
        };
        let mut frames = Vec::with_capacity(1);
        let root_source = RootInitializationSource::before_push(&initial, &scores).ok_or(
            CanonicalProtobufAdmissionError::CutoverInvariant(
                "admission root request is not a message or canonical path",
            ),
        )?;
        let root_pushed = push_admission_frame(&mut frames, initial, &mut stats)?;
        if root_pushed.before_len != 0
            || root_pushed.after_len != frames.len()
            || root_pushed.after_capacity != frames.capacity()
            || root_pushed.frame_variant_bytes != stats.frame_variant_bytes
            || !root_source.matches_root(&root_pushed, &frames, &scores)
        {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "admission root disagrees with its source or checked storage observation",
            ));
        }
        let root_request = frames.first().map(observed_frame_request).ok_or(
            CanonicalProtobufAdmissionError::CutoverInvariant(
                "admission root frame has no observable request",
            ),
        )?;
        let monitor = AdmissionMonitor::new(
            &root_request,
            scores.source_len(),
            usize::MAX,
            MONITOR_INITIAL_FRAME_CAPACITY,
        )
        .map_err(
            |refusal| CanonicalProtobufAdmissionError::VerifiedMonitorRefused { step: 0, refusal },
        )?;
        Ok(Self {
            frames,
            scores,
            stats,
            scratch: AdmissionScratch::allocate(),
            monitor: Box::new(monitor),
            iterations: 0,
        })
    }

    fn check_observed_event(
        &mut self,
        observed: ObservedEvent,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        let step = self.monitor.checked_events();
        #[cfg(test)]
        let Some(observed) = verified_monitor_seam::seamed_event(step, observed) else {
            return Ok(());
        };
        self.monitor.check_event(&observed).map_err(|refusal| {
            CanonicalProtobufAdmissionError::VerifiedMonitorRefused { step, refusal }
        })
    }

    #[cfg_attr(debug_assertions, inline(never))]
    fn begin_iteration(&mut self) -> Result<PreStepTicket<'src>, CanonicalProtobufAdmissionError> {
        self.iterations = self
            .iterations
            .checked_add(1)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        PreStepTicket::before(&self.frames, &self.scores, &self.stats)
    }

    #[cfg_attr(debug_assertions, inline(never))]
    fn check_noncompletion_event(
        &mut self,
        kind: AdmissionStepKind,
        ticket: &PreStepTicket<'src>,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        let observed = emit_admission_noncompletion_event(
            kind,
            &self.scratch,
            &self.frames,
            &self.scores,
            ticket,
        )?;
        self.check_observed_event(observed)
    }

    #[cfg_attr(debug_assertions, inline(never))]
    fn check_completion_event(
        &mut self,
        kind: AdmissionStepKind,
        ticket: &PreStepTicket<'src>,
        root: bool,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        let observed = emit_admission_completion_event(
            kind,
            &self.scratch,
            &self.frames,
            &self.scores,
            &self.stats,
            ticket,
        )?;
        self.check_observed_event(observed)?;
        if root {
            let checked_events = self.monitor.checked_events();
            if !self.monitor.is_accepted() || checked_events != self.iterations {
                return Err(CanonicalProtobufAdmissionError::VerifiedMonitorIncomplete {
                    checked_events,
                });
            }
            #[cfg(test)]
            verified_monitor_seam::record_monitor_finish(
                checked_events,
                self.iterations,
                self.monitor.is_accepted(),
            );
        }
        Ok(())
    }

    fn finish_message_complete(
        &mut self,
        ticket: PreStepTicket<'src>,
    ) -> Result<Option<FrameSummary<'src>>, CanonicalProtobufAdmissionError> {
        let root = process_admission_completion(
            AdmissionStepKind::MessageComplete,
            &mut self.scratch,
            &mut self.frames,
            &self.scores,
            &mut self.stats,
        )?;
        if !ticket.matches_message_complete(&self.scratch, &self.scores, &self.stats) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "message completion disagrees with its private pre-step ticket",
            ));
        }
        self.check_completion_event(AdmissionStepKind::MessageComplete, &ticket, root)?;
        Ok(if root {
            let Some(CompleteAdmissionOutcome::Root(summary)) = self.scratch.outcome.take() else {
                unreachable!("checked root completion has a root summary")
            };
            Some(summary)
        } else {
            None
        })
    }

    fn finish_other_complete(
        &mut self,
        ticket: PreStepTicket<'src>,
    ) -> Result<Option<FrameSummary<'src>>, CanonicalProtobufAdmissionError> {
        let root = process_admission_completion(
            AdmissionStepKind::OtherComplete,
            &mut self.scratch,
            &mut self.frames,
            &self.scores,
            &mut self.stats,
        )?;
        self.check_completion_event(AdmissionStepKind::OtherComplete, &ticket, root)?;
        Ok(if root {
            let Some(CompleteAdmissionOutcome::Root(summary)) = self.scratch.outcome.take() else {
                unreachable!("checked root completion has a root summary")
            };
            Some(summary)
        } else {
            None
        })
    }

    pub(super) fn run(
        mut self,
    ) -> Result<
        (
            FrameSummary<'src>,
            CanonicalScoreArena<'src>,
            CanonicalProtobufAdmissionStats,
        ),
        CanonicalProtobufAdmissionError,
    > {
        loop {
            self.scratch.clear();
            #[cfg(test)]
            rpb3_root_finish_trace::set_root_step(self.frames.len() == 1);
            #[cfg(test)]
            {
                self.scratch.step_before =
                    rpb3_root_finish_trace::driver_step_before(&self.frames, &self.scores);
            }
            let ticket = self.begin_iteration()?;
            self.scratch.field_preimage.capture(
                self.frames
                    .last()
                    .expect("canonical protobuf admission retains its root frame"),
            );
            self.scratch.driver_source = Some(DriverStepSource::before(&self.frames, &self.scores));
            let kind = self
                .frames
                .last_mut()
                .expect("canonical protobuf admission retains its root frame")
                .step_into(&mut self.stats, &mut self.scores, &mut self.scratch)?;
            let root = match kind {
                AdmissionStepKind::MessageContinue
                | AdmissionStepKind::MessagePush
                | AdmissionStepKind::OtherContinue
                | AdmissionStepKind::OtherPush => {
                    process_admission_noncompletion(
                        kind,
                        &mut self.scratch,
                        &mut self.frames,
                        &self.scores,
                        &mut self.stats,
                    )?;
                    self.check_noncompletion_event(kind, &ticket)?;
                    None
                }
                AdmissionStepKind::MessageComplete => self.finish_message_complete(ticket)?,
                AdmissionStepKind::OtherComplete => self.finish_other_complete(ticket)?,
            };
            if let Some(summary) = root {
                return Ok((summary, self.scores, self.stats));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhoapi::{
        Bundle, CostBoundSignature, CostSignature, CostTokenStack, If, Match, New, Receive, Send,
    };
    use crate::rust::rholang::canonical_semantics::CanonicalScoreAlgebra;
    use crate::rust::rholang::protobuf_encoder;

    #[test]
    fn message_completion_source_rejects_changed_private_pre_cursor() {
        let bytes = protobuf_encoder::encode_to_vec(&Send::default());
        let region = IndexedByteRegion::from_borrowed(&bytes, 71, 103);
        let mut core = AdmissionLiveCore::new(
            FrameRequest::Message {
                region,
                program: &SEND_RHO_MATCH_PROGRAM,
            },
            bytes.len(),
        )
        .expect("generated Send has a checked root frame");
        let mut ticket = PreStepTicket::before(&core.frames, &core.scores, &core.stats).unwrap();
        let ActivePreStepTicket::Message(before) = &mut ticket.active else {
            panic!("the root is a Message frame");
        };
        before.cursor.bytes_read += 1;
        core.scratch.driver_source = Some(DriverStepSource::before(&core.frames, &core.scores));
        let kind = core
            .frames
            .last_mut()
            .expect("root frame")
            .step_into(&mut core.stats, &mut core.scores, &mut core.scratch)
            .expect("generated empty Send reaches EOF");
        assert!(matches!(kind, AdmissionStepKind::MessageComplete));
        assert!(matches!(
            core.finish_message_complete(ticket),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "message completion disagrees with its private pre-step ticket"
            ))
        ));
    }

    #[test]
    fn send_source_checker_rejects_equal_length_wrong_scalar_and_child() {
        let bytes = protobuf_encoder::encode_to_vec(&Send::default());
        let region = IndexedByteRegion::from_borrowed(&bytes, 72, 107);
        let frame = MessageFrame::from_message_region(region, &SEND_RHO_MATCH_PROGRAM).unwrap();

        let mut wrong_scalar = CanonicalScoreArena::new();
        let scalar_start = wrong_scalar.source_len();
        let scalar = wrong_scalar.integer(0).unwrap();
        wrong_scalar.node(vec![scalar]).unwrap();
        assert!(wrong_scalar.source_suffix_valid(scalar_start));
        assert!(matches!(
            check_send_append(&frame, &wrong_scalar, scalar_start),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "Send EOF score append disagrees with pre-EOF semantic slots"
            ))
        ));

        let mut wrong_child = CanonicalScoreArena::new();
        let old = wrong_child.integer(17).unwrap();
        let child_start = wrong_child.source_len();
        wrong_child
            .integer(i64::from(semantics::NONSEMANTIC_SCORE_CONSTRUCTOR))
            .unwrap();
        wrong_child.node(vec![old]).unwrap();
        assert!(wrong_child.source_suffix_valid(child_start));
        assert!(matches!(
            check_send_append(&frame, &wrong_child, child_start),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "Send EOF score append disagrees with pre-EOF semantic slots"
            ))
        ));
    }

    #[test]
    fn bundle_if_match_source_checkers_reject_equal_length_wrong_scalar_and_child() {
        macro_rules! rejects_corrupt_incomplete {
            ($message:expr, $program:expr, $check:ident) => {{
                let bytes = protobuf_encoder::encode_to_vec(&$message);
                let region = IndexedByteRegion::from_borrowed(&bytes, 73, 109);
                let frame = MessageFrame::from_message_region(region, $program).unwrap();

                let mut wrong_scalar = CanonicalScoreArena::new();
                let scalar_start = wrong_scalar.source_len();
                let scalar = wrong_scalar.integer(0).unwrap();
                wrong_scalar.node(vec![scalar]).unwrap();
                assert!(wrong_scalar.source_suffix_valid(scalar_start));
                assert!(matches!(
                    $check(&frame, &wrong_scalar, scalar_start),
                    Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                        "component EOF score append disagrees with pre-EOF semantic slots"
                    ))
                ));

                let mut wrong_child = CanonicalScoreArena::new();
                let old = wrong_child.integer(17).unwrap();
                let child_start = wrong_child.source_len();
                wrong_child
                    .integer(i64::from(semantics::NONSEMANTIC_SCORE_CONSTRUCTOR))
                    .unwrap();
                wrong_child.node(vec![old]).unwrap();
                assert!(wrong_child.source_suffix_valid(child_start));
                assert!(matches!(
                    $check(&frame, &wrong_child, child_start),
                    Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                        "component EOF score append disagrees with pre-EOF semantic slots"
                    ))
                ));
            }};
        }
        rejects_corrupt_incomplete!(
            Bundle::default(),
            &BUNDLE_RHO_MATCH_PROGRAM,
            check_bundle_append
        );
        rejects_corrupt_incomplete!(If::default(), &IF_RHO_MATCH_PROGRAM, check_if_append);
        rejects_corrupt_incomplete!(
            Match::default(),
            &MATCH_RHO_MATCH_PROGRAM,
            check_match_append
        );
    }

    #[test]
    fn cost_list_source_checkers_reject_same_shape_wrong_scalar_and_child() {
        macro_rules! rejects_corrupt_empty_list {
            ($message:expr, $program:expr, $constructor:expr, $check:ident) => {{
                let bytes = protobuf_encoder::encode_to_vec(&$message);
                let region = IndexedByteRegion::from_borrowed(&bytes, 74, 113);
                let frame = MessageFrame::from_message_region(region, $program).unwrap();

                let mut wrong_scalar = CanonicalScoreArena::new();
                let scalar_start = wrong_scalar.source_len();
                let constructor = wrong_scalar.integer(i64::from($constructor) + 1).unwrap();
                wrong_scalar.node(vec![constructor]).unwrap();
                assert!(wrong_scalar.source_suffix_valid(scalar_start));
                assert!(matches!(
                    $check(&frame, &wrong_scalar, scalar_start),
                    Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                        "component EOF score append disagrees with pre-EOF semantic slots"
                    ))
                ));

                let mut wrong_child = CanonicalScoreArena::new();
                let old = wrong_child.integer(17).unwrap();
                let child_start = wrong_child.source_len();
                wrong_child.integer(i64::from($constructor)).unwrap();
                wrong_child.node(vec![old]).unwrap();
                assert!(wrong_child.source_suffix_valid(child_start));
                assert!(matches!(
                    $check(&frame, &wrong_child, child_start),
                    Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                        "component EOF score append disagrees with pre-EOF semantic slots"
                    ))
                ));
            }};
        }
        rejects_corrupt_empty_list!(
            CostSignature::default(),
            &COSTSIGNATURE_RHO_MATCH_PROGRAM,
            Score::COST_SIGNATURE,
            check_cost_signature_append
        );
        rejects_corrupt_empty_list!(
            CostTokenStack::default(),
            &COSTTOKENSTACK_RHO_MATCH_PROGRAM,
            Score::COST_TOKEN_STACK,
            check_cost_token_stack_append
        );
    }

    #[test]
    fn cost_bound_source_checker_rejects_same_shape_wrong_scalar_and_child() {
        let bytes = protobuf_encoder::encode_to_vec(&CostBoundSignature::default());
        let region = IndexedByteRegion::from_borrowed(&bytes, 75, 127);
        let frame =
            MessageFrame::from_message_region(region, &COSTBOUNDSIGNATURE_RHO_MATCH_PROGRAM)
                .unwrap();

        let mut wrong_scalar = CanonicalScoreArena::new();
        let scalar_start = wrong_scalar.source_len();
        let index = wrong_scalar.integer(1).unwrap();
        let constructor = wrong_scalar.integer(i64::from(Score::COST_BOUND)).unwrap();
        wrong_scalar.node(vec![constructor, index]).unwrap();
        assert!(wrong_scalar.source_suffix_valid(scalar_start));
        assert!(matches!(
            check_cost_bound_append(&frame, &wrong_scalar, scalar_start),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "component EOF score append disagrees with pre-EOF semantic slots"
            ))
        ));

        let mut wrong_child = CanonicalScoreArena::new();
        let old = wrong_child.integer(17).unwrap();
        let child_start = wrong_child.source_len();
        wrong_child.integer(0).unwrap();
        let constructor = wrong_child.integer(i64::from(Score::COST_BOUND)).unwrap();
        wrong_child.node(vec![constructor, old]).unwrap();
        assert!(wrong_child.source_suffix_valid(child_start));
        assert!(matches!(
            check_cost_bound_append(&frame, &wrong_child, child_start),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "component EOF score append disagrees with pre-EOF semantic slots"
            ))
        ));
    }

    #[test]
    fn receive_new_source_checkers_reject_same_shape_wrong_scalar_and_child() {
        macro_rules! rejects_corrupt_incomplete {
            ($message:expr, $program:expr, $check:ident) => {{
                let bytes = protobuf_encoder::encode_to_vec(&$message);
                let region = IndexedByteRegion::from_borrowed(&bytes, 76, 131);
                let frame = MessageFrame::from_message_region(region, $program).unwrap();

                let mut wrong_scalar = CanonicalScoreArena::new();
                let scalar_start = wrong_scalar.source_len();
                let constructor = wrong_scalar
                    .integer(i64::from(semantics::NONSEMANTIC_SCORE_CONSTRUCTOR) - 1)
                    .unwrap();
                wrong_scalar.node(vec![constructor]).unwrap();
                assert!(wrong_scalar.source_suffix_valid(scalar_start));
                assert!(matches!(
                    $check(&frame, &wrong_scalar, scalar_start),
                    Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                        "component EOF score append disagrees with pre-EOF semantic slots"
                    ))
                ));

                let mut wrong_child = CanonicalScoreArena::new();
                let old = wrong_child.integer(17).unwrap();
                let child_start = wrong_child.source_len();
                wrong_child
                    .integer(i64::from(semantics::NONSEMANTIC_SCORE_CONSTRUCTOR))
                    .unwrap();
                wrong_child.node(vec![old]).unwrap();
                assert!(wrong_child.source_suffix_valid(child_start));
                assert!(matches!(
                    $check(&frame, &wrong_child, child_start),
                    Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                        "component EOF score append disagrees with pre-EOF semantic slots"
                    ))
                ));
            }};
        }
        rejects_corrupt_incomplete!(
            Receive::default(),
            &RECEIVE_RHO_MATCH_PROGRAM,
            check_receive_append
        );
        rejects_corrupt_incomplete!(New::default(), &NEW_RHO_MATCH_PROGRAM, check_new_append);
    }

    #[test]
    fn indexed_score_source_rejects_equal_bytes_from_a_different_view() {
        let bytes = b"new-injection-key";
        let source = IndexedByteRegion::from_borrowed(bytes, 77, 137);
        let wrong_view = IndexedByteRegion::from_borrowed(bytes, 78, 137);
        let mut scores = CanonicalScoreArena::new();
        scores.indexed_string(source).unwrap();
        assert!(scores.source_suffix_valid(0));
        let mut cursor = ScoreAppendSourceCursor::new(
            &scores,
            0,
            "New EOF score append disagrees with pre-EOF semantic slots",
        );
        assert!(matches!(
            cursor.indexed_string(&wrong_view),
            Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "New EOF score append disagrees with pre-EOF semantic slots"
            ))
        ));
    }
}
