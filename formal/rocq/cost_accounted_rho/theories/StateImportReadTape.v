From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportCodec StateImportCursor StateImportTraversal
  StateImportStack StateImportPage StateImportExport StateImportExecution StateImportWire
  StateImportOverlay StateImportHistoryObservations.
Import ListNotations.

Inductive ImportTapeError : Type :=
| ImportTapeMissing (key : list nat)
| ImportTapeUnexpected (expected actual : list nat)
| ImportTapeRejected (key : list nat) (result : ImportHistoryAccess)
| ImportTapeInvalidStack
| ImportTapeInvalidCursor
| ImportTapeInvalidBudget
| ImportTapeUnprepared
| ImportTapePreparationError (result : ImportHistoryOverlayResult)
| ImportTapeTrailing
| ImportTapeFuelExhausted.

Inductive ImportTapeResult (A : Type) : Type :=
| ImportTapeReturn (value : A) (remaining : list ImportHistoryLogicalRead)
| ImportTapeFail (error : ImportTapeError).
Arguments ImportTapeReturn {A}.
Arguments ImportTapeFail {A}.

Definition import_tape_bind {A B : Type} (result : ImportTapeResult A)
    (next : A -> list ImportHistoryLogicalRead -> ImportTapeResult B) : ImportTapeResult B :=
  match result with
  | ImportTapeReturn value remaining => next value remaining
  | ImportTapeFail error => ImportTapeFail error
  end.

Definition import_consume_history_read (key : list nat) (tape : list ImportHistoryLogicalRead)
    : ImportTapeResult (list ImportWireEdge) :=
  match tape with
  | [] => ImportTapeFail (ImportTapeMissing key)
  | record :: remaining =>
      if list_eq_dec Nat.eq_dec key (import_history_logical_key record) then
        match import_history_logical_result record with
        | ImportHistoryAccessResult (ImportCheckedHistoryFound edges) => ImportTapeReturn edges remaining
        | result => ImportTapeFail (ImportTapeRejected key result)
        end
      else ImportTapeFail (ImportTapeUnexpected key (import_history_logical_key record))
  end.

Definition import_tape_successful_record (record : ImportHistoryLogicalRead) : Prop :=
  exists edges, import_history_logical_result record = ImportHistoryAccessResult (ImportCheckedHistoryFound edges).

Definition import_tape_suffix (tape remaining : list ImportHistoryLogicalRead) : Prop :=
  exists prefix, tape = prefix ++ remaining /\ Forall import_tape_successful_record prefix.

Definition import_tape_reader_agrees (read : ImportHistoryReader) (tape : list ImportHistoryLogicalRead) : Prop :=
  forall record edges, In record tape ->
    import_history_logical_result record = ImportHistoryAccessResult (ImportCheckedHistoryFound edges) ->
    read (import_history_logical_key record) = Some edges.

Theorem import_tape_suffix_refl : forall tape, import_tape_suffix tape tape.
Proof. intros. exists []. split; [reflexivity|constructor]. Qed.

Theorem import_tape_suffix_trans : forall first middle last,
  import_tape_suffix first middle -> import_tape_suffix middle last -> import_tape_suffix first last.
Proof.
  intros first middle last [left [First Left]] [right [Middle Right]]. exists (left ++ right).
  split; [now rewrite First, Middle, app_assoc|now apply Forall_app].
Qed.

Theorem import_tape_suffix_preserves_reader_agreement : forall read tape remaining,
  import_tape_reader_agrees read tape -> import_tape_suffix tape remaining ->
  import_tape_reader_agrees read remaining.
Proof.
  intros read tape remaining Agree [prefix [Same _]] record edges Present Found.
  apply Agree; [rewrite Same; apply in_or_app; now right|exact Found].
Qed.

Theorem import_history_read_consumes_exactly_the_next_record : forall key tape edges remaining,
  import_consume_history_read key tape = ImportTapeReturn edges remaining ->
  exists record, tape = record :: remaining /\ import_history_logical_key record = key /\
    import_history_logical_result record = ImportHistoryAccessResult (ImportCheckedHistoryFound edges).
Proof.
  intros key [|record rest] edges remaining Result; [discriminate|].
  unfold import_consume_history_read in Result.
  destruct (list_eq_dec Nat.eq_dec key (import_history_logical_key record)); [|discriminate].
  destruct (import_history_logical_result record) as [| |answer] eqn:Read; try discriminate.
  destruct answer; try discriminate. inversion Result; subst. exists record. auto.
Qed.

Theorem import_tape_read_refines_the_checked_reader : forall read key tape edges remaining,
  import_tape_reader_agrees read tape ->
  import_consume_history_read key tape = ImportTapeReturn edges remaining ->
  read key = Some edges /\ import_tape_suffix tape remaining.
Proof.
  intros read key tape edges remaining Agree Result.
  destruct (import_history_read_consumes_exactly_the_next_record _ _ _ _ Result)
    as [record [Same [Key Found]]]. split.
  - rewrite <- Key. apply Agree; [rewrite Same; now left|exact Found].
  - exists [record]. split; [exact Same|]. constructor; [now exists edges|constructor].
Qed.

Theorem import_tape_rejection_cannot_be_replaced_by_a_later_result : forall key record rest,
  import_history_logical_key record = key ->
  (forall edges, import_history_logical_result record <>
    ImportHistoryAccessResult (ImportCheckedHistoryFound edges)) ->
  import_consume_history_read key (record :: rest) =
    ImportTapeFail (ImportTapeRejected key (import_history_logical_result record)).
Proof.
  intros key record rest Key Rejected. unfold import_consume_history_read. rewrite Key.
  destruct (list_eq_dec Nat.eq_dec key key); [|contradiction].
  destruct (import_history_logical_result record) as [| |answer]; try reflexivity.
  destruct answer as [edges| | |]; try reflexivity. exfalso. apply (Rejected edges). reflexivity.
Qed.

Fixpoint import_tape_build_stack (fuel : nat) (key context path : list nat)
    (tape : list ImportHistoryLogicalRead) : ImportTapeResult (list ImportTraversalFrame) :=
  match fuel with
  | 0 => ImportTapeFail ImportTapeFuelExhausted
  | S remaining => import_tape_bind (import_consume_history_read key tape) (fun edges rest =>
      match path with
      | [] => ImportTapeReturn [import_frame_at key context edges None] rest
      | slot :: tail => match import_wire_slot_lookup slot edges with
        | None => ImportTapeFail ImportTapeInvalidStack
        | Some edge => if import_wire_header edge <? 128 then ImportTapeFail ImportTapeInvalidStack else
            match import_strip_exact_prefix (import_wire_prefix edge) tail with
            | None => ImportTapeFail ImportTapeInvalidStack
            | Some suffix => import_tape_bind
                (import_tape_build_stack remaining (import_wire_hash edge)
                  (context ++ slot :: import_wire_prefix edge) suffix rest)
                (fun frames final =>
                  ImportTapeReturn (frames ++ [import_frame_at key context edges (Some slot)]) final)
            end
        end
      end)
  end.

Theorem import_tape_stack_refines_exact_stack_evaluation : forall fuel read key context path tape frames remaining,
  import_tape_reader_agrees read tape ->
  import_tape_build_stack fuel key context path tape = ImportTapeReturn frames remaining ->
  import_build_history_stack fuel read key context path = Some frames /\ import_tape_suffix tape remaining.
Proof.
  induction fuel as [|fuel IH]; intros read key context path tape frames remaining Agree Result; [discriminate|].
  cbn [import_tape_build_stack] in Result.
  destruct (import_consume_history_read key tape) as [edges rest|error] eqn:Read;
    cbn [import_tape_bind] in Result; [|discriminate].
  destruct (import_tape_read_refines_the_checked_reader _ _ _ _ _ Agree Read) as [Checked Suffix].
  cbn [import_build_history_stack]. rewrite Checked. destruct path as [|slot tail].
  - inversion Result; subst. auto.
  - destruct (import_wire_slot_lookup slot edges) as [edge|] eqn:Lookup; [|discriminate].
    destruct (import_wire_header edge <? 128) eqn:Kind; [discriminate|].
    destruct (import_strip_exact_prefix (import_wire_prefix edge) tail) as [path'|] eqn:Prefix; [|discriminate].
    destruct (import_tape_build_stack fuel (import_wire_hash edge)
      (context ++ slot :: import_wire_prefix edge) path' rest) as [child final|error] eqn:Child;
      cbn [import_tape_bind] in Result; [|discriminate].
    inversion Result; subst frames remaining.
    destruct (IH read _ _ _ _ _ _ (import_tape_suffix_preserves_reader_agreement _ _ _ Agree Suffix) Child)
      as [Built Last]. split; [now rewrite Built|eapply import_tape_suffix_trans; eauto].
Qed.

Definition import_tape_evaluate_step (frames : list ImportTraversalFrame) (tape : list ImportHistoryLogicalRead)
    : ImportTapeResult ImportStepResult :=
  match frames with
  | [] => ImportTapeReturn ImportStepExhausted tape
  | frame :: rest => match import_frame_next frame with
    | None => ImportTapeReturn (ImportStepProgress None rest) tape
    | Some selected =>
        let parent := import_advance_frame frame (import_next_slot selected) in
        let edge := import_next_edge selected in
        if import_wire_header edge <? 128 then
          ImportTapeReturn (ImportStepProgress (Some (import_leaf_edge_entry edge)) (parent :: rest)) tape
        else import_tape_bind (import_consume_history_read (import_wire_hash edge) tape)
          (fun edges remaining => ImportTapeReturn
            (ImportStepProgress
              (Some (import_history_edge_entry (import_frame_prefix frame) (import_next_slot selected) edge))
              (import_child_frame frame selected edges :: parent :: rest)) remaining)
    end
  end.

Theorem import_tape_step_refines_exact_step_evaluation : forall read frames tape value remaining,
  import_tape_reader_agrees read tape ->
  import_tape_evaluate_step frames tape = ImportTapeReturn value remaining ->
  import_evaluate_step read frames = value /\ import_tape_suffix tape remaining.
Proof.
  intros read [|frame rest] tape value remaining Agree Result.
  - inversion Result; subst. split; [reflexivity|apply import_tape_suffix_refl].
  - unfold import_tape_evaluate_step in Result. unfold import_evaluate_step.
    destruct (import_frame_next frame) as [selected|] eqn:Next.
    + destruct (import_wire_header (import_next_edge selected) <? 128) eqn:Kind.
      * inversion Result; subst. split; [reflexivity|apply import_tape_suffix_refl].
      * destruct (import_consume_history_read (import_wire_hash (import_next_edge selected)) tape)
          as [edges rest'|error] eqn:Read; cbn [import_tape_bind] in Result; [|discriminate].
        inversion Result; subst value remaining.
        destruct (import_tape_read_refines_the_checked_reader _ _ _ _ _ Agree Read) as [Checked Suffix].
        rewrite Checked. auto.
    + inversion Result; subst. split; [reflexivity|apply import_tape_suffix_refl].
Qed.

Definition ImportTapeSlice := (list ImportExportEntry * list ImportTraversalFrame)%type.

Definition ImportTapeCollectedSlice (A : Type) := (list A * list ImportTraversalFrame)%type.

Fixpoint import_tape_collect_slice {A : Type}
    (emit : list ImportTraversalFrame -> option ImportExportEntry -> list A)
    (fuel skip budget : nat) (frames : list ImportTraversalFrame)
    (tape : list ImportHistoryLogicalRead) : ImportTapeResult (ImportTapeCollectedSlice A) :=
  match frames with
  | [] => ImportTapeReturn ([], []) tape
  | _ :: _ => if (skip =? 0) && (budget =? 0) then ImportTapeReturn ([], frames) tape else
      match fuel with
      | 0 => ImportTapeFail ImportTapeFuelExhausted
      | S remaining => import_tape_bind (import_tape_evaluate_step frames tape) (fun step rest =>
          match step with
          | ImportStepExhausted => ImportTapeReturn ([], []) rest
          | ImportStepReadFailure key =>
              ImportTapeFail (ImportTapeRejected key (ImportHistoryAccessResult ImportCheckedHistoryStorageFailure))
          | ImportStepProgress event next => if skip =? 0 then
              import_tape_bind
                (import_tape_collect_slice emit remaining 0 (budget - import_event_history_cost event) next rest)
                (fun result final => ImportTapeReturn (emit frames event ++ fst result, snd result) final)
            else import_tape_collect_slice emit remaining (skip - import_event_history_cost event) budget next rest
          end)
      end
  end.

Definition import_tape_evaluate_slice := import_tape_collect_slice (fun _ => import_event_entries).

Theorem import_tape_slice_refines_exact_slice_evaluation : forall fuel read skip budget frames tape entries final remaining,
  import_tape_reader_agrees read tape ->
  import_tape_evaluate_slice fuel skip budget frames tape = ImportTapeReturn (entries, final) remaining ->
  import_evaluate_slice fuel read skip budget frames = ImportEvaluationSuccess entries final /\
  import_tape_suffix tape remaining.
Proof.
  induction fuel as [|fuel IH]; intros read skip budget [|frame frames] tape entries final remaining Agree Result.
  - inversion Result; subst. split; [reflexivity|apply import_tape_suffix_refl].
  - cbn [import_tape_evaluate_slice import_tape_collect_slice] in Result. cbn [import_evaluate_slice].
    destruct ((skip =? 0) && (budget =? 0)); [|discriminate]. inversion Result; subst.
    split; [reflexivity|apply import_tape_suffix_refl].
  - inversion Result; subst. split; [reflexivity|apply import_tape_suffix_refl].
  - cbn [import_tape_evaluate_slice import_tape_collect_slice] in Result. cbn [import_evaluate_slice].
    destruct ((skip =? 0) && (budget =? 0));
      [inversion Result; subst; split; [reflexivity|apply import_tape_suffix_refl]|].
    destruct (import_tape_evaluate_step (frame :: frames) tape) as [step rest|error] eqn:Step;
      cbn [import_tape_bind] in Result; [|discriminate].
    destruct (import_tape_step_refines_exact_step_evaluation _ _ _ _ _ Agree Step) as [Evaluated Suffix].
    rewrite Evaluated. destruct step as [|event next|key]; [| |discriminate].
    + inversion Result; subst. split; [reflexivity|exact Suffix].
    + destruct (skip =? 0) eqn:Skip.
      * destruct (import_tape_evaluate_slice fuel 0 (budget - import_event_history_cost event) next rest)
          as [[tail last] rest'|error] eqn:Tail; cbn [import_tape_bind fst snd] in Result; [|discriminate].
        inversion Result; subst entries final remaining.
        destruct (IH read _ _ _ _ _ _ _
          (import_tape_suffix_preserves_reader_agreement _ _ _ Agree Suffix) Tail) as [Same Last].
        rewrite Same. split; [reflexivity|eapply import_tape_suffix_trans; eauto].
      * destruct (IH read _ _ _ _ _ _ _
          (import_tape_suffix_preserves_reader_agreement _ _ _ Agree Suffix) Result) as [Same Last].
        split; [exact Same|eapply import_tape_suffix_trans; eauto].
Qed.

Definition import_history_observed_reader (Hash : list nat -> list nat) (rows : list ImportReceivedHistoryRow)
    (observations : ImportHistoryObservations) : ImportHistoryReader :=
  fun key => match import_history_cached_access Hash rows observations key with
    | ImportHistoryAccessResult (ImportCheckedHistoryFound edges) => Some edges
    | _ => None
    end.

Theorem import_physical_history_trace_derives_tape_reader_agreement :
  forall Hash rows initial records logical current observations,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_reader_agrees (import_history_observed_reader Hash rows observations) logical.
Proof.
  intros Hash rows initial records logical current observations Trace record edges Present Found.
  unfold import_history_observed_reader.
  now rewrite (import_history_logical_trace_has_exact_successful_final_view _ _ _ _ _ _ _ _ _ Trace Present Found).
Qed.

Theorem import_prepared_observed_reader_equals_overlay_reader : forall Hash rows observations candidate,
  import_prepare_history_overlay Hash (import_project_history_observations observations) rows =
    ImportHistoryOverlayReady candidate ->
  forall key, import_history_observed_reader Hash rows observations key =
    import_overlay_traversal_reader Hash candidate key.
Proof.
  intros Hash rows observations candidate Prepared key.
  unfold import_history_observed_reader, import_history_cached_access.
  destruct (observations key) as [answer|] eqn:Observed; [now rewrite Prepared|].
  assert (~ In key (map fst rows)) as Unreceived.
  { intros Present. apply in_map_iff in Present as [[row_key bytes] [Key Present]]. cbn [fst] in Key. subst row_key.
    destruct (import_prepared_history_rows_have_successful_compatibility_observations
      _ _ _ _ _ _ Prepared Present); congruence. }
  unfold import_overlay_traversal_reader, import_check_overlay_history_lookup.
  rewrite (import_history_overlay_does_not_change_unreceived_keys _ _ _ _ _ Prepared Unreceived).
  unfold import_project_history_observations. now rewrite Observed.
Qed.

Definition import_tape_build_cursor (cursor : ImportCursor) (tape : list ImportHistoryLogicalRead)
    : ImportTapeResult (list ImportTraversalFrame) :=
  match cursor with
  | ImportCursorStart root => import_tape_build_stack 1 root [] [] tape
  | ImportCursorResume root prefix carrier => import_tape_bind
      (import_tape_build_stack (S (length prefix)) root [] prefix tape)
      (fun frames remaining => match frames with
        | [] => ImportTapeFail ImportTapeInvalidStack
        | top :: _ => if list_eq_dec Nat.eq_dec (import_frame_key top) carrier
            then ImportTapeReturn frames remaining else ImportTapeFail ImportTapeInvalidStack
        end)
  end.

Theorem import_tape_cursor_builds_the_exact_initial_stack : forall read cursor tape frames remaining,
  import_tape_reader_agrees read tape ->
  import_tape_build_cursor cursor tape = ImportTapeReturn frames remaining ->
  import_cursor_initial_stack read cursor frames /\ import_tape_suffix tape remaining.
Proof.
  intros read [root|root prefix carrier] tape frames remaining Agree Result.
  - destruct (import_tape_stack_refines_exact_stack_evaluation _ _ _ _ _ _ _ _ Agree Result)
      as [Stack Suffix]. split; [eapply import_stack_builder_preserves_exact_frames; exact Stack|exact Suffix].
  - cbn [import_tape_build_cursor] in Result.
    destruct (import_tape_build_stack (S (length prefix)) root [] prefix tape)
      as [built rest|error] eqn:Built; cbn [import_tape_bind] in Result; [|discriminate].
    destruct built as [|top tail]; [discriminate|].
    destruct (list_eq_dec Nat.eq_dec (import_frame_key top) carrier) as [Carrier|Different]; [|discriminate].
    inversion Result; subst frames remaining.
    destruct (import_tape_stack_refines_exact_stack_evaluation _ _ _ _ _ _ _ _ Agree Built)
      as [Stack Suffix]. split; [|exact Suffix]. split.
    + eapply import_stack_builder_preserves_exact_frames. exact Stack.
    + exists top, tail. auto.
Qed.

Definition import_tape_run_slice (skip budget : nat) (frames : list ImportTraversalFrame)
    (tape : list ImportHistoryLogicalRead) : ImportTapeResult ImportTapeSlice :=
  import_tape_evaluate_slice (S (import_export_potential skip budget frames)) skip budget frames tape.

Definition import_tape_collect_run_slice {A : Type}
    (emit : list ImportTraversalFrame -> option ImportExportEntry -> list A)
    (skip budget : nat) (frames : list ImportTraversalFrame) (tape : list ImportHistoryLogicalRead)
    : ImportTapeResult (ImportTapeCollectedSlice A) :=
  import_tape_collect_slice emit (S (import_export_potential skip budget frames)) skip budget frames tape.

Definition import_tape_collect_anchored_export {A : Type}
    (emit : list ImportTraversalFrame -> option ImportExportEntry -> list A) (emit_root : list nat -> A)
    (anchor : option (list nat)) (skip budget : nat)
    (frames : list ImportTraversalFrame) (tape : list ImportHistoryLogicalRead)
    : ImportTapeResult (ImportTapeCollectedSlice A) :=
  match anchor with
  | None => import_tape_collect_run_slice emit skip budget frames tape
  | Some key => match skip with
    | S rest => import_tape_collect_run_slice emit rest budget frames tape
    | 0 => match budget with
      | 0 => ImportTapeFail ImportTapeInvalidBudget
      | S rest => import_tape_bind (import_tape_collect_run_slice emit 0 rest frames tape)
          (fun result remaining => ImportTapeReturn
            (emit_root key :: fst result, snd result) remaining)
      end
    end
  end.

Definition import_tape_anchored_export := import_tape_collect_anchored_export
  (fun _ => import_event_entries) (fun key => ImportExportHistory (import_key_to_nat key) []).

Theorem import_tape_run_slice_is_operational : forall read skip budget frames tape entries final remaining,
  import_tape_reader_agrees read tape ->
  import_tape_run_slice skip budget frames tape = ImportTapeReturn (entries, final) remaining ->
  import_export_slice read skip budget frames entries final /\ import_tape_suffix tape remaining.
Proof.
  intros read skip budget frames tape entries final remaining Agree Result.
  destruct (import_tape_slice_refines_exact_slice_evaluation _ _ _ _ _ _ _ _ _ Agree Result)
    as [Slice Suffix]. split; [eapply import_successful_slice_evaluation_is_operational; exact Slice|exact Suffix].
Qed.

Theorem import_tape_anchor_preserves_skip_and_budget : forall read anchor skip budget frames tape entries final remaining,
  import_tape_reader_agrees read tape ->
  import_tape_anchored_export anchor skip budget frames tape = ImportTapeReturn (entries, final) remaining ->
  import_anchored_export read anchor skip budget frames entries final /\ import_tape_suffix tape remaining.
Proof.
  intros read [key|] [|skip] budget frames tape entries final remaining Agree Result.
  - destruct budget as [|budget]; [discriminate|].
    cbn [import_tape_anchored_export import_tape_collect_anchored_export] in Result.
    change (import_tape_bind (import_tape_run_slice 0 budget frames tape)
      (fun result rest => ImportTapeReturn
        (ImportExportHistory (import_key_to_nat key) [] :: fst result, snd result) rest) =
      ImportTapeReturn (entries, final) remaining) in Result.
    destruct (import_tape_run_slice 0 budget frames tape) as [[tail last] rest|error] eqn:Slice;
      cbn [import_tape_bind fst snd] in Result; [|discriminate]. inversion Result; subst entries final remaining.
    destruct (import_tape_run_slice_is_operational _ _ _ _ _ _ _ _ Agree Slice) as [Export Suffix].
    split; [apply import_anchored_take_root; now apply import_zero_skip_slice_is_a_page|exact Suffix].
  - destruct (import_tape_run_slice_is_operational _ _ _ _ _ _ _ _ Agree Result) as [Export Suffix].
    split; [now apply import_anchored_skip_root|exact Suffix].
  - destruct (import_tape_run_slice_is_operational _ _ _ _ _ _ _ _ Agree Result) as [Export Suffix].
    split; [now apply import_anchored_resume|exact Suffix].
  - destruct (import_tape_run_slice_is_operational _ _ _ _ _ _ _ _ Agree Result) as [Export Suffix].
    split; [now apply import_anchored_resume|exact Suffix].
Qed.

Definition import_tape_collect_cursor {A : Type}
    (emit : list ImportTraversalFrame -> option ImportExportEntry -> list A) (emit_root : list nat -> A)
    (cursor : ImportCursor) (skip budget : nat) (tape : list ImportHistoryLogicalRead)
    : ImportTapeResult (ImportTapeCollectedSlice A) :=
  if import_cursor_validb cursor then
    if (skip =? 0) && (budget =? 0) then ImportTapeFail ImportTapeInvalidBudget else
      import_tape_bind (import_consume_history_read (import_cursor_root cursor) tape) (fun _ after_probe =>
        import_tape_bind (import_tape_build_cursor cursor after_probe) (fun frames after_stack =>
          import_tape_collect_anchored_export emit emit_root (import_cursor_anchor cursor) skip budget frames after_stack))
  else ImportTapeFail ImportTapeInvalidCursor.

Definition import_tape_evaluate_cursor := import_tape_collect_cursor
  (fun _ => import_event_entries) (fun key => ImportExportHistory (import_key_to_nat key) []).

Definition import_tape_erase_collection {A : Type} (erase : A -> ImportExportEntry)
    (result : ImportTapeResult (ImportTapeCollectedSlice A)) : ImportTapeResult ImportTapeSlice :=
  match result with
  | ImportTapeReturn (entries, frames) remaining => ImportTapeReturn (map erase entries, frames) remaining
  | ImportTapeFail error => ImportTapeFail error
  end.

Theorem import_collector_erasure_preserves_slice_errors_and_reads : forall A
  (emit : list ImportTraversalFrame -> option ImportExportEntry -> list A) erase,
  (forall frames event, map erase (emit frames event) = import_event_entries event) ->
  forall fuel skip budget frames tape,
  import_tape_erase_collection erase (import_tape_collect_slice emit fuel skip budget frames tape) =
    import_tape_evaluate_slice fuel skip budget frames tape.
Proof.
  intros A emit erase Emit fuel. induction fuel as [|fuel IH]; intros skip budget [|frame frames] tape;
    cbn [import_tape_collect_slice import_tape_evaluate_slice import_tape_erase_collection]; try reflexivity.
  - now destruct ((skip =? 0) && (budget =? 0)).
  - destruct ((skip =? 0) && (budget =? 0)); [reflexivity|].
    destruct (import_tape_evaluate_step (frame :: frames) tape) as [[|event next|key] rest|error];
      cbn [import_tape_bind import_tape_erase_collection]; try reflexivity.
    destruct (skip =? 0).
    + pose proof (IH 0 (budget - import_event_history_cost event) next rest) as Tail.
      destruct (import_tape_collect_slice emit fuel 0 (budget - import_event_history_cost event) next rest)
        as [[entries final] remaining|failure]; cbn [import_tape_erase_collection] in Tail;
        rewrite <- Tail; cbn [import_tape_bind import_tape_erase_collection fst snd]; [|reflexivity].
      now rewrite map_app, Emit.
    + apply IH.
Qed.

Theorem import_collector_erasure_preserves_anchor_errors_and_reads : forall A
  (emit : list ImportTraversalFrame -> option ImportExportEntry -> list A) emit_root erase,
  (forall frames event, map erase (emit frames event) = import_event_entries event) ->
  (forall key, erase (emit_root key) = ImportExportHistory (import_key_to_nat key) []) ->
  forall anchor skip budget frames tape,
  import_tape_erase_collection erase
    (import_tape_collect_anchored_export emit emit_root anchor skip budget frames tape) =
    import_tape_anchored_export anchor skip budget frames tape.
Proof.
  intros A emit emit_root erase Emit Root [key|] [|skip] budget frames tape;
    cbn [import_tape_collect_anchored_export import_tape_anchored_export].
  - destruct budget as [|budget]; [reflexivity|].
    pose proof (import_collector_erasure_preserves_slice_errors_and_reads _ _ _ Emit
      (S (import_export_potential 0 budget frames)) 0 budget frames tape) as Tail.
    change (import_tape_erase_collection erase (import_tape_collect_run_slice emit 0 budget frames tape) =
      import_tape_run_slice 0 budget frames tape) in Tail.
    change (import_tape_erase_collection erase
      (import_tape_bind (import_tape_collect_run_slice emit 0 budget frames tape)
        (fun result rest => ImportTapeReturn (emit_root key :: fst result, snd result) rest)) =
      import_tape_bind (import_tape_run_slice 0 budget frames tape)
        (fun result rest => ImportTapeReturn
          (ImportExportHistory (import_key_to_nat key) [] :: fst result, snd result) rest)).
    destruct (import_tape_collect_run_slice emit 0 budget frames tape) as [[entries final] remaining|error];
      cbn [import_tape_erase_collection] in Tail;
      rewrite <- Tail; cbn [import_tape_bind import_tape_erase_collection fst snd map]; [now rewrite Root|reflexivity].
  - apply import_collector_erasure_preserves_slice_errors_and_reads. exact Emit.
  - apply import_collector_erasure_preserves_slice_errors_and_reads. exact Emit.
  - apply import_collector_erasure_preserves_slice_errors_and_reads. exact Emit.
Qed.

Theorem import_collector_erasure_preserves_cursor_errors_and_reads : forall A
  (emit : list ImportTraversalFrame -> option ImportExportEntry -> list A) emit_root erase,
  (forall frames event, map erase (emit frames event) = import_event_entries event) ->
  (forall key, erase (emit_root key) = ImportExportHistory (import_key_to_nat key) []) ->
  forall cursor skip budget tape,
  import_tape_erase_collection erase (import_tape_collect_cursor emit emit_root cursor skip budget tape) =
    import_tape_evaluate_cursor cursor skip budget tape.
Proof.
  intros A emit emit_root erase Emit Root cursor skip budget tape.
  unfold import_tape_evaluate_cursor, import_tape_collect_cursor.
  destruct (import_cursor_validb cursor); [|reflexivity].
  destruct ((skip =? 0) && (budget =? 0)); [reflexivity|].
  destruct (import_consume_history_read (import_cursor_root cursor) tape) as [edges rest|error];
    cbn [import_tape_bind import_tape_erase_collection]; [|reflexivity].
  destruct (import_tape_build_cursor cursor rest) as [frames remaining|error];
    cbn [import_tape_bind import_tape_erase_collection]; [|reflexivity].
  now apply import_collector_erasure_preserves_anchor_errors_and_reads.
Qed.

Theorem import_tape_cursor_refines_operational_export : forall read cursor skip budget tape entries final remaining,
  import_tape_reader_agrees read tape ->
  import_tape_evaluate_cursor cursor skip budget tape = ImportTapeReturn (entries, final) remaining ->
  import_export_from_cursor read cursor skip budget entries final /\ import_tape_suffix tape remaining.
Proof.
  intros read cursor skip budget tape entries final remaining Agree Result.
  unfold import_tape_evaluate_cursor, import_tape_collect_cursor in Result.
  destruct (import_cursor_validb cursor) eqn:Valid; [|discriminate].
  destruct ((skip =? 0) && (budget =? 0)) eqn:Budget; [discriminate|].
  destruct (import_consume_history_read (import_cursor_root cursor) tape)
    as [root_edges after_probe|error] eqn:Probe; cbn [import_tape_bind] in Result; [|discriminate].
  destruct (import_tape_read_refines_the_checked_reader _ _ _ _ _ Agree Probe) as [Root ProbeSuffix].
  pose proof (import_tape_suffix_preserves_reader_agreement _ _ _ Agree ProbeSuffix) as ProbeAgree.
  destruct (import_tape_build_cursor cursor after_probe) as [frames after_stack|error] eqn:Stack;
    cbn [import_tape_bind] in Result; [|discriminate].
  destruct (import_tape_cursor_builds_the_exact_initial_stack _ _ _ _ _ ProbeAgree Stack)
    as [Initial StackSuffix].
  pose proof (import_tape_suffix_preserves_reader_agreement _ _ _ ProbeAgree StackSuffix) as StackAgree.
  destruct (import_tape_anchor_preserves_skip_and_budget _ _ _ _ _ _ _ _ _ StackAgree Result)
    as [Export FinalSuffix]. split.
  - split; [exact Valid|]. split.
    + apply andb_false_iff in Budget. destruct Budget as [Skip|Take]; apply Nat.eqb_neq in Skip || apply Nat.eqb_neq in Take; lia.
    + exists frames. auto.
  - eapply import_tape_suffix_trans; [exact ProbeSuffix|]. eapply import_tape_suffix_trans; eauto.
Qed.

Definition import_tape_prepared_complete_attempt {A : Type}
    (Hash : list nat -> list nat) (rows : list ImportReceivedHistoryRow)
    (evaluate : list ImportHistoryLogicalRead -> ImportTapeResult A) (tape : list ImportHistoryLogicalRead)
    : ImportTapeResult A :=
  match tape with
  | [] => ImportTapeFail ImportTapeUnprepared
  | first :: _ => match import_prepare_history_overlay Hash
        (import_project_history_observations (import_history_logical_cache first)) rows with
    | ImportHistoryOverlayReady _ => import_tape_bind (evaluate tape)
        (fun result remaining => match remaining with
          | [] => ImportTapeReturn result []
          | _ :: _ => ImportTapeFail ImportTapeTrailing
          end)
    | result => ImportTapeFail (ImportTapePreparationError result)
    end
  end.

Theorem import_prepared_complete_attempt_consumes_every_record :
  forall A Hash rows evaluate tape (value : A) remaining,
  import_tape_prepared_complete_attempt Hash rows evaluate tape = ImportTapeReturn value remaining ->
  remaining = [] /\ evaluate tape = ImportTapeReturn value [] /\
  exists first rest candidate, tape = first :: rest /\
    import_prepare_history_overlay Hash (import_project_history_observations (import_history_logical_cache first)) rows =
      ImportHistoryOverlayReady candidate.
Proof.
  intros A Hash rows evaluate [|first rest] value remaining Result; [discriminate|].
  unfold import_tape_prepared_complete_attempt in Result.
  destruct (import_prepare_history_overlay Hash
    (import_project_history_observations (import_history_logical_cache first)) rows) eqn:Prepared; try discriminate.
  destruct (evaluate (first :: rest)) as [returned suffix|error] eqn:Evaluated;
    cbn [import_tape_bind] in Result; [|discriminate].
  destruct suffix; [|discriminate]. inversion Result; subst returned remaining.
  split; [reflexivity|]. split; [reflexivity|]. exists first, rest, candidate. auto.
Qed.

Definition import_tape_checked_attempt (Hash : list nat -> list nat) (rows : list ImportReceivedHistoryRow)
    (cursor : ImportCursor) (skip budget : nat) (tape : list ImportHistoryLogicalRead)
    : ImportTapeResult ImportTapeSlice :=
  import_tape_prepared_complete_attempt Hash rows (import_tape_evaluate_cursor cursor skip budget) tape.

Theorem import_checked_tape_attempt_consumes_the_complete_invocation :
  forall Hash rows cursor skip budget tape entries final remaining,
  import_tape_checked_attempt Hash rows cursor skip budget tape = ImportTapeReturn (entries, final) remaining ->
  remaining = [] /\
  import_tape_evaluate_cursor cursor skip budget tape = ImportTapeReturn (entries, final) [] /\
  exists first rest candidate, tape = first :: rest /\
    import_prepare_history_overlay Hash (import_project_history_observations (import_history_logical_cache first)) rows =
      ImportHistoryOverlayReady candidate.
Proof.
  intros. eapply import_prepared_complete_attempt_consumes_every_record; eauto.
Qed.

Theorem import_history_first_callback_has_an_actual_preparation_prefix :
  forall Hash rows initial records logical current observations,
  import_history_observation_trace Hash rows initial records logical current observations ->
  forall first rest, logical = first :: rest -> exists prefix suffix before,
    records = prefix ++ suffix /\
    import_history_observation_trace Hash rows initial prefix [] before (import_history_logical_cache first) /\
    import_history_store_extension before current.
Proof.
  intros Hash rows initial records logical current observations Trace. induction Trace;
    intros first rest Same; try discriminate.
  - destruct (IHTrace _ _ Same) as [prefix [suffix [before [Records [Prefix Extension]]]]].
    eexists prefix, (suffix ++ [_]), before. split; [now rewrite Records, app_assoc|auto].
  - destruct (IHTrace _ _ Same) as [prefix [suffix [before [Records [Prefix Extension]]]]].
    eexists prefix, (suffix ++ [_]), before. split; [now rewrite Records, app_assoc|auto].
  - destruct (IHTrace _ _ Same) as [prefix [suffix [before [Records [Prefix Extension]]]]].
    exists prefix, suffix, before. split; [exact Records|]. split; [exact Prefix|].
    intros key bytes Read. apply H. now apply Extension.
  - destruct logical as [|head tail].
    + cbn in Same. inversion Same; subst first rest. exists records, [], store.
      split; [now rewrite app_nil_r|]. split; [exact Trace|intros query bytes Read; exact Read].
    + cbn in Same. inversion Same; subst head. apply IHTrace with (rest := tail). reflexivity.
Qed.

Theorem import_checked_actual_tape_has_preparation_and_exact_export :
  forall Hash rows initial records logical current observations cursor skip budget entries final,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_attempt Hash rows cursor skip budget logical = ImportTapeReturn (entries, final) [] ->
  import_export_from_cursor (import_history_observed_reader Hash rows observations) cursor skip budget entries final /\
  exists first rest candidate prefix suffix before,
    logical = first :: rest /\
    records = prefix ++ suffix /\
    import_history_observation_trace Hash rows initial prefix [] before (import_history_logical_cache first) /\
    import_history_store_extension before current /\
    import_prepare_history_overlay Hash (import_project_history_observations (import_history_logical_cache first)) rows =
      ImportHistoryOverlayReady candidate.
Proof.
  intros Hash rows initial records logical current observations cursor skip budget entries final Trace Accepted.
  destruct (import_checked_tape_attempt_consumes_the_complete_invocation _ _ _ _ _ _ _ _ _ Accepted)
    as [_ [Evaluated [first [rest [candidate [Same Prepared]]]]]]. split.
  - exact (proj1 (import_tape_cursor_refines_operational_export _ _ _ _ _ _ _ _
      (import_physical_history_trace_derives_tape_reader_agreement _ _ _ _ _ _ _ Trace) Evaluated)).
  - destruct (import_history_first_callback_has_an_actual_preparation_prefix _ _ _ _ _ _ _ Trace _ _ Same)
      as [prefix [suffix [before [Records [Prefix Extension]]]]].
    exists first, rest, candidate, prefix, suffix, before. auto.
Qed.

Theorem import_checked_actual_tape_preserves_history_budget :
  forall Hash rows initial records logical current observations cursor skip budget entries final,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_attempt Hash rows cursor skip budget logical = ImportTapeReturn (entries, final) [] ->
  length (import_export_history_keys entries) <= budget.
Proof.
  intros Hash rows initial records logical current observations cursor skip budget entries final Trace Accepted.
  destruct (import_checked_actual_tape_has_preparation_and_exact_export _ _ _ _ _ _ _ _ _ _ _ _ Trace Accepted)
    as [Export _]. eapply import_cursor_export_respects_history_budget. exact Export.
Qed.

Theorem import_checked_actual_tape_cannot_hide_an_unsuccessful_callback :
  forall Hash rows initial records logical current observations cursor skip budget entries final,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_attempt Hash rows cursor skip budget logical = ImportTapeReturn (entries, final) [] ->
  Forall import_tape_successful_record logical.
Proof.
  intros Hash rows initial records logical current observations cursor skip budget entries final Trace Accepted.
  destruct (import_checked_tape_attempt_consumes_the_complete_invocation _ _ _ _ _ _ _ _ _ Accepted)
    as [_ [Evaluated _]].
  destruct (import_tape_cursor_refines_operational_export _ _ _ _ _ _ _ _
      (import_physical_history_trace_derives_tape_reader_agreement _ _ _ _ _ _ _ Trace) Evaluated)
    as [_ [prefix [Same Success]]]. rewrite app_nil_r in Same. now subst logical.
Qed.

Theorem import_checked_actual_tape_derives_its_final_checked_overlay :
  forall Hash rows initial records logical current observations cursor skip budget entries final,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_attempt Hash rows cursor skip budget logical = ImportTapeReturn (entries, final) [] ->
  exists candidate,
    import_prepare_history_overlay Hash (import_project_history_observations observations) rows =
      ImportHistoryOverlayReady candidate /\
    (forall key, import_history_observed_reader Hash rows observations key =
      import_overlay_traversal_reader Hash candidate key) /\
    import_export_from_cursor (import_overlay_traversal_reader Hash candidate) cursor skip budget entries final.
Proof.
  intros Hash rows initial records logical current observations cursor skip budget entries final Trace Accepted.
  destruct (import_checked_tape_attempt_consumes_the_complete_invocation _ _ _ _ _ _ _ _ _ Accepted)
    as [_ [Evaluated [first [rest [prepared [Same Prepared]]]]]].
  pose proof (import_history_trace_records_each_logical_lookup _ _ _ _ _ _ _ Trace) as Logical.
  unfold import_history_logical_records_valid in Logical. rewrite Forall_forall in Logical.
  assert (In first logical) as Present by (rewrite Same; now left).
  destruct (Logical _ Present) as [_ Extension].
  destruct (import_retained_history_observations_preserve_prepared_overlays _ _ _ _ _ Extension Prepared)
    as [candidate [FinalPrepared _]].
  pose proof (import_prepared_observed_reader_equals_overlay_reader _ _ _ _ FinalPrepared) as Equal.
  exists candidate. split; [exact FinalPrepared|]. split; [exact Equal|].
  assert (import_tape_reader_agrees (import_overlay_traversal_reader Hash candidate) logical) as Agree.
  { intros record edges Recorded Found. rewrite <- Equal.
    exact (import_physical_history_trace_derives_tape_reader_agreement _ _ _ _ _ _ _ Trace record edges Recorded Found). }
  exact (proj1 (import_tape_cursor_refines_operational_export _ _ _ _ _ _ _ _ Agree Evaluated)).
Qed.

Theorem import_actual_history_commit_derives_tape_reader_agreement :
  forall Hash rows initial records logical current observations storage_ok result,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  import_tape_reader_agrees (import_checked_wire_history Hash result) logical.
Proof.
  intros Hash rows initial records logical current observations storage_ok result Trace Commit event edges Present Success.
  eapply import_history_commit_preserves_every_successful_callback; eauto.
Qed.

Theorem import_accepted_page_after_history_commit_has_the_same_operational_export :
  forall Hash rows initial records logical current observations cursor skip budget entries final storage_ok result,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_attempt Hash rows cursor skip budget logical = ImportTapeReturn (entries, final) [] ->
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  import_export_from_cursor (import_checked_wire_history Hash result) cursor skip budget entries final /\
    Forall import_tape_successful_record logical /\ length (import_export_history_keys entries) <= budget.
Proof.
  intros Hash rows initial records logical current observations cursor skip budget entries final storage_ok result
    Trace Accepted Commit.
  destruct (import_checked_tape_attempt_consumes_the_complete_invocation _ _ _ _ _ _ _ _ _ Accepted)
    as [_ [Evaluated _]].
  pose proof (import_actual_history_commit_derives_tape_reader_agreement _ _ _ _ _ _ _ _ _ Trace Commit) as Agree.
  split; [exact (proj1 (import_tape_cursor_refines_operational_export _ _ _ _ _ _ _ _ Agree Evaluated))|].
  split.
  - eapply import_checked_actual_tape_cannot_hide_an_unsuccessful_callback; eauto.
  - eapply import_checked_actual_tape_preserves_history_budget; eauto.
Qed.

Theorem import_tape_stack_never_exhausts_path_derived_fuel : forall fuel key context path tape,
  length path < fuel ->
  import_tape_build_stack fuel key context path tape <> ImportTapeFail ImportTapeFuelExhausted.
Proof.
  induction fuel as [|fuel IH]; intros key context path tape Bound; [lia|].
  cbn [import_tape_build_stack].
  destruct (import_consume_history_read key tape) as [edges rest|error] eqn:Read; cbn [import_tape_bind].
  - destruct path as [|slot tail]; [discriminate|].
    destruct (import_wire_slot_lookup slot edges) as [edge|]; [|discriminate].
    destruct (import_wire_header edge <? 128); [discriminate|].
    destruct (import_strip_exact_prefix (import_wire_prefix edge) tail) as [suffix|] eqn:Prefix; [|discriminate].
    apply import_prefix_strip_is_exact in Prefix. subst tail. cbn [length] in Bound. rewrite length_app in Bound.
    assert (length suffix < fuel) as Smaller by lia.
    specialize (IH (import_wire_hash edge) (context ++ slot :: import_wire_prefix edge) suffix rest Smaller).
    destruct (import_tape_build_stack fuel (import_wire_hash edge)
      (context ++ slot :: import_wire_prefix edge) suffix rest);
      cbn [import_tape_bind]; congruence.
  - unfold import_consume_history_read in Read. destruct tape as [|record tail]; [inversion Read; discriminate|].
    destruct (list_eq_dec Nat.eq_dec key (import_history_logical_key record)); [|inversion Read; discriminate].
    destruct (import_history_logical_result record) as [| |answer]; try (inversion Read; discriminate).
    destruct answer; inversion Read; discriminate.
Qed.

Theorem import_tape_step_cannot_exhaust_fuel : forall frames tape,
  import_tape_evaluate_step frames tape <> ImportTapeFail ImportTapeFuelExhausted.
Proof.
  intros [|frame rest] tape; [discriminate|]. unfold import_tape_evaluate_step.
  destruct (import_frame_next frame) as [selected|]; [|discriminate].
  destruct (import_wire_header (import_next_edge selected) <? 128); [discriminate|].
  unfold import_consume_history_read. destruct tape as [|record tail]; [discriminate|].
  destruct (list_eq_dec Nat.eq_dec (import_wire_hash (import_next_edge selected)) (import_history_logical_key record));
    [|discriminate]. destruct (import_history_logical_result record) as [| |answer]; try discriminate.
  destruct answer; discriminate.
Qed.

Theorem import_tape_step_never_returns_an_unclassified_read_failure : forall frames tape key remaining,
  import_tape_evaluate_step frames tape <> ImportTapeReturn (ImportStepReadFailure key) remaining.
Proof.
  intros [|frame rest] tape key remaining; [discriminate|]. unfold import_tape_evaluate_step.
  destruct (import_frame_next frame) as [selected|]; [|discriminate].
  destruct (import_wire_header (import_next_edge selected) <? 128); [discriminate|].
  destruct (import_consume_history_read (import_wire_hash (import_next_edge selected)) tape);
    cbn [import_tape_bind]; discriminate.
Qed.

Theorem import_tape_slice_never_exhausts_derived_fuel : forall fuel read skip budget frames tape,
  import_tape_reader_agrees read tape -> import_export_potential skip budget frames < fuel ->
  import_tape_evaluate_slice fuel skip budget frames tape <> ImportTapeFail ImportTapeFuelExhausted.
Proof.
  induction fuel as [|fuel IH]; intros read skip budget [|frame frames] tape Agree Bound; try lia.
  - discriminate.
  - cbn [import_tape_evaluate_slice import_tape_collect_slice].
    destruct ((skip =? 0) && (budget =? 0)) eqn:Stop; [discriminate|].
    destruct (import_tape_evaluate_step (frame :: frames) tape) as [step rest|error] eqn:Step;
      cbn [import_tape_bind].
    + destruct (import_tape_step_refines_exact_step_evaluation _ _ _ _ _ Agree Step) as [Evaluated Suffix].
      destruct step as [|event next|key]; try discriminate.
      apply import_evaluated_step_is_operational in Evaluated.
      pose proof (import_tape_suffix_preserves_reader_agreement _ _ _ Agree Suffix) as RestAgree.
      destruct (skip =? 0) eqn:Skipping.
      * apply Nat.eqb_eq in Skipping. subst skip.
        assert (0 < budget) as Positive by (destruct budget; [discriminate|lia]).
        pose proof (import_taking_step_strictly_reduces_export_potential _ _ _ _ _ Positive Evaluated) as Decrease.
        assert (import_export_potential 0 (budget - import_event_history_cost event) next < fuel) as Smaller by lia.
        specialize (IH read 0 (budget - import_event_history_cost event) next rest RestAgree Smaller).
        destruct (import_tape_evaluate_slice fuel 0 (budget - import_event_history_cost event) next rest)
          as [[entries final] remaining|error]; cbn [import_tape_bind]; congruence.
      * apply Nat.eqb_neq in Skipping. assert (0 < skip) as Positive by lia.
        pose proof (import_skipping_step_strictly_reduces_export_potential
          read skip budget (frame :: frames) event next Positive Evaluated) as Decrease.
        apply (IH read); [exact RestAgree|lia].
    + pose proof (import_tape_step_cannot_exhaust_fuel (frame :: frames) tape). congruence.
Qed.

Definition import_tape_example_record (key : list nat) (result : ImportHistoryAccess) : ImportHistoryLogicalRead :=
  {| import_history_logical_key := key; import_history_logical_cache := import_empty_history_observations;
     import_history_logical_result := result |}.

Example import_tape_failed_read_cannot_skip_to_a_success :
  import_consume_history_read [1]
    [import_tape_example_record [1] (ImportHistoryAccessResult ImportCheckedHistoryStorageFailure);
     import_tape_example_record [2] (ImportHistoryAccessResult (ImportCheckedHistoryFound []))] =
  ImportTapeFail (ImportTapeRejected [1] (ImportHistoryAccessResult ImportCheckedHistoryStorageFailure)).
Proof. reflexivity. Qed.

Example import_tape_unrecorded_ancestor_is_not_a_lookup :
  import_tape_build_stack 1 [1] [] [] [] = ImportTapeFail (ImportTapeMissing [1]).
Proof. reflexivity. Qed.

Example import_tape_misordered_reads_reject :
  import_consume_history_read [1]
    [import_tape_example_record [2] (ImportHistoryAccessResult (ImportCheckedHistoryFound []));
     import_tape_example_record [1] (ImportHistoryAccessResult (ImportCheckedHistoryFound []))] =
  ImportTapeFail (ImportTapeUnexpected [1] [2]).
Proof. reflexivity. Qed.

Example import_tape_unfinished_observation_is_not_a_success :
  import_consume_history_read [1] [import_tape_example_record [1] ImportHistoryNeedObservation] =
  ImportTapeFail (ImportTapeRejected [1] ImportHistoryNeedObservation).
Proof. reflexivity. Qed.

Example import_tape_two_root_callbacks_share_one_actual_physical_read :
  import_history_observation_trace import_history_demo_hash [] import_history_demo_store
    [import_history_demo_read] [import_history_demo_logical; import_history_demo_logical]
    import_history_demo_store import_history_demo_observations /\
  import_tape_checked_attempt import_history_demo_hash [] (ImportCursorStart import_history_demo_key) 0 1
    [import_history_demo_logical; import_history_demo_logical] =
    ImportTapeReturn ([ImportExportHistory (import_key_to_nat import_history_demo_key) []],
      [import_frame_at import_history_demo_key [] [] None]) [].
Proof.
  split; [exact import_two_history_callbacks_reuse_one_physical_read|]. vm_compute. reflexivity.
Qed.

Example import_tape_one_root_callback_cannot_supply_root_setup_and_stack :
  import_tape_checked_attempt import_history_demo_hash [] (ImportCursorStart import_history_demo_key) 0 1
    [import_history_demo_logical] = ImportTapeFail (ImportTapeMissing import_history_demo_key).
Proof. vm_compute. reflexivity. Qed.

Example import_tape_extra_root_callback_rejects_after_the_budget :
  import_tape_checked_attempt import_history_demo_hash [] (ImportCursorStart import_history_demo_key) 0 1
    [import_history_demo_logical; import_history_demo_logical; import_history_demo_logical] =
    ImportTapeFail ImportTapeTrailing.
Proof. vm_compute. reflexivity. Qed.

Example import_tape_resume_checks_the_exact_carrier :
  import_tape_checked_attempt import_history_demo_hash []
    (ImportCursorResume import_history_demo_key [] (repeat 1 32)) 0 1
    [import_history_demo_logical; import_history_demo_logical] = ImportTapeFail ImportTapeInvalidStack.
Proof. vm_compute. reflexivity. Qed.

Example import_tape_root_absence_is_not_an_accepted_empty_page :
  let record := import_tape_example_record import_history_demo_key (ImportHistoryAccessResult ImportCheckedHistoryAbsent) in
  import_tape_checked_attempt import_history_demo_hash [] (ImportCursorStart import_history_demo_key) 0 1 [record] =
    ImportTapeFail (ImportTapeRejected import_history_demo_key (ImportHistoryAccessResult ImportCheckedHistoryAbsent)).
Proof. vm_compute. reflexivity. Qed.

Example import_tape_preparation_cannot_ignore_an_unread_received_row :
  import_tape_checked_attempt import_history_demo_hash [(import_history_demo_key, [])]
    (ImportCursorStart import_history_demo_key) 0 1
    [import_tape_example_record import_history_demo_key (ImportHistoryAccessResult (ImportCheckedHistoryFound []))] =
    ImportTapeFail (ImportTapePreparationError (ImportHistoryOverlayStorageFailure import_history_demo_key)).
Proof. vm_compute. reflexivity. Qed.
