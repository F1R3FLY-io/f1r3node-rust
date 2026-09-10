From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportCodec StateImportCursor StateImportTraversal StateImportStack StateImportPage StateImportExport.
Import ListNotations.

Inductive ImportStepResult : Type :=
| ImportStepExhausted : ImportStepResult
| ImportStepProgress : option ImportExportEntry -> list ImportTraversalFrame -> ImportStepResult
| ImportStepReadFailure : list nat -> ImportStepResult.

Definition import_evaluate_step (read : ImportHistoryReader) (frames : list ImportTraversalFrame)
    : ImportStepResult :=
  match frames with
  | [] => ImportStepExhausted
  | frame :: rest => match import_frame_next frame with
    | None => ImportStepProgress None rest
    | Some selected =>
        let parent := import_advance_frame frame (import_next_slot selected) in
        let edge := import_next_edge selected in
        if import_wire_header edge <? 128 then
          ImportStepProgress (Some (import_leaf_edge_entry edge)) (parent :: rest)
        else match read (import_wire_hash edge) with
          | None => ImportStepReadFailure (import_wire_hash edge)
          | Some edges => ImportStepProgress
              (Some (import_history_edge_entry (import_frame_prefix frame) (import_next_slot selected) edge))
              (import_child_frame frame selected edges :: parent :: rest)
          end
    end
  end.

Theorem import_evaluated_step_is_operational : forall read initial event final,
  import_evaluate_step read initial = ImportStepProgress event final ->
  import_export_step read initial event final.
Proof.
  intros read [|frame frames] event final Result; [discriminate|].
  unfold import_evaluate_step in Result.
  destruct (import_frame_next frame) as [selected|] eqn:Found.
  - destruct (import_wire_header (import_next_edge selected) <? 128) eqn:Kind.
    + inversion Result; subst. now apply import_export_step_leaf.
    + destruct (read (import_wire_hash (import_next_edge selected))) as [edges|] eqn:Read;
        [|discriminate]. inversion Result; subst. now apply import_export_step_history.
  - inversion Result; subst. now apply import_export_step_pop.
Qed.

Theorem import_operational_step_has_exact_evaluation : forall read initial event final,
  import_export_step read initial event final ->
  import_evaluate_step read initial = ImportStepProgress event final.
Proof.
  intros read initial event final Step. destruct Step as
    [frame frames Empty|frame frames selected Found Leaf|frame frames selected edges Found History Read];
    unfold import_evaluate_step.
  - now rewrite Empty.
  - now rewrite Found, Leaf.
  - now rewrite Found, History, Read.
Qed.

Theorem import_step_evaluation_characterization : forall read initial event final,
  import_evaluate_step read initial = ImportStepProgress event final <->
  import_export_step read initial event final.
Proof. split; auto using import_evaluated_step_is_operational, import_operational_step_has_exact_evaluation. Qed.

Theorem import_evaluated_exhaustion_has_empty_stack : forall read frames,
  import_evaluate_step read frames = ImportStepExhausted <-> frames = [].
Proof.
  intros read [|frame frames]; [split; reflexivity|]. unfold import_evaluate_step.
  destruct (import_frame_next frame) as [selected|]; [|split; discriminate].
  destruct (import_wire_header (import_next_edge selected) <? 128); [split; discriminate|].
  destruct (read (import_wire_hash (import_next_edge selected))); split; discriminate.
Qed.

Theorem import_evaluated_read_failure_has_exact_witness : forall read initial key,
  import_evaluate_step read initial = ImportStepReadFailure key <->
  exists frame frames selected,
    initial = frame :: frames /\ import_frame_next frame = Some selected /\
    (import_wire_header (import_next_edge selected) <? 128) = false /\
    key = import_wire_hash (import_next_edge selected) /\ read key = None.
Proof.
  intros read initial key. split.
  - intros Result. destruct initial as [|frame frames]; [discriminate|].
    unfold import_evaluate_step in Result.
    destruct (import_frame_next frame) as [selected|] eqn:Found; [|discriminate].
    destruct (import_wire_header (import_next_edge selected) <? 128) eqn:Kind; [discriminate|].
    destruct (read (import_wire_hash (import_next_edge selected))) as [edges|] eqn:Read; [discriminate|].
    inversion Result; subst key. exists frame, frames, selected. auto.
  - intros [frame [frames [selected [Same [Found [Kind [Key Read]]]]]]].
    subst initial key. unfold import_evaluate_step. now rewrite Found, Kind, Read.
Qed.

Definition import_frames_coherent (read : ImportHistoryReader) (frames : list ImportTraversalFrame) : Prop :=
  Forall (fun frame => read (import_frame_key frame) = Some (import_frame_edges frame)) frames.

Theorem import_successful_step_preserves_frame_coherence : forall read initial event final,
  import_frames_coherent read initial -> import_export_step read initial event final ->
  import_frames_coherent read final.
Proof.
  intros read initial event final Coherent Step. destruct Step;
    inversion Coherent; subst; unfold import_frames_coherent in *.
  - assumption.
  - constructor; auto.
  - constructor; auto.
Qed.

Inductive ImportEvaluation : Type :=
| ImportEvaluationSuccess : list ImportExportEntry -> list ImportTraversalFrame -> ImportEvaluation
| ImportEvaluationReadFailure : list nat -> ImportEvaluation
| ImportEvaluationFuelExhausted : ImportEvaluation.

Definition import_add_evaluated_entries (prefix : list ImportExportEntry) (result : ImportEvaluation)
    : ImportEvaluation :=
  match result with
  | ImportEvaluationSuccess entries final => ImportEvaluationSuccess (prefix ++ entries) final
  | ImportEvaluationReadFailure key => ImportEvaluationReadFailure key
  | ImportEvaluationFuelExhausted => ImportEvaluationFuelExhausted
  end.

Fixpoint import_evaluate_slice (fuel : nat) (read : ImportHistoryReader)
    (skip budget : nat) (frames : list ImportTraversalFrame) : ImportEvaluation :=
  match frames with
  | [] => ImportEvaluationSuccess [] []
  | _ :: _ =>
      if (skip =? 0) && (budget =? 0) then ImportEvaluationSuccess [] frames else
      match fuel with
      | 0 => ImportEvaluationFuelExhausted
      | S remaining => match import_evaluate_step read frames with
        | ImportStepExhausted => ImportEvaluationSuccess [] []
        | ImportStepReadFailure key => ImportEvaluationReadFailure key
        | ImportStepProgress event next =>
            if skip =? 0 then
              import_add_evaluated_entries (import_event_entries event)
                (import_evaluate_slice remaining read 0 (budget - import_event_history_cost event) next)
            else import_evaluate_slice remaining read (skip - import_event_history_cost event) budget next
        end
      end
  end.

Theorem import_evaluation_never_exhausts_its_derived_fuel : forall fuel read skip budget frames,
  import_export_potential skip budget frames < fuel ->
  import_evaluate_slice fuel read skip budget frames <> ImportEvaluationFuelExhausted.
Proof.
  induction fuel as [|fuel IH]; intros read skip budget [|frame frames] Bound; try lia.
  - discriminate.
  - cbn [import_evaluate_slice].
    destruct ((skip =? 0) && (budget =? 0)) eqn:Stop; [discriminate|].
    destruct (import_evaluate_step read (frame :: frames)) as [|event next|key] eqn:Step;
      try discriminate.
    apply import_evaluated_step_is_operational in Step.
    destruct (skip =? 0) eqn:Skipping.
    + apply Nat.eqb_eq in Skipping. subst skip.
      assert (0 < budget) as Positive.
      { destruct budget; [discriminate|lia]. }
      pose proof (import_taking_step_strictly_reduces_export_potential _ _ _ _ _ Positive Step) as Decrease.
      assert (import_export_potential 0 (budget - import_event_history_cost event) next < fuel) as Smaller by lia.
      specialize (IH read 0 (budget - import_event_history_cost event) next Smaller).
      destruct (import_evaluate_slice fuel read 0 (budget - import_event_history_cost event) next);
        cbn [import_add_evaluated_entries]; congruence.
    + apply Nat.eqb_neq in Skipping.
      assert (0 < skip) as Positive by lia.
      pose proof (import_skipping_step_strictly_reduces_export_potential
        read skip budget (frame :: frames) event next Positive Step) as Decrease.
      apply IH. lia.
Qed.

Definition import_evaluate_bounded_slice (read : ImportHistoryReader) (skip budget : nat)
    (frames : list ImportTraversalFrame) : ImportEvaluation :=
  import_evaluate_slice (S (import_export_potential skip budget frames)) read skip budget frames.

Theorem import_bounded_slice_returns_success_or_read_failure : forall read skip budget frames,
  (exists entries final, import_evaluate_bounded_slice read skip budget frames = ImportEvaluationSuccess entries final) \/
  (exists key, import_evaluate_bounded_slice read skip budget frames = ImportEvaluationReadFailure key).
Proof.
  intros read skip budget frames.
  assert (import_evaluate_bounded_slice read skip budget frames <> ImportEvaluationFuelExhausted) as Enough.
  { unfold import_evaluate_bounded_slice. apply import_evaluation_never_exhausts_its_derived_fuel. lia. }
  destruct (import_evaluate_bounded_slice read skip budget frames); eauto. contradiction.
Qed.

Theorem import_zero_skip_slice_is_a_page : forall read budget initial entries final,
  import_export_slice read 0 budget initial entries final ->
  import_export_page read budget initial entries final.
Proof.
  intros read budget initial entries final Slice. inversion Slice; subst; auto; try lia.
  apply import_export_page_done.
Qed.

Theorem import_successful_slice_evaluation_is_operational : forall fuel read skip budget initial entries final,
  import_evaluate_slice fuel read skip budget initial = ImportEvaluationSuccess entries final ->
  import_export_slice read skip budget initial entries final.
Proof.
  induction fuel as [|fuel IH]; intros read skip budget [|frame frames] entries final Result;
    cbn [import_evaluate_slice] in Result.
  - inversion Result; subst. apply import_export_slice_done.
  - destruct ((skip =? 0) && (budget =? 0)) eqn:Stop; [|discriminate].
    apply andb_true_iff in Stop as [Skip Budget]. apply Nat.eqb_eq in Skip, Budget.
    subst skip budget. inversion Result; subst. apply import_export_slice_ready, import_export_page_limit.
  - inversion Result; subst. apply import_export_slice_done.
  - destruct ((skip =? 0) && (budget =? 0)) eqn:Stop.
    + apply andb_true_iff in Stop as [Skip Budget]. apply Nat.eqb_eq in Skip, Budget.
      subst skip budget. inversion Result; subst. apply import_export_slice_ready, import_export_page_limit.
    + destruct (import_evaluate_step read (frame :: frames)) as [|event next|key] eqn:Step;
        try discriminate.
      * apply import_evaluated_exhaustion_has_empty_stack in Step. discriminate.
      * apply import_evaluated_step_is_operational in Step.
        destruct (skip =? 0) eqn:Skipping.
        -- apply Nat.eqb_eq in Skipping. subst skip.
           assert (0 < budget) as Positive by (destruct budget; [discriminate|lia]).
           destruct (import_evaluate_slice fuel read 0 (budget - import_event_history_cost event) next)
             as [tail tail_final|missing|] eqn:Tail; try discriminate.
           inversion Result; subst entries final. apply import_export_slice_ready.
           eapply import_export_page_next; eauto.
           apply import_zero_skip_slice_is_a_page. eapply IH. exact Tail.
        -- apply Nat.eqb_neq in Skipping.
           eapply import_export_slice_skip; eauto. lia.
Qed.

Theorem import_bounded_evaluation_respects_history_budget : forall read skip budget initial entries final,
  import_evaluate_bounded_slice read skip budget initial = ImportEvaluationSuccess entries final ->
  length (import_export_history_keys entries) <= budget.
Proof.
  intros read skip budget initial entries final Result. unfold import_evaluate_bounded_slice in Result.
  apply import_successful_slice_evaluation_is_operational in Result.
  now apply import_slice_execution_respects_history_budget in Result.
Qed.

Theorem import_bounded_evaluation_has_exact_occurrence_split : forall read skip budget initial entries final trace,
  import_evaluate_bounded_slice read skip budget initial = ImportEvaluationSuccess entries final ->
  import_stack_trace read initial trace -> exists suffix,
    import_take_history_occurrences budget (import_drop_history_occurrences skip trace) = (entries, suffix) /\
    import_stack_trace read final suffix.
Proof.
  intros read skip budget initial entries final trace Result Trace. unfold import_evaluate_bounded_slice in Result.
  apply import_successful_slice_evaluation_is_operational in Result.
  now apply (import_slice_execution_matches_history_skip_and_take _ _ _ _ _ _ Result).
Qed.

Theorem import_slice_error_has_a_reachable_failed_read : forall fuel read skip budget initial key,
  import_evaluate_slice fuel read skip budget initial = ImportEvaluationReadFailure key ->
  exists traversed stopped,
    import_export_run read initial traversed stopped /\
    import_evaluate_step read stopped = ImportStepReadFailure key.
Proof.
  induction fuel as [|fuel IH]; intros read skip budget [|frame frames] key Result;
    cbn [import_evaluate_slice] in Result; try discriminate.
  - destruct ((skip =? 0) && (budget =? 0)); discriminate.
  - destruct ((skip =? 0) && (budget =? 0)); [discriminate|].
    destruct (import_evaluate_step read (frame :: frames)) as [|event next|missing] eqn:Step;
      [discriminate| |].
    + apply import_evaluated_step_is_operational in Step.
      destruct (skip =? 0) eqn:Skipping.
      * destruct (import_evaluate_slice fuel read 0 (budget - import_event_history_cost event) next)
          as [tail final|missing|] eqn:Tail; try discriminate.
        inversion Result; subst missing.
        destruct (IH _ _ _ _ _ Tail) as [traversed [stopped [Run Failed]]].
        exists (import_event_entries event ++ traversed), stopped. split; auto.
        eapply import_export_run_next; eauto.
      * destruct (IH _ _ _ _ _ Result) as [traversed [stopped [Run Failed]]].
        exists (import_event_entries event ++ traversed), stopped. split; auto.
        eapply import_export_run_next; eauto.
    + inversion Result; subst missing. exists [], (frame :: frames). split; auto.
      apply import_export_run_stop.
Qed.

Theorem import_reported_read_failure_is_not_fabricated : forall read skip budget initial key,
  import_evaluate_bounded_slice read skip budget initial = ImportEvaluationReadFailure key -> read key = None.
Proof.
  intros read skip budget initial key Result. unfold import_evaluate_bounded_slice in Result.
  apply import_slice_error_has_a_reachable_failed_read in Result as [traversed [stopped [Run Failed]]].
  apply import_evaluated_read_failure_has_exact_witness in Failed.
  destruct Failed as [frame [frames [selected [_ [_ [_ [_ Read]]]]]]]. exact Read.
Qed.

Theorem import_operational_page_has_exact_evaluation : forall read budget initial entries final,
  import_export_page read budget initial entries final -> forall fuel,
  import_export_potential 0 budget initial < fuel ->
  import_evaluate_slice fuel read 0 budget initial = ImportEvaluationSuccess entries final.
Proof.
  intros read budget initial entries final Page. induction Page as
    [frames|budget|budget initial event next entries final Positive Step Page IH]; intros fuel Bound.
  - destruct fuel, frames; reflexivity.
  - destruct fuel; reflexivity.
  - destruct fuel as [|fuel]; [lia|]. destruct initial as [|frame frames]; [inversion Step|].
    cbn [import_evaluate_slice].
    assert (budget =? 0 = false) as NotZero by (apply Nat.eqb_neq; lia).
    rewrite NotZero. cbn [andb].
    rewrite (import_operational_step_has_exact_evaluation _ _ _ _ Step).
    pose proof (import_taking_step_strictly_reduces_export_potential _ _ _ _ _ Positive Step) as Decrease.
    assert (import_export_potential 0 (budget - import_event_history_cost event) next < fuel) as Smaller by lia.
    rewrite (IH fuel Smaller). reflexivity.
Qed.

Theorem import_operational_slice_has_exact_evaluation : forall read skip budget initial entries final,
  import_export_slice read skip budget initial entries final -> forall fuel,
  import_export_potential skip budget initial < fuel ->
  import_evaluate_slice fuel read skip budget initial = ImportEvaluationSuccess entries final.
Proof.
  intros read skip budget initial entries final Slice. induction Slice as
    [budget initial entries final Page|skip budget|
     skip budget initial event next entries final Positive Step Slice IH]; intros fuel Bound.
  - now apply (import_operational_page_has_exact_evaluation _ _ _ _ _ Page).
  - destruct fuel; reflexivity.
  - destruct fuel as [|fuel]; [lia|]. destruct initial as [|frame frames]; [inversion Step|].
    cbn [import_evaluate_slice].
    assert (skip =? 0 = false) as NotZero by (apply Nat.eqb_neq; lia).
    rewrite NotZero. cbn [andb].
    rewrite (import_operational_step_has_exact_evaluation _ _ _ _ Step).
    pose proof (import_skipping_step_strictly_reduces_export_potential
      read skip budget (frame :: frames) event next Positive Step) as Decrease.
    apply IH. lia.
Qed.

Theorem import_bounded_slice_success_characterization : forall read skip budget initial entries final,
  import_evaluate_bounded_slice read skip budget initial = ImportEvaluationSuccess entries final <->
  import_export_slice read skip budget initial entries final.
Proof.
  intros read skip budget initial entries final. unfold import_evaluate_bounded_slice. split.
  - apply import_successful_slice_evaluation_is_operational.
  - intros Slice. apply (import_operational_slice_has_exact_evaluation _ _ _ _ _ _ Slice). lia.
Qed.

Theorem import_successful_slices_are_deterministic : forall read skip budget initial first first_final second second_final,
  import_export_slice read skip budget initial first first_final ->
  import_export_slice read skip budget initial second second_final ->
  first = second /\ first_final = second_final.
Proof.
  intros read skip budget initial first first_final second second_final First Second.
  apply import_bounded_slice_success_characterization in First, Second.
  assert (ImportEvaluationSuccess first first_final = ImportEvaluationSuccess second second_final) as Same by congruence.
  inversion Same. auto.
Qed.

Theorem import_page_survives_reader_extension : forall before after budget initial entries final,
  import_history_reader_extension before after -> import_export_page before budget initial entries final ->
  import_export_page after budget initial entries final.
Proof.
  intros before after budget initial entries final Extend Page. induction Page.
  - apply import_export_page_limit.
  - apply import_export_page_done.
  - eapply import_export_page_next; eauto using import_export_step_preserves_reader_extension.
Qed.

Theorem import_slice_survives_reader_extension : forall before after skip budget initial entries final,
  import_history_reader_extension before after -> import_export_slice before skip budget initial entries final ->
  import_export_slice after skip budget initial entries final.
Proof.
  intros before after skip budget initial entries final Extend Slice. induction Slice.
  - apply import_export_slice_ready. eapply import_page_survives_reader_extension; eauto.
  - apply import_export_slice_done.
  - eapply import_export_slice_skip; eauto using import_export_step_preserves_reader_extension.
Qed.

Inductive import_interleaved_slice : ImportHistoryReader -> ImportHistoryReader -> nat -> nat ->
    list ImportTraversalFrame -> list ImportExportEntry -> list ImportTraversalFrame -> Prop :=
| import_interleaved_slice_limit : forall read frames,
    import_interleaved_slice read read 0 0 frames [] frames
| import_interleaved_slice_done : forall read skip budget,
    import_interleaved_slice read read skip budget [] [] []
| import_interleaved_slice_take : forall first middle last budget initial event next entries final,
    0 < budget -> import_export_step first initial event next ->
    import_history_reader_extension first middle ->
    import_interleaved_slice middle last 0 (budget - import_event_history_cost event) next entries final ->
    import_interleaved_slice first last 0 budget initial (import_event_entries event ++ entries) final
| import_interleaved_slice_skip : forall first middle last skip budget initial event next entries final,
    0 < skip -> import_export_step first initial event next ->
    import_history_reader_extension first middle ->
    import_interleaved_slice middle last (skip - import_event_history_cost event) budget next entries final ->
    import_interleaved_slice first last skip budget initial entries final
| import_interleaved_slice_write : forall first middle last skip budget initial entries final,
    import_history_reader_extension first middle ->
    import_interleaved_slice middle last skip budget initial entries final ->
    import_interleaved_slice first last skip budget initial entries final.

Theorem import_interleaved_slice_preserves_reader_bindings : forall first last skip budget initial entries final,
  import_interleaved_slice first last skip budget initial entries final ->
  import_history_reader_extension first last.
Proof.
  intros first last skip budget initial entries final Slice. induction Slice.
  - intros key edges Read. exact Read.
  - intros key edges Read. exact Read.
  - eapply import_history_reader_extension_is_transitive; eauto.
  - eapply import_history_reader_extension_is_transitive; eauto.
  - eapply import_history_reader_extension_is_transitive; eauto.
Qed.

Theorem import_interleaved_slice_has_exact_final_view_page : forall first last skip budget initial entries final,
  import_interleaved_slice first last skip budget initial entries final ->
  import_export_slice last skip budget initial entries final.
Proof.
  intros first last skip budget initial entries final Slice. induction Slice as
    [read frames|read skip budget|
     first middle last budget initial event next entries final Positive Step Extend Slice IH|
     first middle last skip budget initial event next entries final Positive Step Extend Slice IH|
     first middle last skip budget initial entries final Extend Slice IH].
  - apply import_export_slice_ready, import_export_page_limit.
  - apply import_export_slice_done.
  - apply import_export_slice_ready. eapply import_export_page_next; [exact Positive| |].
    + eapply import_export_step_preserves_reader_extension; [|exact Step].
      eapply import_history_reader_extension_is_transitive; [exact Extend|].
      eapply import_interleaved_slice_preserves_reader_bindings. exact Slice.
    + now apply import_zero_skip_slice_is_a_page in IH.
  - eapply import_export_slice_skip; [exact Positive| |exact IH].
    eapply import_export_step_preserves_reader_extension; [|exact Step].
    eapply import_history_reader_extension_is_transitive; [exact Extend|].
    eapply import_interleaved_slice_preserves_reader_bindings. exact Slice.
  - exact IH.
Qed.

Theorem import_interleaved_slice_matches_final_evaluator : forall first last skip budget initial entries final,
  import_interleaved_slice first last skip budget initial entries final ->
  import_evaluate_bounded_slice last skip budget initial = ImportEvaluationSuccess entries final.
Proof.
  intros first last skip budget initial entries final Slice.
  apply import_bounded_slice_success_characterization.
  now apply import_interleaved_slice_has_exact_final_view_page in Slice.
Qed.

Theorem import_interleaved_slice_and_trailing_writes_preserve_page_result :
  forall first last trailing skip budget initial entries final,
  import_interleaved_slice first last skip budget initial entries final ->
  import_history_reader_extension last trailing ->
  import_evaluate_bounded_slice trailing skip budget initial = ImportEvaluationSuccess entries final.
Proof.
  intros first last trailing skip budget initial entries final Slice Extend.
  apply import_interleaved_slice_has_exact_final_view_page in Slice.
  apply import_bounded_slice_success_characterization.
  eapply import_slice_survives_reader_extension; eauto.
Qed.

Theorem import_interleaved_slice_cannot_exceed_history_budget : forall first last skip budget initial entries final,
  import_interleaved_slice first last skip budget initial entries final ->
  length (import_export_history_keys entries) <= budget.
Proof.
  intros first last skip budget initial entries final Slice.
  apply import_interleaved_slice_has_exact_final_view_page in Slice.
  now apply import_slice_execution_respects_history_budget in Slice.
Qed.

Inductive import_interleaved_anchored_export : ImportHistoryReader -> ImportHistoryReader ->
    option (list nat) -> nat -> nat -> list ImportTraversalFrame ->
    list ImportExportEntry -> list ImportTraversalFrame -> Prop :=
| import_interleaved_anchored_resume : forall first last skip budget initial entries final,
    import_interleaved_slice first last skip budget initial entries final ->
    import_interleaved_anchored_export first last None skip budget initial entries final
| import_interleaved_anchored_skip_root : forall first last key skip budget initial entries final,
    import_interleaved_slice first last skip budget initial entries final ->
    import_interleaved_anchored_export first last (Some key) (S skip) budget initial entries final
| import_interleaved_anchored_take_root : forall first last key budget initial entries final,
    import_interleaved_slice first last 0 budget initial entries final ->
    import_interleaved_anchored_export first last (Some key) 0 (S budget) initial
      (ImportExportHistory (import_key_to_nat key) [] :: entries) final.

Theorem import_interleaved_anchor_preserves_reader_bindings :
  forall first last anchor skip budget initial entries final,
  import_interleaved_anchored_export first last anchor skip budget initial entries final ->
  import_history_reader_extension first last.
Proof.
  intros first last anchor skip budget initial entries final Export. destruct Export;
    eapply import_interleaved_slice_preserves_reader_bindings; eauto.
Qed.

Theorem import_interleaved_anchor_has_exact_final_view_export :
  forall first last anchor skip budget initial entries final,
  import_interleaved_anchored_export first last anchor skip budget initial entries final ->
  import_anchored_export last anchor skip budget initial entries final.
Proof.
  intros first last anchor skip budget initial entries final Export. destruct Export.
  - apply import_anchored_resume. eapply import_interleaved_slice_has_exact_final_view_page; eauto.
  - apply import_anchored_skip_root. eapply import_interleaved_slice_has_exact_final_view_page; eauto.
  - apply import_anchored_take_root, import_zero_skip_slice_is_a_page.
    eapply import_interleaved_slice_has_exact_final_view_page; eauto.
Qed.

Definition import_interleaved_cursor_stack (first last : ImportHistoryReader) (cursor : ImportCursor)
    (frames : list ImportTraversalFrame) : Prop :=
  match cursor with
  | ImportCursorStart root => import_interleaved_history_stack first last root [] [] frames
  | ImportCursorResume root prefix carrier =>
      import_interleaved_history_stack first last root [] prefix frames /\
      exists top rest, frames = top :: rest /\ import_frame_key top = carrier
  end.

Theorem import_interleaved_cursor_stack_has_exact_final_view : forall first last cursor frames,
  import_interleaved_cursor_stack first last cursor frames -> import_cursor_initial_stack last cursor frames.
Proof.
  intros first last [root|root prefix carrier] frames Stack.
  - eapply import_interleaved_stack_has_final_view_witness. exact Stack.
  - destruct Stack as [Stack Carrier]. split; auto.
    eapply import_interleaved_stack_has_final_view_witness. exact Stack.
Qed.

Theorem import_cursor_stack_survives_reader_extension : forall before after cursor frames,
  import_history_reader_extension before after -> import_cursor_initial_stack before cursor frames ->
  import_cursor_initial_stack after cursor frames.
Proof.
  intros before after [root|root prefix carrier] frames Extend Stack.
  - eapply import_history_stack_survives_compatible_writes; eauto.
  - destruct Stack as [Stack Carrier]. split; auto.
    eapply import_history_stack_survives_compatible_writes; eauto.
Qed.

Definition import_interleaved_cursor_export (first last : ImportHistoryReader) (cursor : ImportCursor)
    (skip budget : nat) (entries : list ImportExportEntry) (final : list ImportTraversalFrame) : Prop :=
  import_cursor_validb cursor = true /\ (0 < skip \/ 0 < budget) /\
  exists observed middle frames, import_history_reader_extension first observed /\
    import_interleaved_cursor_stack observed middle cursor frames /\
    import_interleaved_anchored_export middle last (import_cursor_anchor cursor) skip budget frames entries final.

Theorem import_interleaved_cursor_export_has_exact_final_view :
  forall first last cursor skip budget entries final,
  import_interleaved_cursor_export first last cursor skip budget entries final ->
  import_export_from_cursor last cursor skip budget entries final.
Proof.
  intros first last cursor skip budget entries final
    [Valid [Counters [observed [middle [frames [Leading [Stack Export]]]]]]].
  split; auto. split; auto. exists frames. split.
  - eapply import_cursor_stack_survives_reader_extension.
    + eapply import_interleaved_anchor_preserves_reader_bindings. exact Export.
    + eapply import_interleaved_cursor_stack_has_exact_final_view. exact Stack.
  - eapply import_interleaved_anchor_has_exact_final_view_export. exact Export.
Qed.

Theorem import_interleaved_cursor_export_preserves_its_history_budget :
  forall first last cursor skip budget entries final,
  import_interleaved_cursor_export first last cursor skip budget entries final ->
  length (import_export_history_keys entries) <= budget.
Proof.
  intros first last cursor skip budget entries final Export.
  apply import_interleaved_cursor_export_has_exact_final_view in Export.
  now apply import_cursor_export_respects_history_budget in Export.
Qed.

Definition import_execution_test_key (byte : nat) : list nat := byte :: repeat 0 31.

Definition import_execution_test_pointer : ImportWireEdge :=
  {| import_wire_slot := 0; import_wire_header := 128; import_wire_prefix := [];
     import_wire_hash := import_execution_test_key 2 |}.

Definition import_execution_test_leaf : ImportWireEdge :=
  {| import_wire_slot := 0; import_wire_header := 0; import_wire_prefix := [];
     import_wire_hash := import_execution_test_key 3 |}.

Definition import_execution_test_root : ImportTraversalFrame :=
  import_frame_at (import_execution_test_key 1) [] [import_execution_test_pointer] None.

Definition import_execution_before : ImportHistoryReader := fun key =>
  if list_eq_dec Nat.eq_dec key (import_execution_test_key 1) then Some [import_execution_test_pointer] else None.

Definition import_execution_after : ImportHistoryReader := fun key =>
  if list_eq_dec Nat.eq_dec key (import_execution_test_key 2) then Some [import_execution_test_leaf]
  else import_execution_before key.

Example import_read_error_is_not_successful_exhaustion :
  import_evaluate_bounded_slice import_execution_before 0 1 [import_execution_test_root] =
    ImportEvaluationReadFailure (import_execution_test_key 2).
Proof. vm_compute. reflexivity. Qed.

Example import_zero_budget_does_not_probe_an_unavailable_child :
  import_evaluate_bounded_slice import_execution_before 0 0 [import_execution_test_root] =
    ImportEvaluationSuccess [] [import_execution_test_root].
Proof. vm_compute. reflexivity. Qed.

Example import_recovery_read_can_succeed_after_a_prior_failure :
  import_evaluate_bounded_slice import_execution_after 0 2 [import_execution_test_root] =
    ImportEvaluationSuccess [ImportExportHistory 2 [0]; ImportExportLeaf 3] [].
Proof. vm_compute. reflexivity. Qed.

Example import_inserted_row_preserves_all_previous_successful_reads :
  import_history_reader_extension import_execution_before import_execution_after.
Proof.
  intros key edges Read. unfold import_execution_before in Read.
  destruct (list_eq_dec Nat.eq_dec key (import_execution_test_key 1)) as [Same|Different]; [|discriminate].
  subst key. inversion Read; subst edges. vm_compute. reflexivity.
Qed.

Example import_read_failure_need_not_persist_in_the_final_view :
  import_evaluate_step import_execution_before [import_execution_test_root] =
    ImportStepReadFailure (import_execution_test_key 2) /\
  import_execution_after (import_execution_test_key 2) = Some [import_execution_test_leaf] /\
  import_history_reader_extension import_execution_before import_execution_after.
Proof.
  split; [vm_compute; reflexivity|]. split; [vm_compute; reflexivity|].
  apply import_inserted_row_preserves_all_previous_successful_reads.
Qed.

Theorem import_fixed_page_is_a_valid_interleaved_slice : forall read budget initial entries final,
  import_export_page read budget initial entries final ->
  import_interleaved_slice read read 0 budget initial entries final.
Proof.
  intros read budget initial entries final Page. induction Page.
  - apply import_interleaved_slice_limit.
  - apply import_interleaved_slice_done.
  - eapply import_interleaved_slice_take; eauto. intros key edges Read. exact Read.
Qed.

Theorem import_fixed_slice_is_a_valid_interleaved_slice : forall read skip budget initial entries final,
  import_export_slice read skip budget initial entries final ->
  import_interleaved_slice read read skip budget initial entries final.
Proof.
  intros read skip budget initial entries final Slice. induction Slice.
  - now apply import_fixed_page_is_a_valid_interleaved_slice.
  - apply import_interleaved_slice_done.
  - eapply import_interleaved_slice_skip; eauto. intros key edges Read. exact Read.
Qed.

Example import_a_write_before_the_first_child_read_is_allowed :
  import_interleaved_slice import_execution_before import_execution_after 0 2 [import_execution_test_root]
    [ImportExportHistory 2 [0]; ImportExportLeaf 3] [].
Proof.
  eapply import_interleaved_slice_write.
  - apply import_inserted_row_preserves_all_previous_successful_reads.
  - apply import_fixed_slice_is_a_valid_interleaved_slice, import_bounded_slice_success_characterization.
    apply import_recovery_read_can_succeed_after_a_prior_failure.
Qed.

Theorem import_interleaved_cursor_stack_preserves_reader_bindings : forall first last cursor frames,
  import_interleaved_cursor_stack first last cursor frames -> import_history_reader_extension first last.
Proof.
  intros first last [root|root prefix carrier] frames Stack.
  - eapply import_interleaved_stack_preserves_reader_bindings. exact Stack.
  - destruct Stack as [Stack _]. eapply import_interleaved_stack_preserves_reader_bindings. exact Stack.
Qed.

Theorem import_interleaved_invocation_preserves_existing_reader_bindings :
  forall first last cursor skip budget entries final,
  import_interleaved_cursor_export first last cursor skip budget entries final ->
  import_history_reader_extension first last.
Proof.
  intros first last cursor skip budget entries final
    [_ [_ [observed [middle [frames [Leading [Stack Export]]]]]]].
  eapply import_history_reader_extension_is_transitive; [exact Leading|].
  eapply import_history_reader_extension_is_transitive.
  - eapply import_interleaved_cursor_stack_preserves_reader_bindings. exact Stack.
  - eapply import_interleaved_anchor_preserves_reader_bindings. exact Export.
Qed.

Example import_a_root_write_before_the_first_lookup_is_allowed :
  import_interleaved_cursor_export (fun _ => None) import_execution_after
    (ImportCursorStart (import_execution_test_key 1)) 0 3
    [ImportExportHistory 1 []; ImportExportHistory 2 [0]; ImportExportLeaf 3] [].
Proof.
  split; [vm_compute; reflexivity|]. split; [lia|].
  exists import_execution_after, import_execution_after, [import_execution_test_root].
  split; [intros key edges Read; discriminate|]. split.
  - apply import_interleaved_stack_here. vm_compute. reflexivity.
  - apply import_interleaved_anchored_take_root.
    apply import_fixed_slice_is_a_valid_interleaved_slice, import_bounded_slice_success_characterization.
    apply import_recovery_read_can_succeed_after_a_prior_failure.
Qed.
