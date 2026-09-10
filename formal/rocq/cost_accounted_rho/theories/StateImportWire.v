From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportCodec StateImportCursor StateImportTraversal StateImportStack StateImportPage StateImportExport StateImportExecution.
Import ListNotations.

Definition import_fresh_frame (frame : ImportTraversalFrame) : ImportTraversalFrame :=
  import_frame_at (import_frame_key frame) (import_frame_prefix frame) (import_frame_edges frame) None.

Inductive import_frame_spine (read : ImportHistoryReader) (root : list nat) :
    list ImportTraversalFrame -> Prop :=
| import_frame_spine_end : import_frame_spine read root []
| import_frame_spine_root : forall edges last,
    read root = Some edges ->
    import_frame_spine read root [import_frame_at root [] edges last]
| import_frame_spine_child : forall parent rest edge edges last,
    import_frame_spine read root (parent :: rest) ->
    import_frame_last_slot parent = Some (import_wire_slot edge) ->
    import_wire_slot_lookup (import_wire_slot edge) (import_frame_edges parent) = Some edge ->
    128 <= import_wire_header edge -> read (import_wire_hash edge) = Some edges ->
    import_frame_spine read root
      (import_frame_at (import_wire_hash edge)
        (import_frame_prefix parent ++ import_wire_slot edge :: import_wire_prefix edge) edges last ::
        parent :: rest).

Theorem import_frame_spine_survives_top_cursor_update : forall read root frame rest last,
  import_frame_spine read root (frame :: rest) ->
  import_frame_spine read root
    (import_frame_at (import_frame_key frame) (import_frame_prefix frame) (import_frame_edges frame) last :: rest).
Proof.
  intros read root frame rest last Spine. inversion Spine; subst.
  - now apply import_frame_spine_root.
  - eapply import_frame_spine_child; eauto.
Qed.

Theorem import_frame_spine_survives_pop : forall read root frame rest,
  import_frame_spine read root (frame :: rest) -> import_frame_spine read root rest.
Proof. intros read root frame rest Spine. inversion Spine; subst; auto using import_frame_spine_end. Qed.

Theorem import_operational_step_preserves_frame_spine : forall read root initial event final,
  import_frame_spine read root initial -> import_export_step read initial event final ->
  import_frame_spine read root final.
Proof.
  intros read root initial event final Spine Step. destruct Step as
    [frame rest Empty|frame rest selected Found Leaf|frame rest selected edges Found History Read].
  - eapply import_frame_spine_survives_pop. exact Spine.
  - apply import_frame_spine_survives_top_cursor_update. exact Spine.
  - destruct (import_next_entry_preserves_all_skipped_slots _ _ _ Found)
      as [skipped [_ [_ Lookup]]].
    pose proof (import_wire_slot_lookup_is_sound _ _ _ Lookup) as [_ Slot].
    unfold import_child_frame, import_edge_absolute_prefix. rewrite <- Slot.
    eapply import_frame_spine_child with
      (parent := import_advance_frame frame (import_wire_slot (import_next_edge selected)))
      (edge := import_next_edge selected).
    + apply import_frame_spine_survives_top_cursor_update. exact Spine.
    + reflexivity.
    + cbn [import_advance_frame import_frame_edges import_frame_at]. now rewrite Slot.
    + now apply Nat.ltb_ge.
    + exact Read.
Qed.

Theorem import_history_stack_grafts_into_frame_spine : forall read key context path frames,
  import_history_stack read key context path frames -> forall root ancestors,
  (forall edges last, read key = Some edges ->
    import_frame_spine read root (import_frame_at key context edges last :: ancestors)) ->
  import_frame_spine read root (frames ++ ancestors).
Proof.
  intros read key context path frames Stack.
  induction Stack as [key context edges Read|
    key context edges edge suffix frames Read Lookup Kind Child IH]; intros root ancestors Parent.
  - cbn [app]. now apply Parent.
  - rewrite <- app_assoc. cbn [app]. apply IH.
    intros child_edges last ChildRead. eapply import_frame_spine_child with
      (parent := import_frame_at key context edges (Some (import_wire_slot edge))) (edge := edge).
    + apply Parent. exact Read.
    + reflexivity.
    + exact Lookup.
    + exact Kind.
    + exact ChildRead.
Qed.

Theorem import_cursor_stack_has_frame_spine : forall read cursor frames,
  import_cursor_initial_stack read cursor frames ->
  import_frame_spine read (import_cursor_root cursor) frames.
Proof.
  intros read [root|root prefix carrier] frames Stack;
    cbn [import_cursor_root import_cursor_initial_stack] in *.
  - replace frames with (frames ++ []) by apply app_nil_r.
    eapply import_history_stack_grafts_into_frame_spine; eauto.
    intros edges last Read. now apply import_frame_spine_root.
  - destruct Stack as [Stack _]. replace frames with (frames ++ []) by apply app_nil_r.
    eapply import_history_stack_grafts_into_frame_spine; eauto.
    intros edges last Read. now apply import_frame_spine_root.
Qed.

Theorem import_history_stack_extends_to_selected_child : forall read key context path frames,
  import_history_stack read key context path frames -> forall top rest edge edges,
  frames = top :: rest ->
  import_wire_slot_lookup (import_wire_slot edge) (import_frame_edges top) = Some edge ->
  128 <= import_wire_header edge -> read (import_wire_hash edge) = Some edges ->
  import_history_stack read key context (path ++ import_wire_slot edge :: import_wire_prefix edge)
    (import_frame_at (import_wire_hash edge)
      (import_frame_prefix top ++ import_wire_slot edge :: import_wire_prefix edge) edges None ::
      import_advance_frame top (import_wire_slot edge) :: rest).
Proof.
  intros read key context path frames Stack.
  induction Stack as [key context parent_edges Read|
    key context parent_edges parent_edge suffix frames Read Lookup Kind Child IH];
    intros top rest edge edges Frames ChildLookup ChildKind ChildRead.
  - inversion Frames; subst top rest. cbn [app import_frame_prefix import_frame_edges import_frame_at] in *.
    assert (Extended : import_history_stack read key context
      (import_wire_slot edge :: import_wire_prefix edge ++ [])
      ([import_frame_at (import_wire_hash edge)
        (context ++ import_wire_slot edge :: import_wire_prefix edge) edges None] ++
        [import_frame_at key context parent_edges (Some (import_wire_slot edge))])).
    { eapply import_history_stack_child; eauto. now apply import_history_stack_here. }
    rewrite app_nil_r in Extended. exact Extended.
  - destruct frames as [|child child_rest].
    + exfalso. eapply import_history_stack_is_nonempty; eauto.
    + cbn [app] in Frames. inversion Frames; subst top rest.
      specialize (IH child child_rest edge edges eq_refl ChildLookup ChildKind ChildRead).
      cbn [app]. rewrite <- app_assoc.
      change (import_history_stack read key context
        (import_wire_slot parent_edge :: import_wire_prefix parent_edge ++
          (suffix ++ import_wire_slot edge :: import_wire_prefix edge))
        ((import_frame_at (import_wire_hash edge)
          (import_frame_prefix child ++ import_wire_slot edge :: import_wire_prefix edge) edges None ::
          import_advance_frame child (import_wire_slot edge) :: child_rest) ++
          [import_frame_at key context parent_edges (Some (import_wire_slot parent_edge))])).
      eapply import_history_stack_child; eauto.
Qed.

Theorem import_frame_spine_reconstructs_its_current_position : forall read root frames,
  import_frame_spine read root frames -> forall top rest, frames = top :: rest ->
  import_history_stack read root [] (import_frame_prefix top) (import_fresh_frame top :: rest).
Proof.
  intros read root frames Spine. induction Spine as
    [|edges last Read|parent rest edge edges last Parent IH Slot Lookup Kind Read];
    intros top tail Frames; try discriminate.
  - inversion Frames; subst top tail. now apply import_history_stack_here.
  - inversion Frames; subst top tail. specialize (IH parent rest eq_refl).
    pose proof (import_history_stack_extends_to_selected_child read root []
      (import_frame_prefix parent) (import_fresh_frame parent :: rest) IH
      (import_fresh_frame parent) rest edge edges eq_refl Lookup Kind Read) as Extended.
    cbn [import_fresh_frame import_frame_key import_frame_prefix import_frame_edges import_frame_at] in *.
    replace (import_advance_frame (import_fresh_frame parent) (import_wire_slot edge)) with parent in Extended.
    + exact Extended.
    + destruct parent as [key prefix parent_edges parent_last].
      cbn [import_frame_last_slot] in Slot. subst parent_last. reflexivity.
Qed.

Theorem import_frame_spine_with_fresh_top_reconstructs_exactly : forall read root top rest,
  import_frame_spine read root (top :: rest) -> import_frame_last_slot top = None ->
  import_build_history_stack (S (length (import_frame_prefix top))) read root []
    (import_frame_prefix top) = Some (top :: rest).
Proof.
  intros read root top rest Spine Fresh. apply import_stack_builder_characterization.
  pose proof (import_frame_spine_reconstructs_its_current_position _ _ _ Spine top rest eq_refl) as Stack.
  replace (import_fresh_frame top) with top in Stack; auto.
  destruct top as [key prefix edges last]. cbn [import_frame_last_slot] in Fresh. subst last. reflexivity.
Qed.

Definition import_top_is_fresh (frames : list ImportTraversalFrame) : Prop :=
  match frames with [] => True | top :: _ => import_frame_last_slot top = None end.

Theorem import_history_step_creates_fresh_top : forall read initial event final,
  import_export_step read initial event final -> import_event_history_cost event = 1 ->
  import_top_is_fresh final.
Proof.
  intros read initial event final Step Cost. destruct Step;
    cbn [import_event_history_cost import_leaf_edge_entry import_history_edge_entry] in Cost;
    try discriminate. reflexivity.
Qed.

Theorem import_page_stop_has_fresh_top : forall read budget initial entries final,
  import_export_page read budget initial entries final ->
  (budget = 0 -> import_top_is_fresh initial) -> import_top_is_fresh final.
Proof.
  intros read budget initial entries final Page.
  induction Page as [frames|budget|budget initial event next entries final Positive Step Page IH];
    intros Initial.
  - now apply Initial.
  - exact I.
  - apply IH. intros Exhausted. eapply import_history_step_creates_fresh_top; eauto.
    destruct event as [[key prefix|key]|]; cbn [import_event_history_cost] in *; lia.
Qed.

Theorem import_slice_stop_has_fresh_top : forall read skip budget initial entries final,
  import_export_slice read skip budget initial entries final ->
  (skip = 0 -> budget = 0 -> import_top_is_fresh initial) -> import_top_is_fresh final.
Proof.
  intros read skip budget initial entries final Slice.
  induction Slice as [budget initial entries final Page|skip budget|
    skip budget initial event next entries final Positive Step Slice IH]; intros Initial.
  - eapply import_page_stop_has_fresh_top; eauto.
  - exact I.
  - apply IH. intros Exhausted ZeroBudget. eapply import_history_step_creates_fresh_top; eauto.
    destruct event as [[key prefix|key]|]; cbn [import_event_history_cost] in *; lia.
Qed.

Theorem import_cursor_stack_starts_with_fresh_top : forall read cursor frames,
  import_cursor_initial_stack read cursor frames -> import_top_is_fresh frames.
Proof.
  intros read [root|root prefix carrier] frames Stack;
    cbn [import_cursor_initial_stack] in Stack.
  - apply import_history_stack_top_retains_absolute_position in Stack
      as [top [rest [Frames [_ [Fresh _]]]]]. subst frames. exact Fresh.
  - destruct Stack as [Stack _]. apply import_history_stack_top_retains_absolute_position in Stack
      as [top [rest [Frames [_ [Fresh _]]]]]. subst frames. exact Fresh.
Qed.

Theorem import_anchored_stop_has_fresh_top : forall read anchor skip budget initial entries final,
  import_anchored_export read anchor skip budget initial entries final ->
  import_top_is_fresh initial -> import_top_is_fresh final.
Proof.
  intros read anchor skip budget initial entries final Export Fresh. destruct Export.
  - eapply import_slice_stop_has_fresh_top; eauto.
  - eapply import_slice_stop_has_fresh_top; eauto.
  - eapply import_page_stop_has_fresh_top; eauto.
Qed.

Theorem import_page_preserves_frame_spine : forall read budget initial entries final,
  import_export_page read budget initial entries final -> forall root,
  import_frame_spine read root initial -> import_frame_spine read root final.
Proof.
  intros read budget initial entries final Page. induction Page; intros root Spine; auto.
  apply IHPage. eapply import_operational_step_preserves_frame_spine; eauto.
Qed.

Theorem import_slice_preserves_frame_spine : forall read skip budget initial entries final,
  import_export_slice read skip budget initial entries final -> forall root,
  import_frame_spine read root initial -> import_frame_spine read root final.
Proof.
  intros read skip budget initial entries final Slice. induction Slice; intros root Spine; auto.
  - eapply import_page_preserves_frame_spine; eauto.
  - apply IHSlice. eapply import_operational_step_preserves_frame_spine; eauto.
Qed.

Theorem import_anchored_export_preserves_frame_spine : forall read anchor skip budget initial entries final,
  import_anchored_export read anchor skip budget initial entries final -> forall root,
  import_frame_spine read root initial -> import_frame_spine read root final.
Proof.
  intros read anchor skip budget initial entries final Export root Spine. destruct Export.
  - eapply import_slice_preserves_frame_spine; eauto.
  - eapply import_slice_preserves_frame_spine; eauto.
  - eapply import_page_preserves_frame_spine; eauto.
Qed.

Theorem import_cursor_export_final_position_reconstructs_exactly :
  forall read cursor skip budget entries top rest,
  import_export_from_cursor read cursor skip budget entries (top :: rest) ->
  import_build_history_stack (S (length (import_frame_prefix top))) read (import_cursor_root cursor) []
    (import_frame_prefix top) = Some (top :: rest).
Proof.
  intros read cursor skip budget entries top rest [_ [_ [initial [Stack Export]]]].
  apply import_frame_spine_with_fresh_top_reconstructs_exactly.
  - eapply import_anchored_export_preserves_frame_spine; eauto.
    now apply import_cursor_stack_has_frame_spine.
  - change (import_top_is_fresh (top :: rest)).
    eapply import_anchored_stop_has_fresh_top; eauto.
    eapply import_cursor_stack_starts_with_fresh_top. exact Stack.
Qed.

Definition import_frame_position_code (frame : ImportTraversalFrame) : nat :=
  match import_frame_last_slot frame with None => 0 | Some slot => S slot end.

Definition import_stack_position (frames : list ImportTraversalFrame) : list nat :=
  map import_frame_position_code (rev frames).

Inductive import_position_before : list nat -> list nat -> Prop :=
| import_position_exit : forall first rest, import_position_before (first :: rest) []
| import_position_later_slot : forall first second left right,
    first < second -> import_position_before (first :: left) (second :: right)
| import_position_same_slot : forall slot left right,
    import_position_before left right -> import_position_before (slot :: left) (slot :: right).

Theorem import_position_before_is_irreflexive : forall position,
  ~ import_position_before position position.
Proof.
  induction position as [|slot rest IH]; intros Before; inversion Before; subst; try lia.
  now apply IH.
Qed.

Theorem import_position_before_is_transitive : forall first second third,
  import_position_before first second -> import_position_before second third ->
  import_position_before first third.
Proof.
  intros first second third Before. revert third.
  induction Before as [head rest|first second left right Earlier|
    slot left right Before IH]; intros third Later; inversion Later; subst;
    auto using import_position_exit, import_position_same_slot.
  - apply import_position_later_slot. lia.
  - now apply import_position_later_slot.
  - now apply import_position_later_slot.
Qed.

Theorem import_position_prefix_preserves_order : forall left right,
  import_position_before left right -> forall prefix,
  import_position_before (prefix ++ left) (prefix ++ right).
Proof.
  intros left right Before prefix. induction prefix; cbn [app]; auto using import_position_same_slot.
Qed.

Theorem import_selected_slot_advances_frame_code : forall frame selected,
  import_frame_next frame = Some selected ->
  import_frame_position_code frame < S (import_next_slot selected).
Proof.
  intros frame selected Found.
  destruct (import_next_entry_preserves_all_skipped_slots _ _ _ Found)
    as [skipped [Slots _]].
  assert (In (import_next_slot selected) (import_frame_pending_slots frame)) as Present.
  { rewrite Slots. apply in_or_app. right. now left. }
  apply import_pending_slot_membership_is_exact in Present as [_ Later].
  unfold import_frame_position_code. destruct (import_frame_last_slot frame); lia.
Qed.

Theorem import_every_step_strictly_advances_position : forall read initial event final,
  import_export_step read initial event final ->
  import_position_before (import_stack_position initial) (import_stack_position final).
Proof.
  intros read initial event final Step. destruct Step as
    [frame rest Empty|frame rest selected Found Leaf|frame rest selected edges Found History Read];
    unfold import_stack_position; cbn [rev]; rewrite !map_app; cbn [map].
  - replace (map import_frame_position_code (rev rest)) with
      (map import_frame_position_code (rev rest) ++ []) at 2 by apply app_nil_r.
    apply import_position_prefix_preserves_order. constructor.
  - apply import_position_prefix_preserves_order. apply import_position_later_slot.
    now apply import_selected_slot_advances_frame_code.
  - rewrite <- app_assoc. apply import_position_prefix_preserves_order.
    apply import_position_later_slot. now apply import_selected_slot_advances_frame_code.
Qed.

Theorem import_page_position_progress_or_initial_stop : forall read budget initial entries final,
  import_export_page read budget initial entries final ->
  import_position_before (import_stack_position initial) (import_stack_position final) \/
  (initial = final /\ (budget = 0 \/ initial = [])).
Proof.
  intros read budget initial entries final Page. induction Page as
    [frames|budget|budget initial event next entries final Positive Step Page IH].
  - right. auto.
  - right. auto.
  - left. pose proof (import_every_step_strictly_advances_position _ _ _ _ Step) as Progress.
    destruct IH as [Later|[Same _]].
    + eapply import_position_before_is_transitive; eauto.
    + now rewrite <- Same.
Qed.

Theorem import_slice_position_progress_or_initial_stop : forall read skip budget initial entries final,
  import_export_slice read skip budget initial entries final ->
  import_position_before (import_stack_position initial) (import_stack_position final) \/
  (initial = final /\ ((skip = 0 /\ budget = 0) \/ initial = [])).
Proof.
  intros read skip budget initial entries final Slice. induction Slice as
    [budget initial entries final Page|skip budget|
    skip budget initial event next entries final Positive Step Slice IH].
  - destruct (import_page_position_progress_or_initial_stop _ _ _ _ _ Page)
      as [Progress|[Same [Zero|Empty]]]; auto.
  - right. auto.
  - left. pose proof (import_every_step_strictly_advances_position _ _ _ _ Step) as Progress.
    destruct IH as [Later|[Same _]].
    + eapply import_position_before_is_transitive; eauto.
    + now rewrite <- Same.
Qed.

Theorem import_positive_slice_from_nonempty_stack_advances : forall read skip budget initial entries final,
  import_export_slice read skip budget initial entries final ->
  initial <> [] -> (0 < skip \/ 0 < budget) ->
  import_position_before (import_stack_position initial) (import_stack_position final).
Proof.
  intros read skip budget initial entries final Slice Nonempty Positive.
  destruct (import_slice_position_progress_or_initial_stop _ _ _ _ _ _ Slice)
    as [Progress|[_ [[ZeroSkip ZeroBudget]|Empty]]]; auto; try contradiction.
  destruct Positive; lia.
Qed.

Theorem import_cursor_initial_stack_is_unique : forall read cursor first second,
  import_cursor_initial_stack read cursor first -> import_cursor_initial_stack read cursor second -> first = second.
Proof.
  intros read [root|root prefix carrier] first second First Second.
  - eapply import_history_stack_reconstruction_is_deterministic; eauto.
  - destruct First as [First _], Second as [Second _].
    eapply import_history_stack_reconstruction_is_deterministic; eauto.
Qed.

Fixpoint import_last_key (keys : list nat) : option nat :=
  match keys with
  | [] => None
  | key :: rest => match import_last_key rest with None => Some key | Some last => Some last end
  end.

Definition import_wire_next_cursor (input : ImportCursor) (entries : list ImportExportEntry)
    (frames : list ImportTraversalFrame) : ImportCursor :=
  match frames with
  | frame :: _ => ImportCursorResume (import_cursor_root input)
      (import_frame_prefix frame) (import_frame_key frame)
  | [] => match import_last_key (import_export_history_keys entries) with
    | Some key => ImportCursorStart (import_nat_to_key 32 key)
    | None => input
    end
  end.

Record ImportWireReferences : Type := {
  import_reply_history : list nat;
  import_reply_leaves : list nat;
  import_reply_next_path : ImportWirePath
}.

Inductive ImportWireResult : Type :=
| ImportWireSuccess : ImportWireReferences -> ImportWireResult
| ImportWireInvalidInput : ImportWireResult
| ImportWireUnencodableCursor : ImportWireResult
| ImportWireReadFailure : list nat -> ImportWireResult
| ImportWireFuelExhausted : ImportWireResult.

Definition import_adapt_wire_result (input : ImportCursor) (result : ImportEvaluation) : ImportWireResult :=
  if import_cursor_validb input then
    match result with
    | ImportEvaluationSuccess entries frames =>
        match import_cursor_encode (import_wire_next_cursor input entries frames) with
        | None => ImportWireUnencodableCursor
        | Some path => ImportWireSuccess
            {| import_reply_history := import_export_history_keys entries;
               import_reply_leaves := import_export_leaf_keys entries;
               import_reply_next_path := path |}
        end
    | ImportEvaluationReadFailure key => ImportWireReadFailure key
    | ImportEvaluationFuelExhausted => ImportWireFuelExhausted
    end
  else ImportWireInvalidInput.

Theorem import_wire_success_preserves_exact_occurrences : forall input result reply,
  import_adapt_wire_result input result = ImportWireSuccess reply ->
  exists entries frames,
    result = ImportEvaluationSuccess entries frames /\
    import_reply_history reply = import_export_history_keys entries /\
    import_reply_leaves reply = import_export_leaf_keys entries /\
    import_cursor_validb input = true /\
    import_cursor_encode (import_wire_next_cursor input entries frames) =
      Some (import_reply_next_path reply).
Proof.
  intros input result reply Adapted. unfold import_adapt_wire_result in Adapted.
  destruct (import_cursor_validb input) eqn:Valid; [|discriminate].
  destruct result as [entries frames|missing|]; try discriminate.
  destruct (import_cursor_encode (import_wire_next_cursor input entries frames)) as [path|] eqn:Encoded;
    [|discriminate]. inversion Adapted; subst reply. exists entries, frames. auto.
Qed.

Theorem import_wire_success_has_canonical_next_cursor : forall input result reply,
  import_adapt_wire_result input result = ImportWireSuccess reply ->
  exists next, import_cursor_decode (import_reply_next_path reply) = Some next /\
    import_cursor_validb next = true /\
    Forall (fun entry => snd entry = None /\ length (fst entry) = 32 /\
      Forall import_wire_byte (fst entry)) (import_reply_next_path reply).
Proof.
  intros input result reply Adapted.
  destruct (import_wire_success_preserves_exact_occurrences _ _ _ Adapted)
    as [entries [frames [_ [_ [_ [_ Encoded]]]]]].
  pose proof (import_cursor_encoder_requires_validity _ _ Encoded) as [Valid Same].
  exists (import_wire_next_cursor input entries frames). split.
  - now apply import_cursor_decode_encode_round_trip.
  - split; auto.
    pose proof (import_cursor_decode_encode_round_trip _ _ Encoded) as Decoded.
    pose proof (import_accepted_cursor_has_exact_shape _ _ Decoded) as [_ Shape].
    pose proof (import_valid_cursor_encoding_has_fixed_width_words _ Valid) as Words.
    rewrite <- Same in Words. rewrite !Forall_forall in *.
    intros entry Present. split; auto.
Qed.

Theorem import_wire_adapter_never_changes_errors_to_success : forall input key reply,
  import_adapt_wire_result input (ImportEvaluationReadFailure key) <> ImportWireSuccess reply /\
  import_adapt_wire_result input ImportEvaluationFuelExhausted <> ImportWireSuccess reply.
Proof.
  intros. unfold import_adapt_wire_result. destruct (import_cursor_validb input); split; discriminate.
Qed.

Theorem import_wire_adapter_preserves_observed_read_failure : forall input key,
  import_cursor_validb input = true ->
  import_adapt_wire_result input (ImportEvaluationReadFailure key) = ImportWireReadFailure key.
Proof. intros input key Valid. unfold import_adapt_wire_result. now rewrite Valid. Qed.

Theorem import_wire_bounded_execution_cannot_report_fuel_exhaustion : forall input read skip budget frames,
  import_adapt_wire_result input (import_evaluate_bounded_slice read skip budget frames) <>
    ImportWireFuelExhausted.
Proof.
  intros input read skip budget frames.
  destruct (import_bounded_slice_returns_success_or_read_failure read skip budget frames)
    as [[entries [final Result]]|[key Result]]; rewrite Result; unfold import_adapt_wire_result;
    destruct (import_cursor_validb input); try discriminate.
  destruct (import_cursor_encode (import_wire_next_cursor input entries final)); discriminate.
Qed.

Theorem import_wire_bounded_execution_preserves_history_limit : forall input read skip budget initial reply,
  import_adapt_wire_result input (import_evaluate_bounded_slice read skip budget initial) =
    ImportWireSuccess reply -> length (import_reply_history reply) <= budget.
Proof.
  intros input read skip budget initial reply Adapted.
  destruct (import_wire_success_preserves_exact_occurrences _ _ _ Adapted)
    as [entries [frames [Result [History _]]]]. rewrite History.
  eapply import_bounded_evaluation_respects_history_budget. exact Result.
Qed.

Theorem import_nonempty_frames_use_captured_carrier : forall input entries frame rest,
  import_wire_next_cursor input entries (frame :: rest) =
    ImportCursorResume (import_cursor_root input) (import_frame_prefix frame) (import_frame_key frame).
Proof. reflexivity. Qed.

Theorem import_cursor_export_next_cursor_has_exact_residual_stack :
  forall read input skip budget entries top rest,
  import_export_from_cursor read input skip budget entries (top :: rest) ->
  import_cursor_initial_stack read (import_wire_next_cursor input entries (top :: rest)) (top :: rest).
Proof.
  intros read input skip budget entries top rest Export.
  cbn [import_wire_next_cursor import_cursor_initial_stack]. split.
  - apply import_stack_builder_characterization.
    eapply import_cursor_export_final_position_reconstructs_exactly. exact Export.
  - exists top, rest. auto.
Qed.

Theorem import_interleaved_cursor_export_next_cursor_has_exact_residual_stack :
  forall first last input skip budget entries top rest,
  import_interleaved_cursor_export first last input skip budget entries (top :: rest) ->
  import_cursor_initial_stack last (import_wire_next_cursor input entries (top :: rest)) (top :: rest).
Proof.
  intros first last input skip budget entries top rest Export.
  apply import_interleaved_cursor_export_has_exact_final_view in Export.
  eapply import_cursor_export_next_cursor_has_exact_residual_stack. exact Export.
Qed.

Theorem import_nonterminal_cursor_export_changes_its_cursor :
  forall read input skip budget entries top rest,
  import_export_from_cursor read input skip budget entries (top :: rest) ->
  import_wire_next_cursor input entries (top :: rest) <> input.
Proof.
  intros read input skip budget entries top rest Export Same.
  pose proof (import_cursor_export_next_cursor_has_exact_residual_stack _ _ _ _ _ _ _ Export) as FinalStack.
  rewrite Same in FinalStack.
  destruct input as [root|root prefix carrier]; [discriminate|].
  destruct Export as [_ [Positive [initial [InitialStack Export]]]].
  pose proof (import_cursor_initial_stack_is_unique _ _ _ _ InitialStack FinalStack) as Equal.
  subst initial. inversion Export; subst.
  assert (top :: rest <> []) as Nonempty by discriminate.
  pose proof (import_positive_slice_from_nonempty_stack_advances read skip budget (top :: rest)
    entries (top :: rest) H Nonempty Positive) as Progress.
  eapply import_position_before_is_irreflexive. exact Progress.
Qed.

Theorem import_nonterminal_interleaved_cursor_export_changes_its_cursor :
  forall first last input skip budget entries top rest,
  import_interleaved_cursor_export first last input skip budget entries (top :: rest) ->
  import_wire_next_cursor input entries (top :: rest) <> input.
Proof.
  intros first last input skip budget entries top rest Export.
  apply import_interleaved_cursor_export_has_exact_final_view in Export.
  eapply import_nonterminal_cursor_export_changes_its_cursor. exact Export.
Qed.

Theorem import_wire_budget_metadata_is_independent_of_entries : forall input first second frame rest,
  import_wire_next_cursor input first (frame :: rest) =
    import_wire_next_cursor input second (frame :: rest).
Proof. reflexivity. Qed.

Theorem import_wire_rejects_unrepresentable_budget_prefix : forall input entries frame rest,
  import_cursor_validb input = true -> 128 < length (import_frame_prefix frame) ->
  import_adapt_wire_result input (ImportEvaluationSuccess entries (frame :: rest)) =
    ImportWireUnencodableCursor.
Proof.
  intros input entries frame rest Valid TooLong.
  unfold import_adapt_wire_result. rewrite Valid.
  unfold import_cursor_encode, import_wire_next_cursor. cbn [import_cursor_validb].
  assert (length (import_frame_prefix frame) <=? 128 = false) as Width by now apply Nat.leb_gt.
  rewrite Width, Bool.andb_false_r. reflexivity.
Qed.

Theorem import_no_history_exhaustion_retains_input_cursor : forall input entries,
  import_export_history_keys entries = [] -> import_wire_next_cursor input entries [] = input.
Proof. intros input entries Empty. unfold import_wire_next_cursor. now rewrite Empty. Qed.

Theorem import_leaf_only_terminal_page_retains_every_leaf : forall input leaves,
  import_cursor_validb input = true ->
  import_adapt_wire_result input (ImportEvaluationSuccess (map ImportExportLeaf leaves) []) =
    ImportWireSuccess {| import_reply_history := []; import_reply_leaves := leaves;
      import_reply_next_path := import_cursor_encode_raw input |}.
Proof.
  intros input leaves Valid. unfold import_adapt_wire_result. rewrite Valid.
  unfold import_wire_next_cursor. rewrite import_export_leaf_map_has_no_history.
  cbn [import_last_key]. unfold import_cursor_encode. rewrite Valid.
  now rewrite import_export_leaf_map_preserves_keys.
Qed.

Lemma import_last_key_of_nonempty_suffix : forall prefix key suffix,
  import_last_key (prefix ++ key :: suffix) = import_last_key (key :: suffix).
Proof.
  induction prefix as [|head rest IH]; intros key suffix; [reflexivity|].
  cbn [app import_last_key]. rewrite IH.
  cbn [import_last_key]. destruct (import_last_key suffix); reflexivity.
Qed.

Theorem import_exhausted_page_retains_legacy_history_singleton : forall input before key prefix after,
  import_export_history_keys after = [] ->
  import_wire_next_cursor input (before ++ ImportExportHistory key prefix :: after) [] =
    ImportCursorStart (import_nat_to_key 32 key).
Proof.
  intros input before key prefix after NoHistory. unfold import_wire_next_cursor.
  rewrite import_export_history_keys_app. cbn [import_export_history_keys].
  rewrite NoHistory, import_last_key_of_nonempty_suffix. reflexivity.
Qed.

Theorem import_exhausted_singleton_retains_exact_raw_history_key : forall input before bytes prefix after,
  import_cursor_word_validb bytes = true -> import_export_history_keys after = [] ->
  import_wire_next_cursor input (before ++ ImportExportHistory (import_key_to_nat bytes) prefix :: after) [] =
    ImportCursorStart bytes.
Proof.
  intros input before bytes prefix after Valid NoHistory.
  rewrite import_exhausted_page_retains_legacy_history_singleton by assumption.
  apply import_cursor_word_validity_characterization in Valid as [Width Bytes].
  pose proof (import_key_conversion_is_reversible bytes Bytes) as Reversible.
  now rewrite Width in Reversible; rewrite Reversible.
Qed.

Theorem import_wire_adapter_preserves_interleaved_results :
  forall first last skip budget initial entries final input reply,
  import_interleaved_slice first last skip budget initial entries final ->
  import_adapt_wire_result input (ImportEvaluationSuccess entries final) = ImportWireSuccess reply ->
  import_adapt_wire_result input (import_evaluate_bounded_slice last skip budget initial) =
    ImportWireSuccess reply.
Proof.
  intros first last skip budget initial entries final input reply Slice Adapted.
  rewrite (import_interleaved_slice_matches_final_evaluator _ _ _ _ _ _ _ Slice). exact Adapted.
Qed.

Inductive ImportRootLookup : Type :=
| ImportRootLookupAbsent : ImportRootLookup
| ImportRootLookupFound : list nat -> ImportRootLookup
| ImportRootLookupFailure : ImportRootLookup.

Inductive ImportRootAvailability : Type :=
| ImportRootUnavailable : ImportRootAvailability
| ImportRootAvailable : list ImportWireEdge -> ImportRootAvailability
| ImportRootStorageFailure : ImportRootAvailability
| ImportRootInvalidPayload : ImportRootAvailability.

Definition import_observe_root (read : list nat -> ImportRootLookup)
    (check : list nat -> list nat -> option (list ImportWireEdge))
    (input : ImportCursor) : ImportRootAvailability :=
  match read (import_cursor_root input) with
  | ImportRootLookupAbsent => ImportRootUnavailable
  | ImportRootLookupFound bytes => match check (import_cursor_root input) bytes with
    | None => ImportRootInvalidPayload
    | Some edges => ImportRootAvailable edges
    end
  | ImportRootLookupFailure => ImportRootStorageFailure
  end.

Theorem import_unavailable_classification_requires_initial_observation : forall read check input,
  import_observe_root read check input = ImportRootUnavailable <->
    read (import_cursor_root input) = ImportRootLookupAbsent.
Proof.
  intros read check input. unfold import_observe_root.
  destruct (read (import_cursor_root input)) as [|bytes|]; try (split; congruence).
  destruct (check (import_cursor_root input) bytes); split; congruence.
Qed.

Theorem import_root_storage_failure_cannot_be_unavailability : forall read check input,
  read (import_cursor_root input) = ImportRootLookupFailure ->
  import_observe_root read check input = ImportRootStorageFailure.
Proof. intros read check input Failed. unfold import_observe_root. now rewrite Failed. Qed.

Theorem import_invalid_root_payload_cannot_be_unavailability : forall read check input bytes,
  read (import_cursor_root input) = ImportRootLookupFound bytes ->
  check (import_cursor_root input) bytes = None ->
  import_observe_root read check input = ImportRootInvalidPayload.
Proof.
  intros read check input bytes Found Invalid. unfold import_observe_root. now rewrite Found, Invalid.
Qed.

Definition import_wire_unavailable_reply (input : ImportCursor) : ImportWireReferences :=
  {| import_reply_history := []; import_reply_leaves := [];
     import_reply_next_path := import_cursor_encode_raw input |}.

Theorem import_empty_wire_response_does_not_identify_availability : forall input,
  import_cursor_validb input = true ->
  import_adapt_wire_result input (ImportEvaluationSuccess [] []) =
    ImportWireSuccess (import_wire_unavailable_reply input).
Proof.
  intros input Valid. apply (import_leaf_only_terminal_page_retains_every_leaf input [] Valid).
Qed.

Definition import_check_captured_root (Hash : list nat -> list nat) (key bytes : list nat) :=
  import_checked_wire_history Hash (fun _ => Some bytes) key.

Theorem import_available_root_has_captured_authenticated_bytes : forall read Hash input edges,
  import_observe_root read (import_check_captured_root Hash) input = ImportRootAvailable edges ->
  exists bytes,
    read (import_cursor_root input) = ImportRootLookupFound bytes /\
    length (import_cursor_root input) = 32 /\ Forall import_wire_byte (import_cursor_root input) /\
    Hash bytes = import_cursor_root input /\ import_parse_wire_node bytes = Some edges.
Proof.
  intros read Hash input edges Available. unfold import_observe_root in Available.
  destruct (read (import_cursor_root input)) as [|bytes|] eqn:Found; try discriminate.
  destruct (import_check_captured_root Hash (import_cursor_root input) bytes)
    as [checked|] eqn:Checked; try discriminate.
  inversion Available; subst checked.
  apply import_checked_wire_history_has_exact_witness in Checked
    as [Width [Bytes [observed [Same [Hashed Parsed]]]]].
  inversion Same; subst observed. exists bytes. auto.
Qed.

Inductive ImportRowsResult : Type :=
| ImportRowsSuccess : list (nat * list nat) -> ImportRowsResult
| ImportRowsMissing : nat -> ImportRowsResult
| ImportRowsFailure : nat -> ImportRowsResult.

Fixpoint import_load_payload_rows (read : nat -> ImportRootLookup) (keys : list nat) : ImportRowsResult :=
  match keys with
  | [] => ImportRowsSuccess []
  | key :: rest => match read key with
    | ImportRootLookupAbsent => ImportRowsMissing key
    | ImportRootLookupFailure => ImportRowsFailure key
    | ImportRootLookupFound bytes => match import_load_payload_rows read rest with
      | ImportRowsSuccess rows => ImportRowsSuccess ((key, bytes) :: rows)
      | ImportRowsMissing missing => ImportRowsMissing missing
      | ImportRowsFailure failed => ImportRowsFailure failed
      end
    end
  end.

Definition import_exact_key_selection (occurrences selected : list nat) : Prop :=
  NoDup selected /\ forall key, In key selected <-> In key occurrences.

Theorem import_successful_row_loading_retains_every_selected_key : forall read keys rows,
  import_load_payload_rows read keys = ImportRowsSuccess rows ->
  map fst rows = keys /\
  Forall (fun row => read (fst row) = ImportRootLookupFound (snd row)) rows.
Proof.
  intros read keys. induction keys as [|key rest IH]; intros rows Loaded.
  - inversion Loaded; subst rows. split; auto.
  - cbn [import_load_payload_rows] in Loaded.
    destruct (read key) as [|bytes|] eqn:Read; try discriminate.
    destruct (import_load_payload_rows read rest) as [tail|missing|failed] eqn:Tail; try discriminate.
    inversion Loaded; subst rows. specialize (IH tail eq_refl) as [Keys Reads].
    cbn [map fst]. split; [now rewrite Keys|]. constructor; auto.
Qed.

Theorem import_deduplicated_payloads_cover_exact_occurrence_keys : forall read occurrences selected rows,
  import_exact_key_selection occurrences selected ->
  import_load_payload_rows read selected = ImportRowsSuccess rows ->
  NoDup (map fst rows) /\ (forall key, In key (map fst rows) <-> In key occurrences) /\
  Forall (fun row => read (fst row) = ImportRootLookupFound (snd row)) rows.
Proof.
  intros read occurrences selected rows [Unique Cover] Loaded.
  apply import_successful_row_loading_retains_every_selected_key in Loaded as [Keys Reads].
  rewrite Keys. auto.
Qed.

Theorem import_missing_payload_cannot_be_silently_omitted : forall read occurrences selected rows key,
  import_exact_key_selection occurrences selected -> In key occurrences ->
  (read key = ImportRootLookupAbsent \/ read key = ImportRootLookupFailure) ->
  import_load_payload_rows read selected <> ImportRowsSuccess rows.
Proof.
  intros read occurrences selected rows key Selection Present Unreadable Loaded.
  destruct (import_deduplicated_payloads_cover_exact_occurrence_keys _ _ _ _ Selection Loaded)
    as [_ [Cover Reads]]. apply Cover in Present.
  apply in_map_iff in Present as [[found bytes] [Same Present]]. cbn [fst] in Same. subst found.
  apply Forall_forall with (x := (key, bytes)) in Reads; auto.
  cbn [fst snd] in Reads. destruct Unreadable; congruence.
Qed.

Theorem import_independent_payload_orders_preserve_same_key_set :
  forall read occurrences first_keys second_keys first_rows second_rows,
  import_exact_key_selection occurrences first_keys ->
  import_exact_key_selection occurrences second_keys ->
  import_load_payload_rows read first_keys = ImportRowsSuccess first_rows ->
  import_load_payload_rows read second_keys = ImportRowsSuccess second_rows ->
  forall key, In key (map fst first_rows) <-> In key (map fst second_rows).
Proof.
  intros read occurrences first_keys second_keys first_rows second_rows First Second LoadFirst LoadSecond key.
  destruct (import_deduplicated_payloads_cover_exact_occurrence_keys _ _ _ _ First LoadFirst)
    as [_ [FirstKeys _]].
  destruct (import_deduplicated_payloads_cover_exact_occurrence_keys _ _ _ _ Second LoadSecond)
    as [_ [SecondKeys _]]. now rewrite FirstKeys, SecondKeys.
Qed.

Definition import_payload_reader_extension (first last : nat -> ImportRootLookup) : Prop :=
  forall key bytes, first key = ImportRootLookupFound bytes -> last key = ImportRootLookupFound bytes.

Inductive import_interleaved_payload_rows : (nat -> ImportRootLookup) -> (nat -> ImportRootLookup) ->
    list nat -> list (nat * list nat) -> Prop :=
| import_interleaved_payload_done : forall read,
    import_interleaved_payload_rows read read [] []
| import_interleaved_payload_read : forall first middle last key keys bytes rows,
    first key = ImportRootLookupFound bytes -> import_payload_reader_extension first middle ->
    import_interleaved_payload_rows middle last keys rows ->
    import_interleaved_payload_rows first last (key :: keys) ((key, bytes) :: rows)
| import_interleaved_payload_write : forall first middle last keys rows,
    import_payload_reader_extension first middle ->
    import_interleaved_payload_rows middle last keys rows ->
    import_interleaved_payload_rows first last keys rows.

Theorem import_interleaved_payload_loading_preserves_observed_bindings : forall first last keys rows,
  import_interleaved_payload_rows first last keys rows -> import_payload_reader_extension first last.
Proof.
  intros first last keys rows Loaded. induction Loaded;
    intros key0 bytes0 Read; auto.
Qed.

Theorem import_interleaved_payload_loading_matches_final_view : forall first last keys rows,
  import_interleaved_payload_rows first last keys rows ->
  import_load_payload_rows last keys = ImportRowsSuccess rows.
Proof.
  intros first last keys rows Loaded. induction Loaded as
    [read|first middle last key keys bytes rows Read Extend Loaded IH|
    first middle last keys rows Extend Loaded IH]; auto.
  assert (last key = ImportRootLookupFound bytes) as FinalRead.
  { apply (import_interleaved_payload_loading_preserves_observed_bindings _ _ _ _ Loaded).
    now apply Extend. }
  cbn [import_load_payload_rows]. now rewrite FinalRead, IH.
Qed.

Theorem import_interleaved_payload_loading_preserves_exact_coverage :
  forall first last occurrences selected rows,
  import_exact_key_selection occurrences selected ->
  import_interleaved_payload_rows first last selected rows ->
  NoDup (map fst rows) /\ (forall key, In key (map fst rows) <-> In key occurrences) /\
  Forall (fun row => last (fst row) = ImportRootLookupFound (snd row)) rows.
Proof.
  intros first last occurrences selected rows Selection Loaded.
  eapply import_deduplicated_payloads_cover_exact_occurrence_keys; eauto.
  now apply import_interleaved_payload_loading_matches_final_view in Loaded.
Qed.

Example import_payload_can_arrive_before_its_first_read :
  import_interleaved_payload_rows (fun _ => ImportRootLookupAbsent)
    (fun _ => ImportRootLookupFound [1; 2]) [7] [(7, [1; 2])].
Proof.
  eapply import_interleaved_payload_write with (middle := fun _ => ImportRootLookupFound [1; 2]).
  - intros key bytes Read. discriminate.
  - eapply import_interleaved_payload_read.
    + reflexivity.
    + intros key bytes Read. exact Read.
    + constructor.
Qed.

Example import_duplicate_occurrences_need_one_payload_row :
  import_exact_key_selection [7; 7] [7] /\
  import_load_payload_rows (fun _ => ImportRootLookupFound [1; 2]) [7] =
    ImportRowsSuccess [(7, [1; 2])].
Proof.
  split; [|reflexivity]. split.
  - constructor; [simpl; tauto|constructor].
  - intros key. simpl. tauto.
Qed.

Example import_leaf_only_singleton_tail_is_preserved :
  import_adapt_wire_result (ImportCursorStart (import_execution_test_key 1))
    (ImportEvaluationSuccess [ImportExportLeaf 7; ImportExportLeaf 7] []) =
  ImportWireSuccess {| import_reply_history := []; import_reply_leaves := [7; 7];
    import_reply_next_path := [(import_execution_test_key 1, None)] |}.
Proof. reflexivity. Qed.

Example import_zero_entry_budget_boundary_has_seven_cursor_words :
  import_adapt_wire_result (ImportCursorStart (import_execution_test_key 1))
    (ImportEvaluationSuccess [] [import_frame_at (import_execution_test_key 1) [] [] None]) =
  ImportWireSuccess {| import_reply_history := []; import_reply_leaves := [];
    import_reply_next_path := import_cursor_encode_raw
      (ImportCursorResume (import_execution_test_key 1) [] (import_execution_test_key 1)) |}.
Proof. reflexivity. Qed.

Example import_resume_prefix_overflow_cannot_look_terminal :
  import_adapt_wire_result (ImportCursorStart (import_execution_test_key 1))
    (ImportEvaluationSuccess [ImportExportLeaf 7]
      [import_frame_at (import_execution_test_key 2) (repeat 0 129) [] None]) =
  ImportWireUnencodableCursor.
Proof. reflexivity. Qed.
