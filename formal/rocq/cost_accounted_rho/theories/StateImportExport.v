From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportCodec StateImportCursor StateImportTraversal StateImportStack StateImportPage.
Import ListNotations.

Definition import_edge_absolute_prefix (context : list nat) (slot : nat) (edge : ImportWireEdge) :=
  context ++ slot :: import_wire_prefix edge.

Definition import_history_edge_entry (context : list nat) (slot : nat) (edge : ImportWireEdge) :=
  ImportExportHistory (import_key_to_nat (import_wire_hash edge))
    (import_edge_absolute_prefix context slot edge).

Definition import_leaf_edge_entry (edge : ImportWireEdge) :=
  ImportExportLeaf (import_key_to_nat (import_wire_hash edge)).

Inductive import_slots_trace (read : ImportHistoryReader) :
    list nat -> list ImportWireEdge -> list nat -> list ImportExportEntry -> Prop :=
| import_slots_trace_end : forall context edges,
    import_slots_trace read context edges [] []
| import_slots_trace_empty : forall context edges slot slots trace,
    import_wire_slot_lookup slot edges = None ->
    import_slots_trace read context edges slots trace ->
    import_slots_trace read context edges (slot :: slots) trace
| import_slots_trace_leaf : forall context edges slot slots edge trace,
    import_wire_slot_lookup slot edges = Some edge ->
    (import_wire_header edge <? 128) = true ->
    import_slots_trace read context edges slots trace ->
    import_slots_trace read context edges (slot :: slots) (import_leaf_edge_entry edge :: trace)
| import_slots_trace_history : forall context edges slot slots edge child_edges child_trace rest_trace,
    import_wire_slot_lookup slot edges = Some edge ->
    (import_wire_header edge <? 128) = false ->
    read (import_wire_hash edge) = Some child_edges ->
    import_slots_trace read (import_edge_absolute_prefix context slot edge) child_edges (seq 0 256) child_trace ->
    import_slots_trace read context edges slots rest_trace ->
    import_slots_trace read context edges (slot :: slots)
      (import_history_edge_entry context slot edge :: child_trace ++ rest_trace).

Definition import_frame_trace (read : ImportHistoryReader) (frame : ImportTraversalFrame)
    (trace : list ImportExportEntry) : Prop :=
  import_slots_trace read (import_frame_prefix frame) (import_frame_edges frame)
    (import_frame_pending_slots frame) trace.

Inductive import_stack_trace (read : ImportHistoryReader) :
    list ImportTraversalFrame -> list ImportExportEntry -> Prop :=
| import_stack_trace_end : import_stack_trace read [] []
| import_stack_trace_frame : forall frame frames frame_trace rest_trace,
    import_frame_trace read frame frame_trace ->
    import_stack_trace read frames rest_trace ->
    import_stack_trace read (frame :: frames) (frame_trace ++ rest_trace).

Theorem import_empty_slots_do_not_change_trace : forall read context edges skipped slots trace,
  Forall (fun slot => import_wire_slot_lookup slot edges = None) skipped ->
  (import_slots_trace read context edges (skipped ++ slots) trace <->
    import_slots_trace read context edges slots trace).
Proof.
  intros read context edges skipped slots trace Empty.
  induction Empty as [|slot skipped Vacant Empty IH]; [reflexivity|].
  cbn [app]. split.
  - intros Trace. inversion Trace; subst; try congruence. now apply IH.
  - intros Trace. apply import_slots_trace_empty; auto. now apply IH.
Qed.

Theorem import_empty_slot_trace_is_empty : forall read context edges slots trace,
  Forall (fun slot => import_wire_slot_lookup slot edges = None) slots ->
  (import_slots_trace read context edges slots trace <-> trace = []).
Proof.
  intros read context edges slots trace Empty.
  pose proof (import_empty_slots_do_not_change_trace read context edges slots [] trace Empty) as Erase.
  rewrite app_nil_r in Erase. rewrite Erase. split.
  - intros Trace. now inversion Trace.
  - intros Same. subst trace. constructor.
Qed.

Theorem import_next_entry_has_exact_trace_suffix : forall read context edges slots selected trace,
  import_first_occupied_slot slots edges = Some selected ->
  (import_slots_trace read context edges slots trace <->
    import_slots_trace read context edges
      (import_next_slot selected :: import_next_slots selected) trace).
Proof.
  intros read context edges slots selected trace Found.
  destruct (import_next_entry_preserves_all_skipped_slots _ _ _ Found)
    as [skipped [Partition [Empty Lookup]]].
  rewrite Partition. now apply import_empty_slots_do_not_change_trace.
Qed.

Theorem import_leaf_slot_trace_has_exact_tail : forall read context edges slot slots edge trace,
  import_wire_slot_lookup slot edges = Some edge -> (import_wire_header edge <? 128) = true ->
  (import_slots_trace read context edges (slot :: slots) trace <->
    exists tail, trace = import_leaf_edge_entry edge :: tail /\
      import_slots_trace read context edges slots tail).
Proof.
  intros read context edges slot slots edge trace Lookup Kind. split.
  - intros Trace. inversion Trace as [|c es s ss t Vacant Tail|
      c es s ss e t Found Leaf Tail|
      c es s ss e child_edges child_trace rest_trace Found History Read Child Rest]; subst.
    + congruence.
    + assert (e = edge) by congruence. subst e. eauto.
    + assert (e = edge) by congruence. subst e. congruence.
  - intros [tail [Same Tail]]. subst trace. now apply import_slots_trace_leaf.
Qed.

Theorem import_history_slot_trace_has_exact_subtree_and_tail :
  forall read context edges slot slots edge child_edges trace,
  import_wire_slot_lookup slot edges = Some edge -> (import_wire_header edge <? 128) = false ->
  read (import_wire_hash edge) = Some child_edges ->
  (import_slots_trace read context edges (slot :: slots) trace <->
    exists child_trace rest_trace,
      trace = import_history_edge_entry context slot edge :: child_trace ++ rest_trace /\
      import_slots_trace read (import_edge_absolute_prefix context slot edge) child_edges
        (seq 0 256) child_trace /\ import_slots_trace read context edges slots rest_trace).
Proof.
  intros read context edges slot slots edge child_edges trace Lookup Kind Read. split.
  - intros Trace. inversion Trace as [|c es s ss t Vacant Tail|
      c es s ss e t Found Leaf Tail|
      c es s ss e ce ct rt Found History ChildRead Child Rest]; subst.
    + congruence.
    + assert (e = edge) by congruence. subst e. congruence.
    + assert (e = edge) by congruence. subst e.
      assert (ce = child_edges) by congruence. subst ce. eauto.
  - intros [child_trace [rest_trace [Same [Child Rest]]]]. subst trace.
    now apply import_slots_trace_history with (child_edges := child_edges).
Qed.

Definition import_advance_frame (frame : ImportTraversalFrame) (slot : nat) : ImportTraversalFrame :=
  import_frame_at (import_frame_key frame) (import_frame_prefix frame) (import_frame_edges frame) (Some slot).

Definition import_child_frame (frame : ImportTraversalFrame) (selected : ImportNextEntry)
    (child_edges : list ImportWireEdge) : ImportTraversalFrame :=
  import_frame_at (import_wire_hash (import_next_edge selected))
    (import_edge_absolute_prefix (import_frame_prefix frame) (import_next_slot selected) (import_next_edge selected))
    child_edges None.

Definition import_frame_next (frame : ImportTraversalFrame) : option ImportNextEntry :=
  import_first_occupied_slot (import_frame_pending_slots frame) (import_frame_edges frame).

Inductive import_export_step (read : ImportHistoryReader) :
    list ImportTraversalFrame -> option ImportExportEntry -> list ImportTraversalFrame -> Prop :=
| import_export_step_pop : forall frame frames,
    import_frame_next frame = None -> import_export_step read (frame :: frames) None frames
| import_export_step_leaf : forall frame frames selected,
    import_frame_next frame = Some selected ->
    (import_wire_header (import_next_edge selected) <? 128) = true ->
    import_export_step read (frame :: frames) (Some (import_leaf_edge_entry (import_next_edge selected)))
      (import_advance_frame frame (import_next_slot selected) :: frames)
| import_export_step_history : forall frame frames selected child_edges,
    import_frame_next frame = Some selected ->
    (import_wire_header (import_next_edge selected) <? 128) = false ->
    read (import_wire_hash (import_next_edge selected)) = Some child_edges ->
    import_export_step read (frame :: frames)
      (Some (import_history_edge_entry (import_frame_prefix frame) (import_next_slot selected) (import_next_edge selected)))
      (import_child_frame frame selected child_edges :: import_advance_frame frame (import_next_slot selected) :: frames).

Definition import_event_entries (event : option ImportExportEntry) : list ImportExportEntry :=
  match event with None => [] | Some entry => [entry] end.

Theorem import_popped_frame_has_no_remaining_entries : forall read frame trace,
  import_frame_next frame = None -> (import_frame_trace read frame trace <-> trace = []).
Proof.
  intros read frame trace Empty. unfold import_frame_trace.
  apply import_empty_slot_trace_is_empty.
  now apply import_no_next_entry_means_all_slots_are_empty in Empty.
Qed.

Theorem import_advanced_frame_uses_exact_remaining_slots : forall read frame selected trace,
  import_frame_next frame = Some selected ->
  (import_frame_trace read (import_advance_frame frame (import_next_slot selected)) trace <->
    import_slots_trace read (import_frame_prefix frame) (import_frame_edges frame)
      (import_next_slots selected) trace).
Proof.
  intros read frame selected trace Found. unfold import_frame_trace, import_advance_frame.
  cbn [import_frame_prefix import_frame_edges import_frame_at].
  now rewrite <- (import_next_entry_resumes_at_exact_parent_successor _ _ Found).
Qed.

Theorem import_stack_trace_cons_has_exact_parts : forall read frame frames trace,
  import_stack_trace read (frame :: frames) trace <->
  exists first rest, trace = first ++ rest /\
    import_frame_trace read frame first /\ import_stack_trace read frames rest.
Proof.
  intros read frame frames trace. split.
  - intros Trace. inversion Trace; subst. eauto.
  - intros [first [rest [Same [Frame Rest]]]]. subst trace. now constructor.
Qed.

Theorem import_export_step_preserves_complete_trace : forall read initial event final trace,
  import_export_step read initial event final ->
  (import_stack_trace read initial trace <->
    exists suffix, trace = import_event_entries event ++ suffix /\
      import_stack_trace read final suffix).
Proof.
  intros read initial event final trace Step.
  destruct Step as [frame frames Empty|frame frames selected Found Leaf|
    frame frames selected child_edges Found History Read].
  - rewrite import_stack_trace_cons_has_exact_parts. cbn [import_event_entries app]. split.
    + intros [first [rest [Same [Frame Rest]]]].
      apply (import_popped_frame_has_no_remaining_entries _ _ _ Empty) in Frame. subst first.
      cbn [app] in Same. subst trace. eauto.
    + intros [suffix [Same Rest]]. subst trace. exists [], suffix.
      split; [reflexivity|]. split; auto.
      apply (import_popped_frame_has_no_remaining_entries _ _ _ Empty). reflexivity.
  - pose proof (import_next_entry_preserves_all_skipped_slots _ _ _ Found) as [_ [_ [_ Lookup]]].
    cbn [import_event_entries app]. split.
    + intros Trace. apply import_stack_trace_cons_has_exact_parts in Trace
        as [first [rest [Same [Frame Rest]]]]. unfold import_frame_trace in Frame.
      apply (import_next_entry_has_exact_trace_suffix _ _ _ _ _ _ Found) in Frame.
      apply (import_leaf_slot_trace_has_exact_tail _ _ _ _ _ _ _ Lookup Leaf) in Frame
        as [tail [First Tail]]. subst first. cbn [app] in Same. subst trace.
      exists (tail ++ rest). split; [reflexivity|]. apply import_stack_trace_frame; auto.
      now apply (import_advanced_frame_uses_exact_remaining_slots _ _ _ _ Found).
    + intros [suffix [Same Final]]. apply import_stack_trace_cons_has_exact_parts in Final
        as [first [rest [Suffix [Frame Rest]]]].
      apply (import_advanced_frame_uses_exact_remaining_slots _ _ _ _ Found) in Frame.
      apply import_stack_trace_cons_has_exact_parts.
      exists (import_leaf_edge_entry (import_next_edge selected) :: first), rest.
      split; [subst; reflexivity|]. split; auto. unfold import_frame_trace.
      apply (import_next_entry_has_exact_trace_suffix _ _ _ _ _ _ Found).
      apply (import_leaf_slot_trace_has_exact_tail _ _ _ _ _ _ _ Lookup Leaf). eauto.
  - pose proof (import_next_entry_preserves_all_skipped_slots _ _ _ Found) as [_ [_ [_ Lookup]]].
    rewrite import_stack_trace_cons_has_exact_parts. cbn [import_event_entries app]. split.
    + intros [first [rest [Same [Frame Rest]]]]. unfold import_frame_trace in Frame.
      apply (import_next_entry_has_exact_trace_suffix _ _ _ _ _ _ Found) in Frame.
      apply (import_history_slot_trace_has_exact_subtree_and_tail _ _ _ _ _ _ _ _ Lookup History Read)
        in Frame as [child_trace [tail [First [Child Tail]]]].
      subst first. cbn [app] in Same. subst trace.
      exists ((child_trace ++ tail) ++ rest). split; [reflexivity|].
      rewrite <- app_assoc. apply import_stack_trace_frame.
      * exact Child.
      * apply import_stack_trace_frame; auto.
        now apply (import_advanced_frame_uses_exact_remaining_slots _ _ _ _ Found).
    + intros [suffix [Same Final]].
      apply import_stack_trace_cons_has_exact_parts in Final
        as [child_trace [tail_and_rest [Suffix [Child Final]]]].
      apply import_stack_trace_cons_has_exact_parts in Final
        as [tail [rest [TailAndRest [Tail Rest]]]].
      apply (import_advanced_frame_uses_exact_remaining_slots _ _ _ _ Found) in Tail.
      exists (import_history_edge_entry (import_frame_prefix frame) (import_next_slot selected)
        (import_next_edge selected) :: child_trace ++ tail), rest.
      split.
      * subst. cbn [app]. now rewrite app_assoc.
      * split; auto. unfold import_frame_trace.
        apply (import_next_entry_has_exact_trace_suffix _ _ _ _ _ _ Found).
        apply (import_history_slot_trace_has_exact_subtree_and_tail _ _ _ _ _ _ _ _ Lookup History Read).
        exists child_trace, tail. auto.
Qed.

Inductive import_export_run (read : ImportHistoryReader) :
    list ImportTraversalFrame -> list ImportExportEntry -> list ImportTraversalFrame -> Prop :=
| import_export_run_stop : forall frames, import_export_run read frames [] frames
| import_export_run_next : forall initial event next entries final,
    import_export_step read initial event next ->
    import_export_run read next entries final ->
    import_export_run read initial (import_event_entries event ++ entries) final.

Theorem import_export_run_preserves_complete_trace : forall read initial entries final,
  import_export_run read initial entries final -> forall trace,
  (import_stack_trace read initial trace <->
    exists suffix, trace = entries ++ suffix /\ import_stack_trace read final suffix).
Proof.
  intros read initial entries final Run. induction Run as
    [frames|initial event next entries final Step Run IH]; intros trace.
  - cbn [app]. split; [intros Trace; eauto|intros [suffix [Same Trace]]; now subst].
  - rewrite (import_export_step_preserves_complete_trace _ _ _ _ _ Step). split.
    + intros [tail [Trace Tail]]. apply IH in Tail as [suffix [Tail Final]].
      exists suffix. split; auto. subst. now rewrite app_assoc.
    + intros [suffix [Trace Final]]. exists (entries ++ suffix). split.
      * rewrite Trace. symmetry. apply app_assoc.
      * apply IH. eauto.
Qed.

Definition import_event_history_cost (event : option ImportExportEntry) : nat :=
  match event with Some (ImportExportHistory _ _) => 1 | _ => 0 end.

Theorem import_history_split_after_event : forall budget event tail,
  0 < budget ->
  import_take_history_occurrences budget (import_event_entries event ++ tail) =
    let '(selected, suffix) := import_take_history_occurrences
      (budget - import_event_history_cost event) tail in
    (import_event_entries event ++ selected, suffix).
Proof.
  intros [|budget] event tail Positive; [lia|].
  destruct event as [[key prefix|key]|];
    cbn [import_event_history_cost import_event_entries app import_take_history_occurrences Nat.sub];
    rewrite ?Nat.sub_0_r;
    destruct (import_take_history_occurrences _ tail); reflexivity.
Qed.

Inductive import_export_page (read : ImportHistoryReader) :
    nat -> list ImportTraversalFrame -> list ImportExportEntry -> list ImportTraversalFrame -> Prop :=
| import_export_page_limit : forall frames, import_export_page read 0 frames [] frames
| import_export_page_done : forall budget, import_export_page read budget [] [] []
| import_export_page_next : forall budget initial event next entries final,
    0 < budget -> import_export_step read initial event next ->
    import_export_page read (budget - import_event_history_cost event) next entries final ->
    import_export_page read budget initial (import_event_entries event ++ entries) final.

Theorem import_page_execution_is_a_finite_export_run : forall read budget initial entries final,
  import_export_page read budget initial entries final ->
  import_export_run read initial entries final.
Proof.
  intros read budget initial entries final Page. induction Page.
  - apply import_export_run_stop.
  - apply import_export_run_stop.
  - eapply import_export_run_next; eauto.
Qed.

Theorem import_page_execution_matches_history_occurrence_split :
  forall read budget initial entries final,
  import_export_page read budget initial entries final -> forall trace,
  import_stack_trace read initial trace -> exists suffix,
    import_take_history_occurrences budget trace = (entries, suffix) /\
    import_stack_trace read final suffix.
Proof.
  intros read budget initial entries final Page. induction Page as
    [frames|budget|budget initial event next entries final Positive Step Page IH]; intros trace Trace.
  - exists trace. split; [destruct trace; reflexivity|exact Trace].
  - inversion Trace; subst. exists []. split; [destruct budget; reflexivity|constructor].
  - apply (import_export_step_preserves_complete_trace _ _ _ _ _ Step) in Trace
      as [tail [Same Tail]].
    destruct (IH tail Tail) as [suffix [Split Final]]. exists suffix. split; auto.
    rewrite Same, (import_history_split_after_event _ _ _ Positive), Split. reflexivity.
Qed.

Theorem import_page_execution_preserves_all_remaining_occurrences :
  forall read budget initial entries final trace,
  import_export_page read budget initial entries final -> import_stack_trace read initial trace ->
  exists suffix, trace = entries ++ suffix /\ import_stack_trace read final suffix.
Proof.
  intros read budget initial entries final trace Page Trace.
  apply import_page_execution_is_a_finite_export_run in Page.
  now apply (import_export_run_preserves_complete_trace _ _ _ _ Page) in Trace.
Qed.

Theorem import_page_execution_respects_history_budget :
  forall read budget initial entries final,
  import_export_page read budget initial entries final ->
  length (import_export_history_keys entries) <= budget.
Proof.
  intros read budget initial entries final Page. induction Page as
    [frames|budget|budget initial event next entries final Positive Step Page IH].
  - reflexivity.
  - simpl. lia.
  - rewrite import_export_history_keys_app, length_app.
    destruct event as [[key prefix|key]|];
      cbn [import_event_entries import_event_history_cost import_export_history_keys length] in *; lia.
Qed.

Theorem import_export_step_preserves_reader_extension : forall before after initial event final,
  import_history_reader_extension before after -> import_export_step before initial event final ->
  import_export_step after initial event final.
Proof.
  intros before after initial event final Extend Step. destruct Step.
  - now apply import_export_step_pop.
  - now apply import_export_step_leaf.
  - eapply import_export_step_history; eauto.
Qed.

Theorem import_export_run_preserves_reader_extension : forall before after initial entries final,
  import_history_reader_extension before after -> import_export_run before initial entries final ->
  import_export_run after initial entries final.
Proof.
  intros before after initial entries final Extend Run. induction Run.
  - constructor.
  - eapply import_export_run_next; eauto using import_export_step_preserves_reader_extension.
Qed.

Inductive import_interleaved_export_run : ImportHistoryReader -> ImportHistoryReader ->
    list ImportTraversalFrame -> list ImportExportEntry -> list ImportTraversalFrame -> Prop :=
| import_interleaved_export_stop : forall read frames,
    import_interleaved_export_run read read frames [] frames
| import_interleaved_export_next : forall first middle last initial event next entries final,
    import_export_step first initial event next ->
    import_history_reader_extension first middle ->
    import_interleaved_export_run middle last next entries final ->
    import_interleaved_export_run first last initial (import_event_entries event ++ entries) final.

Theorem import_interleaved_export_run_preserves_read_bindings :
  forall first last initial entries final,
  import_interleaved_export_run first last initial entries final ->
  import_history_reader_extension first last.
Proof.
  intros first last initial entries final Run. induction Run.
  - intros key edges Read. exact Read.
  - intros key edges Read. apply IHRun. now apply H0.
Qed.

Theorem import_interleaved_export_has_exact_final_view_run :
  forall first last initial entries final,
  import_interleaved_export_run first last initial entries final ->
  import_export_run last initial entries final.
Proof.
  intros first last initial entries final Run. induction Run as
    [read frames|first middle last initial event next entries final Step Extend Run IH].
  - constructor.
  - eapply import_export_run_next; [|exact IH].
    eapply import_export_step_preserves_reader_extension; [|exact Step].
    intros key edges Read. apply (import_interleaved_export_run_preserves_read_bindings _ _ _ _ _ Run).
    now apply Extend.
Qed.

Theorem import_interleaved_export_and_trailing_writes_preserve_occurrences :
  forall first last trailing initial entries final trace,
  import_interleaved_export_run first last initial entries final ->
  import_history_reader_extension last trailing -> import_stack_trace trailing initial trace ->
  exists suffix, trace = entries ++ suffix /\ import_stack_trace trailing final suffix.
Proof.
  intros first last trailing initial entries final trace Run Extend Trace.
  apply import_interleaved_export_has_exact_final_view_run in Run.
  pose proof (import_export_run_preserves_reader_extension _ _ _ _ _ Extend Run) as FinalRun.
  now apply (import_export_run_preserves_complete_trace _ _ _ _ FinalRun) in Trace.
Qed.

Fixpoint import_drop_history_occurrences (skip : nat) (trace : list ImportExportEntry)
    : list ImportExportEntry :=
  match skip, trace with
  | 0, _ => trace
  | _, [] => []
  | S remaining, entry :: rest =>
      let next_skip := match entry with
        | ImportExportHistory _ _ => remaining
        | ImportExportLeaf _ => S remaining
        end in
      import_drop_history_occurrences next_skip rest
  end.

Theorem import_history_skip_after_event : forall skip event tail,
  0 < skip ->
  import_drop_history_occurrences skip (import_event_entries event ++ tail) =
    import_drop_history_occurrences (skip - import_event_history_cost event) tail.
Proof.
  intros [|skip] event tail Positive; [lia|].
  destruct event as [[key prefix|key]|];
    cbn [import_drop_history_occurrences import_event_entries import_event_history_cost app Nat.sub];
    now rewrite ?Nat.sub_0_r.
Qed.

Inductive import_export_slice (read : ImportHistoryReader) : nat -> nat ->
    list ImportTraversalFrame -> list ImportExportEntry -> list ImportTraversalFrame -> Prop :=
| import_export_slice_ready : forall budget initial entries final,
    import_export_page read budget initial entries final ->
    import_export_slice read 0 budget initial entries final
| import_export_slice_done : forall skip budget, import_export_slice read skip budget [] [] []
| import_export_slice_skip : forall skip budget initial event next entries final,
    0 < skip -> import_export_step read initial event next ->
    import_export_slice read (skip - import_event_history_cost event) budget next entries final ->
    import_export_slice read skip budget initial entries final.

Theorem import_slice_execution_matches_history_skip_and_take :
  forall read skip budget initial entries final,
  import_export_slice read skip budget initial entries final -> forall trace,
  import_stack_trace read initial trace -> exists suffix,
    import_take_history_occurrences budget (import_drop_history_occurrences skip trace) = (entries, suffix) /\
    import_stack_trace read final suffix.
Proof.
  intros read skip budget initial entries final Slice. induction Slice as
    [budget initial entries final Page|skip budget|
     skip budget initial event next entries final Positive Step Slice IH]; intros trace Trace.
  - replace (import_drop_history_occurrences 0 trace) with trace by (destruct trace; reflexivity).
    now apply (import_page_execution_matches_history_occurrence_split _ _ _ _ _ Page).
  - inversion Trace; subst. exists []. split.
    + destruct skip, budget; reflexivity.
    + constructor.
  - apply (import_export_step_preserves_complete_trace _ _ _ _ _ Step) in Trace
      as [tail [Same Tail]].
    destruct (IH tail Tail) as [suffix [Split Final]]. exists suffix. split; auto.
    rewrite Same, (import_history_skip_after_event _ _ _ Positive). exact Split.
Qed.

Theorem import_slice_execution_respects_history_budget :
  forall read skip budget initial entries final,
  import_export_slice read skip budget initial entries final ->
  length (import_export_history_keys entries) <= budget.
Proof.
  intros read skip budget initial entries final Slice. induction Slice.
  - now apply import_page_execution_respects_history_budget in H.
  - simpl. lia.
  - exact IHSlice.
Qed.

Definition import_export_resume_prefix (frames : list ImportTraversalFrame) : option (list nat) :=
  match frames with [] => None | frame :: _ => Some (import_frame_prefix frame) end.

Theorem import_export_exhaustion_requires_an_empty_stack : forall frames,
  import_export_resume_prefix frames = None <-> frames = [].
Proof. intros [|frame frames]; simpl; split; congruence. Qed.

Theorem import_zero_budget_preserves_an_unsearched_frame : forall read frame frames,
  import_export_page read 0 (frame :: frames) [] (frame :: frames) /\
  import_export_resume_prefix (frame :: frames) = Some (import_frame_prefix frame).
Proof. intros. split; [constructor|reflexivity]. Qed.

Definition import_frame_potential (frame : ImportTraversalFrame) : nat :=
  length (import_frame_pending_slots frame) + 1.

Fixpoint import_stack_potential (frames : list ImportTraversalFrame) : nat :=
  match frames with [] => 0 | frame :: rest => import_frame_potential frame + import_stack_potential rest end.

Theorem import_frame_advance_strictly_reduces_potential : forall frame selected,
  import_frame_next frame = Some selected ->
  import_frame_potential (import_advance_frame frame (import_next_slot selected)) < import_frame_potential frame.
Proof.
  intros frame selected Found.
  pose proof (import_next_entry_preserves_all_skipped_slots _ _ _ Found)
    as [skipped [Partition _]].
  pose proof (import_next_entry_resumes_at_exact_parent_successor _ _ Found) as Remaining.
  unfold import_frame_potential, import_advance_frame. rewrite <- Remaining, Partition, length_app.
  simpl. lia.
Qed.

Theorem import_fresh_child_has_exact_potential : forall frame selected edges,
  import_frame_potential (import_child_frame frame selected edges) = 257.
Proof. reflexivity. Qed.

Theorem import_step_has_bounded_stack_growth : forall read initial event final,
  import_export_step read initial event final ->
  import_stack_potential final < import_stack_potential initial + 257 * import_event_history_cost event.
Proof.
  intros read initial event final Step. destruct Step as
    [frame frames Empty|frame frames selected Found Leaf|frame frames selected edges Found History Read].
  - cbn [import_stack_potential import_event_history_cost]. unfold import_frame_potential. lia.
  - pose proof (import_frame_advance_strictly_reduces_potential _ _ Found).
    cbn [import_stack_potential import_event_history_cost import_leaf_edge_entry]. lia.
  - pose proof (import_frame_advance_strictly_reduces_potential _ _ Found).
    cbn [import_stack_potential import_event_history_cost import_history_edge_entry].
    rewrite import_fresh_child_has_exact_potential. lia.
Qed.

Definition import_export_potential (skip budget : nat) (frames : list ImportTraversalFrame) : nat :=
  257 * (skip + budget) + import_stack_potential frames.

Theorem import_taking_step_strictly_reduces_export_potential : forall read budget initial event final,
  0 < budget -> import_export_step read initial event final ->
  import_export_potential 0 (budget - import_event_history_cost event) final <
    import_export_potential 0 budget initial.
Proof.
  intros read budget initial event final Positive Step.
  pose proof (import_step_has_bounded_stack_growth _ _ _ _ Step) as Bound.
  unfold import_export_potential. destruct event as [[key prefix|key]|];
    cbn [import_event_history_cost] in *; lia.
Qed.

Theorem import_skipping_step_strictly_reduces_export_potential : forall read skip budget initial event final,
  0 < skip -> import_export_step read initial event final ->
  import_export_potential (skip - import_event_history_cost event) budget final <
    import_export_potential skip budget initial.
Proof.
  intros read skip budget initial event final Positive Step.
  pose proof (import_step_has_bounded_stack_growth _ _ _ _ Step) as Bound.
  unfold import_export_potential. destruct event as [[key prefix|key]|];
    cbn [import_event_history_cost] in *; lia.
Qed.

Theorem import_missing_required_child_has_no_successful_step : forall read frame frames selected,
  import_frame_next frame = Some selected ->
  (import_wire_header (import_next_edge selected) <? 128) = false ->
  read (import_wire_hash (import_next_edge selected)) = None ->
  forall event final, ~import_export_step read (frame :: frames) event final.
Proof.
  intros read frame frames selected Found History Missing event final Step.
  inversion Step; subst.
  - congruence.
  - assert (selected0 = selected) by congruence. subst selected0. congruence.
  - assert (selected0 = selected) by congruence. subst selected0. congruence.
Qed.

Theorem import_missing_child_is_not_positive_budget_completion : forall read frame frames selected budget entries final,
  import_frame_next frame = Some selected ->
  (import_wire_header (import_next_edge selected) <? 128) = false ->
  read (import_wire_hash (import_next_edge selected)) = None ->
  0 < budget -> ~import_export_page read budget (frame :: frames) entries final.
Proof.
  intros read frame frames selected budget entries final Found History Missing Positive Page.
  inversion Page; subst; try lia.
  eapply import_missing_required_child_has_no_successful_step; eauto.
Qed.

Theorem import_page_adapter_preserves_operational_leaf_occurrences :
  forall read budget initial entries final trace cursor,
  import_export_page read budget initial entries final -> import_stack_trace read initial trace ->
  exists suffix,
    import_export_leaf_keys (import_export_page_entries (import_export_leaf_keys entries)
      (import_export_history_keys entries) cursor) ++ import_export_leaf_keys suffix =
      import_export_leaf_keys trace /\ import_stack_trace read final suffix.
Proof.
  intros read budget initial entries final trace cursor Page Trace.
  destruct (import_page_execution_preserves_all_remaining_occurrences _ _ _ _ _ _ Page Trace)
    as [suffix [Same Final]]. exists suffix. split; auto.
  rewrite import_page_preserves_all_leaf_occurrences, <- import_export_leaf_keys_app, <- Same.
  reflexivity.
Qed.

Theorem import_page_adapter_preserves_operational_history_occurrences :
  forall read budget initial entries final trace cursor,
  import_export_page read budget initial entries final -> import_stack_trace read initial trace ->
  exists suffix,
    import_export_history_keys (import_export_page_entries (import_export_leaf_keys entries)
      (import_export_history_keys entries) cursor) ++ import_export_history_keys suffix =
      import_export_history_keys trace /\ import_stack_trace read final suffix.
Proof.
  intros read budget initial entries final trace cursor Page Trace.
  destruct (import_page_execution_preserves_all_remaining_occurrences _ _ _ _ _ _ Page Trace)
    as [suffix [Same Final]]. exists suffix. split; auto.
  rewrite import_page_preserves_all_history_occurrences, <- import_export_history_keys_app, <- Same.
  reflexivity.
Qed.

Definition import_anchor_entries (anchor : option (list nat)) : list ImportExportEntry :=
  match anchor with None => [] | Some key => [ImportExportHistory (import_key_to_nat key) []] end.

Inductive import_anchored_export (read : ImportHistoryReader) : option (list nat) -> nat -> nat ->
    list ImportTraversalFrame -> list ImportExportEntry -> list ImportTraversalFrame -> Prop :=
| import_anchored_resume : forall skip budget initial entries final,
    import_export_slice read skip budget initial entries final ->
    import_anchored_export read None skip budget initial entries final
| import_anchored_skip_root : forall key skip budget initial entries final,
    import_export_slice read skip budget initial entries final ->
    import_anchored_export read (Some key) (S skip) budget initial entries final
| import_anchored_take_root : forall key budget initial entries final,
    import_export_page read budget initial entries final ->
    import_anchored_export read (Some key) 0 (S budget) initial
      (ImportExportHistory (import_key_to_nat key) [] :: entries) final.

Theorem import_anchored_export_matches_exact_skip_and_take :
  forall read anchor skip budget initial entries final,
  import_anchored_export read anchor skip budget initial entries final -> forall trace,
  import_stack_trace read initial trace -> exists suffix,
    import_take_history_occurrences budget
      (import_drop_history_occurrences skip (import_anchor_entries anchor ++ trace)) = (entries, suffix) /\
    import_stack_trace read final suffix.
Proof.
  intros read anchor skip budget initial entries final Export trace Trace. destruct Export.
  - cbn [import_anchor_entries app].
    now apply (import_slice_execution_matches_history_skip_and_take _ _ _ _ _ _ H).
  - cbn [import_anchor_entries app import_drop_history_occurrences].
    now apply (import_slice_execution_matches_history_skip_and_take _ _ _ _ _ _ H).
  - destruct (import_page_execution_matches_history_occurrence_split _ _ _ _ _ H _ Trace)
      as [suffix [Split Final]]. exists suffix. split; auto.
    cbn [import_anchor_entries app import_drop_history_occurrences import_take_history_occurrences].
    now rewrite Split.
Qed.

Theorem import_anchored_export_respects_history_budget :
  forall read anchor skip budget initial entries final,
  import_anchored_export read anchor skip budget initial entries final ->
  length (import_export_history_keys entries) <= budget.
Proof.
  intros read anchor skip budget initial entries final Export. destruct Export.
  - eapply import_slice_execution_respects_history_budget; eauto.
  - eapply import_slice_execution_respects_history_budget; eauto.
  - cbn [import_export_history_keys length].
    apply le_n_S. eapply import_page_execution_respects_history_budget; eauto.
Qed.

Definition import_cursor_initial_stack (read : ImportHistoryReader) (cursor : ImportCursor)
    (frames : list ImportTraversalFrame) : Prop :=
  match cursor with
  | ImportCursorStart root => import_history_stack read root [] [] frames
  | ImportCursorResume root prefix carrier =>
      import_history_stack read root [] prefix frames /\
      exists top rest, frames = top :: rest /\ import_frame_key top = carrier
  end.

Definition import_cursor_anchor (cursor : ImportCursor) : option (list nat) :=
  match cursor with ImportCursorStart root => Some root | ImportCursorResume _ _ _ => None end.

Definition import_export_from_cursor (read : ImportHistoryReader) (cursor : ImportCursor)
    (skip budget : nat) (entries : list ImportExportEntry) (final : list ImportTraversalFrame) : Prop :=
  import_cursor_validb cursor = true /\ (0 < skip \/ 0 < budget) /\
  exists frames, import_cursor_initial_stack read cursor frames /\
    import_anchored_export read (import_cursor_anchor cursor) skip budget frames entries final.

Theorem import_cursor_initial_frames_have_matching_reads : forall read cursor frames,
  import_cursor_initial_stack read cursor frames ->
  Forall (fun frame => read (import_frame_key frame) = Some (import_frame_edges frame)) frames.
Proof.
  intros read [root|root prefix carrier] frames Stack.
  - eapply import_history_stack_frames_have_exact_reads. exact Stack.
  - destruct Stack as [Stack _]. eapply import_history_stack_frames_have_exact_reads. exact Stack.
Qed.

Theorem import_cursor_export_respects_history_budget : forall read cursor skip budget entries final,
  import_export_from_cursor read cursor skip budget entries final ->
  length (import_export_history_keys entries) <= budget.
Proof.
  intros read cursor skip budget entries final [_ [_ [frames [_ Export]]]].
  now apply import_anchored_export_respects_history_budget in Export.
Qed.

Theorem import_start_one_history_budget_does_not_read_descendants : forall read key frames,
  import_anchored_export read (Some key) 0 1 frames
    [ImportExportHistory (import_key_to_nat key) []] frames.
Proof. intros. apply import_anchored_take_root. apply import_export_page_limit. Qed.

Example import_skip_root_retains_its_leaves :
  import_drop_history_occurrences 1
    [ImportExportHistory 1 []; ImportExportLeaf 7; ImportExportHistory 2 [3]] =
    [ImportExportLeaf 7; ImportExportHistory 2 [3]].
Proof. reflexivity. Qed.

Example import_skip_discards_leaves_only_until_enough_history_occurrences :
  import_drop_history_occurrences 1
    [ImportExportLeaf 7; ImportExportHistory 2 [3]; ImportExportLeaf 9] = [ImportExportLeaf 9].
Proof. reflexivity. Qed.

Example import_budget_boundary_does_not_consume_a_following_leaf :
  import_take_history_occurrences 1 [ImportExportHistory 1 []; ImportExportLeaf 7] =
    ([ImportExportHistory 1 []], [ImportExportLeaf 7]).
Proof. reflexivity. Qed.

Example import_resume_empty_prefix_has_no_root_anchor : forall key,
  import_cursor_anchor (ImportCursorResume key [] key) = None /\
  import_cursor_anchor (ImportCursorStart key) = Some key.
Proof. intros. split; reflexivity. Qed.

Example import_empty_trace_does_not_establish_exhaustion : forall read key,
  import_frame_trace read (import_frame_at key [] [] None) [] /\
  import_export_resume_prefix [import_frame_at key [] [] None] = Some [].
Proof.
  intros read key. split; [|reflexivity]. unfold import_frame_trace.
  apply import_empty_slot_trace_is_empty; [|reflexivity].
  apply Forall_forall. intros slot Member. reflexivity.
Qed.

Definition import_shared_example_key (byte : nat) : list nat := repeat byte 32.

Definition import_shared_example_edge (slot header target : nat) : ImportWireEdge :=
  {| import_wire_slot := slot; import_wire_header := header;
     import_wire_prefix := []; import_wire_hash := import_shared_example_key target |}.

Definition import_shared_example_parent : list ImportWireEdge :=
  [import_shared_example_edge 1 128 7; import_shared_example_edge 2 128 7].

Definition import_shared_example_child : list ImportWireEdge := [import_shared_example_edge 0 0 9].

Definition import_shared_example_reader : ImportHistoryReader := fun key =>
  if list_eq_dec Nat.eq_dec key (import_shared_example_key 1) then Some import_shared_example_parent
  else if list_eq_dec Nat.eq_dec key (import_shared_example_key 7) then Some import_shared_example_child
  else None.

Example import_shared_target_reconstructs_distinct_ancestor_positions :
  import_build_history_stack 2 import_shared_example_reader (import_shared_example_key 1) [] [1] =
    Some [import_frame_at (import_shared_example_key 7) [1] import_shared_example_child None;
          import_frame_at (import_shared_example_key 1) [] import_shared_example_parent (Some 1)] /\
  import_build_history_stack 2 import_shared_example_reader (import_shared_example_key 1) [] [2] =
    Some [import_frame_at (import_shared_example_key 7) [2] import_shared_example_child None;
          import_frame_at (import_shared_example_key 1) [] import_shared_example_parent (Some 2)].
Proof. vm_compute. split; reflexivity. Qed.

Example import_shared_target_keeps_the_second_parent_edge :
  import_frame_next (import_frame_at (import_shared_example_key 1) [] import_shared_example_parent (Some 1)) =
    Some {| import_next_slot := 2; import_next_edge := import_shared_example_edge 2 128 7;
            import_next_slots := seq 3 253 |}.
Proof. vm_compute. reflexivity. Qed.

Example import_shared_leaf_occurrences_are_not_deduplicated :
  import_export_leaf_keys (import_export_page_entries
    [import_key_to_nat (import_shared_example_key 9); import_key_to_nat (import_shared_example_key 9)]
    [import_key_to_nat (import_shared_example_key 7); import_key_to_nat (import_shared_example_key 7)] []) =
    [import_key_to_nat (import_shared_example_key 9); import_key_to_nat (import_shared_example_key 9)].
Proof. apply import_page_preserves_all_leaf_occurrences. Qed.

Definition import_shared_example_parent_frame (last : option nat) :=
  import_frame_at (import_shared_example_key 1) [] import_shared_example_parent last.

Definition import_shared_example_child_frame (slot : nat) (last : option nat) :=
  import_frame_at (import_shared_example_key 7) [slot] import_shared_example_child last.

Example import_shared_subtree_run_visits_both_leaf_occurrences :
  import_export_run import_shared_example_reader [import_shared_example_parent_frame None]
    [ImportExportHistory (import_key_to_nat (import_shared_example_key 7)) [1];
     ImportExportLeaf (import_key_to_nat (import_shared_example_key 9));
     ImportExportHistory (import_key_to_nat (import_shared_example_key 7)) [2];
     ImportExportLeaf (import_key_to_nat (import_shared_example_key 9))] [].
Proof.
  eapply import_export_run_next with
    (event := Some (ImportExportHistory (import_key_to_nat (import_shared_example_key 7)) [1]))
    (next := [import_shared_example_child_frame 1 None; import_shared_example_parent_frame (Some 1)]).
  - apply import_export_step_history with
      (selected := {| import_next_slot := 1; import_next_edge := import_shared_example_edge 1 128 7;
                      import_next_slots := seq 2 254 |}) (child_edges := import_shared_example_child);
      vm_compute; reflexivity.
  - eapply import_export_run_next with
      (event := Some (ImportExportLeaf (import_key_to_nat (import_shared_example_key 9))))
      (next := [import_shared_example_child_frame 1 (Some 0); import_shared_example_parent_frame (Some 1)]).
    + apply import_export_step_leaf with
        (selected := {| import_next_slot := 0; import_next_edge := import_shared_example_edge 0 0 9;
                        import_next_slots := seq 1 255 |}); vm_compute; reflexivity.
    + eapply import_export_run_next with (event := None)
        (next := [import_shared_example_parent_frame (Some 1)]).
      * apply import_export_step_pop. vm_compute. reflexivity.
      * eapply import_export_run_next with
          (event := Some (ImportExportHistory (import_key_to_nat (import_shared_example_key 7)) [2]))
          (next := [import_shared_example_child_frame 2 None; import_shared_example_parent_frame (Some 2)]).
        -- apply import_export_step_history with
            (selected := {| import_next_slot := 2; import_next_edge := import_shared_example_edge 2 128 7;
                            import_next_slots := seq 3 253 |}) (child_edges := import_shared_example_child);
            vm_compute; reflexivity.
        -- eapply import_export_run_next with
            (event := Some (ImportExportLeaf (import_key_to_nat (import_shared_example_key 9))))
            (next := [import_shared_example_child_frame 2 (Some 0); import_shared_example_parent_frame (Some 2)]).
           ++ apply import_export_step_leaf with
               (selected := {| import_next_slot := 0; import_next_edge := import_shared_example_edge 0 0 9;
                               import_next_slots := seq 1 255 |}); vm_compute; reflexivity.
           ++ eapply import_export_run_next with (event := None)
               (next := [import_shared_example_parent_frame (Some 2)]).
              ** apply import_export_step_pop. vm_compute. reflexivity.
              ** eapply import_export_run_next with (event := None) (next := []).
                 --- apply import_export_step_pop. vm_compute. reflexivity.
                 --- apply import_export_run_stop.
Qed.
